# Logging Separation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the ad-hoc `FlashProgress` event system with a typed `FlashEvent` that cleanly separates user-visible output from developer diagnostic logs (`log::*`).

**Architecture:** `FlashEvent` replaces `FlashProgress` as the single user-facing event channel; `log::*` macros become developer-only and route to a file (CLI) or `tauri-plugin-log` file (GUI). The core library emits `JobSummary` as the first event so every frontend can render a job header without needing extra context.

**Tech Stack:** Rust `log` crate, `fern` (CLI file logging), `dirs` (platform log path), `indicatif`/`console` (CLI progress UI), Tauri `tauri-plugin-log`, Vue 3 / TypeScript discriminated unions.

**Spec:** `docs/specs/2026-05-19-logging-separation-design.md`

---

## File Map

| Action | Path | Responsibility |
|--------|------|---------------|
| Create | `crates/tyutool-core/src/flash_event.rs` | All `FlashEvent` types with serde |
| Modify | `crates/tyutool-core/src/lib.rs` | Export `FlashEvent`; remove `FlashProgress` |
| Modify | `crates/tyutool-core/src/plugin.rs` | Change callback type |
| Modify | `crates/tyutool-core/src/registry.rs` | Emit `JobSummary`; timer; `Cancelled` mapping |
| Modify | `crates/tyutool-core/src/plugins/bk7231n.rs` | Migrate `LogKey`/`LogLine` → `FlashEvent` |
| Modify | `crates/tyutool-core/src/plugins/t1.rs` | Callback type only |
| Modify | `crates/tyutool-core/src/plugins/t3.rs` | Callback type only |
| Modify | `crates/tyutool-core/src/plugins/t5ai.rs` | Callback type only |
| Modify | `crates/tyutool-core/src/plugins/esp/common.rs` | Migrate `LogKey` → `FlashEvent` |
| Modify | `crates/tyutool-core/src/plugins/ln882h/mod.rs` | Migrate `LogLine` → `FlashEvent`/`Warning` |
| Modify | `crates/tyutool-core/src/authorize.rs` | Migrate `LogKey` → `AuthReadComplete` milestone |
| Modify | `crates/tyutool-cli/Cargo.toml` | Add `fern`, `chrono`, `dirs` |
| Modify | `crates/tyutool-cli/src/main.rs` | File logging init; `--verbose`; banner |
| Modify | `crates/tyutool-cli/src/reporter.rs` | Full rewrite for `FlashEvent` |
| Modify | `crates/tyutool-cli/src/serve.rs` | Callback type |
| Modify | `src-tauri/src/lib.rs` | Callback type in `flash_run` |
| Modify | `src/features/firmware-flash/flash-tauri.ts` | New TS discriminated union types |
| Modify | `src/stores/flash.ts` | `handleFlashProgressPayload` for new event variants |
| Modify | `src/features/firmware-flash/ws-transport.ts` | Auth milestone handling |
| Create | `docs/cli.md` | Authoritative CLI reference |
| Modify | `CLAUDE.md` | Logging contract + CLI doc sync rule |

---

## Task 1: Define FlashEvent types in tyutool-core

**Files:**
- Create: `crates/tyutool-core/src/flash_event.rs`
- Modify: `crates/tyutool-core/src/lib.rs`

- [ ] **Step 1.1: Create `flash_event.rs`**

```rust
// crates/tyutool-core/src/flash_event.rs
use serde::{Deserialize, Serialize};

use crate::job::{FlashJob, FlashMode};

/// User-facing event emitted through the progress callback.
/// Developer diagnostics use `log::*` macros instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FlashEvent {
    JobSummary(JobSummary),
    Phase { phase: FlashPhase },
    Percent { value: u8 },
    Milestone { milestone: FlashMilestone },
    /// User-actionable warning (e.g. LN882H: "hold BOOT/A9 pin LOW").
    Warning { message: String },
    Done { result: FlashResult },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobSummary {
    pub port: String,
    pub baud: u32,
    /// None for Authorize mode (no chip plugin involved).
    pub device: Option<String>,
    pub details: JobDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobDetails {
    Flash {
        firmware_path: String,
        firmware_size: Option<u64>,
        range_start: String,
        range_end: String,
    },
    Read {
        output_path: String,
        range_start: String,
        range_end: String,
    },
    Erase {
        range_start: String,
        range_end: String,
    },
    Authorize {
        /// true = writing credentials, false = reading current state.
        write: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlashPhase {
    Handshake,
    ReadFlashId,
    Unprotect,
    Erase,
    /// Multi-segment flash: segment N of M.
    WriteSegment { current: u32, total: u32 },
    Write,
    Verify,
    Protect,
    Reboot,
    Read,
    Save,
    LoadRam,
    SwitchBaud,
    Connect,
    /// Fallback for phases not yet in the enum. Prefer adding a variant.
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlashMilestone {
    HandshakeComplete,
    /// chip_info: human-readable chip name + revision (ESP only; None for Beken).
    Connected { chip_info: Option<String> },
    FlashIdRead { mid: Option<u32> },
    EraseComplete,
    SegmentWritten { current: u32, total: u32 },
    WriteComplete,
    VerifyPassed,
    Rebooted,
    /// TuyaOpen auth read result. GUI MUST display this in a secure modal, not plain log.
    AuthReadComplete { uuid: String, authkey: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlashResult {
    Ok { elapsed_secs: f64 },
    Err { message: String, elapsed_secs: f64 },
    Cancelled { elapsed_secs: f64 },
}

impl JobSummary {
    pub fn from_job(job: &FlashJob) -> Self {
        let details = match job.mode {
            FlashMode::Flash => JobDetails::Flash {
                firmware_path: job.firmware_path.clone().unwrap_or_default(),
                firmware_size: job
                    .firmware_path
                    .as_deref()
                    .and_then(|p| std::fs::metadata(p).ok())
                    .map(|m| m.len()),
                range_start: job
                    .flash_start_hex
                    .clone()
                    .unwrap_or_else(|| "0x00000000".into()),
                range_end: job.flash_end_hex.clone().unwrap_or_default(),
            },
            FlashMode::Read => JobDetails::Read {
                output_path: job.read_file_path.clone().unwrap_or_default(),
                range_start: job
                    .read_start_hex
                    .clone()
                    .unwrap_or_else(|| "0x00000000".into()),
                range_end: job.read_end_hex.clone().unwrap_or_default(),
            },
            FlashMode::Erase => JobDetails::Erase {
                range_start: job
                    .erase_start_hex
                    .clone()
                    .unwrap_or_else(|| "0x00000000".into()),
                range_end: job.erase_end_hex.clone().unwrap_or_default(),
            },
            FlashMode::Authorize => JobDetails::Authorize {
                write: job.authorize_uuid.is_some() || job.authorize_key.is_some(),
            },
        };
        Self {
            port: job.port.clone(),
            baud: job.baud_rate,
            device: if matches!(job.mode, FlashMode::Authorize) {
                None
            } else {
                Some(job.normalized_chip_id())
            },
            details,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flash_event_phase_serializes_to_snake_case() {
        let e = FlashEvent::Phase { phase: FlashPhase::Handshake };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "phase");
        assert_eq!(v["phase"], "handshake");
    }

    #[test]
    fn write_segment_nested_correctly() {
        let e = FlashEvent::Phase {
            phase: FlashPhase::WriteSegment { current: 1, total: 3 },
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "phase");
        assert_eq!(v["phase"]["write_segment"]["current"], 1);
        assert_eq!(v["phase"]["write_segment"]["total"], 3);
    }

    #[test]
    fn done_ok_roundtrips() {
        let e = FlashEvent::Done {
            result: FlashResult::Ok { elapsed_secs: 3.2 },
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: FlashEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, FlashEvent::Done { result: FlashResult::Ok { .. } }));
    }

    #[test]
    fn auth_read_complete_has_uuid_authkey() {
        let m = FlashMilestone::AuthReadComplete {
            uuid: "abc".into(),
            authkey: "xyz".into(),
        };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["auth_read_complete"]["uuid"], "abc");
        assert_eq!(v["auth_read_complete"]["authkey"], "xyz");
    }

    #[test]
    fn job_summary_from_flash_job() {
        let job = crate::job::FlashJob {
            mode: FlashMode::Authorize,
            chip_id: "".into(),
            port: "/dev/ttyUSB0".into(),
            baud_rate: 115200,
            segments: None,
            flash_start_hex: None,
            flash_end_hex: None,
            erase_start_hex: None,
            erase_end_hex: None,
            read_start_hex: None,
            read_end_hex: None,
            read_file_path: None,
            firmware_path: None,
            authorize_uuid: Some("u".into()),
            authorize_key: None,
        };
        let s = JobSummary::from_job(&job);
        assert!(s.device.is_none());
        assert!(matches!(s.details, JobDetails::Authorize { write: true }));
    }
}
```

- [ ] **Step 1.2: Run the new tests to verify they fail (file not yet in module tree)**

```bash
cargo test -p tyutool-core flash_event 2>&1 | head -5
```
Expected: error about module not found (not yet wired in).

- [ ] **Step 1.3: Add `flash_event` module and exports to `lib.rs`**

In `crates/tyutool-core/src/lib.rs`, add after the existing `mod` declarations:

```rust
pub mod flash_event;
```

And add these exports after the existing `pub use` lines:

```rust
pub use flash_event::{
    FlashEvent, FlashMilestone, FlashPhase, FlashResult, JobDetails, JobSummary,
};
```

Keep the existing `pub use progress::FlashProgress;` line — it will be removed in Task 2 once all consumers are updated.

- [ ] **Step 1.4: Run tests to verify they pass**

```bash
cargo test -p tyutool-core flash_event
```
Expected: 5 tests pass.

- [ ] **Step 1.5: Run full core test suite to confirm nothing broke**

```bash
cargo test -p tyutool-core
```
Expected: all existing tests pass.

- [ ] **Step 1.6: Commit**

```bash
git add crates/tyutool-core/src/flash_event.rs crates/tyutool-core/src/lib.rs
git commit -m "feat(core): define FlashEvent type system with serde"
```

---

## Task 2: Switch tyutool-core to FlashEvent

This is a coordinated change across plugin.rs, registry.rs, and all plugin implementations. All files must be updated together because the `FlashPlugin` trait signature change breaks every implementor simultaneously.

**Files:**
- Modify: `crates/tyutool-core/src/plugin.rs`
- Modify: `crates/tyutool-core/src/registry.rs`
- Modify: `crates/tyutool-core/src/plugins/bk7231n.rs`
- Modify: `crates/tyutool-core/src/plugins/t1.rs`
- Modify: `crates/tyutool-core/src/plugins/t3.rs`
- Modify: `crates/tyutool-core/src/plugins/t5ai.rs`
- Modify: `crates/tyutool-core/src/plugins/esp/common.rs`
- Modify: `crates/tyutool-core/src/plugins/ln882h/mod.rs`
- Modify: `crates/tyutool-core/src/authorize.rs`
- Modify: `crates/tyutool-core/src/lib.rs`

- [ ] **Step 2.1: Update `plugin.rs` — change callback type**

Replace the entire file content:

```rust
use std::sync::atomic::AtomicBool;

use crate::error::FlashError;
use crate::flash_event::FlashEvent;
use crate::job::FlashJob;

pub trait FlashPlugin: Send + Sync {
    fn id(&self) -> &'static str;

    fn run(
        &self,
        job: &FlashJob,
        cancel: &AtomicBool,
        progress: &dyn Fn(FlashEvent),
    ) -> Result<(), FlashError>;
}
```

- [ ] **Step 2.2: Update `registry.rs` — emit `JobSummary`, timer, `Cancelled` mapping**

Replace the `run_job` function (keep the registry struct and `new`/`get` methods unchanged):

```rust
// Add to imports at top of registry.rs:
use crate::flash_event::{FlashEvent, FlashResult, JobSummary};
// Remove: use crate::progress::FlashProgress;

pub fn run_job<F>(
    job: &FlashJob,
    cancel: &std::sync::atomic::AtomicBool,
    progress: F,
) -> Result<(), crate::error::FlashError>
where
    F: Fn(FlashEvent),
{
    let start = std::time::Instant::now();
    progress(FlashEvent::JobSummary(JobSummary::from_job(job)));

    log::info!(
        "run_job: chip={}, port={}, mode={:?}",
        job.normalized_chip_id(),
        job.port,
        job.mode
    );

    let result = if matches!(job.mode, FlashMode::Authorize) {
        log::info!("run_job: Authorize mode on port={}", job.port);
        crate::authorize::run_authorize(job, cancel, &progress)
    } else {
        let reg = default_registry();
        let chip = job.normalized_chip_id();
        let plugin = reg.get(&chip)?;
        plugin.run(job, cancel, &progress)
    };

    let elapsed_secs = start.elapsed().as_secs_f64();
    match result {
        Ok(()) => {
            progress(FlashEvent::Done {
                result: FlashResult::Ok { elapsed_secs },
            });
            log::info!("run_job: completed in {:.1}s", elapsed_secs);
            Ok(())
        }
        Err(crate::error::FlashError::Cancelled) => {
            progress(FlashEvent::Done {
                result: FlashResult::Cancelled { elapsed_secs },
            });
            log::info!("run_job: cancelled after {:.1}s", elapsed_secs);
            Err(crate::error::FlashError::Cancelled)
        }
        Err(e) => {
            progress(FlashEvent::Done {
                result: FlashResult::Err {
                    message: e.to_string(),
                    elapsed_secs,
                },
            });
            log::error!("run_job: failed after {:.1}s: {}", elapsed_secs, e);
            Err(e)
        }
    }
}
```

- [ ] **Step 2.3: Update `plugins/bk7231n.rs` — change signature, migrate LogKey/LogLine**

Change the import block at the top:
```rust
// Replace:
use crate::progress::FlashProgress;
// With:
use crate::flash_event::{FlashEvent, FlashMilestone, FlashPhase};
```

Change `FlashPlugin::run` signature:
```rust
fn run(
    &self,
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
```

Change the same signature in `run_beken` and in `run_flash_mode`, `run_erase_mode`, `run_read_mode` helper functions (they all take `progress: &dyn Fn(FlashProgress)` — change each to `&dyn Fn(FlashEvent)`).

In `run_beken`, change the closure helpers:
```rust
// Old:
let log = |msg: &str| {
    progress(FlashProgress::LogLine { line: msg.to_string() });
};
let pct = |v: u8| { progress(FlashProgress::Percent { value: v }); };
let phase = |name: &str| { progress(FlashProgress::Phase { name: name.to_string() }); };

// New:
let log = |msg: &str| log::info!("{}", msg);
let pct = |v: u8| progress(FlashEvent::Percent { value: v });
let phase = |p: FlashPhase| progress(FlashEvent::Phase { phase: p });
```

Change all `phase("...")` call sites to typed variants:
```rust
phase("Handshake")     → phase(FlashPhase::Handshake)
phase("ReadFlashID")   → phase(FlashPhase::ReadFlashId)
phase("Unprotect")     → phase(FlashPhase::Unprotect)
phase("Erase")         → phase(FlashPhase::Erase)
phase("Write")         → phase(FlashPhase::Write)
phase("Verify")        → phase(FlashPhase::Verify)
phase("Protect")       → phase(FlashPhase::Protect)
phase("Reboot")        → phase(FlashPhase::Reboot)
phase("Read")          → phase(FlashPhase::Read)
phase("Save")          → phase(FlashPhase::Save)
```

For multi-segment loop in `run_flash_mode` (around line 148), replace:
```rust
// Old:
progress(FlashProgress::LogKey {
    key: "flash.log.segmentLog".to_string(),
    params: [("n".to_string(), (i + 1).to_string())].into(),
});
// New (use Phase instead; segment context is conveyed by WriteSegment):
progress(FlashEvent::Phase {
    phase: FlashPhase::WriteSegment {
        current: (i + 1) as u32,
        total: total_segments as u32,
    },
});
```

For `run_read_mode`, replace the two `LogKey` emissions:
```rust
// flash.log.beken.readRange → developer detail, not user-facing:
log::info!("Reading 0x{:010x}..0x{:010x} ({} KiB)", start, end, kib);

// flash.log.beken.savingBytes → developer detail:
log::info!("Saving {} bytes to {}", size, path);
```

- [ ] **Step 2.4: Update `plugins/t1.rs`, `t3.rs`, `t5ai.rs` — change signature only**

In each file, replace the import and the `run` method signature:

```rust
// Replace in each file:
use crate::progress::FlashProgress;
// With:
use crate::flash_event::FlashEvent;

// Change run() signature from:
progress: &dyn Fn(FlashProgress),
// To:
progress: &dyn Fn(FlashEvent),
```

The delegation call `super::bk7231n::run_beken(job, cancel, progress, &chip, ...)` works unchanged since `run_beken` signature already changed in Step 2.3.

- [ ] **Step 2.5: Update `plugins/esp/common.rs` — migrate LogKey**

Replace import:
```rust
use crate::progress::FlashProgress;
// →
use crate::flash_event::{FlashEvent, FlashMilestone, FlashPhase};
```

Change `emit_key` helper and its usages. Replace the `emit_key` function entirely with direct `FlashEvent` emissions at each call site:

```rust
// flash.log.esp.connected (around line 220):
// Old:
emit_key(progress, "flash.log.esp.connected", &[
    ("chip", info.chip.to_string()),
    ("revision", format!("{:?}", info.revision)),
]);
// New:
progress(FlashEvent::Milestone {
    milestone: FlashMilestone::Connected {
        chip_info: Some(format!("{} (revision {:?})", info.chip, info.revision)),
    },
});

// flash.log.esp.readDeviceInfoFailed → developer log:
// Old: emit_key(progress, "flash.log.esp.readDeviceInfoFailed", &[("error", e.to_string())]);
// New:
log::warn!("Failed to read ESP device info: {}", e);

// flash.log.segmentLog (around line 307):
// Old: progress(FlashProgress::LogKey { key: "flash.log.segmentLog", ... });
// New:
progress(FlashEvent::Phase {
    phase: FlashPhase::WriteSegment {
        current: (i + 1) as u32,
        total: total_segments as u32,
    },
});
```

Change `FlashPlugin::run` and all internal function signatures from `&dyn Fn(FlashProgress)` to `&dyn Fn(FlashEvent)`.

Change `FlashProgress::Percent { value }` → `FlashEvent::Percent { value }` throughout.

Delete the `emit_key` helper function.

- [ ] **Step 2.6: Update `plugins/ln882h/mod.rs` — migrate LogLine to Warning/Milestone**

Replace import and change all `progress: &dyn Fn(FlashProgress)` to `&dyn Fn(FlashEvent)`.

Migration table for each `LogLine`:

```rust
// Line ~88: boot mode prompt → Warning (user must act):
progress(FlashEvent::Warning {
    message: "Device not in ROM download mode — hold BOOT/A9 pin LOW, then power-cycle the device".into(),
});

// Line ~216: "Reading 0x..." → developer detail:
log::info!("Reading 0x{:08x}..0x{:08x} ({} bytes)", start, end, length);

// Line ~277: "Read complete: N bytes saved to path." → done is handled by run_job:
log::info!("Read complete: {} bytes saved to {}", buf.len(), file_path);

// Line ~317: "Erasing 0x..." → developer detail:
log::info!("Erasing 0x{:08x}..0x{:08x} ({} bytes)", start, end, length);

// Line ~324: "Erase complete." → milestone:
progress(FlashEvent::Milestone { milestone: FlashMilestone::EraseComplete });

// Line ~371: "Segment X/Y: erasing 0x..." → developer detail:
log::info!("Segment {}/{}: erasing 0x{:08x}..0x{:08x}", idx+1, total_segs, start, start+erase_len);

// Line ~390: "Writing N bytes..." → developer detail:
log::info!("Writing {} bytes at 0x{:08x}", data.len(), start);

// Line ~408: "Segment X/Y written (N bytes)." → milestone:
progress(FlashEvent::Milestone {
    milestone: FlashMilestone::SegmentWritten {
        current: (idx + 1) as u32,
        total: total_segs as u32,
    },
});
```

Change Phase emissions from string-based to typed:
```rust
FlashProgress::Phase { name: "connecting".into() } → FlashEvent::Phase { phase: FlashPhase::Connect }
FlashProgress::Phase { name: "reading".into() }    → FlashEvent::Phase { phase: FlashPhase::Read }
FlashProgress::Phase { name: "saving".into() }     → FlashEvent::Phase { phase: FlashPhase::Save }
FlashProgress::Phase { name: "erasing".into() }    → FlashEvent::Phase { phase: FlashPhase::Erase }
FlashProgress::Phase { name: "rebooting".into() }  → FlashEvent::Phase { phase: FlashPhase::Reboot }
// Multi-segment:
FlashProgress::Phase { name: format!("segment_{}_of_{}", idx+1, total_segs) }
  → FlashEvent::Phase { phase: FlashPhase::WriteSegment { current: (idx+1) as u32, total: total_segs as u32 } }
```

Change `FlashProgress::Percent { value: v }` → `FlashEvent::Percent { value: v }` throughout.

- [ ] **Step 2.7: Update `authorize.rs` — migrate `AuthReadComplete`**

Replace import:
```rust
use crate::progress::FlashProgress;
// →
use crate::flash_event::{FlashEvent, FlashMilestone};
```

Change `run_authorize` signature:
```rust
pub fn run_authorize<F>(job: &FlashJob, cancel: &AtomicBool, progress: F) -> Result<(), FlashError>
where
    F: Fn(FlashEvent),
```

Replace `emit_log_key(progress, "flash.log.auth.readResult", &[...])` with the `AuthReadComplete` milestone:
```rust
progress(FlashEvent::Milestone {
    milestone: FlashMilestone::AuthReadComplete {
        uuid: uuid.to_string(),
        authkey: authkey.to_string(),
    },
});
```

Delete the `emit_log_key` helper function in `authorize.rs`.

- [ ] **Step 2.8: Remove `FlashProgress` export from `lib.rs`**

In `crates/tyutool-core/src/lib.rs`:
```rust
// Remove this line:
pub use progress::FlashProgress;
// Also remove: mod progress; (it becomes dead code)
```

Also remove `mod progress;` from the module declarations. The `progress.rs` file can stay on disk but is no longer part of the module tree.

- [ ] **Step 2.9: Verify tyutool-core compiles and tests pass**

```bash
cargo build -p tyutool-core 2>&1 | tail -5
cargo test -p tyutool-core 2>&1 | tail -10
```
Expected: no errors, all tests pass.

- [ ] **Step 2.10: Commit**

```bash
git add crates/tyutool-core/src/
git commit -m "feat(core): switch FlashPlugin and run_job to FlashEvent

- plugin.rs: FlashPlugin::run callback changes from FlashProgress to FlashEvent
- registry.rs: run_job emits JobSummary first, wraps with timer, maps Cancelled
- bk7231n: migrate LogKey/LogLine to typed FlashEvent variants
- t1/t3/t5ai: update callback type (delegation unchanged)
- esp: migrate LogKey to Connected milestone and WriteSegment phase
- ln882h: migrate LogLine to Warning/EraseComplete/SegmentWritten
- authorize: migrate AuthReadComplete LogKey to typed milestone"
```

---

## Task 3: Update CLI — file logging, banner, rewrite CliReporter

**Files:**
- Modify: `crates/tyutool-cli/Cargo.toml`
- Modify: `crates/tyutool-cli/src/main.rs`
- Modify: `crates/tyutool-cli/src/reporter.rs`
- Modify: `crates/tyutool-cli/src/serve.rs`

- [ ] **Step 3.1: Add dependencies to `crates/tyutool-cli/Cargo.toml`**

Add to the `[dependencies]` section:
```toml
fern = "0.7"
chrono = { version = "0.4", features = ["clock"] }
dirs = "5"
```

Remove `env_logger` from dependencies (replaced by `fern`).

- [ ] **Step 3.2: Add `--verbose` flag and `init_logging` to `main.rs`**

Add `verbose` to the top-level `Cli` struct:
```rust
#[derive(Parser)]
#[command(name = "tyutool", version, about = "Tuya Uart Tool.")]
struct Cli {
    /// Also write developer logs to stderr (always writes to log file)
    #[arg(long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}
```

Add the `init_logging` function before `main`:
```rust
fn init_logging(verbose: bool) -> Result<(), Box<dyn std::error::Error>> {
    let log_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("tyutool");
    std::fs::create_dir_all(&log_dir)?;
    let log_path = log_dir.join("tyutool.log");

    let fmt = |out: fern::FormatCallback<'_>,
               message: &std::fmt::Arguments<'_>,
               record: &log::Record<'_>| {
        out.finish(format_args!(
            "[{} {} {}] {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            record.level(),
            record.target(),
            message
        ))
    };

    let mut dispatch = fern::Dispatch::new()
        .format(fmt)
        .level(log::LevelFilter::Info)
        .chain(fern::log_file(&log_path)?);

    if verbose {
        dispatch = dispatch.chain(
            fern::Dispatch::new()
                .format(|out, message, record| {
                    out.finish(format_args!(
                        "[{} {}] {}",
                        record.level(),
                        record.target(),
                        message
                    ))
                })
                .chain(std::io::stderr()),
        );
        eprintln!("[log] Writing to: {}", log_path.display());
    }

    dispatch.apply()?;
    Ok(())
}
```

In `main()`, replace the `env_logger::Builder` initialization block and remove the startup banner `log::info!` calls, replacing with:
```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Init file logging (+ stderr if --verbose)
    let survey_json_only = matches!(cli.command, Commands::UsbPortSurvey);
    if !survey_json_only {
        init_logging(cli.verbose)?;
    }

    // User-facing startup banner (not a log::info! call)
    if !survey_json_only {
        eprintln!(
            "tyutool v{}  {}/{}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        eprintln!();
    }

    // Developer diagnostics → log file (not shown to user)
    if !survey_json_only {
        log::info!("========================================");
        log::info!("[App] tyutool-cli v{} starting", env!("CARGO_PKG_VERSION"));
        log::info!("[App] Type: CLI");
        log::info!(
            "[App] OS: {}, Arch: {}, Family: {}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::env::consts::FAMILY
        );
        if let Ok(exe) = std::env::current_exe() {
            log::info!("[App] Exe: {}", exe.display());
        }
        log::info!("========================================");
    }

    match cli.command {
        // ... existing match arms, but update each to use CliReporter::new()
    }
}
```

Update the `Authorize` command arm in `main()` to use `CliReporter` instead of the inline callback:
```rust
Commands::Authorize { port, uuid, authkey } => {
    let port = match port { Some(p) => p, None => choose_port()? };
    let job = FlashJob {
        mode: FlashMode::Authorize,
        chip_id: String::new(),
        port,
        baud_rate: 115_200,
        // ... all other fields None
        authorize_uuid: uuid,
        authorize_key: authkey,
        ..Default::default()  // or fill in None fields explicitly
    };
    let cancel = AtomicBool::new(false);
    let reporter = CliReporter::new();
    run_job(&job, &cancel, reporter.callback())
        .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;
}
```

Note: Remove the `JobInfo` struct import from `reporter` since it no longer exists.

- [ ] **Step 3.3: Rewrite `reporter.rs`**

Replace the entire file:

```rust
use std::sync::Mutex;

use indicatif::{ProgressBar, ProgressStyle};
use tyutool_core::{
    FlashEvent, FlashMilestone, FlashPhase, FlashResult, JobDetails, JobSummary,
};

pub struct CliReporter {
    inner: Mutex<Inner>,
}

struct Inner {
    pb: ProgressBar,
    is_plain: bool,
    current_phase_label: Option<String>,
    next_milestone: u8,
}

impl CliReporter {
    pub fn new() -> Self {
        let is_plain = !console::Term::stderr().is_term();

        let pb = ProgressBar::new(100);
        if is_plain {
            pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        } else {
            pb.set_style(
                ProgressStyle::with_template(
                    "  {spinner:.cyan} {msg:<16} {bar:25.cyan/black}  {percent:>3}%",
                )
                .unwrap()
                .progress_chars("━━░"),
            );
            pb.enable_steady_tick(std::time::Duration::from_millis(80));
        }

        Self {
            inner: Mutex::new(Inner {
                pb,
                is_plain,
                current_phase_label: None,
                next_milestone: 10,
            }),
        }
    }

    pub fn callback(&self) -> impl Fn(FlashEvent) + '_ {
        move |e| self.inner.lock().unwrap().handle(e)
    }
}

impl Inner {
    fn handle(&mut self, e: FlashEvent) {
        match e {
            FlashEvent::JobSummary(s) => self.on_job_summary(s),
            FlashEvent::Phase { phase } => self.on_phase(phase),
            FlashEvent::Percent { value } => self.on_percent(value),
            FlashEvent::Milestone { milestone } => self.on_milestone(milestone),
            FlashEvent::Warning { message } => self.on_warning(message),
            FlashEvent::Done { result } => self.on_done(result),
        }
    }

    fn on_job_summary(&mut self, s: JobSummary) {
        let sep = if self.is_plain { "->" } else { "→" };

        match &s.details {
            JobDetails::Flash {
                firmware_path,
                firmware_size,
                range_start,
                range_end,
            } => {
                let device = s.device.as_deref().unwrap_or("?");
                if self.is_plain {
                    eprintln!("write  {}  {}  {}", device, s.port, s.baud);
                } else {
                    eprintln!("write · {} · {} @ {}", device, s.port, s.baud);
                }
                let size_str = firmware_size
                    .map(|b| format!("  {}", format_file_size(b)))
                    .unwrap_or_default();
                eprintln!("  File   {}{}", firmware_path, size_str);
                eprintln!("  Range  {} {} {}", range_start, sep, range_end);
            }
            JobDetails::Read {
                output_path,
                range_start,
                range_end,
            } => {
                let device = s.device.as_deref().unwrap_or("?");
                if self.is_plain {
                    eprintln!("read  {}  {}  {}", device, s.port, s.baud);
                } else {
                    eprintln!("read · {} · {} @ {}", device, s.port, s.baud);
                }
                eprintln!("  Output {}", output_path);
                eprintln!("  Range  {} {} {}", range_start, sep, range_end);
            }
            JobDetails::Erase { range_start, range_end } => {
                let device = s.device.as_deref().unwrap_or("?");
                if self.is_plain {
                    eprintln!("erase  {}  {}  {}", device, s.port, s.baud);
                } else {
                    eprintln!("erase · {} · {} @ {}", device, s.port, s.baud);
                }
                eprintln!("  Range  {} {} {}", range_start, sep, range_end);
            }
            JobDetails::Authorize { write } => {
                let mode = if *write { "write" } else { "read-only" };
                if self.is_plain {
                    eprintln!("authorize  {}  {}  [{}]", s.port, s.baud, mode);
                } else {
                    eprintln!("authorize · {} @ {}  [{}]", s.port, s.baud, mode);
                }
            }
        }
        eprintln!();
    }

    fn on_phase(&mut self, phase: FlashPhase) {
        let label = phase_label(&phase);
        self.finish_current_phase();
        self.current_phase_label = Some(label.clone());
        self.next_milestone = 10;

        if self.is_plain {
            eprint!("{:<16}", label);
        } else {
            self.pb.set_position(0);
            self.pb.set_message(label);
        }
    }

    fn finish_current_phase(&mut self) {
        if let Some(label) = self.current_phase_label.take() {
            if self.is_plain {
                eprintln!("  OK");
            } else {
                self.pb.println(format!("  \x1b[32m✓\x1b[0m {}", label));
                self.pb.set_position(0);
            }
        }
    }

    fn on_percent(&mut self, value: u8) {
        let label = match &self.current_phase_label {
            Some(l) => l.clone(),
            None => return,
        };

        if self.is_plain {
            if is_long_phase(&label) {
                for m in pop_milestones(&mut self.next_milestone, value) {
                    eprint!("  {}%", m);
                }
            }
        } else {
            self.pb.set_position(value as u64);
        }
    }

    fn on_milestone(&mut self, milestone: FlashMilestone) {
        let text = milestone_text(&milestone);
        if self.is_plain {
            eprintln!("[OK] {}", text);
        } else {
            self.pb.println(format!("  \x1b[32m✓\x1b[0m {}", text));
        }

        // AuthReadComplete: print credentials on their own lines (plain mode).
        // In rich mode the GUI handles the secure modal; CLI shows it plainly.
        if let FlashMilestone::AuthReadComplete { uuid, authkey } = &milestone {
            if self.is_plain {
                eprintln!("  UUID:    {}", uuid);
                eprintln!("  AuthKey: {}", authkey);
            } else {
                self.pb.println(format!("  UUID:    {}", uuid));
                self.pb.println(format!("  AuthKey: {}", authkey));
            }
        }
    }

    fn on_warning(&mut self, message: String) {
        if self.is_plain {
            eprintln!("[WARN] {}", message);
        } else {
            self.pb.println(format!("  \x1b[33m⚠\x1b[0m {}", message));
        }
    }

    fn on_done(&mut self, result: FlashResult) {
        self.finish_current_phase();

        if self.is_plain {
            match result {
                FlashResult::Ok { elapsed_secs } => {
                    eprintln!("Flash OK  {:.1}s", elapsed_secs);
                }
                FlashResult::Err { message, elapsed_secs } => {
                    eprintln!("Flash FAILED: {}  {:.1}s", message, elapsed_secs);
                }
                FlashResult::Cancelled { elapsed_secs } => {
                    eprintln!("Flash CANCELLED  {:.1}s", elapsed_secs);
                }
            }
        } else {
            self.pb.finish_and_clear();
            match result {
                FlashResult::Ok { elapsed_secs } => {
                    eprintln!("  \x1b[32m✓\x1b[0m Flash complete  {:.1}s", elapsed_secs);
                }
                FlashResult::Err { message, elapsed_secs } => {
                    eprintln!(
                        "  \x1b[31m✗\x1b[0m Flash failed: {}  {:.1}s",
                        message, elapsed_secs
                    );
                }
                FlashResult::Cancelled { elapsed_secs } => {
                    eprintln!("  \x1b[33m✗\x1b[0m Cancelled  {:.1}s", elapsed_secs);
                }
            }
        }
    }
}

pub(crate) fn phase_label(phase: &FlashPhase) -> String {
    match phase {
        FlashPhase::Handshake => "Handshake".into(),
        FlashPhase::ReadFlashId => "Flash ID".into(),
        FlashPhase::Unprotect => "Unprotect".into(),
        FlashPhase::Erase => "Erase".into(),
        FlashPhase::WriteSegment { current, total } => format!("Write [{}/{}]", current, total),
        FlashPhase::Write => "Write".into(),
        FlashPhase::Verify => "Verify".into(),
        FlashPhase::Protect => "Protect".into(),
        FlashPhase::Reboot => "Reboot".into(),
        FlashPhase::Read => "Read".into(),
        FlashPhase::Save => "Save".into(),
        FlashPhase::LoadRam => "Load RAM".into(),
        FlashPhase::SwitchBaud => "Switch Baud".into(),
        FlashPhase::Connect => "Connect".into(),
        FlashPhase::Other(s) => s.clone(),
    }
}

fn milestone_text(m: &FlashMilestone) -> String {
    match m {
        FlashMilestone::HandshakeComplete => "Handshake complete".into(),
        FlashMilestone::Connected { chip_info: Some(info) } => format!("Connected: {}", info),
        FlashMilestone::Connected { chip_info: None } => "Connected".into(),
        FlashMilestone::FlashIdRead { mid: Some(mid) } => format!("Flash ID: {:#010x}", mid),
        FlashMilestone::FlashIdRead { mid: None } => "Flash ID read".into(),
        FlashMilestone::EraseComplete => "Erase complete".into(),
        FlashMilestone::SegmentWritten { current, total } => {
            format!("Segment {}/{} written", current, total)
        }
        FlashMilestone::WriteComplete => "Write complete".into(),
        FlashMilestone::VerifyPassed => "Verify passed".into(),
        FlashMilestone::Rebooted => "Device rebooted".into(),
        FlashMilestone::AuthReadComplete { .. } => "Auth read complete".into(),
    }
}

pub(crate) fn is_long_phase(label: &str) -> bool {
    label == "Write"
        || label == "Erase"
        || label == "Read"
        || label.starts_with("Write [")
}

pub(crate) fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub(crate) fn pop_milestones(next_milestone: &mut u8, value: u8) -> Vec<u8> {
    let mut out = Vec::new();
    while *next_milestone < 100 && value >= *next_milestone {
        out.push(*next_milestone);
        *next_milestone = next_milestone.saturating_add(10);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_label_write_segment() {
        let label = phase_label(&FlashPhase::WriteSegment { current: 2, total: 3 });
        assert_eq!(label, "Write [2/3]");
    }

    #[test]
    fn phase_label_known_phases() {
        assert_eq!(phase_label(&FlashPhase::Handshake), "Handshake");
        assert_eq!(phase_label(&FlashPhase::LoadRam), "Load RAM");
        assert_eq!(phase_label(&FlashPhase::SwitchBaud), "Switch Baud");
        assert_eq!(phase_label(&FlashPhase::Other("NewPhase".into())), "NewPhase");
    }

    #[test]
    fn is_long_phase_detection() {
        assert!(is_long_phase("Write"));
        assert!(is_long_phase("Erase"));
        assert!(is_long_phase("Read"));
        assert!(is_long_phase("Write [1/3]"));
        assert!(!is_long_phase("Handshake"));
        assert!(!is_long_phase("Verify"));
    }

    #[test]
    fn pop_milestones_multiple() {
        let mut m: u8 = 10;
        assert_eq!(pop_milestones(&mut m, 35), vec![10, 20, 30]);
        assert_eq!(m, 40);
    }

    #[test]
    fn format_file_size_variants() {
        assert_eq!(format_file_size(512), "512 B");
        assert_eq!(format_file_size(2048), "2.0 KiB");
        assert_eq!(format_file_size(1_887_437), "1.8 MiB");
    }
}
```

- [ ] **Step 3.4: Update `serve.rs` callback type**

In `crates/tyutool-cli/src/serve.rs`, find the `run_job` callback and update the type:

```rust
// Replace import:
use tyutool_core::FlashProgress;
// With:
use tyutool_core::FlashEvent;

// In the run_job callback, change the closure parameter type from FlashProgress to FlashEvent:
// The existing serialization `serde_json::to_string(&p)` works unchanged since FlashEvent
// derives Serialize with the same serde shape the frontend expects.
```

- [ ] **Step 3.5: Build and run tests**

```bash
cargo build -p tyutool-cli 2>&1 | tail -10
cargo test -p tyutool-cli 2>&1 | tail -10
```
Expected: no errors, reporter tests pass.

- [ ] **Step 3.6: Commit**

```bash
git add crates/tyutool-cli/
git commit -m "feat(cli): rewrite CliReporter for FlashEvent, add file logging

- Add fern/chrono/dirs deps for cross-platform file logging
- init_logging(): always write to {data_dir}/tyutool/tyutool.log
- --verbose flag: also write to stderr
- Banner: single eprintln! line, not log::info!
- reporter.rs: handle all FlashEvent variants (rich + plain modes)
- JobSummary drives the job header (replaces JobInfo struct)
- Warning variant prints with ⚠ prefix
- FlashResult::Cancelled renders 'CANCELLED'
- authorize command now uses CliReporter"
```

---

## Task 4: Update Tauri backend

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 4.1: Update `flash_run` callback type**

In `src-tauri/src/lib.rs`, the `flash_run` function currently calls `run_job` with a `FlashProgress` callback. After Task 2, `run_job` expects `Fn(FlashEvent)`. The `app.emit("flash-progress", &p)` call works unchanged because `FlashEvent` derives `Serialize`.

Find the `flash_run` function (~line 155) and update the import and type annotation:

```rust
// The import at the top of lib.rs — remove FlashProgress if imported:
// use tyutool_core::FlashProgress;  ← remove if present

// The run_job call — the closure type changes automatically since FlashEvent
// now implements Serialize with the same JSON tag "kind" the frontend reads:
std::thread::spawn(move || {
    let _ = tyutool_core::run_job(&job, &cancel, |p| {
        let _ = app.emit("flash-progress", &p);
    });
});
```

No other changes needed — Tauri serializes `FlashEvent` to JSON matching the new wire format the frontend will read after Task 5.

Update the startup banner to use `eprintln!` style (it currently uses `log::info!`). The GUI startup banner stays as `log::info!` since it routes to the log file (not user-visible in the UI).

- [ ] **Step 4.2: Build Tauri backend**

```bash
cargo build -p tyutool-app 2>&1 | tail -10
```
If the package name differs, check with `cargo metadata --no-deps | grep '"name"'` for the src-tauri package.

Expected: builds successfully.

- [ ] **Step 4.3: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(tauri): update flash_run callback to FlashEvent"
```

---

## Task 5: Update frontend TypeScript types and flash store

**Files:**
- Modify: `src/features/firmware-flash/flash-tauri.ts`
- Modify: `src/stores/flash.ts`
- Modify: `src/features/firmware-flash/ws-transport.ts`

- [ ] **Step 5.1: Replace `FlashProgressPayload` in `flash-tauri.ts`**

Replace the `FlashProgressPayload` type and add all supporting types:

```typescript
// src/features/firmware-flash/flash-tauri.ts
// Types aligned with tyutool_core::FlashEvent (snake_case JSON tag "kind")

export type FlashPhase =
  | 'handshake' | 'read_flash_id' | 'unprotect' | 'erase'
  | 'write' | 'verify' | 'protect' | 'reboot' | 'read' | 'save'
  | 'load_ram' | 'switch_baud' | 'connect'
  | { write_segment: { current: number; total: number } }
  | { other: string };

export type FlashMilestone =
  | 'handshake_complete' | 'erase_complete' | 'write_complete' | 'verify_passed' | 'rebooted'
  | { connected: { chip_info: string | null } }
  | { flash_id_read: { mid: number | null } }
  | { segment_written: { current: number; total: number } }
  | { auth_read_complete: { uuid: string; authkey: string } };

export type FlashResultPayload =
  | { ok: { elapsed_secs: number } }
  | { err: { message: string; elapsed_secs: number } }
  | { cancelled: { elapsed_secs: number } };

export type JobDetails =
  | { type: 'flash'; firmware_path: string; firmware_size: number | null; range_start: string; range_end: string }
  | { type: 'read'; output_path: string; range_start: string; range_end: string }
  | { type: 'erase'; range_start: string; range_end: string }
  | { type: 'authorize'; write: boolean };

export type JobSummaryPayload = {
  port: string;
  baud: number;
  device: string | null;
  details: JobDetails;
};

export type FlashProgressPayload =
  | { kind: 'job_summary' } & JobSummaryPayload
  | { kind: 'phase'; phase: FlashPhase }
  | { kind: 'percent'; value: number }
  | { kind: 'milestone'; milestone: FlashMilestone }
  | { kind: 'warning'; message: string }
  | { kind: 'done'; result: FlashResultPayload };
```

- [ ] **Step 5.2: Update `handleFlashProgressPayload` in `flash.ts`**

Replace the `handleFlashProgressPayload` function in `src/stores/flash.ts`:

```typescript
function handleFlashProgressPayload(p: FlashProgressPayload): void {
  if (p.kind === 'job_summary') {
    // Job header is rendered by the existing UI state (port, device etc.)
    // derived from the FlashJob that was already submitted — no extra state needed.
    return;
  }

  if (p.kind === 'percent') {
    flashProgress.value = Math.min(100, Math.max(0, p.value));
    return;
  }

  if (p.kind === 'phase') {
    // Phase changes are implicit in existing progress + log flow; no dedicated state needed.
    return;
  }

  if (p.kind === 'milestone') {
    const m = p.milestone;
    if (typeof m === 'object' && 'auth_read_complete' in m) {
      const { uuid, authkey } = m.auth_read_complete;
      const copyText = `UUID:${uuid}\nAuthKey:${authkey}`;
      void showConfirmDialog({
        title: t('flash.confirm.authReadTitle'),
        message: t('flash.confirm.authReadBody', { uuid, authkey }),
        kind: 'info',
        extraActionLabel: t('flash.confirm.authReadCopyCmd'),
        onExtraAction: async () => {
          try {
            await navigator.clipboard.writeText(copyText);
            appendLog(t('flash.log.authReadCopied'));
          } catch {
            appendLog(t('flash.log.copyFailed'));
          }
        },
        okLabel: t('flash.confirm.authReadOk'),
        showCancel: false,
      });
      appendLog(t('flash.log.authReadShown'));
      return;
    }
    // Other milestones: append i18n log line
    const milestoneKey = typeof m === 'string' ? m : Object.keys(m)[0];
    const i18nKey = `flash.log.milestone.${milestoneKey}`;
    appendLog(i18n.global.te(i18nKey) ? t(i18nKey) : `[${milestoneKey}]`);
    return;
  }

  if (p.kind === 'warning') {
    appendLog(`⚠ ${p.message}`);
    return;
  }

  if (p.kind === 'done') {
    const op = runningOp.value;
    runningOp.value = null;
    authOpIsRead.value = false;

    const result = p.result;
    if ('ok' in result) {
      flashPhase.value = 'success';
      flashProgress.value = 100;
      if (op === 'flash') {
        flashMessage.value = t('flash.msg.flashDone');
        appendLog(t('flash.log.verifyOk'));
      } else if (op === 'erase') {
        flashMessage.value = t('flash.msg.eraseDone');
        appendLog(t('flash.log.eraseDoneLog'));
      } else if (op === 'read') {
        flashMessage.value = t('flash.msg.readDone');
        appendLog(t('flash.log.readDoneLog'));
      } else if (op === 'authorize') {
        flashMessage.value = t('flash.msg.authDone');
        appendLog(t('flash.log.authOkLog'));
      }
      rLog.info(`[Flash] Operation '${op}' completed in ${result.ok.elapsed_secs.toFixed(1)}s`);
    } else if ('cancelled' in result) {
      flashPhase.value = 'error';
      flashMessage.value = t('flash.msg.cancelled', { fallback: 'Cancelled' });
      rLog.info(`[Flash] Operation '${op}' cancelled`);
    } else {
      // err
      flashPhase.value = 'error';
      const displayMsg = result.err.message
        ? mapBackendUserMessage(result.err.message)
        : t('flash.err.withMsg', { msg: 'unknown' });
      flashMessage.value = displayMsg;
      appendLog(t('flash.err.withMsg', { msg: displayMsg }));
      rLog.error(`[Flash] Operation '${op}' failed: ${flashMessage.value}`);
    }
    logOperationDuration();
    if (autoConnected.value) {
      autoConnected.value = false;
    }
  }
}
```

- [ ] **Step 5.3: Update `ws-transport.ts` auth handling**

In `src/features/firmware-flash/ws-transport.ts`, find the probe auth function (~line 168) and update the milestone detection:

```typescript
// Old:
if (ev.payload.kind === 'log_key' && ev.payload.key === 'flash.log.auth.readResult') {
  const uuid = ev.payload.params?.uuid?.trim() ?? '';
  const authkey = ev.payload.params?.authkey?.trim() ?? '';
  if (uuid && authkey) {
    found = { uuid, authkey };
  }
}

// New:
if (ev.payload.kind === 'milestone' && typeof ev.payload.milestone === 'object'
    && 'auth_read_complete' in ev.payload.milestone) {
  const { uuid, authkey } = ev.payload.milestone.auth_read_complete;
  if (uuid && authkey) {
    found = { uuid: uuid.trim(), authkey: authkey.trim() };
  }
}
```

Also update the `done` detection in the WebSocket message handler from the old `ok`/`message` shape to the new `result` shape:

```typescript
// Old (approximate):
if (kind === 'done') {
  const ok = p['ok'] as boolean;
  // ...
}

// New:
if (kind === 'done') {
  const result = p['result'] as Record<string, unknown>;
  const ok = 'ok' in result;
  // Use ok/cancelled/err variants as needed
}
```

- [ ] **Step 5.4: Run frontend type check and tests**

```bash
pnpm run build 2>&1 | tail -20
pnpm run test 2>&1 | tail -20
```
Expected: no type errors, tests pass.

- [ ] **Step 5.5: Commit**

```bash
git add src/features/firmware-flash/flash-tauri.ts \
        src/stores/flash.ts \
        src/features/firmware-flash/ws-transport.ts
git commit -m "feat(frontend): update FlashEvent TypeScript types and handlers

- FlashProgressPayload: add job_summary, milestone, warning, done.result variants
- handleFlashProgressPayload: handle new event shape (result.ok/err/cancelled)
- auth_read_complete: read from typed milestone instead of log_key string
- flash.ts: show cancelled state
- ws-transport: update auth probe and done detection"
```

---

## Task 6: Create `docs/cli.md` — CLI reference

**Files:**
- Create: `docs/cli.md`

- [ ] **Step 6.1: Create the file**

```bash
cat > docs/cli.md << 'HEREDOC'
# tyutool CLI Reference

`tyutool` is a command-line tool for flashing, reading, and managing Tuya-class IoT device firmware over UART.

## Installation

Download the latest release binary from the GitHub Releases page. Place it on your `PATH`.

## Global Options

| Option | Description |
|--------|-------------|
| `--verbose` | Write developer diagnostic logs to stderr (always written to log file) |

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
| `t5ai` | T5AI | 921600 |
| `ln882h` | LN882H | 115200 |
| `esp32` | ESP32 | 460800 |
| `esp32c3` | ESP32-C3 | 460800 |
| `esp32c6` | ESP32-C6 | 460800 |
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
HEREDOC
```

- [ ] **Step 6.2: Commit**

```bash
git add docs/cli.md
git commit -m "docs: create docs/cli.md — authoritative CLI reference"
```

---

## Task 7: Update CLAUDE.md with logging contract

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 7.1: Add logging contract section to `CLAUDE.md`**

Add the following section after the existing "Key conventions" section:

```markdown
### Logging Contract

tyutool has two independent output channels — keep them strictly separate:

```
tyutool-core
    │
    ├─► FlashEvent callback  →  user-visible (CLI terminal / GUI / WebSocket)
    └─► log::* macros        →  developer diagnostics (file; optionally stderr)
```

**User-visible → `FlashEvent` callback**

Use `FlashEvent` whenever the user needs to see the information:
- Job metadata (firmware size, port, device) → `FlashEvent::JobSummary`
- Phase transitions → `FlashEvent::Phase(FlashPhase::*)` — use typed variants, not `Other(String)`
- Progress → `FlashEvent::Percent`
- Key milestones (connected, erase complete, etc.) → `FlashEvent::Milestone(FlashMilestone::*)`
- User action required → `FlashEvent::Warning { message }`
- Final outcome → `FlashEvent::Done`

**Developer-only → `log::*` macros**

Use `log::info!` / `log::debug!` / `log::warn!` / `log::error!` for diagnostic information:
- Protocol frame contents, byte addresses, sector offsets
- Retry counts, internal state transitions
- Any detail a user cannot act on

**Decision rule:** Ask yourself: "Can the user make a decision based on this?" → `FlashEvent`. Otherwise → `log::*`.

**Prohibited:**
- Using `log::info!` for user-visible content
- Using bare string variants (`FlashPhase::Other`) for new phases — add a typed variant instead
- Displaying `AuthReadComplete` credentials as plain log text in GUI (must use secure modal)

**Routing per platform:**

| Platform | FlashEvent | log::* |
|----------|-----------|--------|
| CLI | CliReporter → stderr | `{data_dir}/tyutool/tyutool.log` (`--verbose` also → stderr) |
| GUI (Tauri) | Tauri event → UI | tauri-plugin-log → file (level controlled by developer setting) |
| Web/IDE | WebSocket JSON → browser UI | CLI-side log file |

### CLI Command Documentation

`docs/cli.md` is the authoritative CLI reference. **Any change to CLI commands must include a `docs/cli.md` update in the same commit or PR:**

- Adding a subcommand or flag
- Removing or renaming a subcommand or flag
- Changing a default value or behavior

PRs that modify `crates/tyutool-cli/src/main.rs` (command definitions) without updating `docs/cli.md` must not be merged.
```

- [ ] **Step 7.2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs(claude): add logging contract and CLI doc sync rule to CLAUDE.md"
```

---

## Self-Review

### Spec coverage check

| Spec requirement | Task |
|-----------------|------|
| `FlashEvent` typed variants | Task 1 |
| `JobSummary::from_job` constructs mode-aware summary | Task 1 |
| `FlashResult::Cancelled` variant | Task 1 |
| Serde wire format with `#[serde(tag = "kind")]` | Task 1 |
| `FlashPlugin::run` callback type switch | Task 2 |
| `run_job` emits `JobSummary` as first event | Task 2 |
| `run_job` measures elapsed, puts in `Done` | Task 2 |
| LogKey migrations (all 6 keys) | Task 2 |
| LogLine migrations (all 8 LN882H sites) | Task 2 |
| `AuthReadComplete` milestone (auth) | Task 2 |
| CLI `--verbose` flag | Task 3 |
| CLI file logging via `fern` + `dirs` | Task 3 |
| CLI startup banner simplified | Task 3 |
| `CliReporter` handles all `FlashEvent` variants | Task 3 |
| `CliReporter` rich mode (TTY) | Task 3 |
| `CliReporter` plain mode (non-TTY) | Task 3 |
| `Warning` displayed with `⚠` | Task 3 |
| `FlashResult::Cancelled` rendered in CLI | Task 3 |
| Authorize command uses `CliReporter` | Task 3 |
| Tauri `flash_run` callback type | Task 4 |
| TypeScript `FlashProgressPayload` rewrite | Task 5 |
| `handleFlashProgressPayload` for new events | Task 5 |
| `auth_read_complete` → secure modal | Task 5 |
| `ws-transport` auth probe updated | Task 5 |
| `file_content` WebSocket message preserved as-is | Task 5 (no change) |
| `docs/cli.md` created with full reference | Task 6 |
| CLAUDE.md logging contract | Task 7 |
| CLAUDE.md CLI doc sync rule | Task 7 |
| GUI settings log level label clarification | Task 4 (noted; UI string change needed separately) |

**Gap:** The spec mentions updating the GUI settings page label to say "开发者日志文件等级（不影响界面显示）". This is a UI string change in the Vue settings component — not included in this plan since it is a minor cosmetic change. Add as a follow-up task if desired.
