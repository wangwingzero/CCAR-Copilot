# Repository Guidelines

## Project Structure & Module Organization

- `src/`: Vue 3 + TypeScript frontend views, components, composables, stores, services, and locales.
- `src-tauri/`: Tauri 2 Rust backend for commands, desktop integration, OCR, hotkeys, screenshots, and window logic.
- `src-tauri/file-search-service/`: standalone Rust sidecar service for file search.
- `public/`: static assets served by Vite.
- `resources/`: application resources and packaging icons.
- `test-fixtures/`: deterministic payloads, images, and shared test data.
- `docs/`: documentation, migration notes, and troubleshooting material.

## Build, Test, and Development Commands

- `npm run dev`: start the Vite frontend dev server only.
- `npm run tauri:dev`: run the full desktop app with frontend and Rust backend.
- `npm run build`: run `vue-tsc --noEmit` and build the frontend bundle.
- `npm run lint`: lint frontend source under `src/`; use `npm run lint:fix` for safe fixes.
- `npm run type-check`: run Vue and TypeScript type checking.
- `npm run test`: run Vitest in watch mode.
- `npm run test:run`: run Vitest once in CI style.
- `npm run test:coverage`: run Vitest with coverage output.
- `cd src-tauri && cargo test`: run Rust tests for the Tauri backend.

## Coding Style & Naming Conventions

Formatting is controlled by `.editorconfig` and Prettier. Use UTF-8, LF endings, and final newlines. TypeScript, Vue, JavaScript, JSON, TOML, CSS, and HTML use 2 spaces; Rust and Python use 4 spaces. Markdown may keep trailing spaces for line breaks.

Prettier uses single quotes, no semicolons, ES5 trailing commas, and a 100 character print width. Vue SFC blocks must be ordered `script`, `template`, `style`.

Use `PascalCase.vue` for Vue components, `useXxx.ts` for composables, camelCase domain names for stores such as `fileSearch.ts`, and `snake_case` for Rust modules and files.

## Testing Guidelines

Frontend tests use Vitest, Vue Test Utils, and `jsdom`; Vitest includes `src/**/*.{test,spec}.{js,ts}`. Place tests close to feature code, for example `src/components/**/__tests__/`. Property tests use `*.property.spec.ts`. Store complex deterministic inputs in `test-fixtures/`.

Run `npm run test:run` for frontend changes and `cd src-tauri && cargo test` for Rust changes. Use coverage when changing shared behavior.

## Commit & Pull Request Guidelines

Recent history follows Conventional Commits, including `refactor:`, `fix:`, and `chore:`. Keep commits scoped and atomic; separate frontend and Rust refactors when practical.

Pull requests should include a clear summary, rationale, linked issue or task ID, test evidence with commands and results, and screenshots or GIFs for UI changes. Mention skipped checks and why they were not run.
