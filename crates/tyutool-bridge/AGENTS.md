# Cobuilder Bridge Conventions (crates/tyutool-bridge/)

This crate is a **fork-only** resident helper: a user's-machine-local WebSocket server plus a system-tray shell that lets a remote web client (cobuilder-web) flash devices and write authorization codes through the `tyutool-core` engine. It is **not** part of upstream tyutool and is deliberately isolated — see "Branch / release model" below.

> **Read `PROTOCOL.md` first for anything about the wire.** This file covers the crate's *structure, conventions, and load-bearing invariants* that PROTOCOL.md does not. When the two disagree, PROTOCOL.md is the source of truth for on-the-wire behaviour.

## What this crate is (and is not)

| | Yes | No |
|---|---|---|
| Ships to end users? | ✅ installer + bare binary | — |
| Runs unattended? | ✅ (`--headless`) | — |
| Exposed to the network? | localhost `127.0.0.1` only (port 18730) | never binds a public/LAN interface |
| Auth model? | Origin allowlist + per-connection token grants + native confirm dialog | no passwords, no TLS, no global "trust me" flag |
| Talks to `tyutool-core`? | ✅ (`FlashMode::Authorize`, `run_job`, `wait_after_firmware_flash`, `check_port_available`, serial-debug engine) | does **not** reimplement any flash/auth logic |
| Has a GUI? | tray status item only (`tao`/`tray-icon`/`muda`); menu bar on macOS, no Dock tile (`LSUIElement`) | no WebView, no window |

### `tyutool-bridge` vs `tyutool-serve` — do not confuse them

Both expose `tyutool-core` over a localhost WebSocket, but they are **different crates for different consumers**:

| | `tyutool-serve` | `tyutool-bridge` |
|---|---|---|
| Crate | `crates/tyutool-serve/` (a **lib**, linked into `tyutool-cli serve`) | `crates/tyutool-bridge/` (a **binary** `tyutool-bridge`) |
| Port | **9527** | **18730** |
| Consumer | the Vite dev server (`pnpm run dev:web`) | the shipped cobuilder-web product |
| Ships to users? | no — dev-only | yes |
| Origin gate? | no | **yes** (compile-time allowlist) |
| Auth / grants? | none | token grants in `grants.json` + native confirm dialog |
| Protocol | its own (older) frames | its own (B1–B7, see `PROTOCOL.md`) |
| Shared code | `tyutool-core` | `tyutool-core` |

The two ports are deliberately disjoint (`bridge.yml` notes this) so a developer's own `tyutool-cli serve` never collides with an installed Bridge. **They share no protocol** — bridge frames are independent and not backwards-compatible with serve's.

## Module layout

```
tyutool-bridge/
├── src/
│   ├── main.rs        # Binary entry: tray shell (default) or --headless; flag parsing, logging init,
│   │                  #   autostart reconcile, tray menu wiring. Owns the event loop (tao).
│   ├── lib.rs         # The server: WS handshake (Origin check), ConnContext, dispatch, arbitration,
│   │                  #   auth/grant model, FlashBackend + DebugSession traits, all wire frames.
│   │                  # ~4900 lines; the B1–B7 scope comment at the top is the map.
│   ├── autostart.rs   # OS login-item registration + reconcile (default-on, user can turn off, survives app-move).
│   ├── lang.rs        # zh / en tray-label selection from sys_locale (LaunchAgent has no LANG).
│   ├── proc.rs        # hidden_command() + attach_parent_console() — Win GUI-subsystem stdio fixups.
│   ├── status.rs      # StatsSnapshot (connections/devices) on a watch channel for the tray; startup-error diagnosis.
│   └── tray_glyph.rs  # Embedded 44×44 PNG decode → tray icon (bundle-proof, no loose file).
├── tests/             # Integration tests (run a real WS server on an ephemeral port, drive it with a client).
│   ├── common/mod.rs  # Shared test harness: spin BridgeServer, connect, send/recv frames.
│   ├── auth_jobs.rs flash_jobs.rs local_auth.rs ports_discovery.rs serial_debug.rs stats.rs ws_server.rs
│   └── build_config.rs # Guards the workspace .cargo/config.toml `+crt-static` flag for MSVC.
├── packaging/linux/99-tyutool-bridge.rules  # udev rule: keeps ModemManager/brltty off flash ports.
├── icons/             # Tray artwork for all platforms (icns/ico/png).
├── PROTOCOL.md        # ⚠ Authoritative WS protocol contract. Update it whenever a frame changes.
├── build.rs           # Windows resource (icon) embedding.
└── Cargo.toml         # [package.metadata.packager] + [.generate-rpm] — installer config is here, not in CI.
```

## Public API surface (from `src/lib.rs`)

The crate exports both a library API (consumed by `main.rs` and the tests) and the binary. The interesting boundaries:

- **`BridgeServer`** — owns the listener; `main.rs` calls it. Injection points (`PortEnumerator`, `FlashBackend`, `AuthPrompt`, `TokenStore`, `AuditSink`) are traits so tests run without hardware or a GUI.
- **Two arbiters (the heart of the safety model):**
  - `PortArbiter` — one holder per port across **tasks + serial-debug sessions** (`run_job` / `run_auth` / `serial_debug_open` share one table). Plus the **烧后交接窗口 / handoff window** (3 s reservation for the connection whose job just succeeded).
  - `ExecutionArbiter` — **one dangerous op in flight, process-wide**, including time spent waiting for the user to confirm. This is what stops a second tab from flashing while the first's confirm dialog is still up.
- **Grant model:** `Authority` + `TokenStore` (`FileTokenStore` → `grants.json`, `MemoryTokenStore` for tests) + `GrantPolicy` (`Prompt` vs `Ignore` for `--headless`). `AuthPrompt` is the native-confirm-dialog trait (`DenyPrompt` in headless/tests).
- **Redacted Debug impls** — `Redacted<'a>` / `Base64Len` + hand-written `Debug` for `WireAuth` / `AuthJobSpec` / `ClientMessage` / `Grant` render credentials as `<redacted len=N>` and firmware as `<base64 len=N>`. **This is a compile-time guarantee, not a review promise** — never replace these with derived `Debug`.

Key constants: `DEFAULT_PORT = 18730`, `PROTOCOL_VERSION = 1`, `ORIGIN_ALLOWLIST` (compile-time), `VID_ALLOWLIST = [0x1A86, 0x10C4, 0x0403]`, `CONFIRM_TIMEOUT = 60 s`, `HANDOFF_WINDOW = 3 s`, `SINK_QUEUE_CAPACITY = 256`.

## Load-bearing invariants (do not break these)

1. **`ORIGIN_ALLOWLIST` is a compile-time constant.** It ships *inside* the binary. An origin left out cannot be fixed by editing a config file on a user's machine — the only remedy is a new Bridge release **and getting every existing user to reinstall**, because their installed binary keeps answering 403 forever. Add new cobuilder-web origins *before* they ship, and prefer over-including a legit origin over discovering it after release. Comparison is **byte-for-byte only** — never relax to wildcard/suffix (`*.wgine.com` would hand flash+auth to anyone who takes over a sibling subdomain).
2. **`Origin` is a filter, not a trust root.** A native local process can forge it. The actual trust root is the user's one click in the native confirm dialog; the token is the persisted *receipt* of that click, not a capability. See PROTOCOL.md §安全模型.
3. **At most one dangerous op (`run_job` / `run_auth`) runs process-wide at a time, confirm-wait included.** Never "queue" — reject with `execution_busy`. Release happens on every terminal path (RAII `ExecutionGuard`), including reject/timeout/disconnect.
4. **A rejected/aborted dangerous op must never have held the port.** Port claim happens *after* confirmation, not before. The narrow race this leaves (a `cancel` landing between confirm-accept and port-claim) is a deliberate, documented tradeoff — see PROTOCOL.md §确认流程. Do not "fix" it by claiming the port earlier.
5. **Credentials never leak.** `uuid` / `auth_key` / full token are never logged, never in audit lines, never in `job_result.message` (even on error / `cancelled_after_write`, which carries only the port name). The redaction is enforced by hand-written `Debug` impls and by `ConfirmRequest` not carrying credentials at all. Follow this for any new field that touches a credential.
6. **`run_auth` uses `FlashMode::Authorize` (single-device path), never `run_batch_auth_slot`.** The batch path reads MAC as a table key; CoBuilder has no table, so MAC-read failures (`Failed to read MAC address`) were unreachable noise. `run_auth` also calls `wait_after_firmware_flash` *before* the slot resets the device, so a post-flash first boot is not interrupted. Both halves use the same `baud_rate` (default 115200 — the firmware console, **not** the 921600 flash baud).
7. **`--allow-unattended-writes` is a flag, never an env var.** Env vars inherit silently into processes that never meant to opt in; a flag is visible in the process table. It is inert unless `--headless` is also set.
8. **Confirm-dialog text is escaped before being handed to a markup renderer** (`&`/`<`/`>` → entities, once). `chip_id` / `port` are client-controlled; zenity renders Pango, kdialog auto-detects Qt rich text. An unescaped dialog can be rewritten by a local malicious process, defeating the whole confirm design.

## Logging & credentials

The bridge is a resident process with a **separate** log file namespace from the CLI/GUI:

- **File:** `tyutool-bridge-<timestamp>.log` in the OS log location under the bridge's own id (`tyutool_core::paths::log_dir(BRIDGE_ID)`; it was the data dir before the ids were unified). Pruned at startup (`MAX_LOG_FILES = 20`, `MAX_LOG_BYTES_TOTAL = 50 MB`) — a smaller budget than the interactive tools, since it runs continuously.
- **Banner:** uses the shared `tyutool_core::diagnostics::log_session_banner` (name/type/version/OS/session id) — never re-inline a per-binary banner.
- **`grants.json`** at `{config_dir}/com.tyutool.bridge/grants.json` (unix `0600`, atomic temp-file + rename; `migrate_legacy_config_files` moves it and `autostart.json` once from the old `tyutool-bridge` directory) **contains tokens and is a credential** — never attach it to an issue or log it. On read/parse failure the bridge starts as "no grants" and warns, it does not crash.
- **Audit channel** (`AuditSink` → log target `bridge::audit`): one line per event, format frozen — see PROTOCOL.md §审计行. Every dangerous op leaves exactly one `confirm` line (incl. `preauthorized` / `execution_busy` / `cancelled`).

The bridge's redaction discipline is stricter than the rest of the repo because it sits closest to credentials crossing the wire.

## Branch / release model (fork-only)

- **Everything bridge-related lives in `.github/workflows/bridge.yml` and this crate.** Do not move bridge steps into `ci.yml` / `release.yml` — keeping them isolated makes a future upstream merge a whole-file accept/reject with zero hunk conflicts.
- **Tag namespace is disjoint:** upstream `v*.*.*` (release.yml) vs bridge `bridge-v*` (bridge.yml). Neither triggers the other.
- **Independent version line.** `tyutool-bridge` does **not** inherit the workspace `version.workspace = true` — it has its own literal `version = "…"` in its `Cargo.toml`, because `bridge.yml` greps that line to label artifacts. See the comment at the top of the root `Cargo.toml` and `INDEPENDENT_VERSION_MEMBERS` in `scripts/lib/version-files.test.ts`.
- **Packaging config lives in `Cargo.toml`** (`[package.metadata.packager]` / `[.macos]` / `[.nsis]` / `[.deb]` + `[package.metadata.generate-rpm]`), **not** in CI. `cargo-packager` cds into this crate's dir before reading it, and `cargo-generate-rpm` resolves paths against the workspace root — the two use *different* bases, so do not copy one into the other. The long comments in `Cargo.toml` document each decision (AppImage retired, udev rule, MSVC static CRT, the autostart-uninstall leftover).
- **Signing is fork-aware:** `APPLE_CERTIFICATE` presence auto-enables macOS signing+notarization; absence ships unsigned (Gatekeeper/SmartScreen apply). Windows is not signed here. Never put signing secrets in fork secrets — see `bridge.yml` header.

## Testing

- `cargo test -p tyutool-bridge` — integration tests run a real `BridgeServer` on an ephemeral port and drive it with a WS client (`tests/common/mod.rs`). The backend, port enumerator, confirm dialog, and token store are all injected (test fakes), so **no hardware and no GUI** are required.
- `tests/build_config.rs` guards the workspace `.cargo/config.toml` `+crt-static` flag (the MSVC static-CRT guard); the Windows build job also runs it against a real MSVC target because Ubuntu can only assert the file *content*.
- Tests are the only place that exercise the redacted `Debug` impls and the arbitration invariants — keep them honest when touching those paths.

## When you change this crate

- **Any frame / error_code / handshake change → update `PROTOCOL.md` in the same PR.** It is the cobuilder-web team's integration contract; treat it the way `docs/cli.md` is treated for CLI changes.
- **Adding a cobuilder-web origin → `ORIGIN_ALLOWLIST` literal only** (never a pattern), and add it before that environment ships.
- **New dangerous op → wire it through both arbiters** (`PortArbiter` + `ExecutionArbiter`) and add an audit `confirm` line.
- **New credential-adjacent field → add a redacted `Debug` impl** and audit what `job_result.message` / logs print for it.
- **Logging changes → keep the `tyutool-bridge-*` naming and the prune budget bounded**, and never route tokens/credentials into the log.
