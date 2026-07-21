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

use crate::error::FlashError;
use crate::flash_event::{FlashEvent, FlashMilestone, FlashPhase};
use crate::job::{FlashJob, FlashMode};

use super::chips::EspChipDef;

// ── Progress adapter ─────────────────────────────────────────────────────────

/// Bridges espflash `ProgressCallbacks` to our `FlashEvent` enum.
struct ProgressAdapter<'a> {
    progress: &'a dyn Fn(FlashEvent),
    total: usize,
    current: usize,
    /// Percent range mapped to [pct_start, pct_end).
    pct_start: u8,
    pct_end: u8,
}

impl<'a> ProgressAdapter<'a> {
    fn new(progress: &'a dyn Fn(FlashEvent), pct_start: u8, pct_end: u8) -> Self {
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
        (self.progress)(FlashEvent::Percent {
            value: pct.min(self.pct_end as u64) as u8,
        });
    }

    fn verifying(&mut self) {
        (self.progress)(FlashEvent::Phase {
            phase: FlashPhase::Verify,
        });
        // Reset so Verify emits an independent 0-100% range, not a continuation
        // of Write's range.  NOTE: espflash is called with verify=false (see
        // Flasher::connect below), so verifying() is currently unreachable.
        // The reset is kept as a safety net in case verify is enabled in future.
        self.pct_start = 0;
        self.pct_end = 100;
        self.total = 1;
        self.current = 0;
    }

    fn finish(&mut self, skipped: bool) {
        if !skipped {
            (self.progress)(FlashEvent::Percent {
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

/// Espressif's USB vendor id (native USB-Serial-JTAG / USB-OTG peripherals).
const ESPRESSIF_VID: u16 = 0x303a;
/// PID of the built-in USB-Serial-JTAG peripheral.
const USB_SERIAL_JTAG_PID: u16 = 0x1001;

/// Choose the pre-connect reset strategy.
///
/// espflash's `DefaultReset` inspects the USB pid and only selects the
/// USB-Serial-JTAG reset sequence when `pid == 0x1001`; otherwise it falls back
/// to the classic DTR/RTS reset. Chips reached over a native USB peripheral
/// (e.g. ESP32-P4) cannot be reset via DTR/RTS, so when we recognise a
/// native-USB port whose pid was *not* surfaced as `0x1001` by the OS we force
/// `UsbReset` instead of letting espflash pick the ineffective classic reset.
///
/// UART-bridge adapters (CP210x/CH340, etc.) keep `DefaultReset` — their vid and
/// port names don't match, so existing ESP32/C3/S3-over-bridge setups are
/// unaffected.
fn choose_before_reset(port_name: &str, info: &UsbPortInfo) -> ResetBeforeOperation {
    // pid already flags USB-Serial-JTAG: espflash's DefaultReset resolves to the
    // correct sequence on its own — nothing to override.
    if info.pid == USB_SERIAL_JTAG_PID {
        return ResetBeforeOperation::DefaultReset;
    }
    let native_usb = info.vid == ESPRESSIF_VID
        || port_name.contains("usbmodem") // macOS native USB CDC
        || port_name.contains("ttyACM"); // Linux native USB CDC
    if native_usb {
        ResetBeforeOperation::UsbReset
    } else {
        ResetBeforeOperation::DefaultReset
    }
}

/// Turn an espflash connect error into an actionable `FlashError`.
///
/// espflash's `Error::Connection` renders only as "Error while connecting to
/// device" and boxes the real cause without exposing it via `source()`, so we
/// attach our own guidance here and surface it to the user via
/// `FlashEvent::Warning`. Non-connection variants keep espflash's own message.
fn map_connect_error(
    e: espflash::Error,
    def: &EspChipDef,
    progress: &dyn Fn(FlashEvent),
) -> FlashError {
    use espflash::Error as E;
    match e {
        E::Connection(_) => {
            let guidance = format!(
                "could not connect to {}. Put the device into download mode \
                 (hold BOOT/GPIO0 while tapping RESET), then re-check the cable and port",
                def.id
            );
            log::error!("ESP connect failed for {}: {e:#?}", def.id);
            progress(FlashEvent::Warning {
                message: guidance.clone(),
            });
            FlashError::Plugin(guidance)
        }
        E::ChipMismatch(expected, got) => FlashError::Plugin(format!(
            "chip mismatch: selected {expected} but device reports {got}; pick the matching chip"
        )),
        other => esp_err(other),
    }
}

// ── Main entry point ─────────────────────────────────────────────────────────

/// Shared flash/erase/read implementation for all ESP32 plugin variants.
pub(crate) fn run_esp(
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
    def: &EspChipDef,
) -> Result<(), FlashError> {
    log::info!(
        "ESP plugin starting: chip={}, port={}, mode={:?}",
        def.id,
        job.port,
        job.mode
    );

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    // ── Open serial port ─────────────────────────────────────────────
    progress(FlashEvent::Phase {
        phase: FlashPhase::Connect,
    });
    log::info!("Opening port {}", job.port);

    let port_info = usb_port_info(&job.port);
    let before_reset = choose_before_reset(&job.port, &port_info);
    log::info!(
        "ESP connect: reset strategy {:?} (vid={:#06x}, pid={:#06x})",
        before_reset,
        port_info.vid,
        port_info.pid
    );

    let serial = serialport::new(&job.port, 115_200)
        .timeout(Duration::from_millis(500))
        .open_native()
        .map_err(|e| FlashError::Plugin(format!("cannot open port {}: {e}", job.port)))?;

    let conn = Connection::new(
        serial,
        port_info,
        ResetAfterOperation::HardReset,
        before_reset,
        115_200,
    );

    // ── Connect & detect chip ────────────────────────────────────────
    log::info!("Connecting to ESP device");

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    let mut flasher = match Flasher::connect(
        conn,
        true,           // use_stub — loads RAM stub for faster flash ops
        false,          // verify — we do our own progress reporting
        false,          // skip
        Some(def.chip), // expected chip; mismatch → error
        None,           // baud — will change after stub loads if needed
    ) {
        Ok(f) => f,
        Err(e) => return Err(map_connect_error(e, def, progress)),
    };

    // Log device information
    match flasher.device_info() {
        Ok(info) => {
            progress(FlashEvent::Milestone {
                milestone: FlashMilestone::Connected {
                    chip_info: Some(format!("{} (revision {:?})", info.chip, info.revision)),
                },
            });
        }
        Err(e) => {
            log::warn!("Failed to read ESP device info: {}", e);
        }
    }

    // Switch to user-requested baud rate if higher than default
    let target_baud = job.baud_rate.max(115_200);
    if target_baud > 115_200 {
        log::info!("Switching baud rate to {}", target_baud);
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

    // espflash only applies the ResetAfterOperation when asked explicitly (its
    // CLI calls this after every operation). Without it the chip stays in the
    // ROM bootloader once the port closes, so a follow-up step that talks to
    // the application firmware — e.g. batch authorize right after flashing —
    // fails until someone power-cycles the board. HardReset toggles EN only
    // (GPIO0 untouched), so the freshly flashed firmware boots normally.
    log::info!("Resetting ESP device to exit download mode");
    if let Err(e) = flasher.connection().reset_after(true, def.chip) {
        log::warn!("ESP reset after operation failed: {e}");
        progress(FlashEvent::Warning {
            message: "could not reset the device after the operation; \
                      power-cycle or reset it manually before authorizing"
                .to_string(),
        });
    }

    log::info!("ESP plugin completed successfully");
    Ok(())
}

// ── Flash mode ───────────────────────────────────────────────────────────────

fn run_flash(
    job: &FlashJob,
    flasher: &mut Flasher,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
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
        progress(FlashEvent::Phase {
            phase: FlashPhase::WriteSegment {
                current: (i + 1) as u32,
                total: total_segments as u32,
            },
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

        log::info!("Flashing {} bytes at 0x{:08X}", firmware.len(), flash_addr);

        if cancel.load(Ordering::Relaxed) {
            return Err(FlashError::Cancelled);
        }

        progress(FlashEvent::Phase {
            phase: FlashPhase::Write,
        });
        // Each segment emits an independent 0-100% range.
        let mut cb = ProgressAdapter::new(progress, 0, 100);
        flasher
            .write_bin_to_flash(flash_addr, &firmware, &mut cb)
            .map_err(esp_err)?;

        log::info!(
            "Flash write complete for segment {}/{}",
            i + 1,
            total_segments
        );
    }

    progress(FlashEvent::Percent { value: 100 });
    Ok(())
}

// ── Erase mode ───────────────────────────────────────────────────────────────

fn run_erase(
    job: &FlashJob,
    flasher: &mut Flasher,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
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
            log::info!(
                "Erasing region 0x{:08X}..0x{:08X} ({} bytes)",
                aligned_start,
                aligned_exclusive_end,
                size
            );
            progress(FlashEvent::Phase {
                phase: FlashPhase::Erase,
            });
            progress(FlashEvent::Percent { value: 10 });
            flasher.erase_region(aligned_start, size).map_err(esp_err)?;
        }
        _ => {
            log::info!("Erasing all flash");
            progress(FlashEvent::Phase {
                phase: FlashPhase::Erase,
            });
            progress(FlashEvent::Percent { value: 10 });
            flasher.erase_flash().map_err(esp_err)?;
        }
    }

    progress(FlashEvent::Milestone {
        milestone: FlashMilestone::EraseComplete,
    });
    progress(FlashEvent::Percent { value: 100 });
    Ok(())
}

// ── Read mode ────────────────────────────────────────────────────────────────

fn run_read(
    job: &FlashJob,
    flasher: &mut Flasher,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
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

    log::info!(
        "Reading {} bytes at 0x{:08X} to {}",
        size,
        read_start,
        file_path
    );

    if cancel.load(Ordering::Relaxed) {
        return Err(FlashError::Cancelled);
    }

    progress(FlashEvent::Phase {
        phase: FlashPhase::Read,
    });
    progress(FlashEvent::Percent { value: 10 });

    // espflash::Flasher::read_flash() does not expose ProgressCallbacks, so we
    // replicate its block-read loop here using the public connection() API to
    // emit per-block progress updates (10 % → 90 %).
    read_flash_with_progress(
        flasher, read_start, size, 0x1000, 64, file_path, cancel, progress,
    )?;

    log::info!("Read complete");
    progress(FlashEvent::Percent { value: 100 });
    Ok(())
}

/// Replicate `espflash::Flasher::read_flash` with per-block progress reporting.
///
/// `espflash` does not expose `ProgressCallbacks` for the read path, so we
/// drive the same `ReadFlash` command / `read_flash_response` / `write_raw`
/// protocol ourselves using the public `connection()` accessor.
/// Progress advances linearly from `pct_start` (10 %) to `pct_end` (90 %).
#[allow(clippy::too_many_arguments)]
fn read_flash_with_progress(
    flasher: &mut Flasher,
    offset: u32,
    size: u32,
    block_size: u32,
    max_in_flight: u32,
    file_path: &str,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
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
        progress(FlashEvent::Percent {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn usb_info(vid: u16, pid: u16) -> UsbPortInfo {
        UsbPortInfo {
            vid,
            pid,
            serial_number: None,
            manufacturer: None,
            product: None,
            interface: None,
        }
    }

    #[test]
    fn usb_serial_jtag_pid_keeps_default_reset() {
        // espflash's DefaultReset already resolves pid 0x1001 to the JTAG sequence.
        let info = usb_info(ESPRESSIF_VID, USB_SERIAL_JTAG_PID);
        assert!(matches!(
            choose_before_reset("/dev/cu.usbmodem1234", &info),
            ResetBeforeOperation::DefaultReset
        ));
    }

    #[test]
    fn espressif_vid_without_jtag_pid_forces_usb_reset() {
        // Native USB port whose pid the OS did not surface as 0x1001.
        let info = usb_info(ESPRESSIF_VID, 0x0000);
        assert!(matches!(
            choose_before_reset("COM7", &info),
            ResetBeforeOperation::UsbReset
        ));
    }

    #[test]
    fn macos_usbmodem_name_forces_usb_reset() {
        let info = usb_info(0, 0); // pid/vid not surfaced by the OS
        assert!(matches!(
            choose_before_reset("/dev/cu.usbmodem5B5E1349761", &info),
            ResetBeforeOperation::UsbReset
        ));
    }

    #[test]
    fn uart_bridge_keeps_default_reset() {
        // CP210x bridge: not Espressif vid, name doesn't look like native USB.
        let info = usb_info(0x10c4, 0xea60);
        assert!(matches!(
            choose_before_reset("/dev/cu.SLAB_USBtoUART", &info),
            ResetBeforeOperation::DefaultReset
        ));
    }

    /// Collects every `FlashEvent` the adapter emits and returns the percent
    /// values seen (in order). Non-percent events are ignored for percent runs.
    fn percents_from(events: &[FlashEvent]) -> Vec<u8> {
        events
            .iter()
            .filter_map(|e| match e {
                FlashEvent::Percent { value } => Some(*value),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn maps_full_range_linearly() {
        let seen: RefCell<Vec<FlashEvent>> = RefCell::new(Vec::new());
        let cb = |e: FlashEvent| seen.borrow_mut().push(e);
        let mut a = ProgressAdapter::new(&cb, 0, 100);
        a.init(0, 100);
        a.update(0);
        a.update(50);
        a.update(100);
        assert_eq!(percents_from(&seen.borrow()), vec![0, 50, 100]);
    }

    #[test]
    fn maps_into_subrange() {
        let seen: RefCell<Vec<FlashEvent>> = RefCell::new(Vec::new());
        let cb = |e: FlashEvent| seen.borrow_mut().push(e);
        // Range [40, 80): start at 40%, full progress lands at 80%.
        let mut a = ProgressAdapter::new(&cb, 40, 80);
        a.init(0, 10);
        a.update(0);
        a.update(5);
        a.update(10);
        assert_eq!(percents_from(&seen.borrow()), vec![40, 60, 80]);
    }

    #[test]
    fn update_is_clamped_to_pct_end() {
        let seen: RefCell<Vec<FlashEvent>> = RefCell::new(Vec::new());
        let cb = |e: FlashEvent| seen.borrow_mut().push(e);
        let mut a = ProgressAdapter::new(&cb, 0, 50);
        a.init(0, 10);
        // current beyond total would overshoot; must clamp to pct_end.
        a.update(20);
        assert_eq!(percents_from(&seen.borrow()), vec![50]);
    }

    #[test]
    fn init_zero_total_does_not_divide_by_zero() {
        let seen: RefCell<Vec<FlashEvent>> = RefCell::new(Vec::new());
        let cb = |e: FlashEvent| seen.borrow_mut().push(e);
        let mut a = ProgressAdapter::new(&cb, 0, 100);
        a.init(0, 0); // clamped internally to 1
        a.update(0);
        a.update(1);
        assert_eq!(percents_from(&seen.borrow()), vec![0, 100]);
    }

    #[test]
    fn finish_emits_pct_end_when_not_skipped() {
        let seen: RefCell<Vec<FlashEvent>> = RefCell::new(Vec::new());
        let cb = |e: FlashEvent| seen.borrow_mut().push(e);
        let mut a = ProgressAdapter::new(&cb, 0, 90);
        a.finish(false);
        assert_eq!(percents_from(&seen.borrow()), vec![90]);
    }

    #[test]
    fn finish_emits_nothing_when_skipped() {
        let seen: RefCell<Vec<FlashEvent>> = RefCell::new(Vec::new());
        let cb = |e: FlashEvent| seen.borrow_mut().push(e);
        let mut a = ProgressAdapter::new(&cb, 0, 90);
        a.finish(true);
        assert!(seen.borrow().is_empty());
    }

    #[test]
    fn verifying_resets_to_full_range_and_emits_verify_phase() {
        let seen: RefCell<Vec<FlashEvent>> = RefCell::new(Vec::new());
        let cb = |e: FlashEvent| seen.borrow_mut().push(e);
        // Start in a write subrange, then enter verify.
        let mut a = ProgressAdapter::new(&cb, 40, 80);
        a.init(0, 10);
        a.verifying();
        // First event is the Verify phase transition.
        assert!(matches!(
            seen.borrow()[0],
            FlashEvent::Phase {
                phase: FlashPhase::Verify
            }
        ));
        // After verifying(), range is reset to [0, 100): update maps linearly.
        a.update(0);
        a.update(10);
        assert_eq!(percents_from(&seen.borrow()), vec![0, 100]);
    }
}
