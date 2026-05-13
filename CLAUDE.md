# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

CCAR Copilot is a Windows-only Tauri 2 desktop application for CAAC (中国民航局) civil aviation regulation search and synchronization. The app crawls/downloads CAAC regulations, OCRs scanned PDFs locally, builds a Tantivy full-text index with jieba Chinese segmentation, and exposes a Vue 3 search UI. Note: `README.md` and `CHANGELOG.md` still contain residual content from the original "虎哥截图" (HuGe Screenshot) project this codebase was forked from — the live product is CCAR Copilot. Trust `Cargo.toml`, `tauri.conf.json`, and `src/` over those legacy docs.

## Common Commands

Frontend / Tauri:
- `npm run tauri:dev` — full desktop app (frontend + Rust backend, hot reload)
- `npm run dev` — Vite frontend only on port 1430 (strict)
- `npm run build` — `vue-tsc --noEmit` + Vite production bundle
- `npm run lint` / `npm run lint:fix` — ESLint over `src/`
- `npm run type-check` — Vue + TS type check
- `npm run test` — Vitest watch mode
- `npm run test:run` — Vitest CI mode (single run)
- `npm run test:coverage` — Vitest with v8 coverage

Rust (run inside `src-tauri/`):
- `cargo test` — run all backend tests
- `cargo test <name>` — run a single test by name
- `cargo test --features proptest` — enable property tests (use `*.property.spec.ts` for frontend property tests via `fast-check`)
- `cargo fmt --all -- --check` — same check CI runs
- `cargo clippy` — lint

OpenVINO dev (the Rust OCR engine links against OpenVINO DLLs in `src-tauri/openvino/`):
- `pwsh -File dev-openvino.ps1` — sets `OPENVINO_DIR` + `PATH` then runs `npm run tauri dev`. Use this instead of `npm run tauri:dev` whenever you touch OCR code, otherwise the OpenVINO runtime won't load.

Release (private channel, requires `scripts/release.env.ps1` with signing key + SSH + Cloudflare creds):
- `pwsh -File scripts/release.ps1` — builds signed NSIS installer, generates `latest.json`, scp uploads to `ccar-dl.hudawang.cn`, purges Cloudflare cache.
- Flags: `-SkipBuild`, `-SkipUpload`, `-SkipPurge`.

## Architecture

Two-tier architecture; the Python sidecar described in legacy README docs is **not** in the active code path.

### Rust backend (`src-tauri/src/`)

`lib.rs::run()` is the single entry point that wires everything. Module map:

- `regulation/` — the core domain. Tantivy index (`index.rs`), jieba-based search (`search.rs`), CAAC online crawler (`online_search.rs`, `crawler.rs`), download/sync state (`sync.rs`), filename canonicalization (`filename.rs`), MinerU cloud OCR for PDFs (`mineru_ocr.rs`), local OpenVINO PDF OCR (`pdf_ocr.rs`), text extraction (`text_extractor.rs`), AI knowledge-base export (`knowledge.rs`), and all `regulation_*` Tauri commands (`commands.rs`).
- `ocr/` — native OCR engine: PP-OCRv4 detection + recognition models embedded via `models/`, OpenVINO inference (`openvino_engine.rs`), DB post-processing (`postprocessor.rs`), CTC decoding (`recognizer.rs`). `OcrEngine::warmup()` is spawned on a 3-second delay during app setup so first-use latency is hidden.
- `file_search/` — global filename search index (walkdir + bincode-cached). Cache load happens on a background thread (`init_file_search_state` in `commands/file_search_cmd.rs`) and emits an `app:ready` event; the main thread setup must stay <100ms to avoid the Windows "not responding" black-screen.
- `database/` — `settings::AppConfig` JSON config (cached in `OnceCell`, written to `app_data_dir`) and SQLite-backed regulation/history storage via `rusqlite` + `r2d2` pool.
- `commands/` — Tauri command handlers grouped by feature. Every command must be registered in the giant `tauri::generate_handler![…]` block in `lib.rs`; a missing entry there is the most common cause of "command X not found".
- `tray/`, `crash_report/`, `logging/` — system tray (close → hide-to-tray), panic-hook crash dump writer, `tracing` + `tracing-appender` rolling logs in `%APPDATA%/com.wangh.ccarcopilot/logs/` with `LOG_RETENTION_DAYS` cleanup.

State lives in Tauri-managed singletons (`app.manage(...)`): `RegulationIndexState`, `BatchDownloadState`, `FileSearchState`. Access from commands via `tauri::State<'_, T>`.

### Vue 3 frontend (`src/`)

- `App.vue` — shell, owns global `Ctrl+P` (file search) / `Ctrl+,` (settings) shortcuts and the daily auto-sync timer (`REGULATION_AUTO_SYNC_DATE_KEY`).
- `components/regulation/RegulationSearchPanel.vue` — primary search UI.
- `components/settings/` — `SettingsPanel` + `SettingsSidebar` + section/control sub-folders, opened via `provide('openSettings')`.
- `components/FileSearch/SearchDialog.vue` — `Ctrl+P` palette.
- `stores/` — Pinia stores: `regulation`, `fileSearch`, `settings`. Components must read store state through composables (e.g. `useRegulationQuery`), **not** by importing the store directly — see the "规章状态管理重构" entry in `CHANGELOG.md`.
- `composables/` — `useRegulationQuery`, `useRegulationIndex`, `useTheme`, `useLocale`, `useUpdate`, `useToast`. These are the public façade over stores + Tauri `invoke`.
- `locales/` — vue-i18n message bundles.

The Tauri ↔ Vue boundary is `@tauri-apps/api/core::invoke` for commands and `@tauri-apps/api/event::listen` for events. The Rust side emits `app:ready` once background init finishes; the frontend shows a splash screen until then with a timeout fallback.

### Update channel (`workers/ccar-update/`)

A Cloudflare Worker fronts the private updater origin. `tauri.conf.json` points the Tauri updater at `https://ccar-update.031986.xyz/latest.json` (primary) and `https://ccar-dl.hudawang.cn/latest.json` (fallback). Worker source is TypeScript with `wrangler.toml`; deploy via `npm run deploy` inside that folder.

## Conventions

Style is enforced by `.editorconfig`, Prettier, `rustfmt.toml`, `clippy.toml`, and `eslint.config.js`. Highlights worth knowing before editing:

- **Prettier**: single quotes, no semicolons, ES5 trailing commas, 100-col width. Vue SFC block order is fixed: `<script>` → `<template>` → `<style>`.
- **Indent**: 2 spaces for TS/Vue/JSON/TOML/CSS/HTML; 4 spaces for Rust and Python.
- **Rust**: `imports_granularity = "Crate"`, `group_imports = "StdExternalCrate"`, `use_small_heuristics = "Max"` — let `cargo fmt` decide layout, don't hand-format imports.
- **Naming**: `PascalCase.vue` for components, `useXxx.ts` for composables, camelCase for Pinia store filenames (`fileSearch.ts`), `snake_case` for Rust modules. The `@/` alias resolves to `src/`.
- **Comments**: Chinese is the project default for both code comments and commit messages. Conventional Commit prefixes (`feat:`/`fix:`/`refactor:`/`chore:`/`docs:`) are used in history; keep frontend and Rust changes in separate commits when practical.
- **Tests**: frontend tests live next to features under `__tests__/` and match `src/**/*.{test,spec}.{js,ts}`; deterministic fixtures go in `test-fixtures/`. `vitest.config.ts` uses `jsdom` + globals.

## Things that bite

- **Forgetting to register a new Tauri command** in `lib.rs::invoke_handler` produces a runtime "not found" error, not a compile error. Always update the handler list.
- **OCR DLLs**: `OpenVINO` and `pdfium.dll` are bundled as Tauri resources from `src-tauri/openvino/*` and `src-tauri/pdfium/`. In dev runs without `dev-openvino.ps1`, OpenVINO model loading fails; in installed builds the `lib.rs` setup hook prepends `<exe>/openvino` to `PATH` automatically.
- **Closing the main window minimizes to tray** (see `on_window_event` in `lib.rs`). Use the tray menu or `commands::tray_cmd::show_main_window` to bring it back; `--minimized` CLI arg starts hidden (used by auto-start).
- **Singletons**: `OcrEngine` and `RegulationIndex` are `OnceCell`-style globals. Don't instantiate them in tests without `serial_test::serial` — concurrent tests will race on shared state.
- **Updater pubkey** is embedded in `tauri.conf.json`. Bumping the signing keypair requires updating both the config and `scripts/release.env.ps1`.
