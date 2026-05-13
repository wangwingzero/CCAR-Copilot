# Repository Guidelines

## Project Structure & Module Organization
`src/` contains the Vue 3 frontend: UI components in `src/components/`, shared state in `src/stores/`, composables in `src/composables/`, and shared types in `src/types/`. Desktop and native logic lives in `src-tauri/`; use `src-tauri/src/commands/` for Tauri IPC, `src-tauri/src/regulation/` for regulation indexing/search, `src-tauri/src/ocr/` for OCR, and `src-tauri/src/database/` for persistence. Static assets live under `public/` and `resources/`. Test inputs and golden data live in `test-fixtures/`. Cloudflare update delivery code is isolated in `workers/ccar-update/`.

## Build, Test, and Development Commands
Run `npm install` once at the repo root. Use `npm run dev` for the frontend only and `npm run tauri:dev` for the Windows desktop app. Build the web bundle with `npm run build`; type-check only with `npm run type-check`. Lint with `npm run lint` or auto-fix with `npm run lint:fix`. Run frontend tests with `npm run test:run` and coverage with `npm run test:coverage`. For Rust checks, use `cd src-tauri; cargo test` and `cargo test --features proptest` when touching property-tested code. For the update worker, use `cd workers/ccar-update; npm run dev` or `npm run deploy`.

## Coding Style & Naming Conventions
Follow `.editorconfig`: 2 spaces for Vue/TS/JS/JSON, 4 spaces for Rust and Python, LF endings, UTF-8. Prettier enforces single quotes, no semicolons, trailing commas (`es5`), and `printWidth: 100`. ESLint covers `src/` and warns on unused vars unless prefixed with `_`. Use PascalCase for Vue component files (`SettingsPanel.vue`), camelCase for composables/utilities (`useTheme.ts`, `sanitize.ts`), and snake_case for Rust modules (`update_cmd.rs`).

## Testing Guidelines
Vitest is the frontend test runner; place specs near the feature in `__tests__/` and prefer `*.spec.ts` or `*.property.spec.ts` for property-based cases. Reuse fixtures from `test-fixtures/` instead of embedding large payloads inline. Add or update Rust tests when changing command, OCR, indexing, or database behavior, especially in modules that already have `proptest-regressions/` coverage.

## Commit & Pull Request Guidelines
Recent history follows Conventional Commits such as `feat(regulation): ...`, `refactor(ocr): ...`, and `chore: ...`. Keep subjects imperative and scoped when useful. PRs should explain the user-facing change, list affected areas (`src/`, `src-tauri/`, worker, docs), and include screenshots or short recordings for UI updates. Link related issues, mention new config or model assets, and paste the verification commands you ran.

## Configuration Notes
This repository is Windows-first. When changing dev-server ports, update both `vite.config.ts` and `src-tauri/tauri.conf.json`; `tauri:dev` fails if those drift.
