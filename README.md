# tyutool

[![Release](https://img.shields.io/github/v/release/tuya/tyutool?style=flat-square)](https://github.com/tuya/tyutool/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/tuya/tyutool/release.yml?style=flat-square&label=build)](https://github.com/tuya/tyutool/actions/workflows/release.yml)
[![License](https://img.shields.io/github/license/tuya/tyutool?style=flat-square)](LICENSE.txt)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-blue?style=flat-square)](https://github.com/tuya/tyutool/releases/latest)

Firmware flash tool for Tuya-class IoT devices. Available as a cross-platform desktop GUI (Tauri 2 + Vue 3) and a standalone CLI binary.

## Supported Chips

| Family | Chips |
|--------|-------|
| Tuya   | T1, T2, T3, T5 |
| Beken  | BK7231N |
| Espressif | ESP32, ESP32-C3, ESP32-C6, ESP32-S3 |

## Download

Grab the latest release from [GitHub Releases](https://github.com/tuya/tyutool/releases/latest) or the [Gitee mirror](https://gitee.com/tuya-open/tyutool/releases).

### GUI

| Platform | Package |
|----------|---------|
| Linux x86\_64 | `.deb`, `.rpm`, `.AppImage`, portable tar.gz |
| Linux aarch64 | `.deb`, `.AppImage`, portable tar.gz |
| macOS (Universal) | `.dmg`, portable tar.gz |
| Windows x86\_64 | NSIS installer (`.exe`), portable `.zip` |

The GUI supports automatic in-app updates.

### CLI

| Platform | File |
|----------|------|
| Linux x86\_64 | `tyutool-cli_linux_x86_64_<ver>.tar.gz` |
| Linux aarch64 | `tyutool-cli_linux_aarch64_<ver>.tar.gz` |
| macOS x86\_64 | `tyutool-cli_macos_x86_64_<ver>.tar.gz` |
| macOS aarch64 | `tyutool-cli_macos_aarch64_<ver>.tar.gz` |
| Windows x86\_64 | `tyutool-cli_windows_x86_64_<ver>.zip` |

Extract and run `tyutool_cli` (or `tyutool_cli.exe` on Windows).

## CLI Usage

```
tyutool <COMMAND>
```

### Flash firmware

```bash
# Auto-detect port, default baud 921600
tyutool write -d bk7231n -f firmware.bin

# Specify port
tyutool write -d bk7231n -p /dev/ttyUSB0 -f firmware.bin

# Full options
tyutool write -d <DEVICE> -p <PORT> -b <BAUD> -s <START_ADDR> --end <END_ADDR> -f <FILE>
```

Supported `-d` values: `bk7231n`, `t2`, `t5`

### Read flash

```bash
# Read 2 MB from address 0x0 (default)
tyutool read -d bk7231n -p /dev/ttyUSB0 -f dump.bin

# Custom range
tyutool read -d t5 -p /dev/ttyUSB0 -s 0x0 -l 0x100000 -f dump.bin
```

### List serial ports

```bash
tyutool list-ports
```

### Device authorization

```bash
# Read current auth state
tyutool authorize -p /dev/ttyUSB0

# Write UUID and AuthKey
tyutool authorize -p /dev/ttyUSB0 --uuid <UUID> --authkey <AUTHKEY>
```

### Reset device

```bash
tyutool reset -p /dev/ttyUSB0 -d bk7231n
```

### Self-update

```bash
# Check latest version
tyutool update --check

# Update from GitHub (default)
tyutool update

# Update from Gitee mirror
tyutool update --source gitee
```

### Verbose logging

```bash
RUST_LOG=debug tyutool write -d bk7231n -f firmware.bin
```

## Build from Source

**Prerequisites:** Rust (stable), Node.js 22+, pnpm 10+

### CLI only

```bash
cargo build -p tyutool-cli --release
# Output: target/release/tyutool_cli
```

### Desktop GUI

```bash
pnpm install
pnpm run tauri:build
```

### Development

```bash
pnpm install
pnpm run tauri:dev   # GUI dev server with hot-reload
pnpm run dev:web     # Frontend-only dev server (no Tauri)
```

## Architecture

```
tyutool/
├── crates/
│   ├── tyutool-core/   # Rust library — all flash logic, chip plugins, serial utils
│   └── tyutool-cli/    # Standalone CLI binary (depends on tyutool-core only)
├── src-tauri/          # Tauri 2 shell (Rust backend for the desktop GUI)
└── src/                # Vue 3 frontend (Vite, Pinia, Tailwind CSS, DaisyUI)
```

`tyutool-core` is shared by both the GUI and CLI. Flash logic lives there and is never duplicated.

## License

Apache-2.0 — see [LICENSE.txt](LICENSE.txt).
