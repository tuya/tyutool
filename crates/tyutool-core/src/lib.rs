//! tyutool — shared flash plugin registry, jobs, and serial listing for GUI (Tauri) and CLI.

mod authorize;
#[cfg(feature = "excel")]
pub mod batch_auth;
pub mod diagnostics;
mod error;
pub mod flash_event;
mod job;
mod plugin;
pub mod plugins;
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
pub use plugin::FlashPlugin;
pub use registry::{default_registry, normalize_chip_id, run_job, FlashPluginRegistry};
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
