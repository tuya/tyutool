# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Behavioral guidelines to reduce common LLM coding mistakes. Merge with project-specific instructions as needed.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

---

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, and clarifying questions come before implementation rather than after mistakes.

---

## Project: tyutool

Firmware flash tool for Tuya-class IoT devices. Supports a desktop GUI (Tauri 2 + Vue 3) and a standalone CLI binary. **Prerequisites:** Rust (stable), Node.js 22+, pnpm 10+.

### Commands

```bash
# Frontend
pnpm install           # install JS deps
pnpm run dev:web       # frontend-only dev server (no Tauri)
pnpm run tauri:dev     # full GUI dev server with hot-reload
pnpm run build         # type-check + vite build
pnpm run test          # run frontend tests (vitest)
pnpm run test:coverage # run with coverage
pnpm run lint          # ESLint on src/
pnpm run lint:fix      # ESLint with auto-fix
pnpm run format        # Prettier format src/

# Run a single frontend test file
pnpm exec vitest run src/features/firmware-flash/hex.test.ts

# Rust (CLI only)
cargo build -p tyutool-cli --release
cargo test -p tyutool-core
cargo test -p tyutool-cli

# Full GUI build
pnpm run tauri:build
```

### Architecture

```
tyutool/
├── crates/
│   ├── tyutool-core/   # Rust library — all flash logic, chip plugins, serial utils
│   └── tyutool-cli/    # Standalone CLI binary (uses tyutool-core only)
├── src-tauri/          # Tauri 2 shell (Rust backend for the desktop GUI)
│   └── src/lib.rs      # Tauri commands bridging the WebView to tyutool-core
└── src/                # Vue 3 frontend (Vite, Pinia, Tailwind CSS v4, DaisyUI)
    ├── features/firmware-flash/  # Flash feature: UI logic, chip manifests, Tauri/WS transport
    ├── stores/          # Pinia stores (flash state, workspace persistence)
    ├── components/      # Shared Vue components
    ├── composables/     # Shared Vue composables
    ├── locales/         # i18n strings (vue-i18n)
    └── router/          # Vue Router routes
```

**`tyutool-core` is the single source of truth for flash logic** — it is shared by both the CLI and the GUI Tauri backend. Flash logic must never be duplicated into the frontend.

#### Chip plugin system (Rust)

Each supported chip is a `FlashPlugin` (`crates/tyutool-core/src/plugin.rs`). Plugins are registered in `FlashPluginRegistry` (`registry.rs`) by uppercase ID (e.g. `"BK7231N"`, `"T5"`). To add a chip: implement `FlashPlugin`, add the file under `crates/tyutool-core/src/plugins/`, and register it in `FlashPluginRegistry::new()`.

#### Chip manifest system (Frontend)

`src/features/firmware-flash/chip-manifests.ts` is the **single source of truth for per-chip UI parameters** (baud rate, flash size, erase presets, 4 KiB alignment requirement). The `rustPluginId` field maps each frontend `ChipId` to the Rust registry key. When adding a chip, update both the Rust registry and `CHIP_MANIFEST`.

#### GUI ↔ Rust bridge

The frontend calls Rust via Tauri commands defined in `src-tauri/src/lib.rs` (e.g. `flash_run`, `list_serial_ports_cmd`). Progress is streamed back as `flash-progress` events via `app.emit(...)`. In web-only dev mode (`dev:web`), `src/features/firmware-flash/ws-transport.ts` provides a WebSocket shim instead of real Tauri IPC.

#### Frontend state

All flash UI state lives in the Pinia store at `src/stores/flash.ts` (`useFlashStore`). Workspace persistence (serialized form fields) is handled by `src/stores/flash-workspace.ts` using `@tauri-apps/plugin-store`.

### Key conventions

- Frontend tests (`*.test.ts`) live alongside source files in `src/`. Run with vitest; `node` environment (no DOM).
- Rust tests live alongside source in `crates/` or in `src-tauri/src/lib.rs`.
- Pre-commit hooks (lefthook) auto-format staged `.ts`/`.vue` files with Prettier and staged `.rs` files with `cargo fmt`.
- `@` alias resolves to `src/` in both Vite and vitest configs.

---

## Conventions

### File and directory naming

- `.ts` / `.rs` files: kebab-case (`hex-format.ts`, `serial_debug.rs`)
- `.vue` files: PascalCase (`SerialDebugPage.vue`)
- Feature directories: kebab-case (`firmware-flash/`, `serial-debug/`)

### Tauri IPC contract

- Command names: snake_case; add `_cmd` suffix when a Tauri entry point shares a name with an internal function (`list_serial_ports_cmd`)
- Event names: kebab-case, `feature-noun` format (`serial-debug-chunk`, `flash-progress`)
- Frontend types manually mirror the corresponding Rust types; annotate with a comment pointing to the Rust source (see `serial-debug/types.ts`)
- Tauri APIs (`@tauri-apps/api/*`) and `@tauri-apps/plugin-store` must be dynamically imported (`await import(...)`), never top-level imported
- All Tauri-only code must be gated behind `isTauriRuntime()`; never invoke Tauri commands in web mode

### Testing

- Test files live next to their source, same name with `.test.ts` suffix; Rust uses inline `#[cfg(test)] mod tests`
- Pure logic (utility functions, type conversions) must have unit tests; Vue components and stores as needed
- Frontend tests run in the `node` environment — no DOM
