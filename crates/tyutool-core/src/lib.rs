//! tyutool — shared flash plugin registry, jobs, and serial listing for GUI (Tauri) and CLI.

mod authorize;
mod error;
mod job;
mod plugin;
pub mod plugins;
mod progress;
mod registry;
mod serial;
mod tuya_dev_usb;
mod usb_port_survey;

pub use authorize::{probe_device_authorization, DeviceAuthorization};
pub use error::FlashError;
pub use job::{FlashJob, FlashMode};
pub use plugin::FlashPlugin;
pub use progress::FlashProgress;
pub use registry::{default_registry, run_job, FlashPluginRegistry};
pub use serial::{
    check_port_available, device_reset_dtr_rts, list_serial_ports, PortCheckResult, SerialPortEntry,
};
pub use usb_port_survey::{usb_port_survey, UsbPortSurveyEntry, UsbPortSurveyUsb};
