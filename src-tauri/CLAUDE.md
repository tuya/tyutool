# Tauri Bridge Conventions (src-tauri/)

## Command naming

- snake_case; both verb-first and noun-first patterns exist — match the style of nearby commands
- Add a `_cmd` suffix when a Tauri entry point would otherwise share a name with an internal helper function (`list_serial_ports_cmd`, `authorize_probe_cmd`)
- Event names: kebab-case, `feature-noun` format (`serial-debug-chunk`, `flash-progress`)

## Business logic boundary

- `lib.rs` handles parameter unpacking, state lookup, and result serialization only; all business logic is delegated to `tyutool-core`
- Commands return `Result<T, String>`; convert Rust errors with `.map_err(|e| e.to_string())`

## State management

- Session-scoped state (openable/closable) uses `Mutex<Option<T>>` (see `DebugState`, `FlashState`)
- All Tauri `State` types are registered via `.manage()` in `tauri::Builder`; no global variables
