"""探查本地 history.db 中各 OCR engine x status 的分布，并抽样列出文件路径。

只读，不写。用于 pdfium vs mineru 对比测试前的态势分析。
"""

from __future__ import annotations

import os
import sqlite3
import sys
from pathlib import Path


def default_db() -> Path:
    if sys.platform.startswith("win"):
        return Path(os.environ["APPDATA"]) / "com.wangh.ccarcopilot" / "history.db"
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "com.wangh.ccarcopilot" / "history.db"
    return Path.home() / ".local" / "share" / "com.wangh.ccarcopilot" / "history.db"


def main() -> int:
    db_path = Path(sys.argv[1]) if len(sys.argv) > 1 else default_db()
    if not db_path.exists():
        print(f"[FATAL] DB not found: {db_path}", file=sys.stderr)
        return 1

    print(f"[info] DB: {db_path} ({db_path.stat().st_size:,} bytes)")
    conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)

    # 1. 总体分布
    print("\n--- ocr_engine x ocr_status ---")
    rows = conn.execute(
        """
        SELECT COALESCE(ocr_engine, 'NULL') AS engine,
               ocr_status,
               COUNT(*) AS n
        FROM regulation_files
        GROUP BY ocr_engine, ocr_status
        ORDER BY engine, ocr_status
        """
    ).fetchall()
    print(f"{'engine':>10s}  {'status':>14s}  {'count':>6s}")
    for engine, status, n in rows:
        print(f"{engine:>10s}  {status:>14s}  {n:>6d}")

    # 2. 总数 + indexed 一致性
    print("\n--- indexed sanity ---")
    indexed_total = conn.execute(
        "SELECT COUNT(*) FROM regulation_files WHERE indexed = 1"
    ).fetchone()[0]
    done_total = conn.execute(
        "SELECT COUNT(*) FROM regulation_files WHERE ocr_status = 'done'"
    ).fetchone()[0]
    print(f"indexed=1   total: {indexed_total}")
    print(f"status=done total: {done_total}")

    # 3. 抽样：按 engine 各取 3 条，列出 file_path 与文本长度提示
    print("\n--- samples by engine (file_size, page_count) ---")
    for engine in ("pdfium", "pp_ocrv4", "mineru", "unknown"):
        rows = conn.execute(
            """
            SELECT id, title, file_path, file_size, page_count, ocr_status
            FROM regulation_files
            WHERE ocr_engine = ?
            ORDER BY file_size DESC
            LIMIT 3
            """,
            (engine,),
        ).fetchall()
        if not rows:
            continue
        print(f"\n  [{engine}]")
        for fid, title, fp, fs, pc, st in rows:
            exists = Path(fp).exists() if fp else False
            mark = "OK" if exists else "MISSING"
            print(f"    id={fid:<5d} {st:>10s} {fs:>10,d}B p={pc:<3d} [{mark}] {title[:50]}")
            print(f"      -> {fp}")

    conn.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
