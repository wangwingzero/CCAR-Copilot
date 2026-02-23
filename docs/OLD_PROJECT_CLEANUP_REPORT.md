# 旧项目 CCAR 清理报告（`D:\screenshot\HuGeScreenshot-tauri`）

## 1. 清理范围

已从旧项目删除 CCAR 前端、Rust 后端、Python sidecar 规章服务路径。

## 2. 删除文件

- `src/components/regulation/RegulationSearchPanel.vue`
- `src/composables/useRegulationQuery.ts`
- `src/composables/useRegulationIndex.ts`
- `src/stores/regulation.ts`
- `src/types/regulation.ts`
- `src-tauri/src/database/regulation.rs`
- `src-tauri/src/regulation/*`（整个模块）
- `python/huge_sidecar/services/regulation_service.py`

## 3. 修改文件

- `src/App.vue`：移除 CCAR 菜单入口与面板挂载
- `src/stores/index.ts`：移除 `useRegulationStore` 导出
- `src/stores/sidecar.ts`：移除 regulation sidecar 调用
- `src/types/sidecar.ts`：移除 regulation 协议类型
- `src/types/index.ts`：移除 regulation 类型导出
- `src-tauri/src/lib.rs`：移除 regulation 模块注册、state 管理和命令注册
- `src-tauri/src/database/mod.rs`：移除 regulation 模块导出
- `src-tauri/src/converter/pdf.rs`：去除对 `regulation::pdf_ocr` 的依赖
- `python/huge_sidecar/__main__.py`：移除 regulation 服务注册
- `python/huge_sidecar/services/__init__.py`：移除 regulation 导出
- `package.json`：修复 JSON 结构，恢复可构建状态

## 4. 验证结果

- 代码检索：`src/`, `src-tauri/src/`, `python/` 内 `regulation|CCAR` 业务引用为 0（验证结果：`NO_MATCH_IN_OLD_PROJECT_CODE`）。
- 前端构建：`npm run build` 通过。
- Rust 构建：`cargo check` 通过。

## 5. 备注

- `python -m pytest -q` 在旧项目当前基线上仍有与翻译服务相关的既有失败（非 CCAR 改动引入）。
