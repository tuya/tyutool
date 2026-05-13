# Rust Core Conventions (crates/)

## FlashPlugin system

- Each chip family implements the `FlashPlugin` trait (`crates/tyutool-core/src/plugin.rs`)
- Simple chips use a single file; complex chips use a subdirectory (`plugins/<chip>/mod.rs` + protocol layer files)
- Register in `FlashPluginRegistry::new()` in `registry.rs` using an uppercase chip ID key (`"BK7231N"`)
- Adding a new chip requires updating both: the Rust registry and `src/features/firmware-flash/chip-manifests.ts`

## Error handling

- All flash operations return `Result<T, FlashError>`
- Plugin-internal errors are wrapped in `FlashError::Plugin(String)`; do not add new variants
- Never use `unwrap()` or `panic!()`; cancellation checks read `cancel.load(Ordering::Relaxed)` and return `FlashError::Cancelled`

## Serial I/O

- Serial I/O goes through utility functions in `crates/tyutool-core/src/serial.rs`; do not use the `serialport` crate directly
- Timeout and retry logic belongs in core, not scattered across individual plugins
