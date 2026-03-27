# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

tyuTool (Tuya Uart Tool) is a cross-platform serial utility for IoT developers. It supports firmware flashing (write) and reading across multiple chip platforms (BK7231N, T5, ESP32, LN882H, RTL8720CF, etc.), with both a PySide6 GUI and a Click-based CLI. Releases ship as standalone PyInstaller binaries.

## Development Environment Setup

```bash
# Linux/macOS — must source, not execute
. ./export.sh

# Windows
.\export.bat
```

This creates a `.venv`, activates it, and installs `requirements.txt`. Requires Python 3.8+ (script accepts >= 3.6).

## Common Commands

```bash
# Run GUI
python tyutool_gui.py

# Run CLI
python tyutool_cli.py --help
python tyutool_cli.py write -d BK7231N -p /dev/ttyACM0 -b 921600 -f firmware.bin
python tyutool_cli.py read -d BK7231N -p /dev/ttyACM0 -b 921600 -s 0x11000 -l 0x200000 -f read.bin

# Build standalone executables (outputs to dist/)
./tools/build_package.sh

# UI development
pyside6-designer ./tyutool/gui/ui_main.ui
pyside6-uic ./tyutool/gui/ui_main.ui -o ./tyutool/gui/ui_main.py

# Update logo
python ./tools/logo2bytes.py
```

There is no formal test suite (pytest). The `tests/` directory contains helper scripts and sample `.bin` files for manual serial/ESP testing.

## Architecture

### Entry Points
- `tyutool_gui.py` → `tyutool.gui.gui()` → `tyutool/gui/main.py` (PySide6 window)
- `tyutool_cli.py` → `tyutool.cli.cli()` → `tyutool/cli/main.py` (Click multi-command)

### Key Layers
- **CLI** (`tyutool/cli/`): Click command groups registered via `CLIS` dict in `main.py`. Subcommands: `write`, `read`, `monitor`, `upgrade`, `debug`, `audio`.
- **GUI** (`tyutool/gui/`): Main window `MyWidget` mixes in `FlashGUI`, `SerialGUI`, `SerDebugGUI`, `WebDebugGUI`. Long operations use `QThread` + `Signal`. UI file is `ui_main.ui` → compiled to `ui_main.py` via `pyside6-uic`. Do not hand-edit `ui_main.py`.
- **Flash** (`tyutool/flash/`): Strategy pattern — `FlashHandler` base class in `flash_base.py`; per-chip implementations in subpackages (e.g., `bk7231n/`, `t5/`, `esp/`). `FlashInterface` in `flash_interface.py` maps uppercase SoC names to handlers/metadata.
- **AI Debug** (`tyutool/ai_debug/`): WebSocket-based debug protocol for audio/video/text/image data collection. Includes vendored PyOgg library at `web/libs/PyOgg/`.
- **Utilities** (`tyutool/util/`): Logging (`set_logger`), version constant (`TYUTOOL_VERSION`), online upgrade logic, shared helpers.

### Key Design Patterns
- **Chip registry**: `FlashInterface.SocList` dict maps uppercase SoC names → `{handler, baudrate, modules, ...}`. Adding a chip means implementing `FlashHandler` and registering here.
- **Progress abstraction**: Pluggable `ProgressHandler` so CLI (tqdm) and GUI share the same flash core.
- **Lazy imports**: `gui()` / `cli()` import their `main` module inside the function body.

## Conventions

- **File headers**: `#!/usr/bin/env python` and `# -*- coding: utf-8 -*-`
- **Naming**: modules use `snake_case`, classes use `PascalCase`, SoC keys are UPPERCASE, constants are `UPPER_SNAKE_CASE`
- **Version**: `TYUTOOL_VERSION` in `tyutool/util/util.py` — must align with release tags
- **Dependencies**: When adding deps, update `requirements.txt` and consider PyInstaller hidden-import needs
- **This is a Python-only project** — no Node/TypeScript toolchain
