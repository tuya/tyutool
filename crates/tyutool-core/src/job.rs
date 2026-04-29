use serde::{Deserialize, Serialize};

/// Mirrors Python `FlashArgv.mode` (write / read); extended for GUI tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlashMode {
    Flash,
    Erase,
    Read,
    /// TuyaOpen UART authorization (`tos.py monitor` + `auth` / `auth-read`). Not part of any
    /// chip [`crate::plugin::FlashPlugin`] — [`crate::registry::run_job`] handles it before plugin dispatch.
    Authorize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlashSegment {
    pub firmware_path: String,
    pub start_addr: String,
    pub end_addr: String,
}

/// One flash/erase/read/authorize job; shared by CLI and Tauri `invoke`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlashJob {
    pub mode: FlashMode,
    /// Registry key for [`crate::registry::FlashPluginRegistry`] (e.g. `ESP32`, `ESP32C3`).
    /// Ignored for plugin lookup when `mode` is [`FlashMode::Authorize`] (UART-level flow).
    pub chip_id: String,
    pub port: String,
    pub baud_rate: u32,
    pub segments: Option<Vec<FlashSegment>>,
    pub flash_start_hex: Option<String>,
    pub flash_end_hex: Option<String>,
    pub erase_start_hex: Option<String>,
    pub erase_end_hex: Option<String>,
    pub read_start_hex: Option<String>,
    pub read_end_hex: Option<String>,
    pub read_file_path: Option<String>,
    pub firmware_path: Option<String>,
    pub authorize_uuid: Option<String>,
    pub authorize_key: Option<String>,
}

impl FlashJob {
    pub fn normalized_chip_id(&self) -> String {
        self.chip_id.trim().to_ascii_uppercase()
    }
}
