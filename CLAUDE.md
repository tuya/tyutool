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

Firmware flash tool for Tuya-class IoT devices. Supports a desktop GUI (Tauri 2 + Vue 3) and a standalone CLI binary. **Prerequisites:** Rust (stable), Node.js 22+, pnpm 10+. Use pnpm only (`pnpm-lock.yaml`); do not `npm install`. On Windows: Rust + VS Build Tools (MSVC) for Tauri. Postinstall allowlist: `pnpm-workspace.yaml` (`esbuild`, `lefthook`).

### Commands

```bash
# Frontend
pnpm install           # install JS deps
pnpm run dev           # Vite only (no Tauri, no CLI serve)
pnpm run dev:web       # tyutool-cli serve + Vite (cross-platform Node script)
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
├── scripts/            # Build/release orchestration only (not runtime shared code)
├── vite/               # Vite plugins and Node helpers (dev toolchain, not bundled)
├── src/                # Vue 3 frontend (Vite, Pinia, Tailwind CSS v4, DaisyUI)
│   ├── app-init.ts     # Post-mount bootstrap (workspace restore, device refresh)
│   ├── runtime.ts      # isTauriRuntime(), getRuntime()
│   ├── transport/      # WebSocket client (dev:web / browser mode)
│   ├── features/       # firmware-flash, serial-debug, settings, toolbox (hub) + batch-flash-auth
│   ├── stores/         # Pinia stores + *-workspace.ts persistence
│   ├── components/     # Cross-feature Ty* components + AppShell.vue
│   ├── config/         # Static app constants (version, Tauri path hints)
│   └── router/         # Vue Router routes
└── vite.config.ts      # Vite entry (imports from `vite/`)
```

**Layer boundaries:** `scripts/` orchestrates cargo/pnpm/tauri for CI and release — do not put frontend-importable application constants there. Dev-only Vite plugins and Node helpers live under `vite/`; shared dev constants (e.g. middleware paths) stay in `src/config/`.

**`tyutool-core` is the single source of truth for flash logic** — it is shared by both the CLI and the GUI Tauri backend. Flash logic must never be duplicated into the frontend.

#### Chip plugin system (Rust)

Each supported chip is a `FlashPlugin` (`crates/tyutool-core/src/plugin.rs`). Plugins are registered in `FlashPluginRegistry` (`registry.rs`) by uppercase ID (e.g. `"BK7231N"`, `"T5AI"`). To add a chip: implement `FlashPlugin`, add the file under `crates/tyutool-core/src/plugins/`, and register it in `FlashPluginRegistry::new()`.

#### Chip manifest system (Frontend)

`src/features/firmware-flash/chip-manifests.ts` is the **single source of truth for per-chip UI parameters** (baud rate, flash size, erase presets, 4 KiB alignment requirement). The `rustPluginId` field maps each frontend `ChipId` to the Rust registry key. When adding a chip, update both the Rust registry and `CHIP_MANIFEST`.

**Auth-only chip (`AUTH_ONLY_CHIP_ID = "other"`):** a frontend-only chip option on the authorize tab for devices that only need authorization, not flashing. It has no flash plugin — its `rustPluginId` is `"OTHER"` and authorization runs via `FlashMode::Authorize`, which bypasses the chip registry entirely (`run_job` in `registry.rs`). Flash/erase/read are disabled for it in the UI.

#### GUI ↔ Rust bridge

The frontend calls Rust via Tauri commands defined in `src-tauri/src/lib.rs` (e.g. `flash_run`, `list_serial_ports_cmd`). Progress is streamed back as `flash-progress` events via `app.emit(...)`. In web-only dev mode (`dev:web`), `src/transport/ws-transport.ts` provides a WebSocket shim instead of real Tauri IPC.

#### Frontend state

All flash UI state lives in the Pinia store at `src/stores/flash.ts` (`useFlashStore`). Workspace persistence (serialized form fields) is handled by `src/stores/flash-workspace.ts` using `@tauri-apps/plugin-store`.

Other stores follow the same pattern (store + matching `*-workspace.ts`): `serial-debug.ts`, `settings.ts`, and `batch-flash-auth.ts` (state for the batch-flash-auth toolbox tool). `port-manager.ts` (`usePortManagerStore`) is the **cross-feature serial-port ownership coordinator** — any feature that opens a serial port must claim/release it through this store rather than opening ports independently.

#### Toolbox hub

`/toolbox` is a hub landing page (`features/toolbox/`) listing tools defined in `features/toolbox/tools.ts`. Each tool is its own feature directory mounted at `/toolbox/<tool-id>` (e.g. `batch-flash-auth`). Sub-tool pages render `features/toolbox/components/ToolboxBreadcrumb.vue`.

### Key conventions

- Frontend tests (`*.test.ts`) live alongside source files in `src/`. Run with vitest; `node` environment (no DOM).
- Rust tests live alongside source in `crates/` or in `src-tauri/src/lib.rs`.
- Pre-commit hooks (lefthook) auto-format staged `.ts`/`.vue` files with Prettier and staged `.rs` files with `cargo fmt`.
- `@` alias resolves to `src/` in both Vite and vitest configs.

### Logging Contract

tyutool has two independent output channels — keep them strictly separate:

```
tyutool-core
    │
    ├─► FlashEvent callback  →  user-visible (CLI terminal / GUI / WebSocket)
    └─► log::* macros        →  developer diagnostics (file; optionally stderr)
```

**User-visible → `FlashEvent` callback**

Use `FlashEvent` whenever the user needs to see the information:
- Job metadata (firmware size, port, device) → `FlashEvent::JobSummary`
- Phase transitions → `FlashEvent::Phase(FlashPhase::*)` — use typed variants, not `Other(String)`
- Progress → `FlashEvent::Percent`
- Key milestones (connected, erase complete, etc.) → `FlashEvent::Milestone(FlashMilestone::*)`
- User action required → `FlashEvent::Warning { message }`
- Final outcome → `FlashEvent::Done`

**Developer-only → `log::*` macros**

Use `log::info!` / `log::debug!` / `log::warn!` / `log::error!` for diagnostic information:
- Protocol frame contents, byte addresses, sector offsets
- Retry counts, internal state transitions
- Any detail a user cannot act on

**Decision rule:** Ask yourself: "Can the user make a decision based on this?" → `FlashEvent`. Otherwise → `log::*`.

**Prohibited:**
- Using `log::info!` for user-visible content
- Using bare string variants (`FlashPhase::Other`) for new phases — add a typed variant instead
- Displaying `AuthReadComplete` credentials as plain log text in GUI (must use secure modal)

**Routing per platform:**

| Platform | FlashEvent | log::* |
|----------|-----------|--------|
| CLI | CliReporter → stderr | `{data_dir}/tyutool/tyutool-<timestamp>.log` (`--verbose` also → stderr) |
| GUI (Tauri) | Tauri event → UI | tauri-plugin-log → file (level controlled by developer setting) |
| Web/IDE | WebSocket JSON → browser UI | CLI-side log file |

### Issue-reporting support

Logs exist partly so users can file good bug reports. Preserve these guarantees:

- **Locatable & exportable:** the GUI in-app log viewer (`read_log_tail`) and zip export
  (`export_logs_zip`) must keep working. Don't break `appLogDir`/log-dir path assumptions.
  Logs are per-session files named `tyutool-<timestamp>.log`; the active file is resolved by
  `pick_active_log` (newest `*.log` by mtime). Don't change that naming/resolution scheme
  without updating `pick_active_log` and the issue template.
- **Startup banner parity:** CLI and GUI must emit the same banner via the single shared
  helper `tyutool_core::diagnostics::log_session_banner` (name, type, version, OS, session
  id). Never re-inline a per-platform banner.
- **Bounded growth:** each session log is size-capped at 10 MB and rolls over when exceeded
  (CLI: `SessionLogWriter` → `tyutool-<ts>-N.log`; GUI: `tauri-plugin-log` `max_file_size` +
  `RotationStrategy::KeepAll`). Across sessions, `prune_log_files` trims old files at startup
  (≤100 files / ≤100 MB total). New log sinks must stay bounded too.
- **Custom-command ACL:** new Tauri commands for logs need no capability entry — register
  them only in `invoke_handler`. Don't add redundant `fs`/`dialog` permissions.
- Any change to log file locations must update `.github/ISSUE_TEMPLATE/bug_report.yml`.

### CLI Command Documentation

`docs/cli.md` is the authoritative CLI reference. **Any change to CLI commands must include a `docs/cli.md` update in the same commit or PR:**

- Adding a subcommand or flag
- Removing or renaming a subcommand or flag
- Changing a default value or behavior

PRs that modify `crates/tyutool-cli/src/main.rs` (command definitions) without updating `docs/cli.md` must not be merged.

---

## Branch Model

```
stable/master  ← production; hotfix PRs only
refactor/v3    ← main development branch (default); feature PRs merge here
<initials>/*   ← personal feature branches, based off refactor/v3
```

- **Never commit directly to `refactor/v3` or `master`.**
- Feature work: branch off `refactor/v3` as `<initials>/<description>` (e.g. `yj/batch-auth-log`), PR back to `refactor/v3`.
- Hotfix: branch off `master` as `hotfix/<description>`, PR to `master`.
- When creating a PR, the base must be `refactor/v3` (not `master`) for normal feature work.

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
- All Tauri-only code must be gated behind `isTauriRuntime()` from `src/runtime.ts`; never invoke Tauri commands in web mode

### Testing

- Test files live next to their source, same name with `.test.ts` suffix; Rust uses inline `#[cfg(test)] mod tests`
- **Before creating a test file:** run `ls` in the source file's directory to check whether a co-located `.test.ts` already exists. If it does, append to it — never create a parallel file with suffixes like `-extended`, `-v2`, etc.
- Pure logic (utility functions, type conversions) must have unit tests; Vue components and stores as needed
- Frontend tests run in the `node` environment — no DOM
