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
#[derive(Serialize, Deserialize)]
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
    /// Called when a conflicting credential is found on-device during authorize.
    /// Returns `true` to proceed with overwrite, `false` to abort.
    /// `None` in CLI mode — conflict is always overwritten without prompting.
    #[serde(skip)]
    pub confirm_overwrite: Option<Box<dyn Fn(String, String) -> bool + Send>>,
}

impl FlashJob {
    pub fn normalized_chip_id(&self) -> String {
        crate::registry::normalize_chip_id(&self.chip_id)
    }
}

impl std::fmt::Debug for FlashJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlashJob")
            .field("mode", &self.mode)
            .field("chip_id", &self.chip_id)
            .field("port", &self.port)
            .field("baud_rate", &self.baud_rate)
            .field("segments", &self.segments)
            .field("flash_start_hex", &self.flash_start_hex)
            .field("flash_end_hex", &self.flash_end_hex)
            .field("erase_start_hex", &self.erase_start_hex)
            .field("erase_end_hex", &self.erase_end_hex)
            .field("read_start_hex", &self.read_start_hex)
            .field("read_end_hex", &self.read_end_hex)
            .field("read_file_path", &self.read_file_path)
            .field("firmware_path", &self.firmware_path)
            .field("authorize_uuid", &self.authorize_uuid)
            .field("authorize_key", &self.authorize_key)
            .field(
                "confirm_overwrite",
                &self.confirm_overwrite.as_ref().map(|_| "<closure>"),
            )
            .finish()
    }
}

impl Clone for FlashJob {
    fn clone(&self) -> Self {
        Self {
            mode: self.mode,
            chip_id: self.chip_id.clone(),
            port: self.port.clone(),
            baud_rate: self.baud_rate,
            segments: self.segments.clone(),
            flash_start_hex: self.flash_start_hex.clone(),
            flash_end_hex: self.flash_end_hex.clone(),
            erase_start_hex: self.erase_start_hex.clone(),
            erase_end_hex: self.erase_end_hex.clone(),
            read_start_hex: self.read_start_hex.clone(),
            read_end_hex: self.read_end_hex.clone(),
            read_file_path: self.read_file_path.clone(),
            firmware_path: self.firmware_path.clone(),
            authorize_uuid: self.authorize_uuid.clone(),
            authorize_key: self.authorize_key.clone(),
            confirm_overwrite: None, // closures are not Clone
        }
    }
}
