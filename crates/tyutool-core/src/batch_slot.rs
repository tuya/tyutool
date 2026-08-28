//! Batch-auth slot orchestration: what one serial port goes through during a
//! batch run — optional firmware flash, then the authorize transaction, with
//! the Excel row allocated, updated and released around it.
//!
//! This is the layer above `authorize::run_batch_auth_slot` (which performs the
//! single device transaction) and above `batch_auth::ExcelRowAllocator` (which
//! owns the sheet). It depends on no frontend: progress is reported through an
//! `emit` callback, so a Tauri command forwards to `app.emit` while a test
//! records the payloads. The thread pool that runs several of these at once
//! stays with the frontend that owns the port list.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use crate::batch_auth;

/// Excel allocator + count of slot threads using it, guarded by ONE mutex so
/// acquire (batch_auth_start) and release (last SlotSessionGuard drop) can
/// never interleave. Invariant: `alloc.is_some() ⇒ active > 0` — the file
/// lock is held exactly while slots run, so the sheet can be edited between
/// batches and every new batch re-reads it from disk.
pub struct AllocatorSession {
    pub alloc: Option<std::sync::Arc<batch_auth::ExcelRowAllocator>>,
    pub active: usize,
}

/// Decrements the session's active count when a slot thread exits (any path,
/// including early returns and panics); the last one out releases the file
/// lock and drops the allocator.
struct SlotSessionGuard(Arc<StdMutex<AllocatorSession>>);

impl Drop for SlotSessionGuard {
    fn drop(&mut self) {
        if let Ok(mut session) = self.0.lock() {
            session.active = session.active.saturating_sub(1);
            if session.active == 0 {
                if let Some(alloc) = session.alloc.take() {
                    alloc.release_lock();
                    log::info!("[batch-auth] last slot finished; excel session closed");
                }
            }
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchAuthStartConfig {
    pub chip_id: String,
    pub baud_rate: u32,
    pub auth_baud_rate: u32,
    pub firmware_path: Option<String>,
    pub flash_start_hex: Option<String>,
    pub flash_end_hex: Option<String>,
    pub excel_path: String,
    pub conflict_policy: String,
    pub auth_storage: Option<String>,
    /// false ⇒ flash-only batch: skip the Excel session and the auth step.
    #[serde(default = "default_authorize_enabled")]
    pub authorize_enabled: bool,
}

fn default_authorize_enabled() -> bool {
    true
}

/// One batch-auth slot: optional firmware flash, then the authorize pipeline,
/// for a single port. Runs on its own thread.
///
/// `emit` receives every `batch-auth-progress` payload. Taking a callback rather
/// than an `AppHandle` is what makes this callable from a test: the production
/// caller forwards to `app.emit`, while a test can record the payloads and
/// assert the slot's step sequence.
#[allow(clippy::too_many_arguments)]
pub fn run_batch_slot(
    port: String,
    config: BatchAuthStartConfig,
    cancel: Arc<AtomicBool>,
    allocator: Option<Arc<batch_auth::ExcelRowAllocator>>,
    session: Arc<StdMutex<AllocatorSession>>,
    trace: Option<Arc<StdMutex<crate::BatchAuthTraceWriter>>>,
    conflict_policy: crate::ConflictPolicy,
    auth_storage: crate::AuthStorage,
    emit: &dyn Fn(serde_json::Value),
) {
    // Must be the first statement: every exit path (early returns,
    // panics) has to decrement the session count. Flash-only batches
    // never incremented it, so they get no guard.
    let _session_guard = allocator.is_some().then(|| SlotSessionGuard(session));
    // Last Excel write failure for this slot; attached to the final
    // progress emit so the operator sees the sheet was NOT updated.
    let excel_err: Arc<StdMutex<Option<String>>> = Arc::new(StdMutex::new(None));
    log::info!(
        "[batch-auth] slot begin  port={port} chip={}",
        config.chip_id
    );
    if let Some(ref fw_path) = config.firmware_path {
        if !fw_path.is_empty() {
            let job = crate::FlashJob {
                firmware_path: Some(fw_path.clone()),
                flash_start_hex: config.flash_start_hex.clone(),
                flash_end_hex: config.flash_end_hex.clone(),
                ..crate::FlashJob::new(
                    crate::FlashMode::Flash,
                    config.chip_id.clone(),
                    port.clone(),
                    config.baud_rate,
                )
            };
            log::info!(
                "[batch-auth] flash start  port={port} chip={} firmware={fw_path}",
                config.chip_id
            );
            let port2 = port.clone();
            let flash_result = crate::run_job(&job, &cancel, |p| {
                emit(serde_json::json!({
                    "port": port2,
                    "step": "flashing",
                    "event": p
                }));
            });
            if !config.authorize_enabled {
                // Flash-only: a user cancel is not a failure — mirror
                // the auth pipeline's "cancelled" step so the slot
                // returns to idle without polluting the cumulative
                // stats. On Ok, a late cancel changes nothing: the
                // run_job Done event forwarded above is already the
                // terminal signal, so emitting a second outcome here
                // would double-count the slot.
                match flash_result {
                    Err(crate::FlashError::Cancelled) => {
                        log::info!("[batch-auth] flash cancelled  port={port}");
                        emit(serde_json::json!({
                            "port": port,
                            "step": "cancelled"
                        }));
                        return;
                    }
                    Err(e) => {
                        log::warn!("[batch-auth] flash failed  port={port} error={e}");
                        emit(serde_json::json!({
                            "port": port,
                            "step": "failed",
                            "error": e.to_string()
                        }));
                        return;
                    }
                    Ok(()) => {}
                }
            } else if flash_result.is_err() || cancel.load(Ordering::Relaxed) {
                let error = flash_result
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "cancelled".into());
                log::warn!("[batch-auth] flash failed  port={port} error={error}");
                emit(serde_json::json!({
                    "port": port,
                    "step": "failed",
                    "error": error
                }));
                return;
            }
            log::info!("[batch-auth] flash done  port={port}");
            if config.authorize_enabled {
                // Wait for the device to boot naturally after flash before the auth
                // slot issues a hardware reset. Non-fatal, and passive: it exits as
                // soon as the device answers, so only a silent device pays the full
                // WAIT_AFTER_FLASH_MAX window. Each port has its own thread, so a
                // longer worst case does not accumulate across the batch.
                crate::wait_after_firmware_flash(
                    &port,
                    config.auth_baud_rate,
                    &config.chip_id,
                    &cancel,
                );
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
            }
        }
    }

    // Flash-only batch: run_job's Done event (forwarded above under
    // step "flashing") is the terminal signal for the frontend —
    // nothing to authorize, no Excel to update.
    let Some(allocator) = allocator else {
        log::info!("[batch-auth] slot done (flash-only)  port={port}");
        return;
    };

    // find_by_mac: look up Excel row by device MAC address
    let alloc_find = allocator.clone();
    let find_by_mac =
        move |mac: &str| -> Option<(usize, String, String)> { alloc_find.find_by_mac(mac) };

    // allocate_row: claim a new unused row
    let alloc_alloc = allocator.clone();
    let allocate_row = move || -> Option<(usize, String, String)> {
        match alloc_alloc.allocate_row() {
            Ok(row) => Some((row.row_idx, row.uuid, row.authkey)),
            Err(e) => {
                log::warn!("[batch-auth] allocate_row error: {e}");
                None
            }
        }
    };

    // update_row: translate BatchAuthRowUpdate → RowStatus + step_name, then write to Excel
    let alloc_update = allocator.clone();
    let port_for_update = port.clone();
    let excel_err_update = excel_err.clone();
    let update_row = move |row_idx: usize, mac: &str, update: crate::BatchAuthRowUpdate| {
        use crate::batch_auth::RowStatus;
        use crate::BatchAuthRowUpdate as U;
        let (status, step_name, error): (RowStatus, Option<&str>, Option<String>) = match update {
            U::MacRead => (RowStatus::MacRead, Some("mac_read"), None),
            U::AuthWritten => (RowStatus::AuthWritten, Some("auth_written"), None),
            U::AuthVerified => (RowStatus::AuthVerified, Some("auth_verified"), None),
            // Done: keep last step in Excel (STATUS=DONE is sufficient)
            U::Done => (RowStatus::Done, None, None),
            U::StepFailed { step, error } => {
                let status = if step == "auth_write" {
                    RowStatus::MacRead
                } else {
                    RowStatus::AuthWritten
                };
                (status, Some(step), Some(error))
            }
        };
        if let Err(e) =
            alloc_update.update_row_state(row_idx, mac, status, step_name, error.as_deref())
        {
            log::error!(
                "[batch-auth] excel-update-failed  port={port_for_update} row={row_idx} err={e}"
            );
            if let Ok(mut slot) = excel_err_update.lock() {
                *slot = Some(e);
            }
        }
    };

    let slot_config = crate::BatchAuthSlotConfig {
        auth_baud_rate: config.auth_baud_rate,
        conflict_policy,
        auth_storage,
    };
    let result = crate::run_batch_auth_slot(
        &port,
        &config.chip_id,
        &slot_config,
        find_by_mac,
        allocate_row,
        update_row,
        &cancel,
        |step| {
            let step_str = match step {
                crate::BatchAuthStep::ReadingMac => "reading_mac",
                crate::BatchAuthStep::ReadingAuth => "reading_auth",
                crate::BatchAuthStep::WritingAuth => "writing_auth",
                crate::BatchAuthStep::Verifying => "verifying",
            };
            emit(serde_json::json!({ "port": port, "step": step_str }));
        },
        |line: &str| {
            if let Some(tw) = &trace {
                if let Ok(mut w) = tw.lock() {
                    w.writeln(line);
                }
            }
        },
    );

    // Attach the captured Excel write failure (if any) to a final
    // emit — the frontend renders it as a per-slot warning.
    let emit_final = |mut payload: serde_json::Value| {
        if let Some(e) = excel_err.lock().ok().and_then(|g| g.clone()) {
            payload["excelError"] = serde_json::Value::String(e);
        }
        emit(payload);
    };

    match result {
        // Done — state already written to Excel by update_row(Done)
        Ok(crate::BatchAuthSlotResult::Done { mac }) => {
            log::info!("[batch-auth] slot done  port={port} mac={mac}");
            emit_final(serde_json::json!({ "port": port, "step": "done", "mac": mac }));
        }
        // AlreadyDone — state already written to Excel by update_row(Done) in authorize.rs
        Ok(crate::BatchAuthSlotResult::AlreadyDone { mac }) => {
            log::info!("[batch-auth] slot already-done  port={port} mac={mac}");
            emit_final(serde_json::json!({ "port": port, "step": "done", "mac": mac }));
        }
        // InsufficientCodes — no row was allocated, nothing to write
        Ok(crate::BatchAuthSlotResult::InsufficientCodes { mac }) => {
            log::info!("[batch-auth] slot no-code  port={port} mac={mac}");
            emit(serde_json::json!({ "port": port, "step": "no_code", "mac": mac }));
        }
        // Skipped — device already carries auth we didn't (knowingly) write.
        // If that UUID is still an unclaimed row in our sheet, claim it for this
        // MAC so the same code can't later be handed to a different device.
        Ok(crate::BatchAuthSlotResult::Skipped { mac, existing_uuid }) => {
            log::info!(
                "[batch-auth] slot skipped  port={port} mac={mac} existing_uuid={existing_uuid}"
            );
            if let Err(e) = allocator.confirm_existing_uuid(&existing_uuid, &mac) {
                log::error!("[batch-auth] excel-confirm-skipped-failed  port={port} existing_uuid={existing_uuid} err={e}");
                if let Ok(mut slot) = excel_err.lock() {
                    *slot = Some(e);
                }
            }
            emit_final(
                serde_json::json!({ "port": port, "step": "skipped", "mac": mac, "existingUuid": existing_uuid }),
            );
        }
        // Cancelled — pre-write cancel; if MacRead was written, row stays in MacRead state (recoverable)
        Ok(crate::BatchAuthSlotResult::Cancelled) => {
            log::info!("[batch-auth] slot cancelled  port={port}");
            emit(serde_json::json!({ "port": port, "step": "cancelled" }));
        }
        // CancelledAfterWrite — state already written to Excel by update_row(AuthWritten)
        Ok(crate::BatchAuthSlotResult::CancelledAfterWrite { mac, uuid }) => {
            log::warn!(
                "[batch-auth] slot cancelled AFTER auth_write  port={port} mac={mac} uuid={uuid}"
            );
            emit_final(
                serde_json::json!({ "port": port, "step": "cancelled_after_write", "mac": mac, "uuid": uuid }),
            );
        }
        // DefaultMac — T5/T5AI factory default MAC; no row allocation
        Ok(crate::BatchAuthSlotResult::DefaultMac { mac }) => {
            log::warn!("[batch-auth] slot default-mac  port={port} mac={mac}");
            emit(serde_json::json!({ "port": port, "step": "default_mac", "mac": mac }));
        }
        // Err — state already written by update_row(StepFailed) inside authorize.rs
        Err(e) => {
            log::warn!("[batch-auth] slot failed  port={port} error={e}");
            emit_final(
                serde_json::json!({ "port": port, "step": "failed", "error": e.to_string() }),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The point of `run_batch_slot` taking an `emit` callback instead of an
    /// `AppHandle`: the slot body can be driven from a test. This covers the
    /// flash-only-with-nothing-to-do path, which touches no serial port.
    #[test]
    fn flash_only_slot_without_firmware_or_allocator_emits_nothing() {
        let seen: Arc<StdMutex<Vec<serde_json::Value>>> = Arc::new(StdMutex::new(Vec::new()));
        let sink = seen.clone();

        run_batch_slot(
            "COM_TEST".to_string(),
            BatchAuthStartConfig {
                chip_id: "T5AI".into(),
                baud_rate: 921_600,
                auth_baud_rate: 115_200,
                firmware_path: None,
                flash_start_hex: None,
                flash_end_hex: None,
                excel_path: String::new(),
                conflict_policy: "skip".into(),
                auth_storage: None,
                authorize_enabled: false,
            },
            Arc::new(AtomicBool::new(false)),
            None,
            Arc::new(StdMutex::new(AllocatorSession {
                alloc: None,
                active: 0,
            })),
            None,
            crate::ConflictPolicy::Skip,
            crate::AuthStorage::Kv,
            &move |payload| sink.lock().unwrap().push(payload),
        );

        assert!(
            seen.lock().unwrap().is_empty(),
            "a flash-only slot with no firmware has nothing to report"
        );
    }

    #[test]
    fn batch_auth_start_config_defaults_authorize_enabled_to_true() {
        let cfg: BatchAuthStartConfig = serde_json::from_str(
            r#"{"chipId":"esp32","baudRate":921600,"authBaudRate":115200,
                "excelPath":"codes.xlsx","conflictPolicy":"skip"}"#,
        )
        .unwrap();
        assert!(cfg.authorize_enabled);

        let cfg: BatchAuthStartConfig = serde_json::from_str(
            r#"{"chipId":"esp32","baudRate":921600,"authBaudRate":115200,
                "excelPath":"","conflictPolicy":"skip","authorizeEnabled":false}"#,
        )
        .unwrap();
        assert!(!cfg.authorize_enabled);
    }
}
