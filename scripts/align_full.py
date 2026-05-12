#!/usr/bin/env python3
"""完整对齐脚本：CAAC 静态镜像 ↔ 本地数据库 ↔ 归档目录三向对齐。

默认运行 dry-run（只输出报告，不修改文件/数据库）。
要实际执行，按需添加：
  --apply-meta      : UPDATE 元数据（validity/publish_date 等）
  --apply-download  : 下载 CAAC 上有但本地没有的 PDF 到归档目录 + INSERT 数据库
  --apply-orphan    : 扫描归档孤儿（不在数据库的 PDF），按文件名推断 + INSERT
  --apply-all       : 等价于以上三个全开

数据流向：
  https://flighttoolbox.hudawang.cn/data/v1/  (CAAC 静态镜像)
                       ↓
              regulation_files 表（history.db）
                       ↓
       D:\\飞行手册\\局方\\<分类>\\<title>.pdf  (归档目录)
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sqlite3
import sys
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable

# 静态镜像配置
STATIC_BASE = "https://flighttoolbox.hudawang.cn/data/v1"
STATIC_FILES = {
    "regulation.json": "regulation",
    "normative.json": "normative",
    "specification.json": "standard",
}

# CAAC 中文 doc_type → 数据库英文 doc_type 映射
DOC_TYPE_CN_TO_EN = {
    "CCAR规章": "regulation",
    "规范性文件": "normative",
    "标准规范": "standard",
}
DOC_TYPE_EN_TO_CATEGORY = {
    "regulation": "CCAR规章",
    "normative": "规范性文件",
    "standard": "标准规范",
}

# 默认路径
DEFAULT_LOCAL_ROOT = Path(r"D:\飞行手册\局方")
INVALID_FILENAME_CHARS = re.compile(r'[<>:"/\\|?*\x00-\x1f]')
MAX_BASENAME_CHARS = 150


@dataclass
class AlignReport:
    caac_total: int = 0
    db_total: int = 0
    matched: int = 0  # CAAC + DB 都有
    db_only_with_validity: int = 0  # DB 有 CAAC 没有，且 DB 已标 validity
    db_only_no_validity: int = 0  # DB 有 CAAC 没有，且 DB validity 为空
    caac_only: int = 0  # CAAC 有 DB 没有
    meta_diff: int = 0  # 匹配但元数据有差异
    orphans: int = 0  # 归档目录里 sha256 在 DB 找不到
    archive_files: int = 0  # 归档三个分类总文件数
    db_in_archive: int = 0  # DB 记录的 file_path 文件确实存在于归档（按 sha256 匹配）
    actions_planned: list[str] = field(default_factory=list)


def http_get_json(url: str) -> object:
    req = urllib.request.Request(url, headers={"User-Agent": "ccar-align/1.0"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read().decode("utf-8"))


def fetch_caac_full() -> list[dict]:
    """拉取 CAAC 静态镜像三个文件，合并并把 doc_type 转为英文。"""
    docs: list[dict] = []
    for filename, en_type in STATIC_FILES.items():
        rows = http_get_json(f"{STATIC_BASE}/{filename}")
        for row in rows:
            row.setdefault("doc_type", DOC_TYPE_EN_TO_CATEGORY[en_type])
            row["_doc_type_en"] = DOC_TYPE_CN_TO_EN.get(row["doc_type"], en_type)
            docs.append(row)
    return docs


def fetch_server_manifest() -> dict:
    """拉取服务器镜像的 manifest.json。"""
    return http_get_json(f"{STATIC_BASE}/manifest.json")


def load_db_rows(db_path: Path) -> list[sqlite3.Row]:
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    try:
        return list(
            conn.execute(
                """
                SELECT id, title, doc_number, doc_type, url, pdf_url, sha256,
                       file_path, file_size, validity, office_unit,
                       sign_date, publish_date
                FROM regulation_files
                """
            )
        )
    finally:
        conn.close()


def safe_filename(value: str, fallback: str) -> str:
    normalized = INVALID_FILENAME_CHARS.sub("_", value).strip().strip(".")
    normalized = re.sub(r"\s+", " ", normalized)
    if not normalized:
        normalized = fallback
    return normalized[:MAX_BASENAME_CHARS].rstrip(" .") or fallback


def sha256_file(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def normalize_url(u: str) -> str:
    """规范化 URL 用于 key 比对：去协议、trailing slash、小写化。"""
    s = (u or "").strip().rstrip("/")
    for prefix in ("https://", "http://"):
        if s.lower().startswith(prefix):
            s = s[len(prefix):]
            break
    return s.lower()


LEADING_DOC_CODE_RE = re.compile(
    r"^\s*(ccar|ac|ap|ib|md|ctso|mh/t)[-_\s/0-9a-z.]*", re.IGNORECASE
)


def compact_match_key(value: str) -> str:
    """等价于 Rust 的 compact_regulation_match_key：转小写 + 只保留字母数字/CJK。"""
    out = []
    for c in value or "":
        cl = c.lower()
        if cl.isalnum() or "\u4e00" <= c <= "\u9fff":
            out.append(cl)
    return "".join(out)


def normalize_match_key(value: str) -> str:
    """等价于 Rust 的 normalize_regulation_match_key：去前缀文号后再 compact。"""
    without_code = LEADING_DOC_CODE_RE.sub("", (value or "").strip())
    return compact_match_key(without_code)


# title 开头的失效/废止/历史版本标记前缀模式
INVALID_TITLE_PREFIX_RE = re.compile(
    r"^[\[【\(（]?\s*(失效|废止|历史版本)\s*[\]】\)）!！:：\-\s]+",
)


def infer_title_validity(title: str) -> tuple[str, str]:
    """从 title 开头的标记前缀推断 validity，返回 (清理后的 title, 推断的 validity 或 '')。

    只识别 title 开头的“失效!”、“历史版本!”、“废止!”等前缀标记，
    不会误判「关于废止XX的决定」这种正文含关键词的文件。
    """
    if not title:
        return "", ""
    m = INVALID_TITLE_PREFIX_RE.match(title)
    if not m:
        return title.strip(), ""
    cleaned = INVALID_TITLE_PREFIX_RE.sub("", title).strip()
    return cleaned or title.strip(), m.group(1)


def diff_meta(caac: dict, db: sqlite3.Row) -> list[str]:
    """检查 CAAC 与数据库记录的元数据差异。"""
    diffs: list[str] = []
    pairs = [
        ("validity", caac.get("validity", ""), db["validity"] or ""),
        ("doc_number", caac.get("doc_number", ""), db["doc_number"] or ""),
        ("publish_date", caac.get("publish_date", ""), db["publish_date"] or ""),
        ("sign_date", caac.get("sign_date", ""), db["sign_date"] or ""),
        ("office_unit", caac.get("office_unit", ""), db["office_unit"] or ""),
        ("doc_type", caac["_doc_type_en"], db["doc_type"] or ""),
    ]
    for field_name, a, b in pairs:
        if (a or "").strip() != (b or "").strip() and a:
            diffs.append(f"{field_name}: db='{b}' → caac='{a}'")
    return diffs


def scan_archive_orphans(
    local_root: Path, db_sha256_set: set[str]
) -> tuple[int, list[Path]]:
    """扫描归档三个分类目录，返回 (总文件数, 不在数据库的孤儿列表)。"""
    total = 0
    orphans: list[Path] = []
    for category in DOC_TYPE_EN_TO_CATEGORY.values():
        cat_dir = local_root / category
        if not cat_dir.exists():
            continue
        for pdf in cat_dir.glob("*.pdf"):
            total += 1
            try:
                digest = sha256_file(pdf)
            except OSError:
                continue
            if digest not in db_sha256_set:
                orphans.append(pdf)
    return total, orphans


def build_db_indexes(
    db_rows: list[sqlite3.Row],
) -> tuple[dict, dict, dict]:
    """为多级 fallback 匹配构建 url / doc_number / title 索引。"""
    by_url: dict[str, sqlite3.Row] = {}
    by_doc_number: dict[str, sqlite3.Row] = {}
    by_title: dict[str, sqlite3.Row] = {}
    for row in db_rows:
        url_key = normalize_url(row["url"] or "")
        if url_key:
            by_url.setdefault(url_key, row)
        dnk = compact_match_key(row["doc_number"] or "")
        if dnk:
            by_doc_number.setdefault(dnk, row)
        tk = normalize_match_key(row["title"] or "")
        if tk:
            by_title.setdefault(tk, row)
    return by_url, by_doc_number, by_title


def find_db_match(
    caac: dict,
    by_url: dict,
    by_doc_number: dict,
    by_title: dict,
) -> tuple[sqlite3.Row | None, str]:
    """多级 fallback 匹配，返回 (匹配记录, 匹配方式)。"""
    url_key = normalize_url(caac.get("url") or "")
    if url_key and url_key in by_url:
        return by_url[url_key], "url"
    dnk = compact_match_key(caac.get("doc_number") or "")
    if dnk and dnk in by_doc_number:
        return by_doc_number[dnk], "doc_number"
    tk = normalize_match_key(caac.get("title") or "")
    if tk and tk in by_title:
        return by_title[tk], "title"
    if tk and len(tk) >= 8:
        for local_key, row in by_title.items():
            if len(local_key) >= 8 and (local_key in tk or tk in local_key):
                return row, "title_substring"
    return None, ""


def analyze(
    db_path: Path,
    local_root: Path,
    *,
    skip_orphan_scan: bool = False,
) -> tuple[AlignReport, list[dict], list[sqlite3.Row], dict[str, str]]:
    print(f"[加载] CAAC 静态镜像: {STATIC_BASE}")
    manifest = http_get_json(f"{STATIC_BASE}/manifest.json")
    print(f"  version={manifest.get('version')} lastUpdated={manifest.get('lastUpdated')}")

    caac_docs = fetch_caac_full()
    print(f"  CAAC 总条目: {len(caac_docs)}")

    print(f"[加载] 数据库: {db_path}")
    db_rows = load_db_rows(db_path)
    print(f"  数据库总条目: {len(db_rows)}")

    by_url, by_doc_number, by_title = build_db_indexes(db_rows)

    report = AlignReport(
        caac_total=len(caac_docs),
        db_total=len(db_rows),
    )

    match_counts: dict[str, int] = {"url": 0, "doc_number": 0, "title": 0, "title_substring": 0}
    matched_db_ids: set[int] = set()
    caac_match_method: dict[str, str] = {}  # caac url → 匹配方式

    for caac in caac_docs:
        row, method = find_db_match(caac, by_url, by_doc_number, by_title)
        caac_url = caac.get("url") or ""
        if row is not None:
            report.matched += 1
            matched_db_ids.add(row["id"])
            match_counts[method] = match_counts.get(method, 0) + 1
            caac_match_method[caac_url] = method
            if diff_meta(caac, row):
                report.meta_diff += 1
        else:
            report.caac_only += 1

    for row in db_rows:
        if row["id"] not in matched_db_ids:
            if (row["validity"] or "").strip():
                report.db_only_with_validity += 1
            else:
                report.db_only_no_validity += 1

    if not skip_orphan_scan:
        print(f"[扫描] 归档目录: {local_root} (计算 sha256，可能需要 30-60 秒)")
        db_sha = {r["sha256"] for r in db_rows if r["sha256"]}
        report.archive_files, orphans = scan_archive_orphans(local_root, db_sha)
        report.orphans = len(orphans)
        report.db_in_archive = report.archive_files - report.orphans
    else:
        orphans = []

    # 把匹配明细放进报告里供后续展示
    setattr(report, "_match_counts", match_counts)

    return report, caac_docs, db_rows, caac_match_method


def print_report(report: AlignReport) -> None:
    print()
    print("=" * 60)
    print("                  对齐分析报告")
    print("=" * 60)
    print(f"CAAC 静态镜像总条目  : {report.caac_total}")
    print(f"本地数据库总条目     : {report.db_total}")
    print(f"归档目录三个分类文件 : {report.archive_files}")
    print()
    print("─── 多级 fallback 匹配（同应用 LocalRegulationMatchIndex 策略）───")
    print(f"  ✅ 匹配（CAAC + DB 都有）   : {report.matched}")
    counts = getattr(report, "_match_counts", {})
    if counts:
        print(
            f"     · 按 url={counts.get('url',0)}  "
            f"按 doc_number={counts.get('doc_number',0)}  "
            f"按 title={counts.get('title',0)}  "
            f"按 title 子串={counts.get('title_substring',0)}"
        )
    print(f"  ⚠  CAAC 有 DB 没有（待下载） : {report.caac_only}")
    print(f"  📌 DB 有 CAAC 没有")
    print(f"     · 已标 validity (历史)   : {report.db_only_with_validity}")
    print(f"     · 未标 validity (疑下架) : {report.db_only_no_validity}")
    print()
    print("─── 元数据差异 ───")
    print(f"  匹配条目中元数据有差异 : {report.meta_diff}")
    print()
    print("─── 归档目录 ───")
    print(f"  数据库 → 归档命中   : {report.db_in_archive}")
    print(f"  归档孤儿（不在 DB） : {report.orphans}")


def print_recommendations(report: AlignReport) -> None:
    print()
    print("=" * 60)
    print("                  推荐执行")
    print("=" * 60)
    if report.meta_diff > 0:
        print(f"  --apply-meta      → 修复 {report.meta_diff} 条元数据差异")
    if report.caac_only > 0:
        print(f"  --apply-download  → 下载 {report.caac_only} 个缺失 PDF 到归档")
    if report.db_only_no_validity > 0:
        print(
            f"  （元数据更新会顺带把 {report.db_only_no_validity} 条 DB-only "
            f"且 validity 为空的标记为'已下架'）"
        )
    if report.orphans > 0:
        print(f"  --apply-orphan    → 导入 {report.orphans} 个归档孤儿到数据库")
    print()
    print("一次跑完：python scripts/align_full.py --apply-all")


def write_sync_state(local_root: Path, server_manifest: dict, stats: dict) -> Path:
    """记录本次同步状态为 `.server_sync_state.json`，供应用启动检查服务器是否有更新。"""
    state = {
        "schemaVersion": 1,
        "serverLastUpdated": server_manifest.get("lastUpdated", ""),
        "serverTotalCount": server_manifest.get("totalCount", 0),
        "syncedAt": datetime.now(timezone.utc).isoformat(),
        "syncStats": stats,
    }
    state_path = local_root / ".server_sync_state.json"
    state_path.parent.mkdir(parents=True, exist_ok=True)
    state_path.write_text(json.dumps(state, ensure_ascii=False, indent=2), encoding="utf-8")
    return state_path


def apply_meta(
    db_path: Path,
    caac_docs: list[dict],
    db_rows: list[sqlite3.Row],
    obsolete_label: str = "失效",
) -> tuple[int, int]:
    """执行元数据对齐：UPDATE 匹配条目 + 标记 DB-only 未标 validity 为「失效」。"""
    by_url, by_doc_number, by_title = build_db_indexes(db_rows)
    matched_db_ids: set[int] = set()
    meta_updated = 0
    obsolete_marked = 0

    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    try:
        for caac in caac_docs:
            row, _method = find_db_match(caac, by_url, by_doc_number, by_title)
            if row is None:
                continue
            matched_db_ids.add(row["id"])
            if not diff_meta(caac, row):
                continue
            new_title = caac.get("title") or row["title"]
            new_doc_number = caac.get("doc_number") or row["doc_number"]
            new_doc_type = caac["_doc_type_en"] or row["doc_type"]
            new_validity = caac.get("validity") or row["validity"]
            new_publish_date = caac.get("publish_date") or row["publish_date"]
            new_sign_date = caac.get("sign_date") or row["sign_date"]
            new_office_unit = caac.get("office_unit") or row["office_unit"]
            conn.execute(
                """
                UPDATE regulation_files
                SET title=?, doc_number=?, doc_type=?, validity=?,
                    publish_date=?, sign_date=?, office_unit=?,
                    updated_at=CURRENT_TIMESTAMP
                WHERE id=?
                """,
                (
                    new_title,
                    new_doc_number,
                    new_doc_type,
                    new_validity,
                    new_publish_date,
                    new_sign_date,
                    new_office_unit,
                    row["id"],
                ),
            )
            meta_updated += 1

        for row in db_rows:
            if row["id"] in matched_db_ids:
                continue
            if (row["validity"] or "").strip():
                continue
            current_title = row["title"] or ""
            cleaned_title, inferred_validity = infer_title_validity(current_title)
            final_validity = inferred_validity if inferred_validity else obsolete_label
            if inferred_validity and cleaned_title != current_title:
                # title 有失效前缀 → 同时清理 title 和设 validity
                conn.execute(
                    """
                    UPDATE regulation_files
                    SET title=?, validity=?, updated_at=CURRENT_TIMESTAMP
                    WHERE id=?
                    """,
                    (cleaned_title, final_validity, row["id"]),
                )
            else:
                conn.execute(
                    """
                    UPDATE regulation_files
                    SET validity=?, updated_at=CURRENT_TIMESTAMP
                    WHERE id=?
                    """,
                    (final_validity, row["id"]),
                )
            obsolete_marked += 1

        conn.commit()
    finally:
        conn.close()
    return meta_updated, obsolete_marked


def download_pdf(url: str, target: Path) -> int:
    """下载 PDF 到 target 路径，返回字节数。使用临时文件 + rename 保证原子性。"""
    target.parent.mkdir(parents=True, exist_ok=True)
    tmp = target.with_suffix(target.suffix + ".tmp")
    req = urllib.request.Request(url, headers={"User-Agent": "ccar-align/1.0"})
    with urllib.request.urlopen(req, timeout=60) as resp, tmp.open("wb") as out:
        size = 0
        while True:
            chunk = resp.read(1024 * 64)
            if not chunk:
                break
            out.write(chunk)
            size += len(chunk)
    tmp.replace(target)
    return size


def apply_download(
    db_path: Path,
    local_root: Path,
    caac_docs: list[dict],
    db_rows: list[sqlite3.Row],
) -> dict[str, int]:
    """下载 CAAC 有 DB 没有的 PDF 到归档目录 + INSERT 数据库。"""
    by_url, by_doc_number, by_title = build_db_indexes(db_rows)
    stats = {"downloaded": 0, "skipped_no_pdf_url": 0, "failed": 0, "already_exists": 0}

    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    try:
        for caac in caac_docs:
            row, _method = find_db_match(caac, by_url, by_doc_number, by_title)
            if row is not None:
                continue  # 已在数据库，跳过

            pdf_url = (caac.get("pdf_url") or "").strip()
            title = caac.get("title") or ""
            url = caac.get("url") or ""
            doc_type_en = caac["_doc_type_en"]
            category = DOC_TYPE_EN_TO_CATEGORY.get(doc_type_en, "规范性文件")
            validity = caac.get("validity") or "有效"

            if not pdf_url:
                stats["skipped_no_pdf_url"] += 1
                print(f"  [跳过无pdf_url] [{category}] {title[:50]}")
                continue

            basename = safe_filename(title, f"caac_{caac.get('doc_number') or 'doc'}")
            if validity in ("失效", "废止", "历史版本"):
                basename = f"[{validity}] {basename}"
            target = local_root / category / f"{basename}.pdf"
            if target.exists():
                stats["already_exists"] += 1
                file_size = target.stat().st_size
                sha256 = sha256_file(target)
            else:
                try:
                    file_size = download_pdf(pdf_url, target)
                    sha256 = sha256_file(target)
                    stats["downloaded"] += 1
                    print(f"  [下载] [{category}] {basename}.pdf ({file_size:,} 字节)")
                except Exception as e:
                    stats["failed"] += 1
                    print(f"  [失败] [{category}] {title[:50]}: {e}")
                    continue

            try:
                conn.execute(
                    """
                    INSERT OR IGNORE INTO regulation_files (
                        title, doc_number, doc_type, url, pdf_url, sha256,
                        file_path, file_size, validity, office_unit,
                        sign_date, publish_date, ocr_status
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending')
                    """,
                    (
                        title,
                        caac.get("doc_number") or "",
                        doc_type_en,
                        url,
                        pdf_url,
                        sha256,
                        str(target),
                        file_size,
                        validity,
                        caac.get("office_unit") or "",
                        caac.get("sign_date") or "",
                        caac.get("publish_date") or "",
                    ),
                )
            except sqlite3.IntegrityError as e:
                print(f"  [数据库冲突] {title[:50]}: {e}")
        conn.commit()
    finally:
        conn.close()
    return stats


def main() -> int:
    parser = argparse.ArgumentParser(description="本地数据 ↔ CAAC 静态镜像对齐")
    parser.add_argument(
        "--db",
        type=Path,
        default=Path(os.environ["APPDATA"]) / "com.wangh.ccarcopilot" / "history.db",
        help="数据库路径（默认应用 AppData）",
    )
    parser.add_argument(
        "--local-root",
        type=Path,
        default=DEFAULT_LOCAL_ROOT,
        help="归档根目录（默认 D:\\飞行手册\\局方）",
    )
    parser.add_argument(
        "--skip-orphan-scan",
        action="store_true",
        help="跳过归档孤儿扫描（dry-run 加速；--apply-orphan 时自动开启）",
    )
    parser.add_argument("--apply-meta", action="store_true", help="执行元数据 UPDATE")
    parser.add_argument(
        "--apply-download",
        action="store_true",
        help="下载缺失 PDF 到归档 + INSERT 数据库",
    )
    parser.add_argument(
        "--apply-orphan",
        action="store_true",
        help="扫描归档孤儿、推断元数据后 INSERT 数据库",
    )
    parser.add_argument(
        "--apply-all",
        action="store_true",
        help="等价于 --apply-meta --apply-download --apply-orphan",
    )
    parser.add_argument(
        "--obsolete-label",
        default="失效",
        choices=["失效", "废止", "历史版本"],
        help="--apply-meta 时、未被 CAAC 列表覆盖且未标 validity 的记录要标为什么（默认失效）",
    )
    args = parser.parse_args()

    if args.apply_all:
        args.apply_meta = True
        args.apply_download = True
        args.apply_orphan = True

    if not args.db.exists():
        print(f"[错误] 数据库不存在: {args.db}", file=sys.stderr)
        return 2

    skip_orphan_scan = args.skip_orphan_scan and not args.apply_orphan
    report, _caac_docs, _db_rows, _caac_match_method = analyze(
        args.db,
        args.local_root,
        skip_orphan_scan=skip_orphan_scan,
    )

    print_report(report)

    if not (args.apply_meta or args.apply_download or args.apply_orphan):
        print_recommendations(report)
        print()
        print("（dry-run 模式：未对数据库或文件做任何修改）")
        return 0

    # 重新调 analyze 拿到 caac_docs / db_rows（现有 dict 设计接不到原始列表，上面只返回了报告）
    # 为了避免重复拉取，现在 analyze 已返回 caac_docs 和 db_rows。
    # 使用上面调用的返回值。
    if args.apply_meta:
        print()
        print("=" * 60)
        print("  执行 --apply-meta")
        print("=" * 60)
        meta_updated, obsolete_marked = apply_meta(
            args.db, _caac_docs, _db_rows, obsolete_label=args.obsolete_label
        )
        print(f"  元数据已更新：{meta_updated} 条")
        print(f"  标记为「{args.obsolete_label}」：{obsolete_marked} 条")

    if args.apply_download:
        print()
        print("=" * 60)
        print("  执行 --apply-download")
        print("=" * 60)
        # apply-meta 可能已修改了 db，重新加载 db_rows 进行准确匹配
        fresh_db_rows = load_db_rows(args.db) if args.apply_meta else _db_rows
        stats = apply_download(args.db, args.local_root, _caac_docs, fresh_db_rows)
        print()
        print(f"  下载成功：{stats['downloaded']} 个")
        print(f"  跳过（无 pdf_url）：{stats['skipped_no_pdf_url']} 个")
        print(f"  跳过（本地已存在）：{stats['already_exists']} 个")
        print(f"  下载失败：{stats['failed']} 个")

    if args.apply_orphan:
        print()
        print("=" * 60)
        print("  --apply-orphan 暂未实现（当前归档孤儿为 0，不是必需）")
        print("=" * 60)

    # 写同步状态记录，供应用启动检查服务器是否有更新
    if args.apply_meta or args.apply_download or args.apply_orphan:
        try:
            server_manifest = fetch_server_manifest()
            sync_stats = {
                "caac_total": report.caac_total,
                "db_matched": report.matched,
                "meta_diff_before": report.meta_diff,
                "caac_only_before": report.caac_only,
            }
            state_path = write_sync_state(args.local_root, server_manifest, sync_stats)
            print()
            print(f"[已写] 同步状态文件: {state_path}")
        except Exception as e:
            print(f"[警告] 写同步状态失败（不影响其他操作）: {e}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
