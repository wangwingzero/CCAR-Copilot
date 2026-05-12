"""对比测试：mineru 标记的文档，pdfium 是否能提取出足够文本？

目的：判断当前的 needs_ocr 判定（pdfium fallback 阈值）是否正确。
- 若 mineru 标记的 N 份文档，pdfium 都提不出文本 → 阈值正确，mineru 必要
- 若 pdfium 也能提取大量文字 → fallback 阈值过严，浪费了 mineru 配额

同时对 pdfium 标记的若干份做对照，确认 pdfium 路径稳定。

只读，不修改 DB / 文件。
"""

from __future__ import annotations

import os
import sqlite3
import sys
import time
from pathlib import Path

import pypdfium2 as pdfium  # type: ignore


def default_db() -> Path:
    if sys.platform.startswith("win"):
        return Path(os.environ["APPDATA"]) / "com.wangh.ccarcopilot" / "history.db"
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "com.wangh.ccarcopilot" / "history.db"
    return Path.home() / ".local" / "share" / "com.wangh.ccarcopilot" / "history.db"


# 与 src-tauri/src/regulation/text_extractor.rs 中 needs_ocr 阈值大致对齐：
# 默认认为整本 PDF 非空白字符 < 200 算扫描件 / 需要 OCR
NEEDS_OCR_NONSPACE_CHARS = 200


def extract_pdfium(path: Path, max_pages: int | None = None) -> dict:
    """用 pypdfium2 模拟客户端 pdfium 提取路径。"""
    t0 = time.perf_counter()
    pdf = pdfium.PdfDocument(str(path))
    n = len(pdf)
    take = min(n, max_pages) if max_pages else n
    parts: list[str] = []
    for i in range(take):
        try:
            page = pdf[i]
            tp = page.get_textpage()
            text = tp.get_text_range() or ""
            parts.append(text)
            tp.close()
            page.close()
        except Exception as e:
            parts.append(f"[ERR p{i}: {e}]")
    pdf.close()
    text = "\n".join(parts)
    nonspace = sum(1 for c in text if not c.isspace())
    t1 = time.perf_counter()
    return {
        "page_count": n,
        "extracted_pages": take,
        "total_chars": len(text),
        "nonspace_chars": nonspace,
        "needs_ocr": nonspace < NEEDS_OCR_NONSPACE_CHARS,
        "elapsed_s": round(t1 - t0, 3),
        "preview": text[:300].replace("\n", " ⏎ "),
    }


def main() -> int:
    db_path = Path(sys.argv[1]) if len(sys.argv) > 1 else default_db()
    if not db_path.exists():
        print(f"[FATAL] DB not found: {db_path}", file=sys.stderr)
        return 1

    conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    print(f"[info] DB: {db_path}\n")

    samples_per_engine = 5
    for engine in ("mineru", "pdfium", "pp_ocrv4"):
        rows = conn.execute(
            """
            SELECT id, title, file_path, file_size, page_count
            FROM regulation_files
            WHERE ocr_engine = ?
            ORDER BY RANDOM()
            LIMIT ?
            """,
            (engine, samples_per_engine),
        ).fetchall()
        if not rows:
            print(f"--- {engine}: 0 samples ---\n")
            continue

        print(f"--- engine={engine}, samples={len(rows)} ---")
        needs_ocr_count = 0
        ok_count = 0
        for fid, title, fp, fs, pc in rows:
            if not Path(fp).exists():
                print(f"  id={fid} MISSING file: {title[:40]}")
                continue
            try:
                r = extract_pdfium(Path(fp), max_pages=10)
            except Exception as e:
                print(f"  id={fid} ERROR: {e}")
                continue
            mark = "[needs_ocr]" if r["needs_ocr"] else "[pdfium_ok]"
            if r["needs_ocr"]:
                needs_ocr_count += 1
            else:
                ok_count += 1
            print(
                f"  id={fid:<5d} {mark:<13s} pages={r['extracted_pages']}/{r['page_count']:<3d} "
                f"chars={r['total_chars']:>7d} nonspace={r['nonspace_chars']:>7d} "
                f"size={fs:>10,d}B  t={r['elapsed_s']:>5.2f}s"
            )
            print(f"      title  : {title[:60]}")
            print(f"      preview: {r['preview'][:200]}")
            print()
        print(
            f"  >>> 小结 [{engine}]: pdfium 已够 {ok_count} / 仍需 OCR {needs_ocr_count}\n"
        )

    conn.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
