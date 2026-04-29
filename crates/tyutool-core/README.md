# tyutool-core

Shared flash plugin registry, serial port helpers, and job execution engine for **tyutool** (GUI and CLI).

## Overview

`tyutool-core` is the Rust library that contains all business logic for firmware flashing, erasing, and reading. Both the Tauri GUI (`src-tauri`) and the CLI (`tyutool-cli`) depend on this crate — no flash logic is duplicated elsewhere.

## Architecture

```
tyutool-core/
├── lib.rs           # Public API re-exports
├── plugin.rs        # FlashPlugin trait definition
├── registry.rs      # FlashPluginRegistry + run_job()
├── job.rs           # FlashJob, FlashMode types
├── progress.rs      # FlashProgress event types
├── error.rs         # FlashError enum
├── serial.rs        # list_serial_ports(), check_port_available()
└── plugins/
    ├── mod.rs        # Plugin module exports
    ├── bk7231n.rs    # BK7231N FlashPlugin implementation
    ├── t5.rs         # T5 FlashPlugin implementation
    ├── t2.rs         # T2 FlashPlugin implementation (BK7231N-compatible)
    └── beken/        # Shared Beken UART protocol layer
        ├── frame.rs       # TX/RX frame encoding/decoding
        ├── command.rs     # Command opcodes + payload builders/parsers
        ├── transport.rs   # IoTransport trait, Transport (send/recv/retry)
        ├── flash_table.rs # Flash MID → params lookup (44 chips)
        ├── chip.rs        # ChipSpec trait (BK7231N vs T5 differences)
        └── ops.rs         # High-level ops: shake, erase, write, crc_check, reboot
```

## Public API

```rust
// Core trait — implement this for each chip family
pub trait FlashPlugin: Send + Sync {
    fn id(&self) -> &'static str;
    fn run(&self, job: &FlashJob, cancel: &AtomicBool, progress: &dyn Fn(FlashProgress)) -> Result<(), FlashError>;
}

// Registry — manages all chip plugins
pub struct FlashPluginRegistry { ... }
pub fn default_registry() -> &'static FlashPluginRegistry;
pub fn run_job(job: &FlashJob, cancel: &AtomicBool, progress: F) -> Result<(), FlashError>;

// Serial port utilities
pub fn list_serial_ports() -> Result<Vec<SerialPortEntry>, FlashError>;
pub fn check_port_available(port: &str) -> PortCheckResult;

// Job & progress types
pub struct FlashJob { ... }       // chip_id, port, baud_rate, mode, addresses, firmware_path
pub enum FlashMode { Flash, Erase, Read, Authorize }
pub enum FlashProgress { Percent, LogLine, Phase, Done }
```

## Usage

```rust
use tyutool_core::{run_job, FlashJob, FlashMode, FlashProgress, list_serial_ports};
use std::sync::atomic::AtomicBool;

// List available serial ports
let ports = list_serial_ports()?;

// Run a flash job
let job = FlashJob {
    mode: FlashMode::Flash,
    chip_id: "BK7231N".into(),
    port: "/dev/ttyUSB0".into(),
    baud_rate: 921600,
    firmware_path: Some("firmware.bin".into()),
    // ... other fields
};
let cancel = AtomicBool::new(false);
run_job(&job, &cancel, |progress| {
    match progress {
        FlashProgress::Percent { value } => println!("{}%", value),
        FlashProgress::Done { ok, message } => println!("Done: {}", ok),
        _ => {}
    }
})?;
```

## Testing

```bash
cargo test -p tyutool-core
```

Unit tests use `MockIo` for transport-level testing. Protocol changes **must also be tested with real hardware** via the CLI — `MockIo` cannot catch frame format, timing, or bootrom behavior issues.

## Adding a New Chip

1. Create `src/plugins/your_chip.rs` implementing `FlashPlugin`
2. For Beken-family chips, implement `ChipSpec` and reuse the `beken/` protocol layer
3. Register in `FlashPluginRegistry::new()` (`registry.rs`)
4. Export from `plugins/mod.rs`

See `bk7231n.rs` and `t5.rs` as reference implementations.
