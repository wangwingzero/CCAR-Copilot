#!/usr/bin/env python3
"""Align regulation PDFs into the three official local folders.

The desktop database may keep PDFs under AppData with hash-like filenames. This
script copies every database PDF into the user-facing library folder:

* CCAR规章
* 规范性文件
* 标准规范

It writes a manifest that the server query service can use to map a document id
to the mirrored server-side PDF path.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import sqlite3
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable


DEFAULT_LOCAL_ROOT = Path(r"D:\飞行手册\局方")
# 完整包含应用数据库里出现过的所有 doc_type；AC/IB/MD/AP 细分类都归入「规范性文件」。
CATEGORY_BY_DOC_TYPE = {
    "regulation": "CCAR规章",
    "standard": "标准规范",
    "normative": "规范性文件",
    "advisory_circular": "规范性文件",
    "administrative_procedure": "规范性文件",
    "information_bulletin": "规范性文件",
    "management_document": "规范性文件",
}
# 应用识别的「失效」同义 validity 值
INVALID_VALIDITY_LABELS = ("失效", "废止", "历史版本")
INVALID_FILENAME_CHARS = re.compile(r'[<>:"/\\|?*\x00-\x1f]')
MAX_BASENAME_CHARS = 150


@dataclass
class PdfManifestEntry:
    sourceFileId: int
    title: str
    docNumber: str
    docType: str
    category: str
    sha256: str
    fileSize: int
    localPath: str
    serverRelativePath: str


def default_app_data() -> Path:
    return Path(os.environ["APPDATA"]) / "com.wangh.ccarcopilot"


def category_for(doc_type: str) -> str:
    return CATEGORY_BY_DOC_TYPE.get(doc_type, "规范性文件")


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


def iter_existing_files(category_dirs: Iterable[Path]) -> dict[tuple[str, str], Path]:
    by_category_hash: dict[tuple[str, str], Path] = {}
    for category_dir in category_dirs:
        category = category_dir.name
        if not category_dir.exists():
            continue
        for path in category_dir.rglob("*.pdf"):
            try:
                digest = sha256_file(path)
            except OSError:
                continue
            by_category_hash.setdefault((category, digest), path)
    return by_category_hash


def unique_target_path(target_dir: Path, basename: str, source_file_id: int, sha256: str) -> Path:
    candidate = target_dir / f"{basename}.pdf"
    if not candidate.exists():
        return candidate
    try:
        if sha256_file(candidate) == sha256:
            return candidate
    except OSError:
        pass

    candidate = target_dir / f"{basename}_{source_file_id}.pdf"
    if not candidate.exists():
        return candidate

    suffix = 2
    while True:
        numbered = target_dir / f"{basename}_{source_file_id}_{suffix}.pdf"
        if not numbered.exists():
            return numbered
        suffix += 1


def load_rows(db_path: Path) -> list[sqlite3.Row]:
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    try:
        return list(
            conn.execute(
                """
                SELECT id, title, doc_number, doc_type, validity,
                       sha256, file_path, file_size
                FROM regulation_files
                ORDER BY id ASC
                """
            )
        )
    finally:
        conn.close()


def compose_basename(row: sqlite3.Row) -> str:
    """生成归档文件名 basename（不含 .pdf 后缀）：失效文件会加 [失效] 前缀。"""
    base = safe_filename(row["title"], f"regulation_{row['id']}")
    validity = (row["validity"] or "").strip()
    if validity in INVALID_VALIDITY_LABELS:
        base = f"[{validity}] {base}"
    return base


def align_library(app_data: Path, local_root: Path) -> dict[str, object]:
    db_path = app_data / "history.db"
    rows = load_rows(db_path)

    category_dirs = [local_root / "CCAR规章", local_root / "规范性文件", local_root / "标准规范"]
    for directory in category_dirs:
        directory.mkdir(parents=True, exist_ok=True)

    existing = iter_existing_files(category_dirs)
    entries: list[PdfManifestEntry] = []
    copied = 0
    reused = 0
    missing_sources: list[dict[str, object]] = []

    for row in rows:
        source = Path(row["file_path"])
        if not source.exists():
            missing_sources.append(
                {"sourceFileId": row["id"], "title": row["title"], "filePath": row["file_path"]}
            )
            continue

        sha256 = row["sha256"] or sha256_file(source)
        category = category_for(row["doc_type"])
        target_dir = local_root / category
        existing_target = existing.get((category, sha256))

        desired_basename = compose_basename(row)
        if existing_target is not None and existing_target.exists():
            # 如果现有文件名与 desired_basename 不同（例如 validity 变为失效需要加前缀），则 rename
            desired_target = target_dir / f"{desired_basename}.pdf"
            if existing_target != desired_target and not desired_target.exists():
                existing_target.rename(desired_target)
                existing[(category, sha256)] = desired_target
                target = desired_target
            else:
                target = existing_target
            reused += 1
        else:
            target = unique_target_path(target_dir, desired_basename, row["id"], sha256)
            if not target.exists() or target.stat().st_size != source.stat().st_size:
                shutil.copy2(source, target)
                copied += 1
            existing[(category, sha256)] = target

        relative = target.relative_to(local_root).as_posix()
        entries.append(
            PdfManifestEntry(
                sourceFileId=row["id"],
                title=row["title"] or "",
                docNumber=row["doc_number"] or "",
                docType=row["doc_type"] or "",
                category=category,
                sha256=sha256,
                fileSize=int(row["file_size"] or target.stat().st_size),
                localPath=str(target),
                serverRelativePath=relative,
            )
        )

    manifest = {
        "schemaVersion": 1,
        "generatedAt": datetime.now(timezone.utc).isoformat(),
        "localRoot": str(local_root),
        "serverPdfRoot": "/www/wwwroot/ccar-knowledge-data/pdfs",
        "documentsTotal": len(rows),
        "entriesTotal": len(entries),
        "copied": copied,
        "reused": reused,
        "missingSources": missing_sources,
        "entries": [asdict(entry) for entry in entries],
    }
    manifest_path = local_root / ".ccar_pdf_manifest.json"
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
    return manifest


def main() -> None:
    parser = argparse.ArgumentParser(description="Align local CAAC regulation PDFs by category")
    parser.add_argument("--app-data", type=Path, default=default_app_data())
    parser.add_argument("--local-root", type=Path, default=DEFAULT_LOCAL_ROOT)
    args = parser.parse_args()

    manifest = align_library(args.app_data, args.local_root)
    print(
        json.dumps(
            {
                "localRoot": manifest["localRoot"],
                "documentsTotal": manifest["documentsTotal"],
                "entriesTotal": manifest["entriesTotal"],
                "copied": manifest["copied"],
                "reused": manifest["reused"],
                "missingSources": len(manifest["missingSources"]),
                "manifestPath": str(args.local_root / ".ccar_pdf_manifest.json"),
            },
            ensure_ascii=False,
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
