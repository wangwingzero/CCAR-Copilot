# CCAR Copilot 迁移与运维说明

## 1. 数据目录

默认落盘目录：`AppData/com.wangh.ccarcopilot/`

- `history.db`：SQLite 主库（含 `regulation_files`）
- `regulation_index/`：Tantivy 本地全文索引
- `regulations/`：受管本地文件目录（`copy_then_register` 模式）

## 2. Rust 命令清单（前端 `invoke`）

- `regulation_online_search(keyword, docType?, validity?, startDate?, endDate?)`
- `regulation_fetch_all_online(docType?, maxPages?)`
- `regulation_sync_compare_online(docType?, maxPages?)`
- `regulation_download_single(request)`
- `regulation_batch_download(request)`
- `regulation_get_download_progress()`
- `regulation_scan_local_dir(dirPath, recursive?, autoOcr?, localCopyMode?, targetDir?)`
- `regulation_discover_local(localCopyMode?, targetDir?)`
- `regulation_import_legacy_data(legacyDataDir, copyFiles?, copyIndex?)`
- `regulation_ocr_pending(batchSize?)`
- `regulation_ocr_update(fileId, text, pageCount?)`
- `regulation_retry_failed_ocr(batchSize?)`
- `regulation_get_ocr_queue(limit?)`
- `regulation_local_search(request)`
- `regulation_index_init()`
- `regulation_index_add(document)`
- `regulation_index_add_batch(documents)`
- `regulation_index_stats()`
- `regulation_index_exists(url)`
- `regulation_index_clear()`

## 3. 本地导入策略

`localCopyMode` 支持两种值：

- `register_only`：只登记原路径
- `copy_then_register`：复制到受管目录再登记（默认）

默认受管目录：`AppData/com.wangh.ccarcopilot/regulations`

## 4. 旧数据导入（首次迁移）

调用 `regulation_import_legacy_data`，参数建议：

- `legacyDataDir`: 旧应用数据目录
- `copyFiles: true`（推荐）
- `copyIndex: false`（推荐，后续让新应用重建索引）

返回：

- `total_found / imported / skipped / failed`
- `copied_files / copied_index_files`

说明：

- 导入前后都会执行 `regulation_files` schema 兼容校验（缺列自动补齐）。
- 导入使用哈希 + URL 双重去重，重复记录自动跳过。

## 5. 常见问题

- 导入后搜不到：
  - 先执行 `regulation_index_init`，再触发 OCR/重建索引流程。
- 扫描导入速度慢：
  - 先关 `autoOcr` 做纯导入，再分批 `regulation_ocr_pending`。
- 全盘发现结果少：
  - 先确保文件搜索索引器已构建完成，再执行 `regulation_discover_local`。
