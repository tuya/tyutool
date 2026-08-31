# tyutool CLI Reference

> A styled, user-facing version of this reference lives in the [usage guide](../usage-guide/en/cli.html) (`../usage-guide/zh/cli.html` for 中文). This markdown file remains the authoritative source — update it whenever the CLI changes.

`tyutool` is a command-line tool for flashing, reading, erasing, monitoring, and authorizing Tuya-class IoT devices over UART.

## Contents

- [Command summary](#command-summary)
- [Installation](#installation)
- [Reading this reference](#reading-this-reference)
- [Global options](#global-options)
- [Conventions that apply to every command](#conventions-that-apply-to-every-command)
- Commands: [`write`](#write--flash-firmware-to-a-device) · [`read`](#read--dump-flash-to-a-file) · [`erase`](#erase--erase-a-flash-region) · [`list-ports`](#list-ports--list-serial-ports) · [`reset`](#reset--hardware-reset-via-dtrrts) · [`monitor`](#monitor--live-serial-monitor) · [`authorize`](#authorize-alias-auth--tuyaopen-device-authorization) · [`update`](#update--self-update-the-binary) · [`serve`](#serve--websocket-server-dev-only) · [`logs`](#logs--inspect-the-session-log-files) · [`completions`](#completions--shell-completion-script) · [`usb-port-survey`](#usb-port-survey--raw-usbserial-metadata)
- [Device and baud table](#device-and-baud-table)
- [Output modes](#output-modes)
- [Exit codes](#exit-codes)
- [Common errors](#common-errors)

## Command summary

| Command | What it does | Talks to a device? |
|---------|--------------|--------------------|
| [`write`](#write--flash-firmware-to-a-device) | Flash a firmware `.bin` to the device | yes |
| [`read`](#read--dump-flash-to-a-file) | Dump a flash region to a local `.bin` | yes |
| [`erase`](#erase--erase-a-flash-region) | Erase a flash region | yes |
| [`list-ports`](#list-ports--list-serial-ports) | List detected serial ports | no |
| [`reset`](#reset--hardware-reset-via-dtrrts) | Pulse DTR/RTS to reboot the device | yes (no protocol) |
| [`monitor`](#monitor--live-serial-monitor) | Stream device output, forward your keystrokes | yes (raw UART) |
| [`authorize`](#authorize-alias-auth--tuyaopen-device-authorization) (`auth`) | Read or write TuyaOpen UUID/AuthKey | yes |
| [`update`](#update--self-update-the-binary) | Check for and install a newer `tyutool` | no |
| [`serve`](#serve--websocket-server-dev-only) | Local WebSocket backend for browser mode (dev only) | on request |
| [`logs`](#logs--inspect-the-session-log-files) | List, tail, or export the session log files | no |
| [`completions`](#completions--shell-completion-script) | Print a shell completion script | no |
| [`usb-port-survey`](#usb-port-survey--raw-usbserial-metadata) | Dump raw USB metadata as JSON | no |

`tyutool help <command>` prints the same information as `tyutool <command> --help`.

## Installation

Download the latest CLI binary from the [GitHub Releases page](https://github.com/tuya/tyutool/releases). Each release ships five prebuilt binaries:

| Platform | Asset |
|----------|-------|
| Linux x86_64 | `tyutool-cli_linux_x86_64_<ver>.tar.gz` |
| Linux aarch64 | `tyutool-cli_linux_aarch64_<ver>.tar.gz` |
| macOS x86_64 (Intel) | `tyutool-cli_macos_x86_64_<ver>.tar.gz` |
| macOS aarch64 (Apple silicon) | `tyutool-cli_macos_aarch64_<ver>.tar.gz` |
| Windows x86_64 | `tyutool-cli_windows_x86_64_<ver>.zip` |

`<ver>` is the release version (e.g. `3.2.8`). Each release also publishes a `latest.json` manifest whose `cli.<platform>.sha256` field gives the SHA-256 of the matching asset — verify it if your download channel is untrusted.

Extract the binary and put it on your `PATH`:

```bash
# Linux / macOS (tar.gz)
tar -xzf tyutool-cli_linux_x86_64_*.tar.gz
sudo mv tyutool_cli /usr/local/bin/tyutool
chmod +x /usr/local/bin/tyutool

# Windows (.zip): extract tyutool_cli.exe and add its folder to PATH
```

Verify the install:

```bash
tyutool --version    # -> tyutool 3.2.8
tyutool list-ports   # lists detected serial ports
```

The binary inside the archive is named `tyutool_cli` (`tyutool_cli.exe` on Windows). Renaming it to `tyutool` is only a convenience — every example below works either way.

> Tip: the CLI can replace itself in place — see [`update`](#update--self-update-the-binary).

## Reading this reference

Synopses follow the usual convention:

| Notation | Meaning |
|----------|---------|
| `-d <DEVICE>` | Option **requires** a value. `-d` on its own is an error, not a "default on" switch. |
| `--json` | Flag with **no** value. Present = on, absent = off. |
| `[ ... ]` | Optional. |
| no brackets | Required — the command refuses to run without it. |

Short and long forms are interchangeable, and `-p COM3`, `-p=COM3`, `--port COM3`, and `--port=COM3` are all accepted.

Every option that takes a value gets it from exactly one place, in this order:

1. what you typed on the command line;
2. otherwise the documented default for that command (some defaults depend on `-d`);
3. otherwise, for `-p/--port` only, auto-detect or an interactive prompt (see [port selection](#port-selection)).

## Global options

These four work on every subcommand and may be placed **before or after** it — `tyutool --plain write …` and `tyutool write --plain …` are identical.

| Option | Value | Description |
|--------|-------|-------------|
| `--verbose` | none | Also print developer diagnostic logs (`Info` and above) to stderr, and print the log file path at startup. The log file always receives full `Trace`-level detail whether or not you pass this. |
| `--plain` | none | Force plain-text output: ASCII only, no spinner, no redrawn progress bar. Already the default when stderr is not a terminal, so you rarely need it by hand — pass it when you want deterministic output *in* a terminal (screenshots, docs, test fixtures). |
| `-h`, `--help` | none | Print help for the current command and exit 0. |
| `-V`, `--version` | none | Print `tyutool <version>` and exit 0. Top level only. |

## Conventions that apply to every command

### Port selection

Every device command takes `-p/--port`. When you omit it:

| Ports detected | Behavior |
|----------------|----------|
| exactly one | it is used, and `Using port: <path>` is printed to stderr |
| several, interactive terminal | the list is printed and you are prompted: `Select port [0-N]:` |
| several, non-interactive (CI, pipe, `< /dev/null`) | the command fails with `multiple serial ports found; specify one with -p/--port (e.g. -p COM6)` |
| none | the command fails with `No serial ports found.` |

In scripts, always pass `-p` explicitly. The prompt is deliberately unavailable when stdin is not a terminal, so a pipeline cannot hang or misread piped data as a menu choice.

### Hex values

`-s/--start`, `--end`, and `-l/--length` are parsed as hexadecimal. All of these are the same address:

```
0x1CE400   0X1CE400   1ce400   1CE400   "  0x1ce400  "
```

The `0x`/`0X` prefix is optional, hex digits are case-insensitive, and surrounding whitespace is trimmed. Decimal is **not** supported — `-l 2097152` is read as `0x2097152` (≈34 MiB), not 2 MiB. A malformed value fails before the port is opened, with `invalid hex address '<value>': …`.

### Device names

`-d/--device` is case-insensitive, and `t5` is accepted as an alias for `t5ai`. `-d T5AI`, `-d t5ai`, `-d t5AI`, and `-d t5` all select T5AI. An unsupported value fails immediately and lists the accepted ones. See the [device and baud table](#device-and-baud-table).

### stdout vs stderr

Everything humans read — the startup banner, progress, phase lines, prompts, monitor banners — goes to **stderr**. Only machine-readable payloads go to **stdout**:

| Command | On stdout |
|---------|-----------|
| `list-ports` | the port table, or the JSON array with `--json` |
| `usb-port-survey` | the JSON array |
| `completions` | the completion script |
| `monitor` | the raw bytes received from the device |
| `serve` | the two listening / `Ctrl+C` lines |

So `tyutool list-ports --json | jq .` and `source <(tyutool completions bash)` work with no extra flags. `usb-port-survey` and `completions` additionally suppress the banner and skip log-file setup entirely, so their stdout stays byte-clean even if a redirect merges the two streams.

### Log files

Every run except `usb-port-survey`, `completions` and `logs` opens a session log file and prints its path in the banner:

```
tyutool v3.2.8  windows/x86_64
log: C:\Users\you\AppData\Roaming\tyutool\tyutool-20260818-184752.log
```

| Platform | Directory |
|----------|-----------|
| Linux | `~/.local/share/tyutool/` |
| macOS | `~/Library/Application Support/tyutool/` |
| Windows | `%APPDATA%\tyutool\` |

Files are named `tyutool-<YYYYMMDD-HHMMSS>.log`, one per run. A session file is capped at 10 MB and rolls over to `tyutool-<timestamp>-1.log`, `-2.log`, … beyond that. At startup the oldest files are pruned until the directory holds at most 100 files and 100 MB in total (at least one file is always kept). Attach the file for the run that failed when filing a bug report — [`logs export`](#logs--inspect-the-session-log-files) packages them for you, and [`logs list`](#logs--inspect-the-session-log-files) tells you which file belongs to which run.

## Commands

### `write` — flash firmware to a device

```
tyutool write -d <DEVICE> -f <FILE> [-p <PORT>] [-b <BAUD>] [-s <START>] [--end <END>]
```

| Option | Value | Required | Default when omitted |
|--------|-------|----------|----------------------|
| `-d`, `--device` | chip name | **yes** | — |
| `-f`, `--file` | path to a firmware `.bin` | **yes** | — |
| `-p`, `--port` | serial port (`/dev/ttyUSB0`, `COM3`) | no | [auto-detect or prompt](#port-selection) |
| `-b`, `--baud` | baud rate, decimal | no | [chip-specific](#device-and-baud-table) — 921600 Beken/T-series, 460800 ESP32, 115200 LN882H |
| `-s`, `--start` | flash start address, hex | no | `0x00000000` |
| `--end` | flash end address, hex (no short form) | no | `start + file size`, formatted `0x%08X` |

The firmware file is written starting at `--start`. Progress runs through the phases the chip's plugin defines — typically handshake, erase, one or more write segments, verify, reboot.

**About `--end`:** you almost never need it. It is filled in for you from the file size, and no chip plugin uses it to bound the transfer — the number of bytes written is the size of the file. The Beken plugin requires the field to be *present* (the CLI always supplies it), and the value is what the job summary prints as the range. Passing a value larger or smaller than the file does not extend or truncate the write.

```bash
# Simplest form — one port plugged in, defaults for everything else
tyutool write -d bk7231n -f firmware.bin

# Pin down the port and baud (what you want in a script or CI job)
tyutool write -d bk7231n -f firmware.bin -p /dev/ttyUSB0 -b 921600

# Write a partition image at its own offset instead of the start of flash
# (0x7CD000 is the T5AI tuya_data partition — see the erase presets)
tyutool write -d t5ai -f tuya_data.bin -p COM3 -s 0x7CD000

# T5AI accepts the legacy alias and any capitalization
tyutool write -d t5 -f app.bin -p COM3

# ESP32-S3 (default baud 460800)
tyutool write -d esp32s3 -f app.bin -p /dev/ttyACM0

# Deterministic output plus full diagnostics, e.g. when reporting a bug
tyutool --plain --verbose write -d ln882h -f app.bin -p /dev/ttyUSB0
```

LN882H needs the BOOT/A9 pin held low to enter download mode; the CLI prints that as an actionable warning while it waits.

---

### `read` — dump flash to a file

```
tyutool read -d <DEVICE> -f <FILE> [-p <PORT>] [-b <BAUD>] [-s <START>] [-l <LENGTH>]
```

| Option | Value | Required | Default when omitted |
|--------|-------|----------|----------------------|
| `-d`, `--device` | chip name | **yes** | — |
| `-f`, `--file` | output `.bin` path | **yes** | — |
| `-p`, `--port` | serial port | no | [auto-detect or prompt](#port-selection) |
| `-b`, `--baud` | baud rate, decimal | no | [chip-specific](#device-and-baud-table) |
| `-s`, `--start` | read start address, hex | no | `0x00000000` |
| `-l`, `--length` | number of bytes to read, hex | no | `0x200000` (2 MiB) |

The region read is `start` up to `start + length`. Note that the `0x200000` default is the *whole* flash on a 2 MiB part but only a quarter of a T5AI — pass `-l` when you want a full dump of a larger chip. The output file is **created or overwritten** — there is no confirmation prompt, so mind the path. Reads are performed in whole sectors, so the file can be slightly longer than `--length` when the region does not end on a sector boundary. On BK7231N the dump is CRC-checked against the device before it is saved; T5AI verifies per sector during the read.

```bash
# Default 2 MiB from address 0 — on BK7231N/T2/LN882H that is the whole flash
tyutool read -d bk7231n -f dump.bin

# Whole 8 MiB flash of a T5AI (the default 0x200000 would stop a quarter in)
tyutool read -d t5ai -f t5ai-full.bin -p COM3 -l 0x800000

# Just the BK7231N auth/KV region: 0x1EE000 to the end of flash
tyutool read -d bk7231n -f kv.bin -p /dev/ttyUSB0 -s 0x1EE000 -l 0x12000

# Read back what you just flashed and compare
tyutool write -d bk7231n -f firmware.bin -p /dev/ttyUSB0
tyutool read  -d bk7231n -f readback.bin -p /dev/ttyUSB0 -l 0x1EE000
cmp firmware.bin readback.bin   # readback is longer: it covers the whole range
```

---

### `erase` — erase a flash region

```
tyutool erase -d <DEVICE> [-p <PORT>] [-b <BAUD>] [-s <START>] [-l <LENGTH>]
```

| Option | Value | Required | Default when omitted |
|--------|-------|----------|----------------------|
| `-d`, `--device` | chip name | **yes** | — |
| `-p`, `--port` | serial port | no | [auto-detect or prompt](#port-selection) |
| `-b`, `--baud` | baud rate, decimal | no | [chip-specific](#device-and-baud-table) |
| `-s`, `--start` | erase start address, hex | no | `0x00000000` |
| `-l`, `--length` | erase length, hex | no | `0x200000` (2 MiB) |

The region erased is `start` up to `start + length`, rounded outwards to the chip's 4 KiB sector size — an unaligned request therefore erases slightly more than you asked for. **There is no confirmation prompt.** With no `-s`/`-l` this wipes the first 2 MiB of flash, firmware included; on BK7231N, T2, and LN882H (2 MiB parts) that is the whole chip.

```bash
# Wipe the first 2 MiB (the defaults) — on BK7231N that is the entire flash
tyutool erase -d bk7231n -p /dev/ttyUSB0

# T5AI: erase only the tuya_data partition (auth + provisioning data, 196 KiB),
# leaving the firmware and the RF calibration blocks alone
tyutool erase -d t5ai -p COM3 -s 0x7CD000 -l 0x31000

# T5AI: erase everything except the last 8 KiB (sys_rf + sys_net calibration —
# erasing those degrades the radio, which is why the GUI preset stops here)
tyutool erase -d t5ai -p COM3 -s 0x0 -l 0x7FE000

# BK7231N: auth/KV region only
tyutool erase -d bk7231n -p /dev/ttyUSB0 -s 0x1EE000 -l 0x12000
```

The GUI's erase presets are the same ranges per chip; if you are unsure which region holds what, check the preset for your chip in `src/features/firmware-flash/chip-manifests.ts`.

---

### `list-ports` — list serial ports

```
tyutool list-ports [--json]
```

| Option | Value | Required | Default when omitted |
|--------|-------|----------|----------------------|
| `--json` | none | no | tab-separated text |

Default output is one tab-separated line per port, five columns:

```
path      vid:pid      usb_interface   port_role   display_name
COM6      1a86:55d2    2               -           wch.cn USB-Enhanced-SERIAL-B CH342 (COM6)
COM7      1a86:55d2    0               -           wch.cn USB-Enhanced-SERIAL-A CH342 (COM7)
```

- `vid:pid` is **hex**; a non-USB port shows `-`.
- `usb_interface` is the USB interface number — on a dual-interface adapter it is what distinguishes the flash port from the log port. `-` when unknown.
- `port_role` is `flash_auth` or `log` for adapters tyutool recognizes, otherwise `-`.
- `display_name` may be empty.

With `--json` you get a pretty-printed array. Three differences from the text form: keys are camelCase, `usbVid`/`usbPid` are **decimal** numbers (6790 = `0x1a86`), and **unknown fields are omitted rather than null**:

```json
[
  {
    "path": "COM6",
    "name": "wch.cn USB-Enhanced-SERIAL-B CH342 (COM6)",
    "usbVid": 6790,
    "usbPid": 21970,
    "usbSerial": "56D7035114",
    "usbInterface": 2
  }
]
```

Possible keys: `path` (always present), `name`, `usbVid`, `usbPid`, `usbSerial`, `usbInterface`, `portRole`.

```bash
tyutool list-ports                                   # human-readable
tyutool list-ports --json | jq -r '.[].path'         # just the paths
tyutool list-ports --json | jq '.[] | select(.portRole == "flash_auth")'
tyutool list-ports | cut -f1                         # first column, no jq needed
```

The banner goes to stderr, so the pipes above need no extra flags. For raw, unfiltered USB metadata use [`usb-port-survey`](#usb-port-survey--raw-usbserial-metadata).

---

### `reset` — hardware-reset via DTR/RTS

```
tyutool reset [-p <PORT>] [-d <DEVICE>]
```

| Option | Value | Required | Default when omitted |
|--------|-------|----------|----------------------|
| `-p`, `--port` | serial port | no | [auto-detect or prompt](#port-selection) |
| `-d`, `--device` | chip name | no | `bk7231n` |

Toggles the DTR/RTS lines to reboot the device without touching flash. `-d` matters because the pulse differs per family: the Beken/T-series parts use the same pulse as their flash handshake (`bk7231n`/`t2` differ from `t5ai`/`t3`/`t1`), and the ESP32 parts use espflash's hard-reset sequence. Using the wrong family usually means the device simply does not reboot.

**Success is silent.** The command prints only the startup banner and exits 0; the confirmation is a diagnostic log line, so add `--verbose` if you want to see it on screen.

```bash
# Reboot a BK7231N on the only port present
tyutool reset

# Named port, T5AI pulse
tyutool reset -p COM3 -d t5ai

# ESP32-S3 hard reset, with the confirmation echoed to the terminal
tyutool reset -p /dev/ttyACM0 -d esp32s3 --verbose

# Reboot, then watch it come up
tyutool reset -p COM7 -d t5ai && tyutool monitor -p COM7 -d t5ai
```

---

### `monitor` — live serial monitor

```
tyutool monitor [-p <PORT>] [-b <BAUD>] [-d <DEVICE>] [-l <FILE>]
```

| Option | Value | Required | Default when omitted |
|--------|-------|----------|----------------------|
| `-p`, `--port` | serial port | no | [auto-detect or prompt](#port-selection) |
| `-b`, `--baud` | baud rate, decimal | no | `460800` with `-d t5ai` or `-d t3`, otherwise `115200` |
| `-d`, `--device` | chip name | no | none — baud falls back to `115200` |
| `-l`, `--log` | **path to a file** | no | no file; output goes to stdout only |

Opens the port at 8 data bits, no parity, 1 stop bit (8N1 — not configurable) and streams whatever the device sends to stdout, byte for byte.

#### `-b`, `-d`: which baud you get

`-d` on this command does one thing only: it picks the default baud. It does not change any protocol or timing.

| You pass | Baud used |
|----------|-----------|
| `-b 921600` | 921600 — an explicit `-b` always wins, `-d` is then irrelevant |
| `-d t5ai` (or `-d t5`), `-d t3` | 460800 |
| any other `-d` | 115200 |
| neither | 115200 |

These are **monitor** defaults and deliberately differ from the flash defaults used by `write`/`read`/`erase` (where the same T5AI is 921600). They mirror each chip's `defaultLogBaudRate` in the GUI's manifest, so the same port reads the same in both. A wrong baud looks like solid garbage bytes rather than an error, so if the output is unreadable, check the baud first.

#### `-l`, `--log`: tee output to a file

`-l` **requires a path.** `tyutool monitor -l` on its own is a usage error (`a value is required for '--log <LOG>' but none was supplied`); there is no "log to a default filename" mode. With a path:

- the file is created if missing and **appended** to if it already exists — nothing is ever truncated, so repeated sessions accumulate in one file;
- every chunk received is written and flushed immediately, so the file stays current even if the process is killed;
- the bytes are **exactly** what the device sent: no timestamps, no line-ending translation, no ANSI-escape stripping, and the `--- Monitor … ---` banners are not included;
- if the file cannot be opened, the command fails *before* the port is opened, with `cannot open log file '<path>': <reason>`.

Because device output goes to stdout while the banners go to stderr, a shell redirect is the other way to capture a session. Use whichever fits:

| | `-l file.log` | `> file.log` |
|---|---|---|
| Output still visible on screen | yes | no |
| Existing file | appended | truncated (`>`) or appended (`>>`) |
| Banners included | no | no |

#### Interacting with the device

On an interactive terminal your keystrokes are forwarded to the device as you type, so the TuyaOpen shell (`tuya>`) can be driven from inside the monitor. There is no local echo — what you see is the device echoing back.

| Key | Sent to the device |
|-----|--------------------|
| printable characters (including non-ASCII) | the UTF-8 bytes of the character |
| Enter | `\r\n` |
| Tab | `\t` |
| Backspace | `0x08` |
| arrow keys, function keys, Esc, Home/End | nothing — ignored |
| **Ctrl+]** | quits the monitor (miniterm-compatible) |
| **Ctrl+C** | quits the monitor |

When stdin is *not* a terminal (a pipe, a CI job), raw keys cannot be read: input is forwarded **line by line**, each line terminated with `\r\n`. After EOF the monitor keeps running read-only, and Ctrl+C (SIGINT) is the only way out.

#### Exit behavior

Quitting with Ctrl+] or Ctrl+C prints `--- Monitor stopped ---` and exits 0. If the adapter is unplugged mid-session the monitor reports it and also exits 0 — a disconnect is not treated as a failure:

```
--- Monitor stopped: serial port COM7 disconnected or unavailable. ---
--- Detail: <driver message> ---
```

```bash
# Auto-detect port, 115200
tyutool monitor

# T5AI log port at its 460800 default
tyutool monitor -p COM7 -d t5ai

# Explicit baud, ignoring the -d defaults entirely
tyutool monitor -p /dev/ttyUSB0 -b 921600

# Watch on screen and append to a file at the same time
tyutool monitor -p COM7 -d t5ai -l boot.log

# Same, with a per-run filename
tyutool monitor -p COM7 -d t5ai -l "boot-$(date +%Y%m%d-%H%M%S).log"

# Capture only (no live view); stop it with Ctrl+C
tyutool monitor -p COM7 -d t5ai > boot.log

# Non-interactive: fire two shell commands at the device, keep watching
printf 'tuya_help\r\nauth-read\r\n' | tyutool monitor -p COM7 -d t5ai -l session.log
```

---

### `authorize` (alias: `auth`) — TuyaOpen device authorization

```
tyutool authorize [-p <PORT>] [-d <DEVICE>] [--uuid <UUID>] [--authkey <AUTHKEY>]
tyutool auth      [-p <PORT>] [-d <DEVICE>] [--uuid <UUID>] [--authkey <AUTHKEY>]
```

| Option | Value | Required | Default when omitted |
|--------|-------|----------|----------------------|
| `-p`, `--port` | serial port | no | [auto-detect or prompt](#port-selection) |
| `-d`, `--device` | chip name | no | generic timing |
| `--uuid` | UUID to write | no (see below) | read-only mode |
| `--authkey` | AuthKey to write | no (see below) | read-only mode |

Drives the TuyaOpen UART shell to read or write the device's authorization pair.

- **Both or neither.** Pass `--uuid` *and* `--authkey` to write; pass neither to read the current state. Exactly one of the two is rejected up front with `authorize: provide both --uuid and --authkey to write, or neither to read`, because a half-specified write would silently degrade into a read.
- **KV storage only.** This command never burns OTP/eFuse; OTP is exclusively a GUI batch-flow feature.
- **The baud rate is fixed at 115200** and there is no `-b` option — the TuyaOpen shell runs at that rate.
- `-d` only adjusts per-chip timing while talking to the shell; omitting it uses generic timing, which works for most parts.
- Credentials that are read back are printed to the terminal in plain text. That is intentional for a CLI diagnosis flow — do not paste the output into a public issue.

```bash
# Read the current authorization state
tyutool authorize -p /dev/ttyUSB0

# Same, with ESP32 timing, using the short alias
tyutool auth -p COM3 -d esp32

# Write a new pair
tyutool authorize -p /dev/ttyUSB0 -d esp32 --uuid uuid1234567890ab --authkey <32-char-key>

# Write, then read back to confirm
tyutool auth -p COM3 --uuid uuid1234567890ab --authkey <32-char-key> && tyutool auth -p COM3
```

UUID length is not assumed — devices in the field return 12, 16, or 20+ characters.

---

### `update` — self-update the binary

```
tyutool update [--check] [--source <github|tuya>]
```

| Option | Value | Required | Default when omitted |
|--------|-------|----------|----------------------|
| `--check` | none | no | download and install when a newer version exists |
| `--source` | `github` or `tuya` | no | same as `github` |

Fetches the release manifest, compares versions numerically (`1.10.0` is newer than `1.2.0`), and replaces the running binary in place.

| `--source` | Manifests tried |
|------------|-----------------|
| omitted, or `github` | GitHub `latest.json` first, then the Tuya OSS mirror if GitHub fails |
| `tuya` | the Tuya OSS mirror only — use this on a mainland-China network |
| anything else | rejected: `Unknown source '<value>'. Use 'github' or 'tuya'.` |

The steps, all printed to stderr: check → report latest and current version → (stop here if already current, exit 0) → download → verify SHA-256 → extract the binary from the archive → replace the current executable. The manifest request times out after 8 s, the download after 120 s. A checksum mismatch aborts before anything is replaced.

`--check` stops after the version comparison and never downloads. It exits 0 whether or not an update exists, so parse the output rather than the exit code.

```bash
# Is there anything newer?
tyutool update --check

# Install it
tyutool update

# Mainland-China network: skip GitHub entirely
tyutool update --source tuya

# See exactly which URLs were tried
tyutool update --check --verbose
```

Self-replacement needs write permission on the binary itself: if it lives in `/usr/local/bin`, run the update with the privileges that own it. Auto-update covers the five released platforms only; anywhere else fails with `Unsupported platform for auto-update.` and you should download the archive by hand.

---

### `serve` — WebSocket server (dev only)

```
tyutool serve [--port <PORT>]
```

| Option | Value | Required | Default when omitted |
|--------|-------|----------|----------------------|
| `--port` | TCP port | no | `9527` |

Starts a local WebSocket backend so a browser page can drive real flash operations without the Tauri shell. This is what `pnpm run dev:web` launches, and what tuyaopen-ide talks to.

```
tyutool-cli serve listening on ws://127.0.0.1:9527
Press Ctrl+C to stop.
```

It binds `127.0.0.1` only, requires a loopback `Host` header (blocking DNS-rebinding tricks), and rejects a handshake whose `Origin` is a remote page — a random web page cannot reach in and flash your hardware. It is still a **development tool**: there is no authentication, and any local process can connect. Do not run it as a background service on a shared machine, and stop it with Ctrl+C when you are done. If the port is taken, the command fails with `failed to bind 127.0.0.1:<port>`. Free 9527 rather than moving the server: the browser client has `9527` hard-coded (`src/transport/ws-transport.ts`) with no setting for it, so pointing `serve` elsewhere leaves the page connecting to nothing. (`pnpm run dev:web` does honour `TYUTOOL_SERVE_PORT`, but that moves only the server half.) `--port` is for a host that tells its own client where to connect — tuyaopen-ide does that by injecting `__TUYAOPEN_IDE_CONFIG.wsUrl`.

For the shipped, hardened variant of this idea — token grants, an Origin allowlist, an audit log — see the separate `tyutool-bridge` binary.

---

### `logs` — inspect the session log files

```
tyutool logs [--dir <DIR>] list [--json]
tyutool logs [--dir <DIR>] tail [-f <FILE>] [-n <BYTES>]
tyutool logs [--dir <DIR>] export <DEST.zip> [--no-redact]
```

| Option | Value | Required | Default when omitted |
|--------|-------|----------|----------------------|
| `--dir` | Directory to read | no | the CLI's own log directory (see [Log files](#log-files)) |
| `--json` (`list`) | flag | no | tab-separated columns |
| `-f`, `--file` (`tail`) | File name inside the directory | no | the newest `tyutool-*.log` |
| `-n`, `--bytes` (`tail`) | Bytes to read back from the end | no | `65536` |
| `<DEST.zip>` (`export`) | Destination path | **yes** (positional) | — |
| `--no-redact` (`export`) | flag | no | credential values are redacted |

Reads the logs already on disk. It talks to no device, and — unlike every other
command — it opens no log file of its own, so `logs list` never reports a file it
just created itself.

`list` prints one row per session file, newest first: name, size in bytes, and local
modification time. `tail` prints the end of one file; with no `-f` it picks the newest,
which is normally the run you just made.

```bash
# Which sessions are on this machine?
tyutool logs list

# What went wrong in the last run?
tyutool logs tail -n 20000

# A specific session, machine-readable listing for a script
tyutool logs list --json
tyutool logs tail -f tyutool-20260828-144739.log

# The GUI keeps its logs elsewhere (Tauri's app log dir, named after the
# bundle identifier com.tyutool.desktop) — point --dir at them
tyutool logs --dir ~/.local/share/com.tyutool.desktop/logs list

# Package everything for a bug report
tyutool logs export ~/tyutool-logs.zip
```

The zip holds every `tyutool-*.log` in the directory plus a `report-info.txt` header
(version, OS, arch, install path). **Credential values are redacted by default** — the
values after `uuid=`, `authkey=`, `existing_uuid=` and `otp_uuid=` are masked, because the
bundle is meant to be attached to an issue. `--no-redact` keeps them in plaintext; use it
only for a bundle that stays on your own machine.

Two things this command will not do, both on purpose:

- It only reads files named `tyutool*.log`. The batch-auth `.trace` files share the
  directory and hold plaintext credentials, so `list` hides them and `tail -f` refuses to
  open them.
- `--file` takes a name, not a path. A value containing `/` or `\` is rejected rather
  than resolved.

---

### `completions` — shell completion script

```
tyutool completions <SHELL>
```

| Argument | Value | Required |
|----------|-------|----------|
| `<SHELL>` | `bash`, `elvish`, `fish`, `powershell`, or `zsh` | **yes** (positional) |

Prints the completion script to stdout and exits. No banner, no log file, so the output can be sourced or redirected directly.

```bash
# Bash, current shell only
source <(tyutool completions bash)

# Bash, permanently
tyutool completions bash > ~/.local/share/bash-completion/completions/tyutool

# Zsh — the file must be on your $fpath and named _<command>
tyutool completions zsh > ~/.zfunc/_tyutool

# Fish
tyutool completions fish > ~/.config/fish/completions/tyutool.fish

# PowerShell, current session
tyutool completions powershell | Out-String | Invoke-Expression

# PowerShell, permanently
tyutool completions powershell >> $PROFILE
```

The script completes the command name it was generated for. If you renamed the binary, generate the script with the name you actually invoke.

---

### `usb-port-survey` — raw USB/serial metadata

```
tyutool usb-port-survey
```

Takes no options. Dumps a pretty-printed JSON array describing every serial port the OS reports, including ones tyutool filters out of `list-ports`. Intended for diagnosing "my device does not show up" reports across operating systems — attach the output to the issue.

Like `completions`, it prints nothing but JSON: no banner, and no log file is created.

```json
[
  {
    "portPath": "COM6",
    "portType": "UsbPort",
    "wouldListInTyutool": true,
    "usb": {
      "vid": 6790,
      "pid": 21970,
      "vidPidHex": "1a86:55d2",
      "manufacturer": "wch.cn",
      "product": "USB-Enhanced-SERIAL-B CH342 (COM6)",
      "serialNumber": "56D7035114",
      "usbInterface": 2
    }
  }
]
```

`wouldListInTyutool` tells you whether the port survives the filter `list-ports` applies; `usb` is absent for non-USB ports.

```bash
tyutool usb-port-survey > survey.json
tyutool usb-port-survey | jq '.[] | select(.wouldListInTyutool == false)'
```

---

## Device and baud table

| `--device` value | Chip | `write`/`read`/`erase` default baud | `monitor` default baud | Flash size |
|-----------------|------|------------------------------------|------------------------|------------|
| `bk7231n` | BK7231N | 921600 | 115200 | 2 MiB (`0x200000`) |
| `t2` | T2 | 921600 | 115200 | 2 MiB (`0x200000`) |
| `t3` | T3 | 921600 | **460800** | 4 MiB (`0x400000`) |
| `t1` | T1 | 921600 | 115200 | 8 MiB (`0x800000`) |
| `t5ai` (alias `t5`) | T5AI | 921600 | **460800** | 8 MiB (`0x800000`) |
| `ln882h` | LN882H | 115200 | 115200 | 2 MiB (`0x200000`) |
| `esp32` | ESP32 | 460800 | 115200 | 4 MiB (`0x400000`) |
| `esp32c3` | ESP32-C3 | 460800 | 115200 | 4 MiB (`0x400000`) |
| `esp32c6` | ESP32-C6 | 460800 | 115200 | 8 MiB (`0x800000`) |
| `esp32p4` | ESP32-P4 | 460800 | 115200 | 16 MiB (`0x1000000`) |
| `esp32s3` | ESP32-S3 | 460800 | 115200 | 16 MiB (`0x1000000`) |

Values are case-insensitive. Every column matches the GUI's per-chip manifest (`src/features/firmware-flash/chip-manifests.ts`) — `defaultBaudRate`, `defaultLogBaudRate`, and `flashSize` respectively. `-b` overrides either baud.

The flash size is what bounds a sensible `-s`/`-l`; the CLI does not clamp them for you. That file is also where each chip's erase presets live, which is the quickest way to look up where the auth/KV and RF-calibration regions sit on a given part.

## Output modes

**Rich mode** — stderr is an interactive terminal and `--plain` was not passed: spinner, redrawn progress bar, ANSI color, `✓`/`✗` marks.

**Plain mode** — stderr is not a terminal (CI, pipe, redirect), or `--plain` was passed: fixed-width phase labels, 10 %-step percent ticks on long phases, ASCII-only separators, one append-only line per phase.

```
tyutool v3.2.8  linux/x86_64

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

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | Success. Also: `--help`/`--version`; `update --check` whether or not a newer version exists; `monitor` quitting on Ctrl+]/Ctrl+C **or** on a device disconnect. |
| non-zero | Anything else — bad arguments, no/ambiguous port, device or protocol failure, checksum mismatch, or a cancelled job. The reason is printed to stderr. |

**Cancellation.** During `write`, `read`, `erase`, and `authorize`, Ctrl+C sets a cancellation flag instead of killing the process: the current operation unwinds, the serial port is closed, `Cancelled` is reported, and the exit code is non-zero. A long erase or write may take a moment to notice the flag. For `monitor`, Ctrl+C is the normal way to quit and exits 0.

## Common errors

| Message | Cause and fix |
|---------|---------------|
| `No serial ports found.` | Nothing detected. Check the cable, the driver (CH34x/CP210x/FTDI), and on Linux your access to `/dev/ttyUSB*` (`dialout` group). |
| `multiple serial ports found; specify one with -p/--port (e.g. -p COM6)` | Several ports and no terminal to prompt on. Pass `-p`; `tyutool list-ports` shows the candidates, and `port_role` hints which is the flash port. |
| `error: a value is required for '--log <LOG>' but none was supplied` | `-l`/`--log` was given without a path. Every value-taking option needs one; there are no bare-flag defaults. |
| `error: invalid value 'x' for '--device <DEVICE>'` | Unsupported chip name. The message lists every accepted value; see the [device table](#device-and-baud-table). |
| `invalid hex address '2097152'`, or a wildly wrong range | Values are hex, not decimal. `-l 0x200000` is 2 MiB; `-l 2097152` is `0x2097152` ≈ 34 MiB. |
| `authorize: provide both --uuid and --authkey to write, or neither to read` | Exactly one half of the pair was given. Supply both to write, or drop both to read. |
| `cannot open log file '<path>': …` | `monitor -l` could not create or append to the path. Check the directory exists and is writable. |
| `--- Monitor stopped: serial port <p> disconnected or unavailable. ---` | The adapter went away mid-session (unplugged, or the device reset the USB link). Not a failure — exit code is 0. |
| Unreadable garbage in `monitor` | Wrong baud. The monitor defaults are 460800 for `t5ai` and 115200 elsewhere — different from the flash defaults. Try `-b 921600`. |
| `All update sources failed. Check your network connection.` | Neither manifest could be fetched. On a mainland-China network try `--source tuya`. |
| `SHA256 checksum mismatch! Download may be corrupted.` | The download did not match the manifest; nothing was replaced. Retry, or fetch the archive from Releases by hand. |
| `Unknown source 'x'. Use 'github' or 'tuya'.` | `--source` accepts only those two values. |
| `Unsupported platform for auto-update.` | Your OS/arch is not one of the five released targets. Install by hand. |
| `failed to bind 127.0.0.1:9527` | Another process holds the port (often an older `serve`). Kill it, or run `serve --port <other>` and point the client at the same port. |
