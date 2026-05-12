-- ============================================================================
-- CCAR-Copilot 服务器端 PostgreSQL Schema
-- ----------------------------------------------------------------------------
-- 用途：支撑服务器端 openclaw（CCAR Q&A）系统，复用本地 SQLite 的元数据 schema，
--       并在此基础上扩展：
--         1. 多客户端来源标识（source_node）
--         2. 文档切片表 regulation_chunks
--         3. pgvector 向量列 + 索引（语义检索）
--         4. 全文搜索（GIN + tsvector）替代 Tantivy
--
-- 依赖：
--   - PostgreSQL >= 15
--   - 扩展：pgvector >= 0.7.0
--
-- 部署：
--   psql -U ccar -d ccar -f docs/server-schema.sql
-- ============================================================================

-- 启用扩展
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS btree_gin;

-- ----------------------------------------------------------------------------
-- 1. 规章文件元数据表
-- ----------------------------------------------------------------------------
-- 与本地 SQLite `regulation_files` 字段一一对应；
-- 主键改为 BIGSERIAL；唯一键以 (source_node, url) 复合，
-- 允许多个客户端独立上传同一份规章而不冲突。
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS regulation_files (
    id              BIGSERIAL PRIMARY KEY,
    source_node     TEXT       NOT NULL DEFAULT 'local',
    title           TEXT       NOT NULL,
    doc_number      TEXT       DEFAULT '',
    doc_type        TEXT       NOT NULL DEFAULT 'regulation',
    validity        TEXT       DEFAULT '',
    office_unit     TEXT       DEFAULT '',
    sign_date       TEXT       DEFAULT '',
    publish_date    TEXT       DEFAULT '',
    url             TEXT       NOT NULL,
    pdf_url         TEXT,
    sha256          TEXT       NOT NULL,
    file_path       TEXT       NOT NULL,
    file_size       BIGINT     DEFAULT 0,
    page_count      INTEGER    DEFAULT 0,
    ocr_status      TEXT       DEFAULT 'pending',
    ocr_progress    INTEGER    DEFAULT 0,
    ocr_current_page INTEGER   DEFAULT 0,
    ocr_error       TEXT,
    ocr_engine      TEXT       NOT NULL DEFAULT 'unknown',
    indexed         BOOLEAN    DEFAULT FALSE,
    indexed_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ DEFAULT NOW(),
    updated_at      TIMESTAMPTZ DEFAULT NOW(),
    CONSTRAINT regulation_files_source_url_uniq UNIQUE (source_node, url)
);

CREATE INDEX IF NOT EXISTS idx_regulation_files_sha256
    ON regulation_files (sha256);
CREATE INDEX IF NOT EXISTS idx_regulation_files_url
    ON regulation_files (url);
CREATE INDEX IF NOT EXISTS idx_regulation_files_ocr_status
    ON regulation_files (ocr_status);
CREATE INDEX IF NOT EXISTS idx_regulation_files_ocr_engine
    ON regulation_files (ocr_engine);
CREATE INDEX IF NOT EXISTS idx_regulation_files_indexed
    ON regulation_files (indexed);
CREATE INDEX IF NOT EXISTS idx_regulation_files_doc_type_validity
    ON regulation_files (doc_type, validity);

-- updated_at 自动维护
CREATE OR REPLACE FUNCTION trg_set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS regulation_files_updated_at ON regulation_files;
CREATE TRIGGER regulation_files_updated_at
    BEFORE UPDATE ON regulation_files
    FOR EACH ROW EXECUTE FUNCTION trg_set_updated_at();

-- ----------------------------------------------------------------------------
-- 2. 文档切片 + 向量表
-- ----------------------------------------------------------------------------
-- 在客户端：Tantivy 倒排索引存储完整 OCR 文本；
-- 在服务器端：拆分为 chunks（按页或按段落），每段独立存向量，便于
-- openclaw 等 RAG 系统做检索 + 上下文召回。
--
-- 向量维度：默认 1024（兼容 BGE-M3 / qwen3-embedding-0.6B）；
-- 实际部署时根据嵌入模型在 ALTER 时调整。
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS regulation_chunks (
    id              BIGSERIAL PRIMARY KEY,
    file_id         BIGINT     NOT NULL REFERENCES regulation_files(id) ON DELETE CASCADE,
    chunk_index     INTEGER    NOT NULL,
    page_start      INTEGER,
    page_end        INTEGER,
    content         TEXT       NOT NULL,
    content_tsv     TSVECTOR,            -- 中文需配合 zhparser/scws 或在应用层切词
    embedding       VECTOR(1024),
    token_count     INTEGER,
    embedding_model TEXT,
    created_at      TIMESTAMPTZ DEFAULT NOW(),
    updated_at      TIMESTAMPTZ DEFAULT NOW(),
    CONSTRAINT regulation_chunks_file_chunk_uniq UNIQUE (file_id, chunk_index)
);

CREATE INDEX IF NOT EXISTS idx_regulation_chunks_file_id
    ON regulation_chunks (file_id);

-- 全文索引（中文需先建 zhparser 词典；此处以 simple 占位，便于英文/数字/术语回退）
CREATE INDEX IF NOT EXISTS idx_regulation_chunks_tsv
    ON regulation_chunks USING GIN (content_tsv);

-- pg_trgm 用于片段模糊匹配（高亮摘要、跨语言）
CREATE INDEX IF NOT EXISTS idx_regulation_chunks_content_trgm
    ON regulation_chunks USING GIN (content gin_trgm_ops);

-- ANN 向量索引（先用 IVFFlat；规模 > 100 万再切 HNSW）
-- lists 经验值：sqrt(N)；100 万行选 1000，10 万行选 300。
CREATE INDEX IF NOT EXISTS idx_regulation_chunks_embedding_cos
    ON regulation_chunks USING ivfflat (embedding vector_cosine_ops)
    WITH (lists = 100);

DROP TRIGGER IF EXISTS regulation_chunks_updated_at ON regulation_chunks;
CREATE TRIGGER regulation_chunks_updated_at
    BEFORE UPDATE ON regulation_chunks
    FOR EACH ROW EXECUTE FUNCTION trg_set_updated_at();

-- ----------------------------------------------------------------------------
-- 3. 同步状态表
-- ----------------------------------------------------------------------------
-- 客户端上次成功同步的检查点；防止重复全量推送。
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS regulation_sync_state (
    source_node      TEXT        PRIMARY KEY,
    last_pushed_id   BIGINT      DEFAULT 0,
    last_pushed_at   TIMESTAMPTZ,
    notes            TEXT,
    updated_at       TIMESTAMPTZ DEFAULT NOW()
);

DROP TRIGGER IF EXISTS regulation_sync_state_updated_at ON regulation_sync_state;
CREATE TRIGGER regulation_sync_state_updated_at
    BEFORE UPDATE ON regulation_sync_state
    FOR EACH ROW EXECUTE FUNCTION trg_set_updated_at();

-- ----------------------------------------------------------------------------
-- 4. 视图：done 文件 OCR 引擎分布（供 dashboard）
-- ----------------------------------------------------------------------------
CREATE OR REPLACE VIEW regulation_ocr_engine_distribution AS
SELECT
    source_node,
    ocr_engine,
    COUNT(*)            AS file_count,
    SUM(file_size)      AS total_bytes,
    SUM(page_count)     AS total_pages
FROM regulation_files
WHERE ocr_status = 'done'
GROUP BY source_node, ocr_engine;

-- ----------------------------------------------------------------------------
-- 5. 推荐索引（按需启用）
-- ----------------------------------------------------------------------------
-- HNSW（pgvector >= 0.5.0），高召回率适合大规模：
--   CREATE INDEX idx_regulation_chunks_embedding_hnsw
--       ON regulation_chunks USING hnsw (embedding vector_cosine_ops)
--       WITH (m = 16, ef_construction = 64);
--
-- 如果切换嵌入维度，先 DROP COLUMN embedding，再 ALTER ADD VECTOR(<dim>)。
-- ============================================================================
