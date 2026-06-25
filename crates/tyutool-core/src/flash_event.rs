use serde::{Deserialize, Serialize};

use crate::job::{FlashJob, FlashMode};

/// User-facing event emitted through the progress callback.
/// Developer diagnostics use `log::*` macros instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FlashEvent {
    JobSummary(JobSummary),
    Phase {
        phase: FlashPhase,
    },
    Percent {
        value: u8,
    },
    Milestone {
        milestone: FlashMilestone,
    },
    /// User-actionable warning (e.g. LN882H: "hold BOOT/A9 pin LOW").
    Warning {
        message: String,
    },
    Done {
        result: FlashResult,
    },
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
    WriteSegment {
        current: u32,
        total: u32,
    },
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
    Connected {
        chip_info: Option<String>,
    },
    FlashIdRead {
        mid: Option<u32>,
    },
    EraseComplete,
    SegmentWritten {
        current: u32,
        total: u32,
    },
    WriteComplete,
    VerifyPassed,
    Rebooted,
    /// TuyaOpen auth read result. GUI MUST display this in a secure modal, not plain log.
    AuthReadComplete {
        uuid: String,
        authkey: String,
    },
    /// Auth read completed but device has no valid authorization.
    /// Covers both placeholder UUID and no-data cases.
    AuthReadEmpty,
    /// Device already has conflicting authorization — GUI must pause and ask user.
    /// `existing_uuid`/`existing_authkey`: what the device currently holds.
    AuthConflict {
        existing_uuid: String,
        existing_authkey: String,
    },
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
            FlashMode::Flash => {
                if let Some(segs) = job.segments.as_deref().filter(|s| !s.is_empty()) {
                    let first = &segs[0];
                    let firmware_path = if segs.len() == 1 {
                        first.firmware_path.clone()
                    } else {
                        format!("{} (+{} more)", first.firmware_path, segs.len() - 1)
                    };
                    let firmware_size = segs.iter().try_fold(0u64, |total, seg| {
                        std::fs::metadata(&seg.firmware_path)
                            .ok()
                            .map(|m| total + m.len())
                    });
                    JobDetails::Flash {
                        firmware_path,
                        firmware_size,
                        range_start: first.start_addr.clone(),
                        range_end: first.end_addr.clone(),
                    }
                } else {
                    JobDetails::Flash {
                        firmware_path: job.firmware_path.clone().unwrap_or_else(|| "?".into()),
                        firmware_size: job
                            .firmware_path
                            .as_deref()
                            .and_then(|p| std::fs::metadata(p).ok())
                            .map(|m| m.len()),
                        range_start: job
                            .flash_start_hex
                            .clone()
                            .unwrap_or_else(|| "0x00000000".into()),
                        range_end: job.flash_end_hex.clone().unwrap_or_else(|| "?".into()),
                    }
                }
            }
            FlashMode::Read => JobDetails::Read {
                output_path: job.read_file_path.clone().unwrap_or_else(|| "?".into()),
                range_start: job
                    .read_start_hex
                    .clone()
                    .unwrap_or_else(|| "0x00000000".into()),
                range_end: job.read_end_hex.clone().unwrap_or_else(|| "?".into()),
            },
            FlashMode::Erase => JobDetails::Erase {
                range_start: job
                    .erase_start_hex
                    .clone()
                    .unwrap_or_else(|| "0x00000000".into()),
                range_end: job.erase_end_hex.clone().unwrap_or_else(|| "?".into()),
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
        let e = FlashEvent::Phase {
            phase: FlashPhase::Handshake,
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "phase");
        assert_eq!(v["phase"], "handshake");
    }

    #[test]
    fn write_segment_nested_correctly() {
        let e = FlashEvent::Phase {
            phase: FlashPhase::WriteSegment {
                current: 1,
                total: 3,
            },
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
        assert!(matches!(
            back,
            FlashEvent::Done {
                result: FlashResult::Ok { .. }
            }
        ));
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
    fn auth_read_empty_serializes_to_string() {
        let m = FlashMilestone::AuthReadEmpty;
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v, serde_json::json!("auth_read_empty"));
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
            confirm_overwrite: None,
        };
        let s = JobSummary::from_job(&job);
        assert!(s.device.is_none());
        assert!(matches!(s.details, JobDetails::Authorize { write: true }));
    }

    #[test]
    fn job_summary_flash_with_segments() {
        use crate::job::FlashSegment;
        let job = crate::job::FlashJob {
            mode: FlashMode::Flash,
            chip_id: "BK7231N".into(),
            port: "/dev/ttyUSB0".into(),
            baud_rate: 921600,
            segments: Some(vec![
                FlashSegment {
                    firmware_path: "app.bin".into(),
                    start_addr: "0x00010000".into(),
                    end_addr: "0x00200000".into(),
                },
                FlashSegment {
                    firmware_path: "boot.bin".into(),
                    start_addr: "0x00000000".into(),
                    end_addr: "0x00010000".into(),
                },
            ]),
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
            confirm_overwrite: None,
        };
        let s = JobSummary::from_job(&job);
        assert_eq!(s.device, Some("BK7231N".into()));
        match &s.details {
            JobDetails::Flash {
                firmware_path,
                range_start,
                range_end,
                ..
            } => {
                assert!(
                    firmware_path.contains("app.bin"),
                    "should show first segment path"
                );
                assert_eq!(range_start, "0x00010000");
                assert_eq!(range_end, "0x00200000");
            }
            _ => panic!("expected Flash details"),
        }
    }
}
