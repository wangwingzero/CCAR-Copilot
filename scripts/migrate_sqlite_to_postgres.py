#!/usr/bin/env python3
"""把本地 SQLite history.db 中的 regulation_files 增量同步到服务器 PostgreSQL。

使用场景：
    本地 CCAR-Copilot 跑完 OCR + 索引后，一次性把元数据推送给服务器，
    供 openclaw 等 Q&A 服务做语义检索（向量、chunk 在服务器上离线生成）。

PostgreSQL schema 见 docs/server-schema.sql。

依赖（建议虚拟环境）：
    pip install psycopg[binary]>=3.1

典型用法：

    # Windows 默认 Tauri 数据目录
    python scripts/migrate_sqlite_to_postgres.py \
        --pg-dsn "postgresql://ccar:secret@1.2.3.4:5432/ccar" \
        --source-node home-pc

    # 干跑：只统计差异，不写 PostgreSQL
    python scripts/migrate_sqlite_to_postgres.py \
        --pg-dsn "postgresql://ccar:secret@1.2.3.4:5432/ccar" \
        --dry-run

    # 显式指定 sqlite 文件
    python scripts/migrate_sqlite_to_postgres.py \
        --sqlite "C:\\Users\\me\\AppData\\Roaming\\com.wangh.ccarcopilot\\history.db" \
        --pg-dsn "$env:CCAR_PG_DSN"

参数：
    --sqlite PATH           本地 history.db 路径（默认按 Tauri identifier 推断）
    --pg-dsn DSN            PostgreSQL 连接串
    --source-node NAME      标识本机来源（落库到 source_node 列），默认 hostname
    --since-id INT          只同步 id > since-id 的记录（默认从 sync_state 读）
    --batch-size INT        每批 UPSERT 行数，默认 200
    --dry-run               只打印将要执行的统计，不写 PostgreSQL
    --include-failed        是否同步 ocr_status='failed'（默认跳过）

退出码：
    0  成功；1  参数 / 连接错误；2  迁移过程中出现致命错误。
"""

from __future__ import annotations

import argparse
import os
import socket
import sqlite3
import sys
from pathlib import Path
from typing import Any, Iterable

try:
    import psycopg  # psycopg 3.x
    from psycopg import sql as pg_sql
except ImportError:  # pragma: no cover
    psycopg = None  # type: ignore
    pg_sql = None  # type: ignore


TAURI_IDENTIFIER = "com.wangh.ccarcopilot"

REGULATION_COLUMNS = [
    "title",
    "doc_number",
    "doc_type",
    "validity",
    "office_unit",
    "sign_date",
    "publish_date",
    "url",
    "pdf_url",
    "sha256",
    "file_path",
    "file_size",
    "page_count",
    "ocr_status",
    "ocr_progress",
    "ocr_current_page",
    "ocr_error",
    "ocr_engine",
    "indexed",
    "indexed_at",
    "created_at",
    "updated_at",
]


def default_sqlite_path() -> Path:
    """猜测 Windows / macOS / Linux 下 Tauri app_data_dir。"""
    if sys.platform.startswith("win"):
        roaming = os.environ.get("APPDATA")
        if not roaming:
            roaming = str(Path.home() / "AppData" / "Roaming")
        return Path(roaming) / TAURI_IDENTIFIER / "history.db"
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / TAURI_IDENTIFIER / "history.db"
    return Path.home() / ".local" / "share" / TAURI_IDENTIFIER / "history.db"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    parser.add_argument("--sqlite", type=Path, default=default_sqlite_path())
    parser.add_argument("--pg-dsn", required=True, help="PostgreSQL DSN，例如 postgresql://user:pass@host:5432/db")
    parser.add_argument("--source-node", default=socket.gethostname(), help="本机标识（落库到 source_node 列）")
    parser.add_argument("--since-id", type=int, default=None, help="只同步 id > 此值；默认从 sync_state 读取")
    parser.add_argument("--batch-size", type=int, default=200)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--include-failed", action="store_true", help="同时同步 ocr_status='failed'")
    return parser.parse_args()


def open_sqlite(path: Path) -> sqlite3.Connection:
    if not path.exists():
        raise FileNotFoundError(f"找不到 SQLite 数据库: {path}")
    conn = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    return conn


def fetch_local_rows(
    conn: sqlite3.Connection,
    since_id: int,
    include_failed: bool,
) -> Iterable[sqlite3.Row]:
    where = ["id > ?"]
    params: list[Any] = [since_id]
    if not include_failed:
        where.append("ocr_status != 'failed'")
    sql = (
        "SELECT id, "
        + ", ".join(REGULATION_COLUMNS)
        + " FROM regulation_files WHERE "
        + " AND ".join(where)
        + " ORDER BY id ASC"
    )
    return conn.execute(sql, params)


def get_last_pushed_id(pg_conn: "psycopg.Connection", source_node: str) -> int:
    with pg_conn.cursor() as cur:
        cur.execute(
            "SELECT last_pushed_id FROM regulation_sync_state WHERE source_node = %s",
            (source_node,),
        )
        row = cur.fetchone()
    return int(row[0]) if row else 0


def update_sync_state(
    pg_conn: "psycopg.Connection",
    source_node: str,
    last_pushed_id: int,
) -> None:
    with pg_conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO regulation_sync_state (source_node, last_pushed_id, last_pushed_at)
            VALUES (%s, %s, NOW())
            ON CONFLICT (source_node) DO UPDATE
              SET last_pushed_id = EXCLUDED.last_pushed_id,
                  last_pushed_at = EXCLUDED.last_pushed_at
            """,
            (source_node, last_pushed_id),
        )


def upsert_batch(
    pg_conn: "psycopg.Connection",
    source_node: str,
    rows: list[sqlite3.Row],
) -> int:
    """批量 UPSERT 到 regulation_files；返回成功行数。"""
    if not rows:
        return 0

    cols = ["source_node", *REGULATION_COLUMNS]
    placeholders = ", ".join(["%s"] * len(cols))
    update_set = ", ".join(
        f"{c} = EXCLUDED.{c}" for c in REGULATION_COLUMNS if c not in ("url",)
    )
    sql_text = f"""
        INSERT INTO regulation_files ({", ".join(cols)})
        VALUES ({placeholders})
        ON CONFLICT (source_node, url) DO UPDATE SET
            {update_set}
    """

    payload = [
        (source_node, *(_normalize_value(row[c]) for c in REGULATION_COLUMNS)) for row in rows
    ]
    with pg_conn.cursor() as cur:
        cur.executemany(sql_text, payload)
    return len(payload)


def _normalize_value(value: Any) -> Any:
    """SQLite int/bool -> PostgreSQL 兼容值。"""
    if isinstance(value, int) and not isinstance(value, bool):
        return value
    if value is None:
        return None
    return value


def main() -> int:
    args = parse_args()

    try:
        sqlite_conn = open_sqlite(args.sqlite)
    except FileNotFoundError as e:
        print(f"[FATAL] {e}", file=sys.stderr)
        return 1

    print(f"[info] SQLite : {args.sqlite}")
    print(f"[info] Source : {args.source_node}")
    print(f"[info] Dry-run: {args.dry_run}")

    # dry-run 不连 PostgreSQL，所以不强制安装 psycopg
    if not args.dry_run and psycopg is None:
        print("[FATAL] 未安装 psycopg；请先运行: pip install 'psycopg[binary]>=3.1'", file=sys.stderr)
        return 1

    if args.dry_run:
        # dry-run 不连 PostgreSQL，只统计本地数据
        since_id = args.since_id or 0
        rows = list(fetch_local_rows(sqlite_conn, since_id, args.include_failed))
        engine_count: dict[str, int] = {}
        for r in rows:
            engine_count[r["ocr_engine"]] = engine_count.get(r["ocr_engine"], 0) + 1
        print(f"[dry-run] 待同步行数: {len(rows)} (since_id={since_id})")
        for engine, n in sorted(engine_count.items(), key=lambda kv: -kv[1]):
            print(f"[dry-run]   {engine:>10s} : {n}")
        return 0

    try:
        pg_conn = psycopg.connect(args.pg_dsn, autocommit=False)
    except Exception as e:  # pragma: no cover
        print(f"[FATAL] 连接 PostgreSQL 失败: {e}", file=sys.stderr)
        return 1

    try:
        since_id = args.since_id
        if since_id is None:
            since_id = get_last_pushed_id(pg_conn, args.source_node)
            print(f"[info] 从 sync_state 读到 last_pushed_id = {since_id}")

        rows_iter = fetch_local_rows(sqlite_conn, since_id, args.include_failed)
        batch: list[sqlite3.Row] = []
        total_pushed = 0
        max_id = since_id

        for row in rows_iter:
            batch.append(row)
            max_id = max(max_id, int(row["id"]))
            if len(batch) >= args.batch_size:
                total_pushed += upsert_batch(pg_conn, args.source_node, batch)
                pg_conn.commit()
                print(f"[info] 已推送 {total_pushed} 行 (last id={max_id})")
                batch = []

        if batch:
            total_pushed += upsert_batch(pg_conn, args.source_node, batch)
            pg_conn.commit()

        update_sync_state(pg_conn, args.source_node, max_id)
        pg_conn.commit()
        print(f"[done] 总计推送 {total_pushed} 行；新 last_pushed_id = {max_id}")
        return 0
    except Exception as e:
        pg_conn.rollback()
        print(f"[FATAL] 迁移失败: {e}", file=sys.stderr)
        return 2
    finally:
        pg_conn.close()
        sqlite_conn.close()


if __name__ == "__main__":
    raise SystemExit(main())
