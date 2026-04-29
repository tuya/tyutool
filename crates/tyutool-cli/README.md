# tyutool-cli

Command-line interface for **tyutool** — firmware flashing, reading, and device management without a GUI.

## Overview

`tyutool-cli` is a standalone binary that uses `tyutool-core` directly, with no Tauri dependency. It provides the same flash/erase/read capabilities as the desktop GUI in a terminal-friendly format.

## Supported Chips

- **BK7231N** — Beken BK7231N
- **T2** — BK7231N-compatible
- **T5** — Beken T5 (extended frame protocol)

## Commands

### List Serial Ports

```bash
tyutool list-ports
```

### Flash Firmware

```bash
# All options
tyutool write -d <DEVICE> -p <PORT> -b <BAUD> -s <START> --end <END> -f <FILE>

# Minimal (auto-detect port, default baud 921600, compute end from file size)
tyutool write -d bk7231n -f firmware.bin

# Examples
tyutool write -d bk7231n -p /dev/ttyUSB0 -f firmware.bin
tyutool write -d t5 -p /dev/ttyUSB0 -b 921600 -s 0x00000000 -f firmware.bin
```

### Read Flash

```bash
# Default: read 2MB from address 0x0
tyutool read -d t5 -p /dev/ttyUSB0 -f dump.bin

# Custom range
tyutool read -d bk7231n -p /dev/ttyUSB0 -s 0x00000000 -l 0x100000 -f dump.bin
```

### Self-Update

```bash
# Check for updates
tyutool update --check

# Update from GitHub (default)
tyutool update

# Update from Gitee mirror
tyutool update --source gitee
```

## Building

```bash
# Development
cargo run -p tyutool-cli -- --help

# Release build
cargo build -p tyutool-cli --release
# Output: target/release/tyutool_cli

# Or use the project script
pnpm run release:cli
# Output: target/release/tyutool
```

## Verbose Logging

Enable debug output via the `RUST_LOG` environment variable:

```bash
RUST_LOG=debug tyutool list-ports
RUST_LOG=debug tyutool write -d bk7231n -f firmware.bin
```

## Project Structure

```
tyutool-cli/
├── Cargo.toml
└── src/
    ├── main.rs     # CLI argument parsing (clap) + job execution
    └── update.rs   # Self-update logic (GitHub/Gitee releases)
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `tyutool-core` | Flash plugin registry, serial port, job execution |
| `clap` | Command-line argument parsing |
| `env_logger` | Logging with `RUST_LOG` filter |
| `reqwest` | HTTP client for self-update |
| `self-replace` | In-place binary replacement during update |
| `semver` | Version comparison for updates |
