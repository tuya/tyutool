
# Batch Auth User Guide

This document describes the **Batch Auth** tab in the tyuTool GUI: writing TuyaOpen authorization over serial and maintaining the license Excel workbook (`.xlsx`).

- **Terms**: Read the project root [Terms_And_Agreements.md](../../Terms_And_Agreements.md) before use.
- **Chinese version**: [Batch_Auth使用指南.md](../zh/Batch_Auth使用指南.md).
- **Firmware / protocol (English)**: [Batch-Auth-Firmware-and-Protocol-Reference.md](./Batch-Auth-Firmware-and-Protocol-Reference.md) — [中文版](../zh/Batch_Auth固件与协议参考.md).

---

## Before You Start

Before you start authorization or load a license sheet for a batch task, confirm:

| Topic | Details |
| --- | --- |
| **Back up the authorization code list Excel sheet** | **Back up the `.xlsx` yourself** before you click **Start** or load the sheet for batch work (separate folder, version control, team storage, etc.). A successful run **directly modifies** rows (status, MAC, timestamp, etc.). The app may create `<filename>.xlsx.bak` in the same folder or copy once when you first confirm, but **that does not replace** a backup you control. **Verify your backup is recoverable before production starts.** |
| **Unique MACs** | The tool correlates rows using the **MAC** reported by each module; it **does not verify** that MACs are unique across devices. You must ensure—at manufacturing and material level—that every module has a **unique MAC** within your operational scope, or authorization records may be wrong or ambiguous. |
| **Firmware and commands** | The device must enable **TAL CLI** and register **`auth`**, **`auth-read`**, and **`read_mac`** (see [Batch Auth — Firmware and Protocol Reference](./Batch-Auth-Firmware-and-Protocol-Reference.md)). |

---

## Feature overview

- **Main UI**: The **Batch Auth** page runs with a background worker thread (`AuthWorker`); after **Start**, authorization and optional flashing run on a worker thread so the UI stays responsive.
- **Core modules**: **AuthHandler** drives serial I/O, device interaction, authorization, and Excel updates; **AuthExcelParser** reads and writes the `.xlsx` file.
- **Optional flash first**: If you also specify chip and firmware, the worker flashes and resets first, then follows the same serial authorization path as “authorization only.”

---

## Procedure

### 1. Environment

1. Run the **tyuTool GUI** (`tyutool_gui.py` or the packaged binary).
2. USB serial adapter connected with working drivers (on Linux, often `/dev/ttyUSB*`, `/dev/ttyACM*`, etc.).
3. Have a **license `.xlsx`** from your process, and satisfy **Back up the authorization code list Excel sheet** in the table above.
4. Excel handling requires **openpyxl** (see `requirements.txt`).

### 2. Excel format

- **Row 1 is the header**. Column names are matched case-insensitively and aliases are supported:
  - **Required**: `UUID`; `AUTHKEY` or **`key`** (both name the authorization key column).
  - **Optional**: If `STATUS`, `MAC`, and `TIMESTAMP` already exist, they are used; if not, the app **appends** them the first time a save is needed.
- **Stats**: From row 2, each non-empty **UUID** counts as one row; `STATUS` equal to `USED` (case-insensitive) counts as used.
- **Allocation**: The **first** row from the top where UUID and AUTHKEY are both set and the row is **not** marked USED is used for the next write.

### 3. UI operations

![tyuTool GUI — Batch Auth](https://images.tuyacn.com/fe-static/docs/img/390a3c27-f6aa-4369-9453-494a60e17ac7.png)

Click **①** to open the **Batch Auth** screen.

**Serial port and baud rate**

- **Port**: After **② Rescan**, click **③** to select the device serial port.
- **Baud**: For ESP32, use `115200`.

**(Optional) Flash then authorize**

- **Chip**: Select **ESP32**, then click **④** to choose the firmware to flash, e.g. `xxx_QIO_x.x.x.bin`.
- If you do not select a firmware file, only serial authorization is performed.

**Select the license file**

- Use **⑤ Browse** to pick the `.xlsx`. After a successful load, **Total / Used / Remain** are shown.
- The UI warns that the workbook **will be modified**—**confirm your own backup**. After confirmation, if no `.bak` exists yet in that folder, the app may create **`<filename>.xlsx.bak`**.

**Start**

- **⑥ Start** is enabled when **Port** and **Excel** are set. Steps and progress are shown while running.

**Read MAC only**

- Set **Port** and **Baud**, then click **Read MAC**: reads MAC only; no authorization write and no Excel change.

### 4. Session log

- Each authorization session creates a log **in the same directory as the license Excel**, named like `workbookname_auth_timestamp.log`; the UI can copy the full path.
- **After each run, archive the relevant logs separately** (e.g. by date or batch) for traceability and troubleshooting.

---

## Firmware and protocol (for developers)

> **Required reading (firmware development)**: Before integrating or trimming TAL CLI, `auth` / `auth-read` / `read_mac`, KV, and related features, read [Batch Auth — Firmware and Protocol Reference](./Batch-Auth-Firmware-and-Protocol-Reference.md) in full. It covers TuyaOpen reference paths, `tal_kv` and key security, and how the tool interacts with Excel and serial. Behavior must match the PC-side protocol; otherwise batch authorization or **Read MAC** may fail.

---

## FAQ

| Symptom | What to do |
| --- | --- |
| **Remain is 0** | No free rows—add licenses or check that rows are not all USED. |
| **Read MAC fails** | Check wiring, baud, and that firmware is in an interactive state; confirm CLI is up and **`read_mac`** is available; flash firmware that supports the auth protocol if needed. |
| **Excel error** | Row 1 must include **UUID** and **AUTHKEY** (or **key**). |
| **Cannot lock / file in use** | Close other tyuTool instances using the same `.xlsx`; one `.lock` per file—a second instance may fail to lock. |

**Other notes**

- **Backup**: Your manual backup is authoritative; app-generated `.bak` is auxiliary only.
- **MAC**: You must ensure each module’s MAC is unique in scope; the tool does not detect cross-device MAC collisions.
- **Flash**: Chip list is **ESP32** for now; with no firmware selected, no flash is performed.
