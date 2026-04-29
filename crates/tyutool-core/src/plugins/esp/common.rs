//! Shared `run_esp()` implementation for all ESP32 chip plugins.
//!
//! Wraps the `espflash` library to provide Flash, Erase, and Read operations
//! via the standard `FlashPlugin` interface.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use espflash::connection::{Connection, ResetAfterOperation, ResetBeforeOperation};
use espflash::flasher::Flasher;
use espflash::target::ProgressCallbacks;
use serialport::UsbPortInfo;

use std::collections::HashMap;

use crate::error::FlashError;
use crate::job::{FlashJob, FlashMode};
use crate::progress::FlashProgress;

use super::chips::EspChipDef;

// ── i18n helper ──────────────────────────────────────────────────────────────

/// Emit a structured i18n log event.  `key` is a frontend i18n path; `pairs`
/// are `(param_name, value)` tuples for substitution.
fn emit_key(progress: &dyn Fn(FlashProgress), key: &'static str, pairs: &[(&str, String)]) {
    let params: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect();
    progress(FlashProgress::LogKey {
        key: key.to_string(),
        params,
    });
}

// ── Progress adapter ─────────────────────────────────────────────────────────

/// Bridges espflash `ProgressCallbacks` to our `FlashProgress` enum.
struct ProgressAdapter<'a> {
    progress: &'a dyn Fn(FlashProgress),
    total: usize,
    current: usize,
    /// Percent range mapped to [pct_start, pct_end).
    pct_start: u8,
    pct_end: u8,
}

impl<'a> ProgressAdapter<'a> {
    fn new(progress: &'a dyn Fn(FlashProgress), pct_start: u8, pct_end: u8) -> Self {
        Self {
            progress,
            total: 1,
            current: 0,
            pct_start,
            pct_end,
        }
    }
}

impl ProgressCallbacks for ProgressAdapter<'_> {
    fn init(&mut self, _addr: u32, total: usize) {
        self.total = total.max(1);
        self.current = 0;
    }

    fn update(&mut self, current: usize) {
        self.current = current;
        let range = self.pct_end.saturating_sub(self.pct_start) as u64;
        let pct = self.pct_start as u64 + (current as u64 * range / self.total as u64);
        (self.progress)(FlashProgress::Percent {
            value: pct.min(self.pct_end as u64) as u8,
        });
    }

    fn verifying(&mut self) {
        (self.progress)(FlashProgress::Phase {
            name: "Verify".to_string(),
        });
    }

    fn finish(&mut self, skipped: bool) {
        if !skipped {
            (self.progress)(FlashProgress::Percent {
                value: self.pct_end,
            });
        }
    }
}

// ── Address helpers ──────────────────────────────────────────────────────────

fn parse_hex(s: Option<&str>, field: &str) -> Result<u32, FlashError> {
    let s = s
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| FlashError::InvalidJob(format!("missing {field}")))?;
    let stripped = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    u32::from_str_radix(stripped, 16)
        .map_err(|_| FlashError::InvalidJob(format!("invalid hex address for {field}: {s}")))
}

// ── Port info helpers ────────────────────────────────────────────────────────

/// Retrieve `UsbPortInfo` for the given port name from the OS port list.
/// Falls back to a zeroed-out struct for non-USB / unlisted ports.
/// UART reset for ESP32 family — uses `Connection::reset` (internally `reset_after_flash`),
/// matching the default post-flash reset path in `run_esp`.
pub(crate) fn esp_uart_hard_reset(port_name: &str) -> Result<(), FlashError> {
    let port_info = usb_port_info(port_name);
    let serial = serialport::new(port_name, 115_200)
        .timeout(Duration::from_millis(500))
        .open_native()
        .map_err(|e| FlashError::Plugin(format!("cannot open port {port_name}: {e}")))?;
    let mut conn = Connection::new(
        serial,
        port_info,
        ResetAfterOperation::HardReset,
        ResetBeforeOperation::DefaultReset,
        115_200,
    );
    conn.reset().map_err(esp_err)
}

fn usb_port_info(port_name: &str) -> UsbPortInfo {
    if let Ok(ports) = serialport::available_ports() {
        for p in &ports {
            if p.port_name == port_name {
                if let serialport::SerialPortType::UsbPort(ref info) = p.port_type {
                    return info.clone();
                }
            }
        }
    }
    // Non-USB port or not found — use defaults (pid=0 → DefaultReset strategy)
    UsbPortInfo {
        vid: 0,
        pid: 0,
        serial_number: None,
        manufacturer: None,
        product: None,
        interface: None,
    }
}

// ── Error conversion ─────────────────────────────────────────────────────────

fn esp_err(e: espflash::Error) -> FlashError {
    FlashError::Plugin(e.to_string())
}

// ── Main entry point ─────────────────────────────────────────────────────────

/// Shared flash/erase/read implementation for all ESP32 plugin variants.
pub(crate) fn run_esp(
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashProgress),
    def: &EspChipDef,
) -> Result<(), FlashError> {
    log::info!(
        "ESP plugin starting: chip={}, port={}, mode={:?}",
        def.id,
        job.port,
        job.mode
    );

    let lk = |key: &'static str, pairs: &[(&str, String)]| {
        emit_key(progress, key, pairs);
    };
    let phase = |name: &str| {
        progress(FlashProgress::Phase {
            name: name.to_string(),
        });
    };

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    // ── Open serial port ─────────────────────────────────────────────
    phase("Connect");
    lk("flash.log.esp.openingPort", &[("port", job.port.clone())]);

    let port_info = usb_port_info(&job.port);

    let serial = serialport::new(&job.port, 115_200)
        .timeout(Duration::from_millis(500))
        .open_native()
        .map_err(|e| FlashError::Plugin(format!("cannot open port {}: {e}", job.port)))?;

    let conn = Connection::new(
        serial,
        port_info,
        ResetAfterOperation::HardReset,
        ResetBeforeOperation::DefaultReset,
        115_200,
    );

    // ── Connect & detect chip ────────────────────────────────────────
    lk("flash.log.esp.connectingDevice", &[]);

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    let mut flasher = Flasher::connect(
        conn,
        true,           // use_stub — loads RAM stub for faster flash ops
        false,          // verify — we do our own progress reporting
        false,          // skip
        Some(def.chip), // expected chip; mismatch → error
        None,           // baud — will change after stub loads if needed
    )
    .map_err(esp_err)?;

    // Log device information
    match flasher.device_info() {
        Ok(info) => {
            lk(
                "flash.log.esp.connected",
                &[
                    ("chip", info.chip.to_string()),
                    ("revision", format!("{:?}", info.revision)),
                ],
            );
        }
        Err(e) => {
            lk(
                "flash.log.esp.readDeviceInfoFailed",
                &[("error", e.to_string())],
            );
        }
    }

    // Switch to user-requested baud rate if higher than default
    let target_baud = job.baud_rate.max(115_200);
    if target_baud > 115_200 {
        lk(
            "flash.log.esp.switchingBaud",
            &[("baud", target_baud.to_string())],
        );
        flasher.change_baud(target_baud).map_err(esp_err)?;
    }

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    // ── Dispatch by mode ─────────────────────────────────────────────
    match job.mode {
        FlashMode::Flash => run_flash(job, &mut flasher, cancel, progress)?,
        FlashMode::Erase => run_erase(job, &mut flasher, cancel, progress)?,
        FlashMode::Read => run_read(job, &mut flasher, cancel, progress)?,
        FlashMode::Authorize => unreachable!("Authorize is handled in run_job before plugin.run"),
    }

    log::info!("ESP plugin completed successfully");
    Ok(())
}

// ── Flash mode ───────────────────────────────────────────────────────────────

fn run_flash(
    job: &FlashJob,
    flasher: &mut Flasher,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashProgress),
) -> Result<(), FlashError> {
    let lk = |key: &'static str, pairs: &[(&str, String)]| emit_key(progress, key, pairs);
    let phase = |name: &str| {
        progress(FlashProgress::Phase {
            name: name.to_string(),
        })
    };

    // Collect segments: either from job.segments or from legacy fields
    let segments = if let Some(ref s) = job.segments {
        s.clone()
    } else {
        let firmware_path = job
            .firmware_path
            .clone()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| FlashError::InvalidJob("missing firmware_path".into()))?;
        let start_addr = job
            .flash_start_hex
            .clone()
            .unwrap_or_else(|| "0x0".to_string());
        vec![crate::job::FlashSegment {
            firmware_path,
            start_addr,
            end_addr: "".to_string(), // not used in ESP plugin for flashing
        }]
    };

    if segments.is_empty() {
        return Err(FlashError::InvalidJob("no flash segments provided".into()));
    }

    let total_segments = segments.len();

    for (i, seg) in segments.iter().enumerate() {
        let seg_start_pct = (i as u64 * 100 / total_segments as u64) as u8;
        let seg_end_pct = ((i + 1) as u64 * 100 / total_segments as u64) as u8;

        progress(FlashProgress::LogKey {
            key: "flash.log.segmentLog".to_string(),
            params: [("n".to_string(), (i + 1).to_string())].into(),
        });

        // Read firmware file
        let firmware = std::fs::read(&seg.firmware_path).map_err(|e| {
            FlashError::Plugin(format!("cannot read firmware '{}': {e}", seg.firmware_path))
        })?;
        if firmware.is_empty() {
            return Err(FlashError::Plugin(format!(
                "firmware file '{}' is empty",
                seg.firmware_path
            )));
        }

        let flash_addr = parse_hex(Some(&seg.start_addr), "start_addr")?;

        lk(
            "flash.log.esp.flashingBytes",
            &[
                ("size", firmware.len().to_string()),
                ("addr", format!("0x{:08X}", flash_addr)),
            ],
        );

        if cancel.load(Ordering::Relaxed) {
            return Err(FlashError::Cancelled);
        }

        phase("Write");
        // Map segment progress to its portion of [0, 100]
        let mut cb = ProgressAdapter::new(progress, seg_start_pct, seg_end_pct);
        flasher
            .write_bin_to_flash(flash_addr, &firmware, &mut cb)
            .map_err(esp_err)?;

        lk("flash.log.esp.flashWriteComplete", &[]);
    }

    progress(FlashProgress::Percent { value: 100 });
    Ok(())
}

// ── Erase mode ───────────────────────────────────────────────────────────────

fn run_erase(
    job: &FlashJob,
    flasher: &mut Flasher,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashProgress),
) -> Result<(), FlashError> {
    let lk = |key: &'static str, pairs: &[(&str, String)]| emit_key(progress, key, pairs);
    let phase = |name: &str| {
        progress(FlashProgress::Phase {
            name: name.to_string(),
        })
    };

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    // Determine erase range — if neither address given, erase entire flash
    let start = job
        .erase_start_hex
        .as_deref()
        .filter(|s| !s.trim().is_empty());
    let end = job
        .erase_end_hex
        .as_deref()
        .filter(|s| !s.trim().is_empty());

    match (start, end) {
        (Some(_), Some(_)) => {
            const ESP_FLASH_SECTOR: u32 = 0x1000;

            let erase_start = parse_hex(job.erase_start_hex.as_deref(), "erase_start_hex")?;
            let erase_end = parse_hex(job.erase_end_hex.as_deref(), "erase_end_hex")?;
            if erase_end <= erase_start {
                return Err(FlashError::InvalidJob(
                    "erase_end_hex must be greater than erase_start_hex".into(),
                ));
            }
            // ROM / espflash require offset and length multiples of 0x1000 (see espflash CLI).
            let aligned_start = erase_start & !(ESP_FLASH_SECTOR - 1);
            let aligned_exclusive_end =
                (erase_end + ESP_FLASH_SECTOR - 1) & !(ESP_FLASH_SECTOR - 1);
            let size = aligned_exclusive_end.saturating_sub(aligned_start);
            if size == 0 {
                return Err(FlashError::InvalidJob(
                    "aligned erase region is empty; check erase_start_hex / erase_end_hex".into(),
                ));
            }
            lk(
                "flash.log.esp.erasingRegion",
                &[
                    ("start", format!("0x{:08X}", aligned_start)),
                    ("end", format!("0x{:08X}", aligned_exclusive_end)),
                    ("size", size.to_string()),
                ],
            );
            phase("Erase");
            progress(FlashProgress::Percent { value: 10 });
            flasher.erase_region(aligned_start, size).map_err(esp_err)?;
        }
        _ => {
            lk("flash.log.esp.eraseAllFlash", &[]);
            phase("Erase");
            progress(FlashProgress::Percent { value: 10 });
            flasher.erase_flash().map_err(esp_err)?;
        }
    }

    lk("flash.log.esp.eraseComplete", &[]);
    progress(FlashProgress::Percent { value: 100 });
    Ok(())
}

// ── Read mode ────────────────────────────────────────────────────────────────

fn run_read(
    job: &FlashJob,
    flasher: &mut Flasher,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashProgress),
) -> Result<(), FlashError> {
    let lk = |key: &'static str, pairs: &[(&str, String)]| emit_key(progress, key, pairs);
    let phase = |name: &str| {
        progress(FlashProgress::Phase {
            name: name.to_string(),
        })
    };

    let read_start = parse_hex(job.read_start_hex.as_deref(), "read_start_hex").unwrap_or(0x0);
    let read_end = parse_hex(job.read_end_hex.as_deref(), "read_end_hex").unwrap_or(0x0040_0000); // default 4 MiB

    if read_end <= read_start {
        return Err(FlashError::InvalidJob(
            "read_end_hex must be greater than read_start_hex".into(),
        ));
    }

    let file_path = job
        .read_file_path
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| FlashError::InvalidJob("missing read_file_path".into()))?;

    let size = read_end - read_start;

    lk(
        "flash.log.esp.readingBytes",
        &[
            ("size", size.to_string()),
            ("addr", format!("0x{:08X}", read_start)),
            ("path", file_path.to_string()),
        ],
    );

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    phase("Read");
    progress(FlashProgress::Percent { value: 10 });

    // espflash::Flasher::read_flash() does not expose ProgressCallbacks, so we
    // replicate its block-read loop here using the public connection() API to
    // emit per-block progress updates (10 % → 90 %).
    read_flash_with_progress(
        flasher, read_start, size, 0x1000, 64, file_path, cancel, progress,
    )?;

    lk("flash.log.esp.readComplete", &[]);
    progress(FlashProgress::Percent { value: 100 });
    Ok(())
}

/// Replicate `espflash::Flasher::read_flash` with per-block progress reporting.
///
/// `espflash` does not expose `ProgressCallbacks` for the read path, so we
/// drive the same `ReadFlash` command / `read_flash_response` / `write_raw`
/// protocol ourselves using the public `connection()` accessor.
/// Progress advances linearly from `pct_start` (10 %) to `pct_end` (90 %).
fn read_flash_with_progress(
    flasher: &mut Flasher,
    offset: u32,
    size: u32,
    block_size: u32,
    max_in_flight: u32,
    file_path: &str,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashProgress),
) -> Result<(), FlashError> {
    use std::fs::OpenOptions;
    use std::io::Write as _;

    use espflash::command::{Command, CommandType};
    use espflash::Error as EspError;
    use md5::{Digest as _, Md5};

    const PCT_START: u64 = 10;
    const PCT_END: u64 = 90;

    let mut data: Vec<u8> = Vec::with_capacity(size as usize);

    // Send the ReadFlash command to begin the transfer.
    flasher
        .connection()
        .with_timeout(CommandType::ReadFlash.timeout(), |conn| {
            conn.command(Command::ReadFlash {
                offset,
                size,
                block_size,
                max_in_flight,
            })
        })
        .map_err(esp_err)?;

    // Read blocks until we have the full image.
    while data.len() < size as usize {
        if cancel.load(Ordering::Relaxed) {
            return Err(FlashError::Cancelled);
        }

        let response = flasher
            .connection()
            .read_flash_response()
            .map_err(esp_err)?;
        let chunk: Vec<u8> = match response {
            Some(resp) => resp.value.try_into().map_err(esp_err)?,
            None => return Err(esp_err(EspError::IncorrectResponse)),
        };

        data.extend_from_slice(&chunk);

        if data.len() < size as usize && chunk.len() < block_size as usize {
            return Err(esp_err(EspError::CorruptData(
                block_size as usize,
                chunk.len(),
            )));
        }

        // Emit per-block progress: PCT_START → PCT_END
        let pct = PCT_START + (data.len() as u64 * (PCT_END - PCT_START) / size as u64);
        progress(FlashProgress::Percent {
            value: pct.min(PCT_END) as u8,
        });

        flasher
            .connection()
            .write_raw(data.len() as u32)
            .map_err(esp_err)?;
    }

    if data.len() > size as usize {
        return Err(esp_err(EspError::ReadMoreThanExpected));
    }

    // Read and verify the trailing MD5 digest sent by the stub.
    let response = flasher
        .connection()
        .read_flash_response()
        .map_err(esp_err)?;
    let digest: Vec<u8> = match response {
        Some(resp) => resp.value.try_into().map_err(esp_err)?,
        None => return Err(esp_err(EspError::IncorrectResponse)),
    };

    if digest.len() != 16 {
        return Err(esp_err(EspError::IncorrectDigestLength(digest.len())));
    }

    let checksum_md5 = Md5::digest(&data);
    if digest != checksum_md5[..] {
        return Err(esp_err(EspError::DigestMismatch(
            digest,
            checksum_md5.to_vec(),
        )));
    }

    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(true)
        .open(file_path)
        .map_err(|e| FlashError::Plugin(format!("cannot create output file: {e}")))?;
    file.write_all(&data)
        .map_err(|e| FlashError::Plugin(format!("cannot write output file: {e}")))?;

    Ok(())
}
