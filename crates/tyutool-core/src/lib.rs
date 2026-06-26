//! tyutool — shared flash plugin registry, jobs, and serial listing for GUI (Tauri) and CLI.

mod authorize;
pub mod diagnostics;
mod error;
pub mod flash_event;
mod job;
mod plugin;
pub mod plugins;
mod registry;
mod serial;
mod serial_debug;
mod tuya_dev_usb;
mod usb_port_survey;

pub use authorize::{
    read_auth_probe, run_batch_auth_slot, AuthStorage, BatchAuthSlotResult, BatchAuthStep,
    ConflictPolicy, ReadAuthProbeResult,
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
    ChunkCallback, DataBits, DebugChunk, DebugConfig, Direction, DisconnectCallback, Parity,
    SerialDebugSession, StopBits,
};
pub use usb_port_survey::{usb_port_survey, UsbPortSurveyEntry, UsbPortSurveyUsb};
