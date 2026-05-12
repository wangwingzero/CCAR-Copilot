#!/usr/bin/env python3
"""HTTP query service for the CCAR regulation knowledge SQLite snapshot."""

from __future__ import annotations

import json
import os
import sqlite3
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any
from urllib.parse import parse_qs, urlparse


DEFAULT_DB_PATH = "/www/wwwroot/ccar-knowledge-data/current/regulation_knowledge.db"
DEFAULT_MANIFEST_PATH = "/www/wwwroot/ccar-knowledge-data/current/manifest.json"
DEFAULT_PDF_ROOT = "/www/wwwroot/ccar-knowledge-data/pdfs"
DEFAULT_PDF_MANIFEST_PATH = "/www/wwwroot/ccar-knowledge-data/pdf_manifest.json"
DEFAULT_BIND_HOST = "127.0.0.1"
DEFAULT_BIND_PORT = 8765
MAX_LIMIT = 50
MAX_CHUNK_TEXT = 1400


def env(name: str, default: str) -> str:
    return os.environ.get(name, default).strip() or default


def connect_db() -> sqlite3.Connection:
    db_path = env("CCAR_KNOWLEDGE_DB", DEFAULT_DB_PATH)
    if not Path(db_path).exists():
        raise FileNotFoundError(f"knowledge database not found: {db_path}")
    conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    return conn


def clamp_limit(raw: str | None, default: int = 10) -> int:
    try:
        value = int(raw or default)
    except ValueError:
        value = default
    return max(1, min(MAX_LIMIT, value))


def row_to_dict(row: sqlite3.Row) -> dict[str, Any]:
    return {key: row[key] for key in row.keys()}


def fts_query_literal(query: str) -> str:
    return '"' + query.replace('"', '""') + '"'


def excerpt(text: str, query: str, limit: int = MAX_CHUNK_TEXT) -> str:
    text = " ".join(text.split())
    if len(text) <= limit:
        return text

    index = text.lower().find(query.lower())
    if index < 0:
        return text[:limit].rstrip() + "..."

    start = max(0, index - limit // 3)
    end = min(len(text), start + limit)
    prefix = "..." if start > 0 else ""
    suffix = "..." if end < len(text) else ""
    return prefix + text[start:end].strip() + suffix


def read_manifest() -> dict[str, Any]:
    manifest_path = Path(env("CCAR_KNOWLEDGE_MANIFEST", DEFAULT_MANIFEST_PATH))
    if not manifest_path.exists():
        return {}
    try:
        return json.loads(manifest_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}


def read_pdf_manifest() -> dict[int, dict[str, Any]]:
    manifest_path = Path(env("CCAR_PDF_MANIFEST", DEFAULT_PDF_MANIFEST_PATH))
    if not manifest_path.exists():
        return {}
    try:
        payload = json.loads(manifest_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}

    entries = payload.get("entries", [])
    mapping: dict[int, dict[str, Any]] = {}
    for entry in entries:
        try:
            source_file_id = int(entry["sourceFileId"])
        except (KeyError, TypeError, ValueError):
            continue
        mapping[source_file_id] = entry
    return mapping


def add_pdf_path(item: dict[str, Any], pdf_map: dict[int, dict[str, Any]] | None = None) -> None:
    source_file_id = item.get("source_file_id") or item.get("sourceFileId")
    if source_file_id is None:
        return
    try:
        entry = (pdf_map or read_pdf_manifest()).get(int(source_file_id))
    except (TypeError, ValueError):
        return
    if not entry:
        return

    relative = entry.get("serverRelativePath")
    if not relative:
        return
    server_path = str(Path(env("CCAR_PDF_ROOT", DEFAULT_PDF_ROOT)) / relative)
    item["serverPdfPath"] = server_path
    item["serverPdfExists"] = Path(server_path).exists()


def stats() -> dict[str, Any]:
    with connect_db() as conn:
        documents = conn.execute("SELECT COUNT(*) FROM documents").fetchone()[0]
        chunks = conn.execute("SELECT COUNT(*) FROM chunks").fetchone()[0]
        with_content = conn.execute(
            "SELECT COUNT(*) FROM documents WHERE chunk_count > 0"
        ).fetchone()[0]
        doc_types = [
            row_to_dict(row)
            for row in conn.execute(
                "SELECT doc_type, COUNT(*) AS count FROM documents GROUP BY doc_type ORDER BY count DESC"
            )
        ]
    pdf_map = read_pdf_manifest()
    pdf_existing = sum(
        1
        for entry in pdf_map.values()
        if Path(env("CCAR_PDF_ROOT", DEFAULT_PDF_ROOT), entry.get("serverRelativePath", "")).exists()
    )
    return {
        "database": env("CCAR_KNOWLEDGE_DB", DEFAULT_DB_PATH),
        "documents": documents,
        "documentsWithContent": with_content,
        "chunks": chunks,
        "docTypes": doc_types,
        "manifest": read_manifest(),
        "pdfRoot": env("CCAR_PDF_ROOT", DEFAULT_PDF_ROOT),
        "pdfManifestEntries": len(pdf_map),
        "pdfFilesPresent": pdf_existing,
    }


def search_chunks(params: dict[str, list[str]]) -> dict[str, Any]:
    query = (params.get("q") or [""])[0].strip()
    if not query:
        return {"query": query, "count": 0, "results": []}

    limit = clamp_limit((params.get("limit") or [None])[0])
    doc_type = (params.get("docType") or params.get("doc_type") or [""])[0].strip()
    validity = (params.get("validity") or [""])[0].strip()

    filters = []
    args: list[Any] = []
    if doc_type and doc_type != "all":
        filters.append("d.doc_type = ?")
        args.append(doc_type)
    if validity and validity != "all":
        filters.append("d.validity = ?")
        args.append(validity)
    where_suffix = (" AND " + " AND ".join(filters)) if filters else ""

    with connect_db() as conn:
        try:
            rows = conn.execute(
                f"""
                SELECT
                    d.id AS document_id,
                    d.source_file_id,
                    d.title,
                    d.doc_number,
                    d.doc_type,
                    d.validity,
                    d.office_unit,
                    d.sign_date,
                    d.publish_date,
                    d.url,
                    d.pdf_url,
                    d.file_path,
                    c.id AS chunk_id,
                    c.chunk_index,
                    c.char_count,
                    c.token_estimate,
                    c.text,
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
            like_args: list[Any] = [f"%{query}%", f"%{query}%", f"%{query}%"]
            rows = conn.execute(
                f"""
                SELECT
                    d.id AS document_id,
                    d.source_file_id,
                    d.title,
                    d.doc_number,
                    d.doc_type,
                    d.validity,
                    d.office_unit,
                    d.sign_date,
                    d.publish_date,
                    d.url,
                    d.pdf_url,
                    d.file_path,
                    c.id AS chunk_id,
                    c.chunk_index,
                    c.char_count,
                    c.token_estimate,
                    c.text,
                    NULL AS score
                FROM chunks c
                JOIN documents d ON d.id = c.document_id
                WHERE (c.text LIKE ? OR d.title LIKE ? OR d.doc_number LIKE ?){where_suffix}
                ORDER BY d.publish_date DESC, d.id DESC, c.chunk_index ASC
                LIMIT ?
                """,
                [*like_args, *args, limit],
            ).fetchall()
            mode = "like"

    pdf_map = read_pdf_manifest()
    results = []
    for row in rows:
        item = row_to_dict(row)
        item["snippet"] = excerpt(item.pop("text") or "", query)
        add_pdf_path(item, pdf_map)
        results.append(item)
    return {"query": query, "mode": mode, "count": len(results), "results": results}


def get_document(params: dict[str, list[str]]) -> dict[str, Any]:
    document_id = (params.get("id") or params.get("document_id") or [""])[0].strip()
    if not document_id:
        raise ValueError("missing id")

    include_chunks = (params.get("includeChunks") or params.get("include_chunks") or ["0"])[0]
    with connect_db() as conn:
        row = conn.execute("SELECT * FROM documents WHERE id = ?", [document_id]).fetchone()
        if row is None:
            raise KeyError(f"document not found: {document_id}")
        document = row_to_dict(row)
        if include_chunks in {"1", "true", "yes"}:
            document["chunks"] = [
                row_to_dict(chunk)
                for chunk in conn.execute(
                    """
                    SELECT id, chunk_index, char_count, token_estimate, text
                    FROM chunks
                    WHERE document_id = ?
                    ORDER BY chunk_index ASC
                    """,
                    [document_id],
                )
            ]
    add_pdf_path(document)
    return {"document": document}


def get_chunks(params: dict[str, list[str]]) -> dict[str, Any]:
    document_id = (params.get("documentId") or params.get("document_id") or [""])[0].strip()
    if not document_id:
        raise ValueError("missing documentId")

    with connect_db() as conn:
        rows = conn.execute(
            """
            SELECT id, document_id, source_file_id, chunk_index, char_count, token_estimate, text
            FROM chunks
            WHERE document_id = ?
            ORDER BY chunk_index ASC
            """,
            [document_id],
        ).fetchall()
    return {"documentId": document_id, "count": len(rows), "chunks": [row_to_dict(row) for row in rows]}


class Handler(BaseHTTPRequestHandler):
    server_version = "CCARKnowledge/1.0"

    def do_GET(self) -> None:
        parsed = urlparse(self.path)
        params = parse_qs(parsed.query)

        try:
            if parsed.path == "/health":
                payload = {"ok": True, **stats()}
            elif parsed.path == "/stats":
                payload = stats()
            elif parsed.path == "/search":
                payload = search_chunks(params)
            elif parsed.path == "/document":
                payload = get_document(params)
            elif parsed.path == "/chunks":
                payload = get_chunks(params)
            else:
                self.write_json({"error": "not found"}, HTTPStatus.NOT_FOUND)
                return
            self.write_json(payload)
        except FileNotFoundError as exc:
            self.write_json({"ok": False, "error": str(exc)}, HTTPStatus.SERVICE_UNAVAILABLE)
        except (KeyError, ValueError) as exc:
            self.write_json({"error": str(exc)}, HTTPStatus.BAD_REQUEST)
        except Exception as exc:  # noqa: BLE001
            self.write_json({"error": str(exc)}, HTTPStatus.INTERNAL_SERVER_ERROR)

    def write_json(self, payload: dict[str, Any], status: HTTPStatus = HTTPStatus.OK) -> None:
        data = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, fmt: str, *args: Any) -> None:
        if env("CCAR_KNOWLEDGE_LOG", "0") == "1":
            super().log_message(fmt, *args)


def main() -> None:
    host = env("CCAR_KNOWLEDGE_BIND", DEFAULT_BIND_HOST)
    port = int(env("CCAR_KNOWLEDGE_PORT", str(DEFAULT_BIND_PORT)))
    server = ThreadingHTTPServer((host, port), Handler)
    print(f"CCAR knowledge service listening on http://{host}:{port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
