
# Batch Auth User Guide

This document describes the **Batch Auth** tab in the tyuTool GUI: writing TuyaOpen authorization over serial and maintaining the license Excel workbook. Also read the project root [Terms_And_Agreements.md](../../Terms_And_Agreements.md).

---

## Important: Before You Start

**Before you click Start or use the license file in production, back up the authorization Excel (`.xlsx`) yourself** (e.g. copy to another folder, version control, or team-approved storage). A successful run **directly modifies** the workbook (status, MAC, timestamp, etc.). The app may create `filename.xlsx.bak` in the same folder or copy once when you first confirm, but **that does not replace** a backup you control. **Verify your backup before batch or production use.**

**The tool correlates rows using the MAC reported by each module; it does not verify that MACs are unique across devices.** **You must ensure—at manufacturing and material level—that every module has a unique MAC within your operational scope.** Duplicate MACs can cause wrong or ambiguous authorization records.

Chinese version: [Batch_Auth使用指南.md](../zh/Batch_Auth使用指南.md).

---

## Part 1: How Batch Auth Works

This section summarizes how the GUI, worker thread, serial protocol, and Excel file work together. End users do not need to read the source code.

### 1. Overall structure

- The **Batch Auth** page runs authorization (and optional flashing) in a **background worker thread** so the UI stays responsive.
- **AuthHandler** drives serial I/O, device authorization, and Excel updates; **AuthExcelParser** reads and writes the `.xlsx` file.

### 2. Device firmware CLI: `auth`, `auth-read`, `read_mac` (required)

Batch Auth talks to the device over **serial** using the **TAL CLI**. Your firmware must **register and expose three commands**: **`auth`**, **`auth-read`**, and **`read_mac`**, with names and behavior matching what tyuTool expects. If they are missing, **Start** and **Read MAC** will not work.

In the public **TuyaOpen** tree, use the following paths (relative to the repo root) as a reference:

- **`apps/tuya_cloud/switch_demo/src/tuya_main.c`**: In `user_main`, after `tal_kv_init`, call **`tal_cli_init`**, then **`tuya_authorize_init`**, then **`tuya_app_cli_init`** to bring up the CLI, register authorization commands, and register app-level commands. On Linux desktop targets, some of this may be `#if`-ed out—follow your platform.
- **`src/tuya_cloud_service/authorize/tuya_authorize.c`**: **`tuya_authorize_init`** registers **`auth`** (writes UUID and AuthKey to KV at fixed lengths), **`auth-read`** (reads current auth from KV), and **`auth-reset`** (clears KV auth when needed). KV keys include **`UUID_TUYAOPEN`** and **`AUTHKEY_TUYAOPEN`**. Batch Auth relies primarily on **`auth`** and **`auth-read`** staying compatible with the PC tool.
- **`apps/tuya_cloud/switch_demo/src/cli_cmd.c`**: **`tuya_app_cli_init`** adds **`read_mac`** and other demo commands to **`s_cli_cmd`**. In the reference, **`read_mac`** may be built only with WiFi-related macros; if your chip or network stack differs, you must still provide a compatible **`read_mac`** implementation so the printed MAC matches what tyuTool parses.

**If you need these CLI commands (or the whole serial debug / auth path) to stop working after some time**, **you must implement that in firmware**. Typical approaches are limited to: **stop registering the relevant commands after a power-on timer or cumulative runtime threshold**, or **do not call `tal_cli_init` / authorize module init in release images**. **tyuTool does not provide “auto-expire CLI” behavior.**

Wire these into your application startup; flashing firmware without enabling the CLI and these three commands will still make Batch Auth fail.

### 3. Session log

- Each run creates a log file **next to the Excel file**, named like `workbookname_auth_timestamp.log`. The UI can copy the full path for troubleshooting.
- **After each session, keep and archive the relevant log files** (e.g. copy to a dated or batch folder) so you can debug issues later.

### 4. Excel handling

- On start, the workbook is loaded and headers are resolved (**UUID**, **AUTHKEY** or **key**, etc.). Missing **STATUS / MAC / TIMESTAMP** columns may be **appended** when the file is first saved.
- A **file lock** (`.lock`) prevents concurrent writers. Stats show **total / used / remaining** unused rows.
- After a successful write and verify, the row is marked **USED** with **MAC** and **timestamp**, then saved. A **`.bak`** copy may be created before the first save if none exists.

### 5. Serial path and optional flash

- **No firmware selected**: open serial → drain boot noise → read MAC and authorize.
- **Firmware selected (ESP32 today)**: verify port → run the existing **flash** flow (handshake, erase, write, verify, reboot) → **DTR/RTS** reset → same MAC read and authorization as above (serial is opened again at the auth baud rate).

### 6. Per-device authorization (summary)

The flow assumes **one unique MAC per physical device**. If two modules share the same MAC, matching and writing by MAC cannot distinguish them—**you must guarantee non-duplicate MACs in hardware / provisioning**.

1. **Read MAC** (retries up to a limit).
2. **Read current auth** from the device; treat placeholder UUID as “not authorized.”
3. **Decide**: if it matches the **Excel row for this MAC**, skip writing; if it conflicts or MAC is missing from the sheet, a dialog may ask to **overwrite** (**Copy** is available).
4. **Write and verify**: take the **next unused** row (or reuse a row), **write** via protocol, **read back** and compare; only then **update Excel**.

### 7. Step list in the UI

- On-screen steps (open serial, flash, reset, read MAC, write auth, verify, close serial) map to the phases above.

---

## Part 2: User Guide

### 1. Prerequisites

1. Run the **tyuTool GUI** (`tyutool_gui.py` or the packaged binary).
2. USB serial adapter connected with working drivers (on Linux, often `/dev/ttyUSB*`, `/dev/ttyACM*`, etc.).
3. A **license `.xlsx`** from your process, with **your own backup** as described above.
4. **Unique MACs per module** in your process—no duplicate MACs across devices (see “Important” above).
5. **TAL CLI enabled on the device with `auth`, `auth-read`, and `read_mac` registered** (see Part 1, section 2).
6. **openpyxl** for Excel (see `requirements.txt`).

### 2. Excel format

- **Row 1 is the header**. Names are matched case-insensitively:
  - **Required**: `UUID`; `AUTHKEY` or **`key`**.
  - **Optional**: `STATUS`, `MAC`, `TIMESTAMP`—created if missing when needed.
- **Stats**: From row 2, each non-empty **UUID** counts; `USED` (case-insensitive) counts as consumed.
- **Allocation**: The **first** row with UUID and AUTHKEY and **not** USED is used next.

### 3. UI workflow

#### 3.1 Port and baud rate

- **Port**: **Rescan**, then select the device.
- **Baud**: Often `115200`; also `230400`, `460800`, `921600` (must match the device).

#### 3.2 Select the license file

- **Browse** to the `.xlsx`.
- **Total / Used / Remain** appear after load.
- You will be warned that the file **will be modified**—**confirm your own backup** (see “Important” at the top). A **`original.xlsx.bak`** may be created if none exists.

#### 3.3 (Optional) Flash then authorize

- **Chip**: **ESP32**; **Firmware**: `.bin`. If omitted, no flash or DTR/RTS reset.

#### 3.4 Start

- **Start** is enabled when **Port** and **Excel** are set. Steps and progress are shown; log path is copyable (see Part 1, section 3 “Session log”).

#### 3.5 Read MAC only

- **Read MAC** reads MAC only; no authorization and no Excel change.

### 4. Firmware: KV keys and confidentiality

If you do **not** want **KV data** on the module (including but not limited to authorization-related content) to be recoverable by third parties who know the same public or default key material, you must change **`tal_kv_init`** in **device firmware**: set **`seed`** and **`key`** to **secrets only your organization controls**, **store them safely**, and **do not leak** them. Data in KV is typically **encrypted and decrypted using these parameters**; if the keys leak, sensitive KV content (including auth data) may be at risk.

Update initialization to use **your own** secrets, for example:

```c
tal_kv_init(&(tal_kv_cfg_t){
    .seed = "vmlkasdh93dlvlcy",
    .key  = "dflfuap134ddlduq",
});
```

**Note:** The snippet above only shows the **structure**—**replace `seed` and `key` with values you generate and keep private**; do not ship long-term with placeholders, samples, or well-known SDK defaults. Keep real keys out of public repos, logs, and distributed docs.

### 5. FAQ

- **Remain is 0**: No free rows—add licenses or check USED rows.
- **Read MAC fails**: Wiring, baud, firmware/CLI readiness; confirm the CLI is up and **`read_mac`** is available (see Part 1, section 2); flash firmware that supports the auth protocol if needed.
- **Excel error**: Row 1 must include **UUID** and **AUTHKEY** (or **key**).
- **Lock failure**: Close other tyuTool instances using the same `.xlsx`.

### 6. Other notes

| Topic | Details |
| --- | --- |
| Backup | **Your manual backup is authoritative**; `.bak` from the app is auxiliary only. |
| MAC uniqueness | **You must ensure** each module’s MAC is unique in scope; the tool does not detect cross-device MAC collisions. |
| KV and `tal_kv_init` | KV data (including auth) is tied to **`seed` / `key`** for crypto; use your own secrets and keep them confidential—see “Firmware: KV keys and confidentiality” above. |
| CLI and three commands | **`auth`**, **`auth-read`**, **`read_mac`** must be enabled—see Part 1, section 2. |
| Time-limited CLI | Unregister commands after a time threshold, or omit **`tal_cli_init`** / authorize init in release images—**you implement** (Part 1, section 2, closing paragraphs). |
| Concurrency | `.lock` per file; a second instance may fail to lock. |
| Flash | Chip list is **ESP32** for now; no firmware path means no flash. |
