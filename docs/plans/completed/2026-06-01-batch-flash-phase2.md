# Batch Auth Tool – Phase 2 (Batch Authorization) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add batch authorization to the existing batch flash page: read MAC from each device via UART, allocate an Excel row (UUID/AUTHKEY), write auth via `auth` CLI command, verify via `auth-read`, and mark the Excel row USED with MAC and timestamp.

**Architecture:** `tyutool-core::authorize` gets `read_mac` + `run_batch_auth_slot` (UART session that reads MAC, reads/writes auth, returns outcome + MAC). A new `ExcelRowAllocator` in `src-tauri/src/batch_auth.rs` owns the xlsx file (calamine for read, rust_xlsxwriter for write), allocates rows atomically, and saves on confirm. The Tauri `batch_auth_start` command pre-allocates rows then spawns per-port threads. The frontend store handles `batch-auth-progress` events and updates auth cumulative stats.

**Tech Stack:** Rust — calamine 0.26 (xlsx read), rust_xlsxwriter 0.79 (xlsx write), tyutool-core (UART), Tauri 2. Vue 3 / Pinia (frontend event handling).

---

## File Map

| File | Action | Purpose |
|---|---|---|
| `src-tauri/Cargo.toml` | Modify | Add calamine, rust_xlsxwriter |
| `crates/tyutool-core/src/authorize.rs` | Modify | Add `read_mac`, `run_batch_auth_slot`, `BatchAuthSlotResult` |
| `crates/tyutool-core/src/lib.rs` | Modify | Export new types/functions |
| `src-tauri/src/batch_auth.rs` | Create | `ExcelRowAllocator`, `ExcelStats`, `ExcelRow` |
| `src-tauri/src/lib.rs` | Modify | `BatchAuthState`, 4 new commands, exit cleanup |
| `src/features/batch-flash/types.ts` | Modify | Add `BatchAuthProgressEvent` |
| `src/features/batch-flash/store.ts` | Modify | `handleAuthProgress`, auth event listener |
| `src/features/batch-flash/components/BatchAuthConfig.vue` | Modify | Excel stats pills, validation feedback |
| `src/locales/zh-CN.json` | Modify | Auth-specific strings |
| `src/locales/en.json` | Modify | Auth-specific strings |

---

## Task 1: Add calamine + rust_xlsxwriter to Cargo.toml

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1.1: Read current Cargo.toml dependencies**

```bash
grep -n 'dependencies\|calamine\|xlsxwriter' src-tauri/Cargo.toml | head -20
```

- [ ] **Step 1.2: Add xlsx crates**

In `src-tauri/Cargo.toml`, inside `[dependencies]`, add:

```toml
calamine = { version = "0.26", features = ["dates"] }
rust_xlsxwriter = "0.79"
```

- [ ] **Step 1.3: Verify compilation**

```bash
cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | grep -E "^error" | head -10
```

Expected: no errors (new crates download and compile).

- [ ] **Step 1.4: Commit**

```bash
git add src-tauri/Cargo.toml Cargo.lock
git commit -m "feat(batch-auth): add calamine + rust_xlsxwriter dependencies"
```

---

## Task 2: Add read_mac + run_batch_auth_slot to tyutool-core

**Files:**
- Modify: `crates/tyutool-core/src/authorize.rs`
- Modify: `crates/tyutool-core/src/lib.rs`

- [ ] **Step 2.1: Add `read_mac` to AuthSession**

In `crates/tyutool-core/src/authorize.rs`, after `fn auth_write(...)` (around line 350), add:

```rust
/// Send `read_mac` and parse the MAC address from the response.
/// Returns `Some("XX:XX:XX:XX:XX:XX")` (uppercase colon-separated) or `None`.
fn read_mac(&mut self) -> Option<String> {
    self.send_cmd("read_mac").ok()?;
    let lines = self.read_response();
    for line in &lines {
        if let Some(mac) = parse_mac_from_str(line) {
            return Some(mac);
        }
    }
    None
}
```

After the `impl AuthSession` block, add the standalone helper:

```rust
fn parse_mac_from_str(s: &str) -> Option<String> {
    // Accept any whitespace-delimited token that looks like XX:XX:XX:XX:XX:XX
    s.split_whitespace().find_map(|token| {
        let parts: Vec<&str> = token.split(':').collect();
        if parts.len() == 6
            && parts.iter().all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
        {
            Some(token.to_uppercase())
        } else {
            None
        }
    })
}
```

- [ ] **Step 2.2: Add BatchAuthSlotResult + ConflictPolicy**

Before the `pub fn run_authorize` function (around line 350), add:

```rust
/// Outcome of a single batch-auth UART session.
#[derive(Debug)]
pub enum BatchAuthSlotResult {
    /// Auth written and verified successfully.
    Done { mac: String },
    /// Device was already authorized with the given uuid/authkey — nothing written.
    AlreadyDone { mac: String },
    /// Auth on device didn't match but conflict_policy=Skip — nothing written.
    Skipped { mac: String },
    /// Operation was cancelled.
    Cancelled,
}

/// What to do when device already has conflicting auth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    Skip,
    Overwrite,
}

/// Per-step progress marker emitted during a batch auth slot.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchAuthStep {
    ReadingMac,
    ReadingAuth,
    WritingAuth,
    Verifying,
}
```

- [ ] **Step 2.3: Add run_batch_auth_slot**

After `pub fn run_authorize(...)`, add:

```rust
/// Single-device batch authorization slot: open UART, read MAC, read/write auth, verify.
///
/// The caller pre-allocates the `uuid`/`authkey` from the Excel row. On return:
/// - `Done`/`AlreadyDone` → caller should confirm the Excel row (mark USED).
/// - `Skipped` → caller should release the Excel row.
/// - `Cancelled`/`Err` → caller should release the Excel row.
pub fn run_batch_auth_slot<F>(
    port: &str,
    uuid: &str,
    authkey: &str,
    conflict_policy: ConflictPolicy,
    cancel: &AtomicBool,
    progress: F,
) -> Result<BatchAuthSlotResult, FlashError>
where
    F: Fn(BatchAuthStep),
{
    macro_rules! check_cancel {
        () => {
            if cancel.load(Ordering::Relaxed) {
                return Ok(BatchAuthSlotResult::Cancelled);
            }
        };
    }

    let mut sess = AuthSession::open(port)?;
    check_cancel!();

    // Drain + reset + wait + wake (same as single-device authorize)
    sess.drain_boot_output();
    check_cancel!();
    sess.hardware_reset()?;
    check_cancel!();

    let wait_end = Instant::now() + POST_RESET_WAIT;
    while Instant::now() < wait_end {
        check_cancel!();
        std::thread::sleep(Duration::from_millis(200));
    }
    sess.drain_boot_output();
    check_cancel!();
    sess.wake_shell();
    check_cancel!();

    // Read MAC
    progress(BatchAuthStep::ReadingMac);
    let mac = {
        let mut mac_opt = None;
        for _ in 0..3u8 {
            check_cancel!();
            mac_opt = sess.read_mac();
            if mac_opt.is_some() { break; }
            std::thread::sleep(Duration::from_millis(500));
        }
        mac_opt.unwrap_or_else(|| "UNKNOWN".to_string())
    };

    // Read existing auth
    progress(BatchAuthStep::ReadingAuth);
    let existing_auth = {
        let mut auth = None;
        for _ in 0..3u8 {
            check_cancel!();
            auth = sess.auth_read();
            if auth.is_some() { break; }
            std::thread::sleep(Duration::from_millis(800));
        }
        auth
    };

    // Check if device already has the exact credentials we want to write
    if let Some((ref ex_uuid, ref ex_key)) = existing_auth {
        if ex_uuid == uuid && ex_key == authkey {
            return Ok(BatchAuthSlotResult::AlreadyDone { mac });
        }
        // Different auth on device
        if conflict_policy == ConflictPolicy::Skip {
            return Ok(BatchAuthSlotResult::Skipped { mac });
        }
        // Overwrite: fall through to write
    }

    // Write auth
    progress(BatchAuthStep::WritingAuth);
    let _lines = sess.auth_write(uuid, authkey);
    check_cancel!();

    // Wait for device to settle after possible reboot
    let wait_end = Instant::now() + POST_RESET_WAIT;
    while Instant::now() < wait_end {
        check_cancel!();
        std::thread::sleep(Duration::from_millis(200));
    }
    sess.drain_boot_output();
    sess.wake_shell();
    check_cancel!();

    // Verify
    progress(BatchAuthStep::Verifying);
    match sess.auth_read() {
        Some((rb_uuid, rb_key)) if rb_uuid == uuid && rb_key == authkey => {
            Ok(BatchAuthSlotResult::Done { mac })
        }
        Some((rb_uuid, rb_key)) => Err(FlashError::Plugin(format!(
            "Verification failed: wrote ({}, {}), read back ({}, {})",
            uuid, authkey, rb_uuid, rb_key
        ))),
        None => Err(FlashError::Plugin(
            "Verification failed: no response from auth-read".into(),
        )),
    }
}
```

- [ ] **Step 2.4: Export from lib.rs**

In `crates/tyutool-core/src/lib.rs`, add to the pub use block:

```rust
pub use authorize::{
    probe_device_authorization, DeviceAuthorization,
    run_batch_auth_slot, BatchAuthSlotResult, BatchAuthStep, ConflictPolicy,
};
```

- [ ] **Step 2.5: Build core to verify**

```bash
cargo build -p tyutool-core 2>&1 | tail -5
```

Expected: `Finished` with no errors.

- [ ] **Step 2.6: Add unit tests for parse_mac_from_str**

In `crates/tyutool-core/src/authorize.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
#[test]
fn parse_mac_detects_colon_format() {
    assert_eq!(
        parse_mac_from_str("WIFI MAC ADDR:11:22:33:AA:BB:CC"),
        Some("11:22:33:AA:BB:CC".to_string())
    );
}

#[test]
fn parse_mac_case_insensitive_input() {
    assert_eq!(
        parse_mac_from_str("mac: aa:bb:cc:dd:ee:ff"),
        Some("AA:BB:CC:DD:EE:FF".to_string())
    );
}

#[test]
fn parse_mac_returns_none_for_no_mac() {
    assert_eq!(parse_mac_from_str("no mac here"), None);
    assert_eq!(parse_mac_from_str(""), None);
}
```

- [ ] **Step 2.7: Run core tests**

```bash
cargo test -p tyutool-core 2>&1 | tail -10
```

Expected: all tests pass.

- [ ] **Step 2.8: Commit**

```bash
git add crates/tyutool-core/src/authorize.rs crates/tyutool-core/src/lib.rs
git commit -m "feat(batch-auth): add read_mac, run_batch_auth_slot, BatchAuthSlotResult to tyutool-core"
```

---

## Task 3: ExcelRowAllocator (src-tauri/src/batch_auth.rs)

**Files:**
- Create: `src-tauri/src/batch_auth.rs`

- [ ] **Step 3.1: Create batch_auth.rs**

```rust
// src-tauri/src/batch_auth.rs
//! Excel-based authorization row allocator for batch auth.
//!
//! Reads UUID/AUTHKEY rows from an .xlsx file, allocates them atomically
//! to concurrent auth threads, and writes results (STATUS=USED, MAC, TIMESTAMP)
//! back to the file using rust_xlsxwriter.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use calamine::{open_workbook_auto, DataType, Reader};
use rust_xlsxwriter::{Workbook as XlsxWorkbook, Format, Color};

// ── Column indices resolved from header row ───────────────────────────────

#[derive(Debug, Clone)]
struct HeaderInfo {
    uuid_col: usize,
    authkey_col: usize,
    status_col: Option<usize>,
    mac_col: Option<usize>,
    timestamp_col: Option<usize>,
    total_cols: usize,
}

// ── Row state ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum RowStatus {
    Available,
    Allocated,      // in-flight; not yet confirmed/released
    Used,           // written USED to disk
    Skipped,        // not consumed (already-done / conflict skip)
}

#[derive(Debug, Clone)]
struct RowData {
    uuid: String,
    authkey: String,
    status: RowStatus,
    mac: Option<String>,
    timestamp: Option<String>,
    /// Raw cell values for columns other than uuid/authkey/status/mac/timestamp
    extra_cells: Vec<(usize, String)>,
}

/// One allocated row returned to the caller.
#[derive(Debug)]
pub struct ExcelRow {
    pub row_idx: usize,   // 0-based index into RowData vec (not the sheet row)
    pub uuid: String,
    pub authkey: String,
}

/// Stats about the workbook.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcelStats {
    pub total: usize,
    pub used: usize,
    pub remaining: usize,
}

// ── Allocator ─────────────────────────────────────────────────────────────

struct AllocatorState {
    path: PathBuf,
    header: HeaderInfo,
    header_raw: Vec<String>,   // original header cell strings for faithful reproduction
    rows: Vec<RowData>,
    backed_up: bool,
}

pub struct ExcelRowAllocator {
    state: Mutex<AllocatorState>,
}

impl ExcelRowAllocator {
    /// Load workbook from `path` and parse header + data rows.
    pub fn load(path: &Path) -> Result<Self, String> {
        let mut wb = open_workbook_auto(path)
            .map_err(|e| format!("Cannot open Excel file: {e}"))?;

        let sheet_name = wb
            .sheet_names()
            .first()
            .cloned()
            .ok_or("Excel file has no sheets")?;

        let range = wb
            .worksheet_range(&sheet_name)
            .map_err(|e| format!("Cannot read sheet '{sheet_name}': {e}"))?;

        let mut rows_iter = range.rows();

        // Parse header row (row 0)
        let header_row = rows_iter
            .next()
            .ok_or("Excel file is empty (no header row)")?;

        let header_strings: Vec<String> = header_row
            .iter()
            .map(|c| c.to_string().trim().to_string())
            .collect();

        let find_col = |names: &[&str]| -> Option<usize> {
            header_strings.iter().position(|h| {
                let lower = h.to_lowercase();
                names.iter().any(|&n| lower == n)
            })
        };

        let uuid_col = find_col(&["uuid"])
            .ok_or("Missing required column: UUID")?;
        let authkey_col = find_col(&["authkey", "key"])
            .ok_or("Missing required column: AUTHKEY (or key)")?;
        let status_col = find_col(&["status"]);
        let mac_col = find_col(&["mac"]);
        let timestamp_col = find_col(&["timestamp"]);
        let total_cols = header_strings.len();

        let header = HeaderInfo {
            uuid_col, authkey_col, status_col, mac_col, timestamp_col, total_cols,
        };

        // Parse data rows (row 1+)
        let mut rows: Vec<RowData> = Vec::new();
        for data_row in rows_iter {
            let get = |idx: usize| -> String {
                data_row.get(idx).map(|c| c.to_string().trim().to_string()).unwrap_or_default()
            };

            let uuid = get(uuid_col);
            if uuid.is_empty() { continue; } // skip blank rows

            let authkey = get(authkey_col);
            let status_str = status_col.map(get).unwrap_or_default();
            let mac = mac_col.map(|i| get(i)).filter(|s| !s.is_empty());
            let timestamp = timestamp_col.map(|i| get(i)).filter(|s| !s.is_empty());

            let status = if status_str.to_uppercase() == "USED" {
                RowStatus::Used
            } else {
                RowStatus::Available
            };

            // Collect other columns
            let known: std::collections::HashSet<usize> = [
                Some(uuid_col), Some(authkey_col), status_col, mac_col, timestamp_col,
            ]
            .into_iter()
            .flatten()
            .collect();

            let extra_cells = (0..data_row.len())
                .filter(|i| !known.contains(i))
                .map(|i| (i, get(i)))
                .collect();

            rows.push(RowData { uuid, authkey, status, mac, timestamp, extra_cells });
        }

        Ok(Self {
            state: Mutex::new(AllocatorState {
                path: path.to_owned(),
                header,
                header_raw: header_strings,
                rows,
                backed_up: false,
            }),
        })
    }

    pub fn stats(&self) -> ExcelStats {
        let state = self.state.lock().unwrap();
        let total = state.rows.len();
        let used = state.rows.iter().filter(|r| r.status == RowStatus::Used).count();
        let remaining = state.rows.iter().filter(|r| r.status == RowStatus::Available).count();
        ExcelStats { total, used, remaining }
    }

    /// Atomically allocate the next available row.
    /// Returns `Err` if no rows remain.
    pub fn allocate_row(&self) -> Result<ExcelRow, String> {
        let mut state = self.state.lock().unwrap();
        for (idx, row) in state.rows.iter_mut().enumerate() {
            if row.status == RowStatus::Available {
                row.status = RowStatus::Allocated;
                return Ok(ExcelRow {
                    row_idx: idx,
                    uuid: row.uuid.clone(),
                    authkey: row.authkey.clone(),
                });
            }
        }
        Err("Authorization codes exhausted — no available rows in Excel".into())
    }

    /// Release an allocated row back to Available (auth failed or skipped).
    pub fn release_row(&self, row_idx: usize) {
        let mut state = self.state.lock().unwrap();
        if let Some(row) = state.rows.get_mut(row_idx) {
            if row.status == RowStatus::Allocated {
                row.status = RowStatus::Available;
            }
        }
    }

    /// Confirm success: mark row USED, record MAC + timestamp, save xlsx.
    pub fn confirm_row(&self, row_idx: usize, mac: String) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();

        // Backup before first write
        if !state.backed_up {
            let bak = state.path.with_extension("xlsx.bak");
            if !bak.exists() {
                std::fs::copy(&state.path, &bak).ok();
            }
            state.backed_up = true;
        }

        if let Some(row) = state.rows.get_mut(row_idx) {
            row.status = RowStatus::Used;
            row.mac = Some(mac);
            row.timestamp = Some(chrono_utc_now());
        }

        save_workbook(&state)
    }
}

fn chrono_utc_now() -> String {
    // ISO 8601 UTC timestamp without chrono dependency
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, mo, d, h, mi, s) = unix_to_utc(secs);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, mi, s)
}

fn unix_to_utc(secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    let s = secs % 60;
    let mins = secs / 60;
    let mi = mins % 60;
    let hours = mins / 60;
    let h = hours % 24;
    let days = (hours / 24) as i32;

    // Days since 1970-01-01
    let mut y = 1970i32;
    let mut remaining = days;
    loop {
        let dy = if is_leap(y) { 366 } else { 365 };
        if remaining < dy { break; }
        remaining -= dy;
        y += 1;
    }
    let months = [31, if is_leap(y) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mo = 1u32;
    for &dm in &months {
        if remaining < dm { break; }
        remaining -= dm;
        mo += 1;
    }
    (y, mo, remaining as u32 + 1, h as u32, mi as u32, s as u32)
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn save_workbook(state: &AllocatorState) -> Result<(), String> {
    let h = &state.header;

    // Determine output column count — may need to add STATUS, MAC, TIMESTAMP
    let mut max_col = h.total_cols;
    let status_col = h.status_col.unwrap_or_else(|| { let c = max_col; max_col += 1; c });
    let mac_col = h.mac_col.unwrap_or_else(|| { let c = max_col; max_col += 1; c });
    let ts_col = h.timestamp_col.unwrap_or_else(|| { let c = max_col; max_col += 1; c });

    let mut wb = XlsxWorkbook::new();
    let ws = wb.add_worksheet();

    let header_fmt = Format::new().set_bold();

    // Header row
    for (col, text) in state.header_raw.iter().enumerate() {
        ws.write_string_with_format(0, col as u16, text, &header_fmt)
            .map_err(|e| e.to_string())?;
    }
    // Extra header cols if appended
    if h.status_col.is_none() {
        ws.write_string_with_format(0, status_col as u16, "STATUS", &header_fmt)
            .map_err(|e| e.to_string())?;
    }
    if h.mac_col.is_none() {
        ws.write_string_with_format(0, mac_col as u16, "MAC", &header_fmt)
            .map_err(|e| e.to_string())?;
    }
    if h.timestamp_col.is_none() {
        ws.write_string_with_format(0, ts_col as u16, "TIMESTAMP", &header_fmt)
            .map_err(|e| e.to_string())?;
    }

    // Data rows
    for (data_row_idx, row) in state.rows.iter().enumerate() {
        let sheet_row = (data_row_idx + 1) as u32;

        ws.write_string(sheet_row, h.uuid_col as u16, &row.uuid)
            .map_err(|e| e.to_string())?;
        ws.write_string(sheet_row, h.authkey_col as u16, &row.authkey)
            .map_err(|e| e.to_string())?;

        let status_str = if row.status == RowStatus::Used { "USED" } else { "" };
        ws.write_string(sheet_row, status_col as u16, status_str)
            .map_err(|e| e.to_string())?;

        if let Some(ref mac) = row.mac {
            ws.write_string(sheet_row, mac_col as u16, mac)
                .map_err(|e| e.to_string())?;
        }
        if let Some(ref ts) = row.timestamp {
            ws.write_string(sheet_row, ts_col as u16, ts)
                .map_err(|e| e.to_string())?;
        }

        for &(col, ref val) in &row.extra_cells {
            ws.write_string(sheet_row, col as u16, val)
                .map_err(|e| e.to_string())?;
        }
    }

    wb.save(&state.path).map_err(|e| format!("Failed to save Excel: {e}"))
}
```

- [ ] **Step 3.2: Register module in lib.rs**

In `src-tauri/src/lib.rs`, add at the top with other module declarations:

```rust
mod batch_auth;
```

- [ ] **Step 3.3: Build to verify**

```bash
cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | grep -E "^error" | head -20
```

Fix any compilation errors. Common issues:
- `rust_xlsxwriter::Workbook` has a method `add_worksheet()` returning a `Worksheet` — the `ws.write_string` method signature may differ by version. Check: `ws.write_string(row: u32, col: u16, text: &str)`.
- If `Format` doesn't take `set_bold()`, use `Format::new()` without format for header.

- [ ] **Step 3.4: Commit**

```bash
git add src-tauri/src/batch_auth.rs src-tauri/src/lib.rs
git commit -m "feat(batch-auth): add ExcelRowAllocator with calamine + rust_xlsxwriter"
```

---

## Task 4: Tauri commands — batch_auth_start + validate_excel_cmd

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 4.1: Add BatchAuthState struct**

In `src-tauri/src/lib.rs`, after `BatchFlashState`, add:

```rust
struct BatchAuthState {
    /// Reuses the same slot map as BatchFlashState for cancel+thread tracking.
    /// Keyed by port. Only one of batch-flash or batch-auth can use a port at a time.
    slots: StdMutex<HashMap<String, BatchSlot>>,
    /// Shared Excel row allocator for the current batch session. Replaced on each batch_auth_start.
    allocator: StdMutex<Option<std::sync::Arc<batch_auth::ExcelRowAllocator>>>,
}
```

- [ ] **Step 4.2: Add BatchAuthStartConfig**

```rust
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchAuthStartConfig {
    chip_id: String,
    baud_rate: u32,
    firmware_path: Option<String>,   // if Some, flash before auth
    excel_path: String,
    conflict_policy: String,         // "skip" | "overwrite"
}
```

- [ ] **Step 4.3: Add validate_excel_cmd**

```rust
#[tauri::command]
fn validate_excel_cmd(path: String) -> Result<batch_auth::ExcelStats, String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err("文件不存在".into());
    }
    if p.extension().and_then(|e| e.to_str()) != Some("xlsx") {
        return Err("请选择 .xlsx 格式文件".into());
    }
    let alloc = batch_auth::ExcelRowAllocator::load(p)?;
    Ok(alloc.stats())
}
```

- [ ] **Step 4.4: Add batch_auth_start**

```rust
#[tauri::command]
fn batch_auth_start(
    app: AppHandle,
    state: State<'_, BatchAuthState>,
    config: BatchAuthStartConfig,
    ports: Vec<String>,
) -> Result<(), String> {
    let conflict_policy = match config.conflict_policy.as_str() {
        "overwrite" => tyutool_core::ConflictPolicy::Overwrite,
        _ => tyutool_core::ConflictPolicy::Skip,
    };

    // Load (or reload) the Excel allocator for this batch
    let allocator = {
        let path = std::path::Path::new(&config.excel_path);
        let alloc = std::sync::Arc::new(batch_auth::ExcelRowAllocator::load(path)?);
        *state.allocator.lock().map_err(|e| e.to_string())? = Some(alloc.clone());
        alloc
    };

    let mut slots = state.slots.lock().map_err(|e| e.to_string())?;

    for port in ports {
        // Wait for any existing thread on this port (up to 3s)
        if let Some(old) = slots.remove(&port) {
            old.cancel.store(true, Ordering::SeqCst);
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            std::thread::spawn(move || { let _ = old.thread.join(); let _ = tx.send(()); });
            if rx.recv_timeout(Duration::from_secs(3)).is_err() {
                return Err(format!("port {} not stopped; retry in a few seconds", port));
            }
        }

        // Pre-allocate Excel row
        let row = match allocator.allocate_row() {
            Ok(r) => r,
            Err(e) => {
                // Emit exhausted failure for this port immediately
                let _ = app.emit("batch-auth-progress", serde_json::json!({
                    "port": port,
                    "step": "failed",
                    "error": e
                }));
                continue;
            }
        };

        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel_clone = cancel.clone();
        let app_clone = app.clone();
        let port_clone = port.clone();
        let config_clone = config.clone();
        let alloc_clone = allocator.clone();
        let row_idx = row.row_idx;
        let uuid = row.uuid.clone();
        let authkey = row.authkey.clone();

        let handle = std::thread::spawn(move || {
            // Optional: flash first
            if let Some(ref fw_path) = config_clone.firmware_path {
                if !fw_path.is_empty() {
                    let job = tyutool_core::FlashJob {
                        mode: tyutool_core::FlashMode::Flash,
                        chip_id: config_clone.chip_id.clone(),
                        port: port_clone.clone(),
                        baud_rate: config_clone.baud_rate,
                        firmware_path: Some(fw_path.clone()),
                        segments: None,
                        flash_start_hex: None, flash_end_hex: None,
                        erase_start_hex: None, erase_end_hex: None,
                        read_start_hex: None, read_end_hex: None,
                        read_file_path: None,
                        authorize_uuid: None, authorize_key: None,
                    };
                    let app2 = app_clone.clone();
                    let port2 = port_clone.clone();
                    let flash_result = tyutool_core::run_job(&job, &cancel_clone, |p| {
                        let _ = app2.emit("batch-auth-progress", serde_json::json!({
                            "port": port2,
                            "step": "flashing",
                            "event": p
                        }));
                    });
                    if flash_result.is_err() || cancel_clone.load(Ordering::Relaxed) {
                        alloc_clone.release_row(row_idx);
                        let _ = app_clone.emit("batch-auth-progress", serde_json::json!({
                            "port": port_clone,
                            "step": "failed",
                            "error": flash_result.err().map(|e| e.to_string()).unwrap_or_else(|| "cancelled".into())
                        }));
                        return;
                    }
                }
            }

            // Run auth UART session
            let result = tyutool_core::run_batch_auth_slot(
                &port_clone, &uuid, &authkey,
                conflict_policy,
                &cancel_clone,
                |step| {
                    let step_str = match step {
                        tyutool_core::BatchAuthStep::ReadingMac => "reading_mac",
                        tyutool_core::BatchAuthStep::ReadingAuth => "reading_auth",
                        tyutool_core::BatchAuthStep::WritingAuth => "writing_auth",
                        tyutool_core::BatchAuthStep::Verifying => "verifying",
                    };
                    let _ = app_clone.emit("batch-auth-progress", serde_json::json!({
                        "port": port_clone,
                        "step": step_str
                    }));
                },
            );

            match result {
                Ok(tyutool_core::BatchAuthSlotResult::Done { mac }) => {
                    let _ = alloc_clone.confirm_row(row_idx, mac.clone());
                    let _ = app_clone.emit("batch-auth-progress", serde_json::json!({
                        "port": port_clone, "step": "done", "mac": mac
                    }));
                }
                Ok(tyutool_core::BatchAuthSlotResult::AlreadyDone { mac }) => {
                    alloc_clone.release_row(row_idx);
                    let _ = app_clone.emit("batch-auth-progress", serde_json::json!({
                        "port": port_clone, "step": "done", "mac": mac
                    }));
                }
                Ok(tyutool_core::BatchAuthSlotResult::Skipped { mac }) => {
                    alloc_clone.release_row(row_idx);
                    let _ = app_clone.emit("batch-auth-progress", serde_json::json!({
                        "port": port_clone, "step": "skipped", "mac": mac
                    }));
                }
                Ok(tyutool_core::BatchAuthSlotResult::Cancelled) => {
                    alloc_clone.release_row(row_idx);
                    // No event — cancelled by user
                }
                Err(e) => {
                    alloc_clone.release_row(row_idx);
                    let _ = app_clone.emit("batch-auth-progress", serde_json::json!({
                        "port": port_clone, "step": "failed", "error": e.to_string()
                    }));
                }
            }
        });

        slots.insert(port, BatchSlot { cancel, thread: handle });
    }

    Ok(())
}
```

- [ ] **Step 4.5: Add cancel commands**

```rust
#[tauri::command]
fn batch_auth_cancel_port(
    state: State<'_, BatchAuthState>,
    port: String,
) -> Result<(), String> {
    let slots = state.slots.lock().map_err(|e| e.to_string())?;
    if let Some(slot) = slots.get(&port) {
        slot.cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
fn batch_auth_cancel_all(state: State<'_, BatchAuthState>) -> Result<(), String> {
    let slots = state.slots.lock().map_err(|e| e.to_string())?;
    for slot in slots.values() {
        slot.cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}
```

- [ ] **Step 4.6: Register BatchAuthState + commands**

In `.manage(...)` chain, add after `BatchFlashState`:
```rust
.manage(BatchAuthState {
    slots: StdMutex::new(HashMap::new()),
    allocator: StdMutex::new(None),
})
```

In `invoke_handler`, add:
```rust
batch_auth_start,
batch_auth_cancel_port,
batch_auth_cancel_all,
validate_excel_cmd,
```

In `RunEvent::ExitRequested`, add cancel for auth slots:
```rust
if let Some(auth_state) = app_handle.try_state::<BatchAuthState>() {
    if let Ok(slots) = auth_state.slots.lock() {
        for slot in slots.values() {
            slot.cancel.store(true, Ordering::SeqCst);
        }
    }
}
```

- [ ] **Step 4.7: Build Tauri to verify**

```bash
cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | grep -E "^error" | head -20
```

Fix any errors. The `BatchSlot` type is defined in `lib.rs`; `BatchAuthState` uses it directly — ensure `batch_auth.rs` doesn't try to import `BatchSlot` (it doesn't need to).

- [ ] **Step 4.8: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(batch-auth): add batch_auth_start, cancel, validate_excel Tauri commands"
```

---

## Task 5: Frontend types + store update

**Files:**
- Modify: `src/features/batch-flash/types.ts`
- Modify: `src/features/batch-flash/store.ts`

- [ ] **Step 5.1: Add BatchAuthProgressEvent to types.ts**

In `src/features/batch-flash/types.ts`, add after `BatchFlashProgressEvent`:

```ts
/** `batch-auth-progress` event from Rust. */
export interface BatchAuthProgressEvent {
  port: string
  step:
    | 'flashing'       // flash sub-step (only when flash-then-auth)
    | 'reading_mac'
    | 'reading_auth'
    | 'writing_auth'
    | 'verifying'
    | 'done'
    | 'failed'
    | 'skipped'
  mac?: string          // filled on done / skipped / already_done
  error?: string        // filled on failed
  event?: unknown       // filled when step='flashing' (FlashProgressPayload)
}

/** Mirrors Rust BatchAuthStartConfig. */
export interface BatchAuthStartConfig {
  chipId: string
  baudRate: number
  firmwarePath?: string
  excelPath: string
  conflictPolicy: 'skip' | 'overwrite'
}

/** Stats returned by validate_excel_cmd. */
export interface ExcelStats {
  total: number
  used: number
  remaining: number
}
```

- [ ] **Step 5.2: Update store.ts — add auth listener + handleAuthProgress**

In `src/features/batch-flash/store.ts`, add a second unlisten variable and auth handler.

After `let unlisten: (() => void) | undefined`, add:
```ts
let unlistenAuth: (() => void) | undefined
```

After the existing `handleFlashProgress` function, add:

```ts
function handleAuthProgress(ev: BatchAuthProgressEvent) {
  const { port, step } = ev
  if (!findSlot(port)) return

  if (step === 'reading_mac') {
    updateSlot(port, { status: 'reading_mac', currentPhase: '读取MAC' })
  } else if (step === 'reading_auth' || step === 'writing_auth' || step === 'verifying') {
    updateSlot(port, { status: 'authorizing', currentPhase: step })
  } else if (step === 'done') {
    updateSlot(port, { status: 'done', progress: 100, currentPhase: '', mac: ev.mac })
    cumulativeStats.value.auth.total++
    cumulativeStats.value.auth.success++
    void saveCumulativeStats()
    checkBatchCompletion()
  } else if (step === 'failed') {
    updateSlot(port, { status: 'failed', error: ev.error ?? 'Unknown auth error' })
    cumulativeStats.value.auth.total++
    cumulativeStats.value.auth.fail++
    void saveCumulativeStats()
    checkBatchCompletion()
  } else if (step === 'skipped') {
    updateSlot(port, { status: 'skipped', currentPhase: '' })
    checkBatchCompletion()
  } else if (step === 'flashing') {
    // Auth flash sub-step: reuse flash progress handler
    if (ev.event) {
      handleFlashProgress({ port, event: ev.event as import('./types').FlashProgressPayload })
    }
  }
}
```

Update `ensureListener` to also attach the auth listener:

```ts
async function ensureListener() {
  if (!isTauriRuntime()) return
  const { listen } = await import('@tauri-apps/api/event')
  if (!unlisten) {
    unlisten = await listen<BatchFlashProgressEvent>('batch-flash-progress', ({ payload }) => {
      handleFlashProgress(payload)
    })
  }
  if (!unlistenAuth) {
    unlistenAuth = await listen<BatchAuthProgressEvent>('batch-auth-progress', ({ payload }) => {
      handleAuthProgress(payload)
    })
  }
}
```

Update `cleanup`:
```ts
function cleanup() {
  unlisten?.(); unlisten = undefined
  unlistenAuth?.(); unlistenAuth = undefined
}
```

Add `startAuth` action:
```ts
async function startAuth() {
  if (!canStart.value) return
  batchStartTime.value = Date.now()
  completionBanner.value = null

  const idlePorts = slots.value.filter(s => s.status === 'idle').map(s => s.port)
  for (const port of idlePorts) {
    updateSlot(port, { status: 'reading_mac', progress: 0, currentPhase: '读取MAC', error: undefined })
  }

  if (!isTauriRuntime()) return
  const { invoke } = await import('@tauri-apps/api/core')
  const config: BatchAuthStartConfig = {
    chipId: chipId.value,
    baudRate: baudRate.value,
    firmwarePath: firmwarePath.value || undefined,
    excelPath: authConfig.value.excelPath,
    conflictPolicy: authConfig.value.conflictPolicy,
  }
  await invoke('batch_auth_start', { config, ports: idlePorts })
}
```

Update `startFlash` to dispatch to the right action based on `opMode`:

Rename existing `startFlash` body to an inner function and add a public dispatcher:
```ts
async function startBatch() {
  if (opMode.value === 'flash-only') {
    await startFlash()
  } else if (opMode.value === 'auth-only') {
    await startAuth()
  } else {
    // flash-then-auth: batch_auth_start handles both
    await startAuth()  // auth command runs flash+auth internally
  }
}
```

Export `startBatch` and `handleAuthProgress` from the return object. Keep `startFlash` exported for internal use.

- [ ] **Step 5.3: Update toolbar to call startBatch**

In `src/features/batch-flash/components/BatchFlashToolbar.vue`, change:
```ts
await store.startFlash()
```
to:
```ts
await store.startBatch()
```

Also update `handleStart`:
```ts
async function handleStart() {
  const idleCount = store.slots.filter(s => s.status === 'idle').length
  if (idleCount > 8) {
    const ok = await showConfirmDialog({
      title: '确认批量操作',
      message: `即将对 ${idleCount} 个端口并行操作`,
      kind: 'warning',
    })
    if (!ok) return
  }
  await store.startBatch()
}
```

- [ ] **Step 5.4: Run store tests**

```bash
node_modules/.bin/vitest run src/features/batch-flash/store.test.ts 2>&1 | tail -10
```

All 24 tests must still pass. If any fail due to the `startBatch` rename, update the test or the export.

- [ ] **Step 5.5: TypeScript check**

```bash
node_modules/.bin/vue-tsc --noEmit 2>&1 | grep "batch-flash" | head -20
```

Fix any errors.

- [ ] **Step 5.6: Commit**

```bash
git add src/features/batch-flash/types.ts src/features/batch-flash/store.ts \
        src/features/batch-flash/components/BatchFlashToolbar.vue
git commit -m "feat(batch-auth): add BatchAuthProgressEvent, handleAuthProgress, startBatch to store"
```

---

## Task 6: BatchAuthConfig.vue — Excel validation UI

**Files:**
- Modify: `src/features/batch-flash/components/BatchAuthConfig.vue`

- [ ] **Step 6.1: Add Excel stats + validation feedback**

Replace the current `BatchAuthConfig.vue` with:

```vue
<!-- src/features/batch-flash/components/BatchAuthConfig.vue -->
<script setup lang="ts">
import { ref, watch } from 'vue'
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri'
import { useBatchFlashStore } from '../store'
import type { ExcelStats } from '../types'

const store = useBatchFlashStore()

const excelStats = ref<ExcelStats | null>(null)
const excelError = ref<string | null>(null)

async function browseExcel() {
  if (!isTauriRuntime()) return
  const { open } = await import('@tauri-apps/plugin-dialog')
  const file = await open({ filters: [{ name: 'Excel', extensions: ['xlsx'] }] })
  if (typeof file === 'string') {
    store.authConfig.excelPath = file
  }
}

async function validateExcel(path: string) {
  if (!path || !isTauriRuntime()) {
    excelStats.value = null
    excelError.value = null
    return
  }
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    const stats = await invoke<ExcelStats>('validate_excel_cmd', { path })
    excelStats.value = stats
    excelError.value = null
  } catch (e) {
    excelStats.value = null
    excelError.value = String(e)
  }
}

watch(() => store.authConfig.excelPath, validateExcel, { immediate: true })
</script>

<template>
  <div
    class="rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] px-4 py-3"
    style="border-left: 3px solid var(--ty-accent);"
  >
    <h3 class="mb-3 text-sm font-semibold text-[var(--ty-text)]">批量授权配置</h3>
    <div class="flex flex-col gap-3">
      <!-- Excel file -->
      <div class="flex flex-col gap-1">
        <label class="text-xs text-[var(--ty-text-muted)]">授权表 (.xlsx)</label>
        <div class="flex gap-2">
          <input
            type="text"
            :value="store.authConfig.excelPath"
            readonly
            :disabled="store.isBusy"
            placeholder="未选择授权表"
            class="min-w-0 flex-1 rounded-lg border border-[var(--ty-border)] bg-[var(--ty-surface-muted)] px-2.5 py-1.5 text-xs text-[var(--ty-text)] placeholder:text-[var(--ty-text-muted)]"
          />
          <button
            type="button"
            class="ops-browse-btn"
            :disabled="store.isBusy"
            @click="browseExcel"
          >浏览</button>
        </div>

        <!-- Validation feedback -->
        <div v-if="excelError" class="text-xs" :style="{ color: 'var(--ty-danger)' }">
          {{ excelError }}
        </div>
        <div v-else-if="excelStats" class="flex gap-3 text-xs">
          <span class="text-[var(--ty-text-muted)]">
            总计 <strong class="text-[var(--ty-text)]">{{ excelStats.total }}</strong>
          </span>
          <span class="text-[var(--ty-text-muted)]">
            已用 <strong class="text-[var(--ty-text)]">{{ excelStats.used }}</strong>
          </span>
          <span
            :style="{ color: excelStats.remaining === 0 ? 'var(--ty-danger)' : 'var(--ty-success)' }"
          >
            剩余 <strong>{{ excelStats.remaining }}</strong>
          </span>
          <span
            v-if="excelStats.remaining === 0"
            class="font-medium"
            :style="{ color: 'var(--ty-accent)' }"
          >⚠ 授权码已全部使用，请补充 Excel</span>
        </div>
      </div>

      <!-- Conflict policy -->
      <div class="flex items-center gap-4 text-xs text-[var(--ty-text-muted)]">
        <span>遇到已授权设备：</span>
        <label class="flex cursor-pointer items-center gap-1">
          <input type="radio" v-model="store.authConfig.conflictPolicy" value="skip" :disabled="store.isBusy" />
          跳过（推荐）
        </label>
        <label class="flex cursor-pointer items-center gap-1">
          <input type="radio" v-model="store.authConfig.conflictPolicy" value="overwrite" :disabled="store.isBusy" />
          覆盖
        </label>
      </div>
    </div>
  </div>
</template>
```

- [ ] **Step 6.2: Update inputsValid in store to gate on excelStats.remaining**

Add to store exports (the frontend now checks remaining via validate_excel_cmd; `inputsValid` already requires excelPath to be non-empty when opMode includes auth — that's sufficient since the component blocks start when remaining=0 via a disabled condition).

The toolbar's `canStart` already uses `store.canStart` which requires `inputsValid`. For the remaining=0 case, we can add a `excelExhausted` ref to the store that BatchAuthConfig.vue updates, OR simply trust that the Tauri backend rejects pre-allocation (emits "failed" with exhausted message immediately).

For simplicity: keep the current `inputsValid` logic. The exhausted case is handled gracefully by the Rust side (immediate "failed" event for ports that can't get a row).

- [ ] **Step 6.3: TypeScript check**

```bash
node_modules/.bin/vue-tsc --noEmit 2>&1 | grep "batch-flash" | head -20
```

- [ ] **Step 6.4: Commit**

```bash
git add src/features/batch-flash/components/BatchAuthConfig.vue
git commit -m "feat(batch-auth): add Excel validation UI with stats pills and error feedback"
```

---

## Task 7: Final build + test verification

**Files:** None new.

- [ ] **Step 7.1: Run all batch-flash tests**

```bash
node_modules/.bin/vitest run src/features/batch-flash/ 2>&1 | tail -10
```

Expected: 30 tests, all passing.

- [ ] **Step 7.2: Full TypeScript check**

```bash
node_modules/.bin/vue-tsc --noEmit 2>&1 | grep -E "error TS" | head -20
```

Expected: zero errors.

- [ ] **Step 7.3: Build Rust**

```bash
cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | tail -5
```

Expected: `Finished` with no errors.

- [ ] **Step 7.4: Commit if any fixes**

```bash
git status
# If any uncommitted fixes:
git add -A
git commit -m "fix(batch-auth): final TypeScript and build fixes"
```

---

## Phase 2 complete ✓

After Phase 2, the batch page supports:
- **auth-only**: Select Excel, start → UART auth for each port, MAC read, row allocated/confirmed
- **flash-then-auth**: Select firmware + Excel, start → flash then auth in one thread per port
- Excel stats shown after file selection (total/used/remaining)
- Conflict policy: skip (default) or overwrite
- Auth cumulative stats tracked separately from flash stats
- `.bak` created before first Excel write

**Remaining for future phases:**
- Session log file generation (`表名_auth_时间戳.log`)
- GD32 chip support (add to `BATCH_AUTH_SUPPORTED_CHIPS` when GD32 branch merges)
