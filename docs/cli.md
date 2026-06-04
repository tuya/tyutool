# tyutool CLI Reference

`tyutool` is a command-line tool for flashing, reading, and managing Tuya-class IoT device firmware over UART.

## Installation

Download the latest release binary from the GitHub Releases page. Place it on your `PATH`.

## Global Options

| Option | Description |
|--------|-------------|
| `--verbose` | Write developer diagnostic logs to stderr (always written to log file) |
| `--plain` | Force plain text output (ASCII-only, no spinner or progress bar) |

**Log file location:**
- Linux: `~/.local/share/tyutool/tyutool.log`
- macOS: `~/Library/Application Support/tyutool/tyutool.log`
- Windows: `%APPDATA%\tyutool\tyutool.log`

## Subcommands

### `write` — Flash firmware to device

```
tyutool write -d <DEVICE> -f <FILE> [-p <PORT>] [-b <BAUD>] [-s <START>] [--end <END>]
```

| Flag | Short | Description | Default |
|------|-------|-------------|---------|
| `--device` | `-d` | Chip name (see supported list) | required |
| `--file` | `-f` | Firmware `.bin` file path | required |
| `--port` | `-p` | Serial port (e.g. `/dev/ttyUSB0`, `COM3`) | auto-detect first port |
| `--baud` | `-b` | UART baud rate | chip-specific (see below) |
| `--start` | `-s` | Flash start address (hex, e.g. `0x0`) | `0x00000000` |
| `--end` | | Flash end address (hex); defaults to `start + file size` | computed |

**Example:**
```bash
tyutool write -d bk7231n -f firmware.bin -p /dev/ttyUSB0
```

---

### `read` — Read flash contents from device

```
tyutool read -d <DEVICE> -f <FILE> [-p <PORT>] [-b <BAUD>] [-s <START>] [-l <LENGTH>]
```

| Flag | Short | Description | Default |
|------|-------|-------------|---------|
| `--device` | `-d` | Chip name | required |
| `--file` | `-f` | Output `.bin` file path | required |
| `--port` | `-p` | Serial port | auto-detect |
| `--baud` | `-b` | UART baud rate | chip-specific |
| `--start` | `-s` | Read start address (hex) | `0x00000000` |
| `--length` | `-l` | Read length (hex) | `0x200000` |

**Example:**
```bash
tyutool read -d bk7231n -f flash_dump.bin -l 0x200000
```

---

### `list-ports` — List available serial ports

```
tyutool list-ports
```

Prints tab-separated columns: `path`, `vid:pid`, `usb_interface`, `port_role`, `display_name`.

---

### `reset` — Hardware-reset device via DTR/RTS

```
tyutool reset [-p <PORT>] [-d <DEVICE>]
```

| Flag | Short | Description | Default |
|------|-------|-------------|---------|
| `--port` | `-p` | Serial port | auto-detect |
| `--device` | `-d` | Chip family (affects reset timing) | `bk7231n` |

---

### `authorize` — TuyaOpen device authorization

```
tyutool authorize [-p <PORT>] [--uuid <UUID>] [--authkey <AUTHKEY>]
```

| Flag | Description |
|------|-------------|
| `--port` | Serial port (default: auto-detect) |
| `--uuid` | UUID to write (omit to read current authorization state only) |
| `--authkey` | AuthKey to write (omit to read only) |

**Read current auth state:**
```bash
tyutool authorize -p /dev/ttyUSB0
```

**Write new authorization:**
```bash
tyutool authorize -p /dev/ttyUSB0 --uuid abc123 --authkey def456
```

---

### `update` — Self-update binary

```
tyutool update [--check] [--source <github|gitee>]
```

| Flag | Description |
|------|-------------|
| `--check` | Only check version, do not download |
| `--source` | Update source (`github` default, `gitee` for China) |

---

### `serve` — WebSocket server (dev/IDE mode)

```
tyutool serve [--port <PORT>]
```

Starts a local WebSocket server for browser-based flash operations (used by tuyaopen-ide). Default port: `9527`.

---

### `usb-port-survey` — USB/serial metadata dump

```
tyutool usb-port-survey
```

Outputs JSON with raw USB metadata for all ports. Used for cross-OS debugging.

---

## Supported Devices

| `--device` value | Chip | Default baud |
|-----------------|------|-------------|
| `bk7231n` | BK7231N | 921600 |
| `t2` | T2 | 921600 |
| `t3` | T3 | 921600 |
| `t1` | T1 | 921600 |
| `t5` | T5 | 921600 |
| `ln882h` | LN882H | 115200 |
| `esp32` | ESP32 | 460800 |
| `esp32c3` | ESP32-C3 | 460800 |
| `esp32c6` | ESP32-C6 | 460800 |
| `esp32p4` | ESP32-P4 | 460800 |
| `esp32s3` | ESP32-S3 | 460800 |

---

## Output Modes

**Rich mode** (interactive TTY): spinner, progress bar with ANSI color, `✓` checkmarks.

**Plain mode** (CI / piped / redirected): fixed-width phase labels, 10%-step percent ticks on long phases, ASCII-only separators.

Plain mode output example:
```
tyutool v3.0.7  linux/x86_64

write  BK7231N  /dev/ttyUSB0  921600
  File   firmware.bin  1.8 MiB
  Range  0x00000000 -> 0x001CE400

Handshake         OK
Erase             10%  20%  30%  40%  50%  60%  70%  80%  90%  OK
Write [1/2]       10%  20%  30%  40%  50%  60%  70%  80%  90%  OK
Write [2/2]       10%  20%  30%  40%  50%  60%  70%  80%  90%  OK
Verify            OK
Reboot            OK
Flash OK  3.2s
```

Exit code `0` on success, non-zero on failure or cancellation.
