# CCAR-Copilot 服务器端架构与 openclaw 集成指南

> 状态：草案 v1（2026-05）。本文档随客户端 OCR 引擎记账落地后整理。

## 1. 总体目标

CCAR-Copilot 桌面端负责本地 PDF 抓取、OCR、Tantivy 索引和搜索；服务器端则承担：

- 多客户端的元数据汇总
- 文档切片 + 向量化（pgvector）
- 给 [openclaw](https://github.com/wangwingzero/openclaw) 等 RAG/Agent 系统提供检索 API
- 跨客户端的 OCR 结果备份与去重

```
┌─────────────────────────────┐        ┌────────────────────────────┐
│ 客户端：CCAR-Copilot (Tauri)│        │  服务器：openclaw + pgvector │
│ ┌────────────┐ ┌──────────┐ │        │ ┌────────────────────────┐ │
│ │ regulations│ │ Tantivy  │ │ push   │ │ regulation_files (SQL) │ │
│ │ history.db │ │  index/  │ ├───────►│ │ regulation_chunks +    │ │
│ │ (SQLite)   │ │  pdfium  │ │        │ │  embedding (vector)    │ │
│ └─────┬──────┘ └──────────┘ │        │ └────────────┬───────────┘ │
│       │                     │        │              │             │
│       │ 本地搜索/OCR        │        │              ▼             │
│       │ pdfium / pp_ocrv4 / │        │     openclaw query API     │
│       │ mineru              │        │   (BM25 + vector + LLM)    │
└───────┴─────────────────────┘        └────────────────────────────┘
```

## 2. 数据模型

### 2.1 元数据表 `regulation_files`

服务器端字段与本地 SQLite 完全对齐，仅做以下 PostgreSQL 化调整（详见
[`docs/server-schema.sql`](./server-schema.sql)）：

- `id BIGSERIAL`：避免与本地 SQLite id 冲突
- `source_node TEXT`：标识上传客户端，主键改为 `(source_node, url)` 复合唯一
- `created_at / updated_at`：`TIMESTAMPTZ` + 触发器自动维护
- `indexed BOOLEAN`：替代 SQLite 的 0/1 整数

### 2.2 `ocr_engine` 字段（关键）

每条记录显式标注 OCR 来源：

| 值          | 含义                                                                     |
| ----------- | ------------------------------------------------------------------------ |
| `pdfium`    | PDF 自带文字层直接提取（无 OCR）                                         |
| `pp_ocrv4`  | 本地 PaddleOCR PP-OCRv4                                                  |
| `mineru`    | MinerU 在线 OCR（推荐，结构化 Markdown）                                 |
| `unknown`   | 无法判定（历史数据回填后剩余）                                           |

服务端保留这一字段后，可以：

- **质量分级**：`mineru` 的 chunks 排序权重更高
- **重做策略**：服务端选择性触发某 source_node 的 `non_mineru` 重做
- **统计仪表盘**：见视图 `regulation_ocr_engine_distribution`

### 2.3 切片 + 向量表 `regulation_chunks`

客户端不存向量；服务器端把 OCR 文本切成段落或页粒度，每段独立计算向量：

```sql
embedding       VECTOR(1024),        -- 默认 BGE-M3 / qwen3-embedding 维度
content_tsv     TSVECTOR,            -- 中文需 zhparser，英文/术语回退 simple
```

向量索引：先用 `IVFFlat (lists=100)`，规模 ≥ 100 万再换 HNSW（见 schema 末尾注释）。

### 2.4 同步状态表 `regulation_sync_state`

每个 source_node 一行，记录 `last_pushed_id` + `last_pushed_at`，避免重复全量推送。

## 3. 数据流

### 3.1 本地 → 服务器（增量元数据推送）

`scripts/migrate_sqlite_to_postgres.py`：

1. 读取 Tauri `app_data_dir/history.db`
2. 从 `regulation_sync_state.last_pushed_id` 起拉取增量（id > last_pushed_id）
3. 按 `--batch-size` 批量 `INSERT ... ON CONFLICT (source_node, url) DO UPDATE`
4. 推送结束更新 `regulation_sync_state.last_pushed_id`

```powershell
# Windows 客户端首次推送
python scripts/migrate_sqlite_to_postgres.py `
    --pg-dsn "postgresql://ccar:secret@server:5432/ccar" `
    --source-node home-pc

# 干跑统计
python scripts/migrate_sqlite_to_postgres.py `
    --pg-dsn "postgresql://ccar:secret@server:5432/ccar" `
    --dry-run
```

> 设计要点：`migrate_sqlite_to_postgres.py` 不上传文件本体或向量。
> PDF 同步走独立的 `align_full.py` / 服务器静态文件目录；
> 向量在服务器侧由切片管线生成，独立于客户端节奏。

### 3.2 服务器端切片管线（独立脚本）

伪代码（待落地为 `scripts/server/build_chunks.py`）：

```python
for file in regulation_files where ocr_status='done' and not_yet_chunked:
    text = read_text_from_local_pdf(file.file_path)        # 服务端有 PDF 镜像
    chunks = split_to_chunks(text, max_tokens=512)         # 按段落 + token 限制
    for i, chunk in enumerate(chunks):
        emb = embedding_model.embed(chunk)                 # 1024 维
        UPSERT regulation_chunks(file_id, chunk_index=i, content=chunk, embedding=emb)
```

切片管线完全独立于客户端，可以重跑、可以切换 embedding 模型（先 DROP COLUMN 再 ADD）。

### 3.3 openclaw 查询路径

openclaw 接到用户问题后：

1. **粗排**：`content_tsv` GIN 全文索引召回 N=200
2. **精排**：用问题向量与候选 chunks 的 `embedding` 做 cosine 取 Top-K
3. **重排（可选）**：`ocr_engine = 'mineru'` 的 chunks 加权
4. **拼装上下文**：取 chunk + 关联 `regulation_files`（标题、文号、有效性）

参考 SQL：

```sql
WITH bm25 AS (
    SELECT id, ts_rank(content_tsv, plainto_tsquery('zhparser', $1)) AS s
    FROM regulation_chunks
    WHERE content_tsv @@ plainto_tsquery('zhparser', $1)
    ORDER BY s DESC LIMIT 200
)
SELECT c.file_id, c.chunk_index, c.content,
       1 - (c.embedding <=> $2::vector) AS sim,
       f.title, f.doc_number, f.validity, f.ocr_engine
FROM regulation_chunks c
JOIN bm25 b ON b.id = c.id
JOIN regulation_files f ON f.id = c.file_id
ORDER BY sim DESC + (CASE WHEN f.ocr_engine='mineru' THEN 0.05 ELSE 0 END)
LIMIT 8;
```

## 4. 部署清单

### 4.1 数据库

```bash
# 1. 安装 PostgreSQL 15 + pgvector + pg_trgm
sudo apt install postgresql-15 postgresql-15-pgvector

# 2. 建库 / 角色
sudo -u postgres psql <<EOF
CREATE ROLE ccar LOGIN PASSWORD 'replace-me';
CREATE DATABASE ccar OWNER ccar;
EOF

# 3. 应用 schema
psql -h localhost -U ccar -d ccar -f docs/server-schema.sql
```

### 4.2 中文分词（可选）

`content_tsv` 默认占位，最后接入 [zhparser](https://github.com/amutu/zhparser)：

```sql
CREATE EXTENSION zhparser;
CREATE TEXT SEARCH CONFIGURATION zhparser (PARSER = zhparser);
ALTER TEXT SEARCH CONFIGURATION zhparser
    ADD MAPPING FOR n,v,a,i,e,l WITH simple;

-- 在切片管线写入时：
UPDATE regulation_chunks
SET content_tsv = to_tsvector('zhparser', content)
WHERE content_tsv IS NULL;
```

### 4.3 备份策略

- `regulation_files`：每日 `pg_dump --table=regulation_files`，保留 14 天
- `regulation_chunks`：体积大，建议 weekly + WAL 归档；切片可重跑，备份优先级低于 files
- 客户端 `history.db`：用户机器自行备份；服务端落地后丢失也能从客户端重新推

## 5. openclaw 接入步骤

1. 在 openclaw 项目中配置 PostgreSQL DSN（指向本 schema 的 `ccar` 数据库）
2. 用 SQL 视图或 SQLAlchemy 模型映射 `regulation_files` + `regulation_chunks`
3. 检索层：复用上面 §3.3 的混合查询
4. 展示层：用 `regulation_files.url` / `pdf_url` 渲染来源链接，
   `validity` 标注规章是否失效

## 6. 安全

- PostgreSQL **不暴露公网**；openclaw 与 DB 同机或经 SSH 隧道 / 内网
- `regulation_files.file_path` 是客户端路径，**不要**直接拼接给前端；
  服务端用自己的 `pdf_root` 重新映射
- `migrate_sqlite_to_postgres.py` 走 TLS（PostgreSQL `sslmode=require`）

## 7. 路线图

- [ ] `scripts/server/build_chunks.py`：切片 + embedding 管线
- [ ] `scripts/server/sync_pdf.py`：PDF 文件本体同步（rsync 替代）
- [ ] openclaw 适配器：把现成 BM25 + vector 检索封装为 MCP / HTTP 工具
- [ ] 监控：`regulation_ocr_engine_distribution` 视图接入 Grafana
- [ ] 增量切片：根据 `updated_at` 自动重切片，避免全量重跑

## 8. 参考

- 客户端 schema：[`src-tauri/src/database/regulation.rs`](../src-tauri/src/database/regulation.rs)
- 服务端 schema：[`docs/server-schema.sql`](./server-schema.sql)
- 同步脚本：[`scripts/migrate_sqlite_to_postgres.py`](../scripts/migrate_sqlite_to_postgres.py)
- pgvector 文档：<https://github.com/pgvector/pgvector>
- zhparser：<https://github.com/amutu/zhparser>
