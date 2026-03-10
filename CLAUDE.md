# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

CCAR Copilot (虎哥截图 Tauri 版) is a Windows-only desktop app for CAAC aviation regulation search, screenshot/OCR, and document processing. Built with **Tauri 2.0 (Rust backend) + Vue 3 (frontend) + Python Sidecar**.

## Common Commands

```bash
# Development (Rust + Vue hot reload together)
npm run tauri:dev

# Frontend-only dev server (port 1420)
npm run dev

# Production build (TypeScript check + Vite + Tauri)
npm run build && npm run tauri build

# Frontend tests
npm run test          # watch mode
npm run test:run      # single run
npm run test:coverage # with v8 coverage

# Rust tests
cd src-tauri && cargo test --lib
cd src-tauri && cargo test --features proptest  # property tests
cd src-tauri && cargo bench                      # criterion benchmarks
```

## Architecture

```
Vue 3 Frontend (WebView)  ──Tauri IPC──▶  Rust Backend (Tauri 2.0 + Tokio)
                                              │
                                              ▼ stdin/stdout JSON
                                         Python Sidecar (PyInstaller)
```

- **Frontend** (`src/`): Vue 3 + Pinia + TypeScript + vue-i18n. Multi-page app with 9 HTML entry points (main, overlay, pin, workbench, recording, OCR result, etc.)
- **Backend** (`src-tauri/src/`): Rust with Tokio async runtime. 50+ Tauri commands in `commands/` module. Major subsystems: screenshot engine (WGC/DXGI/GDI 3-tier fallback), OCR (OpenVINO PP-OCRv4), full-text search (Tantivy + jieba-rs), screen recording (DXGI + WASAPI), file converter (PDF/DOCX/HTML → Markdown)
- **Sidecar**: Python subprocess for translation, Anki, web scraping. Communicates via JSON over stdin/stdout with UUID-matched request-response protocol

### Key IPC Patterns

```typescript
// Simple command
const result = await invoke('command_name', { arg1: value })

// Long-running operations emit progress events
await listen('regulation:scan-progress', (event) => { ... })

// Large files passed by path, not JSON serialized
await invoke('save_screenshot_with_history_from_file', { path })
```

### Frontend Patterns

- **Stores** (`src/stores/`): Pinia composition API (`defineStore` with `setup()` syntax)
- **Composables** (`src/composables/`): `useXxx()` pattern wrapping store + logic
- **Types** (`src/types/`): Centralized TypeScript definitions
- **Components** organized by feature: `regulation/`, `ocr/`, `anki/`, `settings/`, `recording/`, `converter/`, etc.

### Backend Patterns

- Error type: `HuGeError` / `HuGeResult<T>` (defined in `error.rs`)
- Global state: `state.rs` with `AppState` struct managed by Tauri
- Commands: `#[tauri::command]` functions in `commands/*.rs`, registered in `lib.rs`
- SQLite via r2d2 connection pool; Tantivy for full-text search with jieba Chinese tokenization
- OCR: OpenVINO with mixed precision (INT8 detection + FP16 recognition)

## Code Style

### TypeScript/Vue
- Prettier: single quotes, no semicolons, 2-space indent, 100 char width, LF line endings
- ESLint: Vue `block-order` enforced as `script → template → style`
- `no-console` warned (except `warn`/`error`)
- Path alias: `@/` → `src/`

### Rust
- `rustfmt`: 100 char width, 4-space indent, Unix line endings, crate-level import merging
- `clippy`: `unwrap`/`expect` allowed in tests only (clippy.toml)
- Comments in Chinese

### Commit Messages
Chinese, format: `<类型>: <描述>` (e.g., `feat: 添加马赛克工具`, `fix: 修复高 DPI 坐标计算问题`)

## High DPI Handling

Principle: "逻辑坐标负责交互，物理像素负责输出" — frontend uses logical pixels, Rust `CaptureResult` includes `dpr` for physical conversion. Never manually multiply by DPR in frontend code.

## Testing

- **Frontend**: Vitest + jsdom + Vue Test Utils. Property-based tests use `fast-check`. Files: `src/**/*.{test,spec}.{js,ts}`
- **Rust**: Inline `#[cfg(test)]` modules. Property tests behind `--features proptest`. Benchmarks: `criterion` (annotation_bench, screenshot_bench, sidecar_bench)

## Key Files

- `src-tauri/src/lib.rs` — Tauri app setup, all command registrations
- `src-tauri/src/commands/mod.rs` — Command module declarations
- `src-tauri/src/error.rs` — Unified error types
- `src-tauri/src/state.rs` — Global app state
- `vite.config.ts` — Multi-page entry points configuration
- `docs/API.md` — Tauri command API reference
- `docs/CCAR_MIGRATION_GUIDE.md` — Data directory structure and command inventory
