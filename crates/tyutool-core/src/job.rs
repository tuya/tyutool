use crate::authorize::AuthStorage;
use serde::{Deserialize, Serialize};

/// Mirrors Python `FlashArgv.mode` (write / read); extended for GUI tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, export_to = "../../../src/bindings/"))]
#[serde(rename_all = "lowercase")]
pub enum FlashMode {
    Flash,
    Erase,
    Read,
    /// TuyaOpen UART authorization (`tos.py monitor` + `auth` / `auth-read`). Not part of any
    /// chip [`crate::plugin::FlashPlugin`] — [`crate::registry::run_job`] handles it before plugin dispatch.
    Authorize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, export_to = "../../../src/bindings/"))]
#[serde(rename_all = "camelCase")]
pub struct FlashSegment {
    pub firmware_path: String,
    pub start_addr: String,
    pub end_addr: String,
}

/// One flash/erase/read/authorize job; shared by CLI and Tauri `invoke`.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
// `optional_fields = nullable` renders every `Option<T>` field below as `field?: T | null`
// (skipping non-Option fields), matching the hand-written `FlashJobPayload` shape it replaces —
// see the ts-rs binding note in AGENTS.md's Tauri IPC contract section.
#[cfg_attr(
    feature = "ts-rs",
    ts(
        export,
        export_to = "../../../src/bindings/",
        optional_fields = nullable
    )
)]
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
    pub authorize_storage: Option<AuthStorage>,
    /// Called when a conflicting credential is found on-device during authorize.
    /// Returns `true` to proceed with overwrite, `false` to abort.
    /// `None` in CLI mode — conflict is always overwritten without prompting.
    #[serde(skip)]
    #[cfg_attr(feature = "ts-rs", ts(skip))]
    pub confirm_overwrite: Option<Box<dyn Fn(String, String) -> bool + Send>>,
}

/// Stand-in for a redacted credential in [`FlashJob::to_cli_command`]. Deliberately not a
/// valid UUID/AuthKey shape, so it reads as a placeholder rather than a value that could be
/// mistaken for real and replayed.
const REDACTED_PLACEHOLDER: &str = "<REDACTED>";

impl FlashJob {
    /// Builds a job with the fields common to every mode set; every mode-specific field
    /// starts `None`. Callers fill in what they need with struct-update syntax:
    ///
    /// ```ignore
    /// FlashJob { flash_start_hex: Some(start), firmware_path: Some(file), ..FlashJob::new(mode, chip_id, port, baud) }
    /// ```
    ///
    /// No `Default` impl is provided on purpose: `mode` has no meaningful default, and a
    /// made-up one would silently mask a caller that forgot to set it.
    pub fn new(
        mode: FlashMode,
        chip_id: impl Into<String>,
        port: impl Into<String>,
        baud_rate: u32,
    ) -> Self {
        Self {
            mode,
            chip_id: chip_id.into(),
            port: port.into(),
            baud_rate,
            segments: None,
            flash_start_hex: None,
            flash_end_hex: None,
            erase_start_hex: None,
            erase_end_hex: None,
            read_start_hex: None,
            read_end_hex: None,
            read_file_path: None,
            firmware_path: None,
            authorize_uuid: None,
            authorize_key: None,
            authorize_storage: None,
            confirm_overwrite: None,
        }
    }

    pub fn normalized_chip_id(&self) -> String {
        crate::registry::normalize_chip_id(&self.chip_id)
    }

    /// The equivalent `tyutool` CLI command line for this job, for logging and issue
    /// reports (see `run_job` in `registry.rs`). Returns `None` when the job has no
    /// single-command form:
    ///
    /// - `segments` is set (multi-segment flashing is GUI-only; there is no CLI flag for it).
    /// - A field the mode requires is missing (e.g. `flash_start_hex` unset for
    ///   [`FlashMode::Flash`]).
    /// - For [`FlashMode::Read`] / [`FlashMode::Erase`], `*_start_hex` / `*_end_hex` don't
    ///   parse as hex, or `end < start` — the CLI's `-l length` form has no way to express a
    ///   negative length.
    ///
    /// Credentials are never emitted: this string is written via `log::*`, and per the
    /// Logging Contract plaintext `authorize_uuid` / `authorize_key` must never reach a
    /// `tyutool-*.log` file (export redaction matches `uuid=`/`authkey=`-style prefixes,
    /// which the space-separated CLI form doesn't have). A present credential is rendered as
    /// [`REDACTED_PLACEHOLDER`] instead of the real value — defense in depth, not reliance on
    /// redaction-on-export.
    pub fn to_cli_command(&self) -> Option<String> {
        if self.segments.is_some() {
            return None;
        }
        let mut parts: Vec<String> = vec!["tyutool".to_string()];
        match self.mode {
            FlashMode::Flash => {
                parts.push("write".to_string());
                parts.push("-d".to_string());
                parts.push(quote_arg(&self.chip_id));
                parts.push("-p".to_string());
                parts.push(quote_arg(&self.port));
                parts.push("-b".to_string());
                parts.push(self.baud_rate.to_string());
                parts.push("-s".to_string());
                parts.push(self.flash_start_hex.clone()?);
                parts.push("--end".to_string());
                parts.push(self.flash_end_hex.clone()?);
                parts.push("-f".to_string());
                parts.push(quote_arg(self.firmware_path.as_deref()?));
            }
            FlashMode::Read => {
                parts.push("read".to_string());
                parts.push("-d".to_string());
                parts.push(quote_arg(&self.chip_id));
                parts.push("-p".to_string());
                parts.push(quote_arg(&self.port));
                parts.push("-b".to_string());
                parts.push(self.baud_rate.to_string());
                let start = self.read_start_hex.as_deref()?;
                let end = self.read_end_hex.as_deref()?;
                let length = hex_length(start, end)?;
                parts.push("-s".to_string());
                parts.push(start.to_string());
                parts.push("-l".to_string());
                parts.push(length);
                parts.push("-f".to_string());
                parts.push(quote_arg(self.read_file_path.as_deref()?));
            }
            FlashMode::Erase => {
                parts.push("erase".to_string());
                parts.push("-d".to_string());
                parts.push(quote_arg(&self.chip_id));
                parts.push("-p".to_string());
                parts.push(quote_arg(&self.port));
                parts.push("-b".to_string());
                parts.push(self.baud_rate.to_string());
                let start = self.erase_start_hex.as_deref()?;
                let end = self.erase_end_hex.as_deref()?;
                let length = hex_length(start, end)?;
                parts.push("-s".to_string());
                parts.push(start.to_string());
                parts.push("-l".to_string());
                parts.push(length);
            }
            FlashMode::Authorize => {
                parts.push("authorize".to_string());
                // Unlike write/read/erase, `-d` is optional on this subcommand and an empty
                // chip_id (no `-d` given) is a valid job — see main.rs's
                // `device.as_deref().map(normalize_chip_id).unwrap_or_default()`. Emitting
                // `-d ""` wouldn't round-trip through clap's chip value parser.
                if !self.chip_id.is_empty() {
                    parts.push("-d".to_string());
                    parts.push(quote_arg(&self.chip_id));
                }
                parts.push("-p".to_string());
                parts.push(quote_arg(&self.port));
                // No `-b`: the CLI hard-codes 115200 baud for authorize and exposes no flag.
                if self.authorize_uuid.is_some() {
                    parts.push("--uuid".to_string());
                    parts.push(REDACTED_PLACEHOLDER.to_string());
                }
                if self.authorize_key.is_some() {
                    parts.push("--authkey".to_string());
                    parts.push(REDACTED_PLACEHOLDER.to_string());
                }
            }
        }
        Some(parts.join(" "))
    }
}

/// Parses a `0x`/`0X`-prefixed (or bare) hex string, as used throughout `FlashJob`'s
/// `*_start_hex` / `*_end_hex` fields.
fn parse_hex(s: &str) -> Option<u64> {
    let s = s.trim();
    let digits = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u64::from_str_radix(digits, 16).ok()
}

/// `end - start` as a zero-padded hex string, matching the `0x{:08X}` format the CLI already
/// uses when it computes `*_end_hex` from a `-l length` flag (see `main.rs`). Returns `None`
/// if either bound doesn't parse or `end < start`.
fn hex_length(start: &str, end: &str) -> Option<String> {
    let start_val = parse_hex(start)?;
    let end_val = parse_hex(end)?;
    let len = end_val.checked_sub(start_val)?;
    Some(format!("0x{:08X}", len))
}

/// Quotes a CLI argument value if it contains whitespace/quotes/is empty, so the rendered
/// command line stays a single valid token per argument (paths on Windows commonly contain
/// spaces). Not a full shell-quoting implementation — this string is for logs/issue reports,
/// not for feeding to a shell.
fn quote_arg(value: &str) -> String {
    if value.is_empty() || value.chars().any(|c| c.is_whitespace() || c == '"') {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        value.to_string()
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
            .field("authorize_storage", &self.authorize_storage)
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
            authorize_storage: self.authorize_storage,
            confirm_overwrite: None, // closures are not Clone
        }
    }
}

/// Manual, not `#[derive(PartialEq)]`: `confirm_overwrite` is `Option<Box<dyn Fn(..) -> bool>>`,
/// which isn't comparable and can't opt out of a derive. Mirrors the manual `Debug`/`Clone`
/// impls above, which already special-case this field for the same reason. Two jobs are equal
/// when every other field matches; `confirm_overwrite` is ignored (as it is in `Clone`, where it
/// always resets to `None`).
impl PartialEq for FlashJob {
    fn eq(&self, other: &Self) -> bool {
        self.mode == other.mode
            && self.chip_id == other.chip_id
            && self.port == other.port
            && self.baud_rate == other.baud_rate
            && self.segments == other.segments
            && self.flash_start_hex == other.flash_start_hex
            && self.flash_end_hex == other.flash_end_hex
            && self.erase_start_hex == other.erase_start_hex
            && self.erase_end_hex == other.erase_end_hex
            && self.read_start_hex == other.read_start_hex
            && self.read_end_hex == other.read_end_hex
            && self.read_file_path == other.read_file_path
            && self.firmware_path == other.firmware_path
            && self.authorize_uuid == other.authorize_uuid
            && self.authorize_key == other.authorize_key
            && self.authorize_storage == other.authorize_storage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_defaults_every_mode_specific_field_to_none() {
        let job = FlashJob::new(FlashMode::Flash, "T5AI", "/dev/ttyUSB0", 921_600);
        assert_eq!(job.mode, FlashMode::Flash);
        assert_eq!(job.chip_id, "T5AI");
        assert_eq!(job.port, "/dev/ttyUSB0");
        assert_eq!(job.baud_rate, 921_600);
        assert!(job.segments.is_none());
        assert!(job.flash_start_hex.is_none());
        assert!(job.flash_end_hex.is_none());
        assert!(job.erase_start_hex.is_none());
        assert!(job.erase_end_hex.is_none());
        assert!(job.read_start_hex.is_none());
        assert!(job.read_end_hex.is_none());
        assert!(job.read_file_path.is_none());
        assert!(job.firmware_path.is_none());
        assert!(job.authorize_uuid.is_none());
        assert!(job.authorize_key.is_none());
        assert!(job.authorize_storage.is_none());
        assert!(job.confirm_overwrite.is_none());
    }

    #[test]
    fn to_cli_command_flash() {
        let job = FlashJob {
            flash_start_hex: Some("0x00000000".to_string()),
            flash_end_hex: Some("0x00100000".to_string()),
            firmware_path: Some("firmware.bin".to_string()),
            ..FlashJob::new(FlashMode::Flash, "T5AI", "COM3", 921_600)
        };
        assert_eq!(
            job.to_cli_command().as_deref(),
            Some(
                "tyutool write -d T5AI -p COM3 -b 921600 -s 0x00000000 --end 0x00100000 -f firmware.bin"
            )
        );
    }

    #[test]
    fn to_cli_command_read_recomputes_length_from_start_and_end() {
        let job = FlashJob {
            read_start_hex: Some("0x00000000".to_string()),
            read_end_hex: Some("0x00200000".to_string()),
            read_file_path: Some("out.bin".to_string()),
            ..FlashJob::new(FlashMode::Read, "ESP32", "COM4", 460_800)
        };
        assert_eq!(
            job.to_cli_command().as_deref(),
            Some("tyutool read -d ESP32 -p COM4 -b 460800 -s 0x00000000 -l 0x00200000 -f out.bin")
        );
    }

    #[test]
    fn to_cli_command_erase_recomputes_length_from_start_and_end() {
        let job = FlashJob {
            erase_start_hex: Some("0x00010000".to_string()),
            erase_end_hex: Some("0x00020000".to_string()),
            ..FlashJob::new(FlashMode::Erase, "BK7231N", "/dev/ttyUSB1", 921_600)
        };
        assert_eq!(
            job.to_cli_command().as_deref(),
            Some("tyutool erase -d BK7231N -p /dev/ttyUSB1 -b 921600 -s 0x00010000 -l 0x00010000")
        );
    }

    #[test]
    fn to_cli_command_erase_none_when_end_before_start() {
        let job = FlashJob {
            erase_start_hex: Some("0x00020000".to_string()),
            erase_end_hex: Some("0x00010000".to_string()),
            ..FlashJob::new(FlashMode::Erase, "BK7231N", "/dev/ttyUSB1", 921_600)
        };
        assert!(job.to_cli_command().is_none());
    }

    #[test]
    fn to_cli_command_none_when_required_field_missing() {
        // Flash mode with no firmware_path/start/end set (as FlashJob::new leaves them).
        let job = FlashJob::new(FlashMode::Flash, "T5AI", "COM3", 921_600);
        assert!(job.to_cli_command().is_none());
    }

    #[test]
    fn to_cli_command_none_for_multi_segment_job() {
        let job = FlashJob {
            segments: Some(vec![FlashSegment {
                firmware_path: "a.bin".to_string(),
                start_addr: "0x0".to_string(),
                end_addr: "0x1000".to_string(),
            }]),
            ..FlashJob::new(FlashMode::Flash, "T5AI", "COM3", 921_600)
        };
        assert!(job.to_cli_command().is_none());
    }

    #[test]
    fn to_cli_command_authorize_omits_baud_and_device_when_absent() {
        let job = FlashJob::new(FlashMode::Authorize, "", "COM5", 115_200);
        assert_eq!(
            job.to_cli_command().as_deref(),
            Some("tyutool authorize -p COM5")
        );
    }

    #[test]
    fn to_cli_command_authorize_redacts_credentials_never_leaking_real_values() {
        let real_uuid = "uuid-super-secret-0123456789";
        let real_key = "authkey-super-secret-abcdef";
        let job = FlashJob {
            authorize_uuid: Some(real_uuid.to_string()),
            authorize_key: Some(real_key.to_string()),
            ..FlashJob::new(FlashMode::Authorize, "T5AI", "COM5", 115_200)
        };
        let cmd = job
            .to_cli_command()
            .expect("authorize job is representable");
        assert!(
            cmd.contains("--uuid <REDACTED>"),
            "expected redacted uuid placeholder, got: {cmd}"
        );
        assert!(
            cmd.contains("--authkey <REDACTED>"),
            "expected redacted authkey placeholder, got: {cmd}"
        );
        assert!(
            !cmd.contains(real_uuid),
            "real uuid must never appear in the logged command: {cmd}"
        );
        assert!(
            !cmd.contains(real_key),
            "real authkey must never appear in the logged command: {cmd}"
        );
    }

    #[test]
    fn to_cli_command_quotes_values_with_spaces() {
        let job = FlashJob {
            flash_start_hex: Some("0x0".to_string()),
            flash_end_hex: Some("0x100".to_string()),
            firmware_path: Some("C:/My Firmware/app.bin".to_string()),
            ..FlashJob::new(FlashMode::Flash, "T5AI", "COM 3", 921_600)
        };
        let cmd = job.to_cli_command().unwrap();
        assert!(cmd.contains("-p \"COM 3\""));
        assert!(cmd.contains("-f \"C:/My Firmware/app.bin\""));
    }
}
