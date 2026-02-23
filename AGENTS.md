# Repository Guidelines

## Project Structure & Module Organization
- `src/`: Vue 3 + TypeScript frontend (views, components, composables, stores, services, locales).
- `src-tauri/`: Rust backend for Tauri commands, window/screenshot/hotkey/ocr modules, and desktop integration.
- `src-tauri/file-search-service/`: standalone Rust sidecar service.
- `public/`: static web assets bundled by Vite.
- `resources/`: app resources/icons used by desktop packaging.
- `test-fixtures/`: shared fixture payloads, sample images, and test data.
- `docs/`: project documentation and migration/troubleshooting notes.

## Build, Test, and Development Commands
- `npm run dev`: start frontend dev server only (Vite).
- `npm run tauri:dev`: run full desktop app with frontend + Rust backend.
- `npm run build`: type-check (`vue-tsc`) and build frontend bundle.
- `npm run test`: run Vitest in watch mode.
- `npm run test:run`: run Vitest once (CI-style).
- `npm run test:coverage`: run tests with coverage output.
- `cd src-tauri && cargo test`: run Rust unit/integration tests.

## Coding Style & Naming Conventions
- Formatting is enforced by `.editorconfig` + Prettier:
  - TypeScript/Vue/JS: 2 spaces
  - Rust/Python: 4 spaces
  - LF line endings, UTF-8, final newline
- Prettier rules: single quotes, no semicolons, trailing commas (`es5`), print width 100.
- Vue SFC block order: `script`, then `template`, then `style`.
- Naming:
  - Vue components: `PascalCase.vue` (example: `SettingsPanel.vue`)
  - composables: `useXxx.ts` (example: `useOcr.ts`)
  - stores: domain-based camelCase files (example: `fileSearch.ts`)
  - Rust modules/files: `snake_case`.

## Testing Guidelines
- Frontend tests use Vitest + Vue Test Utils; property tests use `*.property.spec.ts` naming.
- Keep tests close to feature code (for example `src/components/**/__tests__/`).
- Use `test-fixtures/` for deterministic inputs; avoid hardcoded inline payloads for complex cases.
- Run both `npm run test:run` and `cargo test` before opening a PR.

## Commit & Pull Request Guidelines
- This branch currently has no commit history; use Conventional Commits going forward (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`).
- Keep commits scoped and atomic; separate frontend and Rust refactors when possible.
- PRs should include:
  - clear summary and rationale
  - linked issue/task ID
  - test evidence (commands + results)
  - screenshots/GIFs for UI changes.
