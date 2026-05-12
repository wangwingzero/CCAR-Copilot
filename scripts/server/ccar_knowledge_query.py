#!/usr/bin/env python3
"""CLI search helper for the CCAR regulation knowledge SQLite snapshot."""

from __future__ import annotations

import argparse
import json
import sqlite3
from pathlib import Path
from typing import Any


DEFAULT_DB_PATH = "/www/wwwroot/ccar-knowledge-data/current/regulation_knowledge.db"
DEFAULT_PDF_ROOT = "/www/wwwroot/ccar-knowledge-data/pdfs"
DEFAULT_PDF_MANIFEST_PATH = "/www/wwwroot/ccar-knowledge-data/pdf_manifest.json"


def fts_query_literal(query: str) -> str:
    return '"' + query.replace('"', '""') + '"'


def row_to_dict(row: sqlite3.Row) -> dict[str, Any]:
    return {key: row[key] for key in row.keys()}


def load_pdf_manifest(path: str) -> dict[int, str]:
    manifest_path = Path(path)
    if not manifest_path.exists():
        return {}
    payload = json.loads(manifest_path.read_text(encoding="utf-8"))
    result: dict[int, str] = {}
    for entry in payload.get("entries", []):
        try:
            result[int(entry["sourceFileId"])] = str(Path(DEFAULT_PDF_ROOT) / entry["serverRelativePath"])
        except (KeyError, TypeError, ValueError):
            continue
    return result


def search(db_path: str, query: str, limit: int, doc_type: str, validity: str) -> dict[str, Any]:
    if not Path(db_path).exists():
        raise FileNotFoundError(f"knowledge database not found: {db_path}")

    filters = []
    args: list[Any] = []
    if doc_type and doc_type != "all":
        filters.append("d.doc_type = ?")
        args.append(doc_type)
    if validity and validity != "all":
        filters.append("d.validity = ?")
        args.append(validity)
    where_suffix = (" AND " + " AND ".join(filters)) if filters else ""

    conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    try:
        try:
            rows = conn.execute(
                f"""
                SELECT d.id AS document_id, d.title, d.doc_number, d.doc_type, d.validity,
                       d.office_unit, d.publish_date, d.file_path,
                       c.id AS chunk_id, c.chunk_index, c.token_estimate,
                       substr(c.text, 1, 900) AS snippet,
                       bm25(chunks_fts) AS score
                FROM chunks_fts
                JOIN chunks c ON c.id = chunks_fts.rowid
                JOIN documents d ON d.id = c.document_id
                WHERE chunks_fts MATCH ?{where_suffix}
                ORDER BY score
                LIMIT ?
                """,
                [fts_query_literal(query), *args, limit],
            ).fetchall()
            mode = "fts5"
        except sqlite3.Error:
            rows = conn.execute(
                f"""
                SELECT d.id AS document_id, d.title, d.doc_number, d.doc_type, d.validity,
                       d.office_unit, d.publish_date, d.file_path,
                       c.id AS chunk_id, c.chunk_index, c.token_estimate,
                       substr(c.text, 1, 900) AS snippet,
                       NULL AS score
                FROM chunks c
                JOIN documents d ON d.id = c.document_id
                WHERE (c.text LIKE ? OR d.title LIKE ? OR d.doc_number LIKE ?){where_suffix}
                ORDER BY d.publish_date DESC, d.id DESC, c.chunk_index ASC
                LIMIT ?
                """,
                [f"%{query}%", f"%{query}%", f"%{query}%", *args, limit],
            ).fetchall()
            mode = "like"
    finally:
        conn.close()

    pdf_map = load_pdf_manifest(DEFAULT_PDF_MANIFEST_PATH)
    results = []
    for row in rows:
        item = row_to_dict(row)
        pdf_path = pdf_map.get(int(item["source_file_id"]))
        if pdf_path:
            item["serverPdfPath"] = pdf_path
            item["serverPdfExists"] = Path(pdf_path).exists()
        results.append(item)
    return {"query": query, "mode": mode, "count": len(results), "results": results}


def main() -> None:
    parser = argparse.ArgumentParser(description="Search CCAR regulation knowledge database")
    parser.add_argument("query")
    parser.add_argument("--db", default=DEFAULT_DB_PATH)
    parser.add_argument("--limit", type=int, default=10)
    parser.add_argument("--doc-type", default="all")
    parser.add_argument("--validity", default="all")
    args = parser.parse_args()

    payload = search(args.db, args.query, max(1, min(args.limit, 50)), args.doc_type, args.validity)
    print(json.dumps(payload, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
