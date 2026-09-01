//! tyutool — shared flash plugin registry, jobs, and serial listing for GUI (Tauri) and CLI.

// ── `mock-chip` must never reach a user ──────────────────────────────────────
//
// The feature registers a fake device in the *default* registry, so a build
// that carries it would offer users a chip that silently pretends to flash.
// Two independent guards keep it out of a shipped artifact; both must stay:
//
//  1. This one. Every release artifact is built with `debug_assertions` off
//     (`cargo build --release -p tyutool-cli`, `tauri build`,
//     `cargo build --release -p tyutool-bridge` — and nothing in this repo
//     overrides `debug-assertions` for the release profile), so enabling the
//     feature there fails the compile outright rather than shipping quietly.
//     It also means `cargo test --release --features mock-chip` will not build;
//     no workflow or script does that today. If one ever needs to, replace this
//     guard with a narrower signal — do not simply delete it.
//  2. `tests/shipped_crates_exclude_mock_chip.rs` — no crate that produces a
//     shipped binary may name the feature in its manifest. That guard runs on
//     the ordinary `cargo test -p tyutool-core`, feature off.
#[cfg(all(feature = "mock-chip", not(debug_assertions)))]
compile_error!(
    "the `mock-chip` feature registers a fake chip plugin in the default registry and must \
     never be enabled in a release build — drop it from this build's --features flag or from \
     the Cargo.toml that pulled it in"
);

mod authorize;
#[cfg(feature = "excel")]
pub mod batch_auth;
#[cfg(feature = "excel")]
pub mod batch_slot;
pub mod diagnostics;
mod error;
pub mod flash_event;
mod job;
mod job_control;
mod plugin;
pub mod plugins;
pub mod ram_loader;
mod registry;
mod serial;
mod serial_debug;
mod serial_debug_bridge;
mod tuya_dev_usb;
mod usb_port_survey;

pub use authorize::{
    is_device_no_response, read_auth_probe, run_batch_auth_slot, validate_auth_credentials,
    wait_after_firmware_flash, AuthStorage, BatchAuthRowUpdate, BatchAuthSlotConfig,
    BatchAuthSlotResult, BatchAuthStep, ConflictPolicy, ReadAuthProbeResult,
    DEVICE_NO_RESPONSE_PREFIX,
};
#[cfg(feature = "excel")]
pub use batch_auth::{ExcelRow, ExcelRowAllocator, ExcelStats, RowStatus};
#[cfg(feature = "excel")]
pub use batch_slot::{run_batch_slot, AllocatorSession, BatchAuthStartConfig};
#[cfg(feature = "zip")]
pub use diagnostics::gather_and_write_logs_zip;
pub use diagnostics::{
    build_report_info, collect_log_files, list_log_files_impl, prune_log_files, prune_trace_files,
    read_log_tail_impl, read_named_log_impl, redact_log_content, resolve_log_open_path,
    BatchAuthTraceWriter, LogFileInfo, LogRetention, REDACT_PREFIXES,
};
pub use error::FlashError;
pub use flash_event::{
    FlashEvent, FlashMilestone, FlashPhase, FlashResult, JobDetails, JobSummary,
};
pub use job::{FlashJob, FlashMode};
pub use job_control::{CancelSlot, ConfirmSlot};
pub use plugin::FlashPlugin;
#[cfg(feature = "mock-chip")]
pub use plugins::mock::MockPlugin;
pub use registry::{
    default_registry, normalize_chip_id, run_job, run_job_with, FlashPluginRegistry,
};
pub use serial::{
    check_port_available, device_reset_dtr_rts, list_serial_ports, PortCheckResult, SerialPortEntry,
};
pub use serial_debug::{
    create_serial_debug_state_resilient, serial_debug_archive_cap_limit_mib,
    serial_debug_archive_cap_sentinel, serial_debug_archive_dir, serial_debug_chunk_drop_bytes,
    serial_debug_chunk_drop_sentinel, serial_debug_fail_backfill_if_current,
    serial_debug_finish_backfill_if_current, serial_debug_now_ms, serial_debug_scan_filter_matches,
    ChunkCallback, DataBits, DebugChunk, DebugConfig, Direction, DisconnectCallback, LogDirection,
    Parity, SerialDebugArchive, SerialDebugArchiveReader, SerialDebugChunkBatchBuffer,
    SerialDebugDropCounter, SerialDebugDropReport, SerialDebugFilterBackfillSnapshot,
    SerialDebugFilterDefinition, SerialDebugFilterIndex, SerialDebugFilterPage,
    SerialDebugFilterStats, SerialDebugFilterStatus, SerialDebugGeneration, SerialDebugLine,
    SerialDebugSession, SerialDebugSessionPage, StopBits,
};
pub use serial_debug_bridge::{
    serial_debug_finalize_pending, serial_debug_flush_chunks, serial_debug_ingest_lines,
    serial_debug_report_drops, serial_debug_spawn_chunk_bridge, ArchivedChunk,
    SerialDebugChunkBridgeHandle, SerialDebugSink, SERIAL_DEBUG_CHUNK_FLUSH_BYTES,
    SERIAL_DEBUG_CHUNK_FLUSH_MS, SERIAL_DEBUG_CHUNK_QUEUE_CAPACITY,
};
pub use usb_port_survey::{usb_port_survey, UsbPortSurveyEntry, UsbPortSurveyUsb};
