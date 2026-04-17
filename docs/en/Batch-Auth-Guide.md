
# Batch Auth User Guide

This document describes the **Batch Auth** tab in the tyuTool GUI: writing TuyaOpen authorization over serial and maintaining the license Excel workbook (`.xlsx`).

- **Terms**: Read the project root [Terms_And_Agreements.md](../../Terms_And_Agreements.md) before use.
- **Chinese version**: [Batch_Auth使用指南.md](../zh/Batch_Auth使用指南.md).

---

## Before You Start

Before you start authorization or use the license file in a batch task, confirm:

| Topic | Details |
| --- | --- |
| **Back up the authorization code list Excel sheet** | **Back up the `.xlsx` yourself** before you click Start or load the sheet for batch work (separate folder, version control, team storage, etc.). A successful run **directly modifies** rows (status, MAC, timestamp, etc.). The app may create `<filename>.xlsx.bak` in the same folder or copy once when you first confirm, but **that does not replace** a backup you control. **Verify your backup is recoverable before production use.** |
| **Unique MACs** | The tool correlates rows using the **MAC** reported by each module; it **does not verify** that MACs are unique across devices. You must ensure—at manufacturing and material level—that every module has a **unique MAC** within your operational scope, or authorization records may be wrong or ambiguous. |
| **Firmware and commands** | The device must enable **TAL CLI** and register **`auth`**, **`auth-read`**, and **`read_mac`** (see **Firmware and protocol requirements** below). |

---

## Feature overview

- **Main UI**: The **Batch Auth** page runs with a background worker thread (`AuthWorker`); after **Start**, authorization and optional flashing run on a worker thread so the UI stays responsive.
- **Core modules**: **AuthHandler** drives serial I/O, device interaction, authorization, and Excel updates; **AuthExcelParser** reads and writes the `.xlsx` file.
- **Optional flash first**: If you also specify chip and firmware, the worker flashes and resets first, then follows the same serial authorization path as “authorization only.”

---

## User workflow

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

**Port and baud rate**

- **Port**: Click **Rescan**, then select the device serial port.
- **Baud**: For ESP32, use `115200`.

**Select the license file**

- Use **Browse** to pick the `.xlsx`. After load, **Total / Used / Remain** are shown.
- The UI warns that the workbook **will be modified**—**confirm your own backup**. After confirmation, if no `.bak` exists yet in that folder, the app may create **`<filename>.xlsx.bak`**.

**(Optional) Flash then authorize**

- **Chip**: **ESP32**; **Firmware**: `.bin`. If you omit firmware, there is no flash or DTR/RTS reset—serial authorization only.

**Start**

- **Start** is enabled when **Port** and **Excel** are set. Steps and progress are shown while running.

**Read MAC only**

- Set **Port** and **Baud**, then click **Read MAC**: reads MAC only; no authorization write and no Excel change.

### 4. Session log

- Each authorization session creates a log **in the same directory as the license Excel**, named like `workbookname_auth_timestamp.log`; the UI can copy the full path.
- **After each run, archive the relevant logs separately** (e.g. by date or batch) for traceability and troubleshooting.

---

## Firmware and protocol requirements

### Serial CLI: `auth`, `auth-read`, `read_mac`

Batch Auth talks to the device over **serial** using the **TAL CLI**. Your firmware must **register and expose three commands**: **`auth`**, **`auth-read`**, and **`read_mac`**, with names and behavior matching tyuTool’s built-in protocol. If they are missing or not equivalent, **Start** and **Read MAC** cannot complete.

In the public **TuyaOpen** repository, use the following paths (relative to the repo root) as a reference:

- **`apps/tuya_cloud/switch_demo/src/tuya_main.c`**: In `user_main`, after `tal_kv_init`, call **`tal_cli_init`**, then **`tuya_authorize_init`**, then **`tuya_app_cli_init`** to bring up the CLI, register authorization subcommands, and register app-level commands. On Linux desktop or other non-embedded targets, some initialization may be conditionally compiled out—follow your actual project.
- **`src/tuya_cloud_service/authorize/tuya_authorize.c`**: **`tuya_authorize_init`** registers **`auth`** (writes UUID and AuthKey to KV at fixed lengths), **`auth-read`** (reads current auth from KV), **`auth-reset`** (clears KV auth when needed), and related commands. KV keys include **`UUID_TUYAOPEN`** and **`AUTHKEY_TUYAOPEN`**. Batch Auth relies primarily on **`auth`** and **`auth-read`** staying aligned with Excel and device state.
- **`apps/tuya_cloud/switch_demo/src/cli_cmd.c`**: **`tuya_app_cli_init`** registers **`read_mac`** and other commands on **`s_cli_cmd`**. In the reference, **`read_mac`** may be built only with Wi‑Fi-related macros; if your chip or network stack differs, you must still provide a **`read_mac`** implementation compatible with the protocol (e.g. read MAC from the local stack and print it) so the PC parses the expected MAC format.

Wire these into your application startup; flashing firmware without enabling the CLI and these three commands will still make Batch Auth fail.

### Time-limited CLI (implement in firmware yourself)

If you need these CLI commands (or the whole serial debug / authorization path) to **stop working after some time**, **you must implement that in firmware**, for example: **stop registering the relevant commands after a power-on timer or cumulative runtime threshold**, or **do not call `tal_cli_init` / authorize module init in release images**. **tyuTool does not provide “auto-expire CLI” behavior.**

### KV keys and data security (confidentiality)

If you do **not** want **KV storage** on the module (including authorization-related data) to be recoverable by third parties who know the same public, default, or shared key material, change **`tal_kv_init`** in **device firmware**: set **`seed`** and **`key`** to **random strings only your organization controls**, store them safely, and **do not leak** them. KV data is typically **derived or encrypted using these `seed` / `key` values**; if the keys leak, sensitive KV content (including authorization data) may be at risk.

Example (structure only—**replace with values you generate and keep private**; do not ship long-term with placeholders, samples, or well-known SDK defaults):

```c
tal_kv_init(&(tal_kv_cfg_t){
    .seed = "vmlkasdh93dlvlcy",
    .key  = "dflfuap134ddlduq",
});
```

Keep real keys out of public repos, logs, and distributed docs in production and release processes.

---

## How it works (implementation summary)

This section explains how the GUI, worker thread, serial protocol, and Excel file work together—useful for interpreting behavior and logs. End users do not need to read the source code.

### Workbook handling

- On start, the workbook is loaded and headers are resolved (**UUID**, **AUTHKEY** or **key**, etc.). Missing **STATUS / MAC / TIMESTAMP** columns are **appended** when a save to disk is needed.
- A **file lock** (`.lock`) prevents concurrent writers from different instances. Stats show **total / used / remaining** unused rows.
- After a successful write and verify on the device, the row is marked **USED** with **MAC** and **timestamp**, then saved. A **`.bak`** copy may be created before the first save if none exists.

### Serial path and optional flash

- **No firmware selected**: open serial → drain boot traffic so the device is interactive → read MAC and authorize.
- **Firmware selected (ESP32 today)**: verify port → run the existing **flash** flow (handshake, erase, write, verify, reboot) → **DTR/RTS** reset → open serial again at the auth baud rate → read MAC, authorize, and verify.

### Per-device authorization (summary)

The flow assumes **one unique MAC per physical device**. If two modules share the same MAC, matching and writing by MAC cannot distinguish them.

1. **Read MAC** (retries up to a limit).
2. **Read current authorization** from the device: if the device is placeholder or not authorized, allocate from the sheet; if already authorized, compare with the **Excel row matched by MAC**.
3. **Decide**: if it matches the sheet, skip writing; if it conflicts or the MAC is missing from the sheet, a dialog may ask to **overwrite** (**Copy** is available for the message).
4. **Write and verify**: take the **next unused** UUID/AUTHKEY row (or reuse a row), **write** via protocol, **read back** and compare; only then **update Excel** to mark the row used.

### Step list in the UI

On-screen steps (open serial, flash, reset, read MAC, write auth, verify, close serial, etc.) map to the phases above and show progress and where failure occurred.

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
