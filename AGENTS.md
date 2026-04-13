# AGENTS.md — tyuTool

This document is for AI coding agents and contributors. It summarizes **tyuTool** purpose, architecture, tech stack, directory layout, coding conventions, and behavioral guidelines. The project is **Python**-based and does **not** include `package.json`, `tsconfig.json`, or other Node/TypeScript config; dependencies and versions follow the root `requirements.txt` and the "Tech stack" section below. `CLAUDE.md` imports this file via `@AGENTS.md`.

---

## Overview

**tyuTool (Tuya Uart Tool)** is a cross-platform serial utility for IoT developers. It supports **firmware flashing (write to Flash)** and **firmware reading** across multiple chip platforms, with both a **GUI** and a **CLI**. Releases ship as standalone binaries (PyInstaller) so end users do not need Python installed.

---

## Architecture

- **Entry layer**
  - `tyutool_gui.py` → `tyutool.gui.gui()` → `tyutool/gui/main.py`: PySide6 main window composing feature panels (flash, serial, debug, etc.).
  - `tyutool_cli.py` → `tyutool.cli.cli()` → `tyutool/cli/main.py`: Click multi-command CLI (`write` / `read` / `monitor` / `upgrade` / `debug` / `audio`, etc.).

- **Logical layers**
  - **CLI** (`tyutool/cli/`): argument parsing; integrates with `flash`, `audio`, `ai_debug`, and related modules.
  - **GUI** (`tyutool/gui/`): Qt UI; `ui_main.ui` is edited in Designer and compiled to `ui_main.py` via `pyside6-uic`.
  - **Flash abstraction** (`tyutool/flash/flash_base.py`): defines `FlashArgv`, `FlashHandler`, `ProgressHandler`; per-chip protocols live in subpackages.
  - **Chip registry** (`tyutool/flash/flash_interface.py`): `FlashInterface.SocList` maps **uppercase SoC names** to handlers, baud rates, module docs, images, and other metadata.
  - **Utilities & upgrade** (`tyutool/util/`): logging, version string, online upgrade, shared helpers.

- **Build & CI**
  - `tools/build_package.sh`: PyInstaller one-file CLI/GUI; copies `resource`, PyOgg native libs, etc. into `dist/`.
  - `.github/workflows/release.yml`: tag validation, multi-platform builds, and release-related steps on tag push.

---

## Tech stack (with versions)

### Runtime & language

| Item | Notes |
|------|--------|
| **Python** | README asks for **3.8+**; `export.sh` accepts **≥ 3.6**; GitHub Actions **release** uses **3.12**. The `pip freeze` snapshot below was taken on **Python 3.12.3**. |
| **App version** | Source constant `TYUTOOL_VERSION` in `tyutool/util/util.py` (must align with release tags; see `tools/develop.md`). |

### Direct dependencies (`requirements.txt`)

Root file lists: `click`, `pyinstaller` (pinned `<=6.15.0`), `pyserial`, `PyYAML`, `requests`, `rich`, `tqdm`, `PySide6`, `numpy`, `scipy`, `matplotlib`, `pygame`.

### Resolved snapshot (for environment alignment)

Produced on **2026-03-25** with `pip install -r requirements.txt` then `pip freeze` on **Python 3.12.3**. Unpinned transitive deps may change; treat your local/CI `pip freeze` as authoritative.

```
altgraph==0.17.5
certifi==2026.2.25
charset-normalizer==3.4.6
click==8.3.1
contourpy==1.3.3
cycler==0.12.1
fonttools==4.62.1
idna==3.11
kiwisolver==1.5.0
markdown-it-py==4.0.0
matplotlib==3.10.8
mdurl==0.1.2
numpy==2.4.3
packaging==26.0
pillow==12.1.1
pygame==2.6.1
Pygments==2.19.2
pyinstaller==6.15.0
pyinstaller-hooks-contrib==2026.3
pyparsing==3.3.2
pyserial==3.5
PySide6==6.11.0
PySide6_Addons==6.11.0
PySide6_Essentials==6.11.0
python-dateutil==2.9.0.post0
PyYAML==6.0.3
requests==2.32.5
rich==14.3.3
scipy==1.17.1
setuptools==82.0.1
shiboken6==6.11.0
six==1.17.0
tqdm==4.67.3
urllib3==2.6.3
```

### Other tooling

- **Qt UI**: PySide6; Designer / `pyside6-uic` workflow in `tools/develop.md`.
- **Packaging**: PyInstaller (`-F` one-file, `--windowed` for GUI in `tools/build_package.sh`).
- **AI debug / audio**: `tyutool/ai_debug/` includes WebSocket-related code and a **vendored PyOgg** tree (`tyutool/ai_debug/web/libs/PyOgg/`, with its own `setup.py` and tests—not an npm package).

---

## Directory layout

```
tyutool/                    # repository root
├── tyutool_gui.py          # GUI entry
├── tyutool_cli.py          # CLI entry
├── requirements.txt        # Python dependencies
├── export.sh / export.bat  # create venv, activate, and pip install (see "Development environment setup")
├── README.md / README_zh.md
├── AGENTS.md               # this file
├── CLAUDE.md               # imports AGENTS.md via @AGENTS.md
├── .github/workflows/      # GitHub Actions (release, sync-to-gitee, etc.)
├── docs/                   # docs and images (zh/en)
├── resource/               # icons, assets bundled with PyInstaller
├── tools/                  # build scripts, tag check, IDE JSON, logo helpers
├── tests/                  # helper serial/ESP scripts and sample .bin (not a full pytest suite)
├── cache/                  # runtime/tool cache (environment-dependent)
├── dist/ / build/         # build output and PyInstaller work dirs (often gitignored)
└── tyutool/                # main Python package
    ├── cli/                # Click CLI: flash, monitor, upgrade, audio, ai_debug, …
    ├── gui/                # PySide6: main, ui_main/ui_logo, feature sub-views
    ├── flash/              # per-chip flash + esptool pieces + flash_interface
    ├── ai_debug/           # AI/serial debug, web protocol, vendored PyOgg
    ├── audio/              # audio-related CLI support
    └── util/               # logging, version, upgrade, shared utils
```

---

## Development environment setup

Use the **export** scripts at the project root to create and activate the virtual environment. Choose the script matching your terminal:

| Terminal | Command |
|----------|---------|
| **Bash / Zsh** (Linux / macOS) | `. ./export.sh` |
| **Windows CMD** | `export.bat` |

The script will:
1. Locate the project root and verify key files exist.
2. Find a suitable Python (≥ 3.6) — on Unix it prefers `python3`, falls back to `python`.
3. Create a `.venv` virtual environment if it does not already exist.
4. Activate the virtual environment and add the project root to `PATH`.
5. Install all dependencies from `requirements.txt` via `pip`.

To deactivate: run `deactivate` (Bash/Zsh) or `exit` (Windows CMD).

> **Note:** Always source the script (`. ./export.sh`), do not execute it (`bash export.sh`), otherwise the virtual environment will not persist in your current shell.

---

## Conventions

### File header & encoding

- Common pattern: `#!/usr/bin/env python` and `# -*- coding: utf-8 -*-` (match surrounding files).

### Naming

- **Modules / files**: lowercase with underscores, e.g. `flash_write.py`, `t5_flash.py`.
- **Classes**: project types such as `FlashHandler`, `FlashArgv` use PascalCase; Qt/uic-generated code follows PySide conventions.
- **SoC / device keys**: in `flash_interface.SocList`, names are **uppercase** (e.g. `BK7231N`, `ESP32`), per existing comments.
- **Constants**: `TYUTOOL_VERSION`, `TYUTOOL_ROOT`, etc. use UPPER_SNAKE_CASE (see `util.py`).

### CLI & GUI

- **CLI**: Click command groups; subcommands registered via the `CLIS` dict in `tyutool/cli/main.py`.
- **GUI**: main window **mixes in** `FlashGUI`, `SerialGUI`, `SerDebugGUI`, `WebDebugGUI`; long work uses `QThread` + `Signal` (see `gui/main.py`).

### Adding a flash protocol (new chip)

- Follow `tools/develop.md`: copy `tyutool/flash/xxxxx`, implement `xxxxx_flash.py`, register the **handler** and metadata (`baudrate`, `modules`, …) in `flash_interface.py`.

### Patterns used in the codebase

- **Strategy / template**: each chip implements `FlashHandler`; `FlashInterface` selects by device name.
- **Progress abstraction**: pluggable `ProgressHandler` so CLI (e.g. `tqdm`) and GUI share the same flash core.
- **Lazy import**: `gui()` / `cli()` import `main` inside the function to keep import paths short at startup.

### Logging

- Standard library `logging`; `set_logger` sets format; DEBUG adds file name and line number (see `util.py`).

### UI workflow

- Edit `tyutool/gui/ui_main.ui`, then run `pyside6-uic` to regenerate `ui_main.py` (commands in `tools/develop.md`). Avoid hand-editing generated files unless the team agrees.

---

## AI behavioral guidelines

These principles reduce common LLM coding mistakes. They apply to all AI agents working on this codebase.

### 1. Think before coding

- State assumptions explicitly. If uncertain about the user's intent, ask before implementing.
- If multiple interpretations exist, present them — do not pick silently.
- If a simpler approach exists than what was requested, say so. Push back when warranted.
- When a task touches multiple files, state a brief plan before making changes.

### 2. Simplicity first

- No features beyond what was asked.
- No abstractions for single-use code.
- No speculative "flexibility" or "configurability" that was not requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

The test: would a senior engineer say this is overcomplicated? If yes, simplify.

### 3. Surgical changes

When editing existing code:

- Do not "improve" adjacent code, comments, or formatting.
- Do not refactor things that are not broken.
- Match existing style, even if you would do it differently.
- If you notice unrelated dead code or issues, mention them — do not fix them silently.

When your changes create orphans:

- Remove imports, variables, or functions that YOUR changes made unused.
- Do not remove pre-existing dead code unless asked.

The test: every changed line should trace directly to the user's request.

### 4. Goal-driven execution

Transform tasks into verifiable goals:

- "Add validation" → write tests for invalid inputs, then make them pass.
- "Fix the bug" → write a test that reproduces it, then make it pass.
- "Refactor X" → ensure tests pass before and after.

For multi-step tasks, state a brief plan:

```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

> **Note:** There is no formal test suite (pytest) in this project. The `tests/` directory contains helper scripts and sample `.bin` files for manual serial/ESP testing. Verify changes by running the CLI/GUI and checking output.

### Change checklist

Before submitting changes, verify:

| Check | Details |
|-------|---------|
| **Version alignment** | If bumping version, align `TYUTOOL_VERSION` in `tyutool/util/util.py`, `tools/check_tag.py`, and tag naming rules |
| **Generated files** | Never hand-edit `ui_main.py`; edit `ui_main.ui` in Designer, then run `pyside6-uic` |
| **SoC naming** | SoC keys in `FlashInterface.SocList` must be **UPPERCASE** (e.g. `BK7231N`, not `bk7231n`) |
| **Dependencies** | When adding deps, update `requirements.txt` and check PyInstaller hidden-import needs |
| **No Node/TS** | This is a Python-only project — do not introduce Node/TypeScript tooling |

---

## Common pitfalls

- **Version bumps**: before releases, align `TYUTOOL_VERSION`, `tools/check_tag.py`, and tag naming rules. Mismatches break the CI release workflow.
- **Adding dependencies**: update `requirements.txt` and check whether PyInstaller needs `--hidden-import` for the new package. Large deps also increase binary size.
- **Python-only repo**: do not assume a Node/TypeScript toolchain exists. There is no `package.json`, `tsconfig.json`, or npm.
- **GUI changes**: always edit `tyutool/gui/ui_main.ui` in Qt Designer, then regenerate `ui_main.py` via `pyside6-uic ./tyutool/gui/ui_main.ui -o ./tyutool/gui/ui_main.py`. Never hand-edit `ui_main.py`.
- **Adding a new chip**: follow the full procedure in `tools/develop.md` — create a subpackage under `tyutool/flash/`, implement `FlashHandler`, and register in `flash_interface.py` with uppercase SoC key.
- **Sensitive data in logs**: do not log complete auth keys, tokens, or user privacy data. Use partial masking (e.g. first 8 chars + `****`).
