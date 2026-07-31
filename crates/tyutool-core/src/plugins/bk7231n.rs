//! BK7231N flash plugin — real hardware implementation.

use std::sync::atomic::AtomicBool;

use crate::error::FlashError;
use crate::flash_event::{FlashEvent, FlashMilestone, FlashPhase};
use crate::job::{FlashJob, FlashMode};
use crate::plugin::FlashPlugin;

use super::beken::chip::Bk7231nSpec;
use super::beken::ops;
use super::beken::transport::{SerialIo, Transport};

/// BK7231N flash plugin using the real Beken UART protocol.
pub struct Bk7231nPlugin;

impl FlashPlugin for Bk7231nPlugin {
    fn id(&self) -> &'static str {
        "BK7231N"
    }

    fn run(
        &self,
        job: &FlashJob,
        cancel: &AtomicBool,
        progress: &dyn Fn(FlashEvent),
    ) -> Result<(), FlashError> {
        let chip = Bk7231nSpec;
        run_beken(job, cancel, progress, &chip, false)
    }
}

/// Shared implementation for BK7231N and T5AI.
///
/// The `chip` trait object controls the behavioural differences;
/// `is_t5ai` selects the appropriate reset sequence.
pub(crate) fn run_beken(
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
    chip: &dyn super::beken::chip::ChipSpec,
    is_t5ai: bool,
) -> Result<(), FlashError> {
    log::info!("Plugin starting: port={}, mode={:?}", job.port, job.mode);

    let log = |msg: &str| log::info!("{}", msg);

    // ── Open serial port ────────────────────────────────────────────
    let serial_io = SerialIo::open(&job.port, chip.initial_baud()).map_err(to_flash_err)?;
    let mut transport = Transport::new(serial_io, &job.port, chip.initial_baud(), cancel, &log);

    run_beken_on_transport(job, &mut transport, progress, chip, is_t5ai)?;

    log::info!("Plugin completed successfully");
    Ok(())
}

/// The chip-protocol flow itself, on an already-open transport.
///
/// Split out of [`run_beken`] so the whole flow (handshake → flash ID → mode
/// dispatch) can be driven over a mock transport in tests.
fn run_beken_on_transport<T: super::beken::transport::IoTransport>(
    job: &FlashJob,
    transport: &mut Transport<'_, T>,
    progress: &dyn Fn(FlashEvent),
    chip: &dyn super::beken::chip::ChipSpec,
    is_t5ai: bool,
) -> Result<(), FlashError> {
    let phase = |p: FlashPhase| {
        progress(FlashEvent::Phase { phase: p });
    };

    // ── Phase: Handshake ────────────────────────────────────────────
    phase(FlashPhase::Handshake);
    ops::shake(transport, job.baud_rate, chip, is_t5ai).map_err(to_flash_err)?;
    progress(FlashEvent::Milestone {
        milestone: FlashMilestone::HandshakeComplete,
    });

    // ── Phase: Read flash parameters ────────────────────────────────
    phase(FlashPhase::ReadFlashId);
    let flash_params = ops::get_flash_params(transport, chip).map_err(to_flash_err)?;
    progress(FlashEvent::Milestone {
        milestone: FlashMilestone::FlashIdRead {
            mid: Some(flash_params.mid),
        },
    });

    // ── Dispatch by mode ────────────────────────────────────────────
    match job.mode {
        FlashMode::Flash => {
            run_flash_mode(job, transport, chip, &flash_params, progress)?;
        }
        FlashMode::Erase => {
            run_erase_mode(job, transport, chip, &flash_params, progress)?;
        }
        FlashMode::Read => {
            run_read_mode(job, transport, chip, &flash_params, progress)?;
        }
        FlashMode::Authorize => unreachable!("Authorize is handled in run_job before plugin.run"),
    }

    Ok(())
}

/// Flash mode: unprotect → erase → write → CRC (BK7231N) → protect → reboot.
fn run_flash_mode<T: super::beken::transport::IoTransport>(
    job: &FlashJob,
    transport: &mut Transport<'_, T>,
    chip: &dyn super::beken::chip::ChipSpec,
    flash_params: &super::beken::flash_table::FlashParams,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    let pct = |v: u8| progress(FlashEvent::Percent { value: v });
    let phase = |p: FlashPhase| progress(FlashEvent::Phase { phase: p });

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
            .ok_or_else(|| FlashError::InvalidJob("missing flash_start_hex".into()))?;
        let end_addr = job
            .flash_end_hex
            .clone()
            .ok_or_else(|| FlashError::InvalidJob("missing flash_end_hex".into()))?;
        vec![crate::job::FlashSegment {
            firmware_path,
            start_addr,
            end_addr,
        }]
    };

    if segments.is_empty() {
        return Err(FlashError::InvalidJob("no flash segments provided".into()));
    }

    // Unprotect
    phase(FlashPhase::Unprotect);
    ops::unprotect_flash(transport, flash_params, chip).map_err(to_flash_err)?;

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

        let base_addr = ops::parse_hex_addr(Some(&seg.start_addr)).map_err(to_flash_err)?;
        let end_addr = base_addr + firmware.len() as u32;

        // Align erase range to sector boundaries
        let erase_start = base_addr & !(super::beken::ops::SECTOR_SIZE_PUB - 1);
        let erase_end = (end_addr + super::beken::ops::SECTOR_SIZE_PUB - 1)
            & !(super::beken::ops::SECTOR_SIZE_PUB - 1);

        // Erase
        phase(FlashPhase::Erase);
        ops::erase(
            transport,
            flash_params,
            chip,
            erase_start,
            erase_end,
            &|done, total| {
                pct((done as u64 * 100 / total.max(1) as u64) as u8);
            },
        )
        .map_err(to_flash_err)?;
        pct(100); // explicitly mark Erase complete before transitioning

        // NOTE: this and WriteComplete below sit *inside* the segment loop, so a
        // multi-segment job would repeat both lines with nothing telling them
        // apart. Unreachable today (CLI and GUI always send a single segment).
        // When multi-segment lands, switch to the LN882H precedent —
        // `FlashMilestone::SegmentWritten { current, total }`
        // (see plugins/ln882h/mod.rs) — which carries the segment index.
        progress(FlashEvent::Milestone {
            milestone: FlashMilestone::EraseComplete,
        });

        // Write
        phase(FlashPhase::Write);
        ops::write(
            transport,
            flash_params,
            chip,
            &firmware,
            base_addr,
            &|done, total| {
                pct((done as u64 * 100 / total.max(1) as u64) as u8);
            },
        )
        .map_err(to_flash_err)?;
        pct(100); // explicitly mark Write complete before transitioning
        progress(FlashEvent::Milestone {
            milestone: FlashMilestone::WriteComplete,
        });

        // CRC check
        if !chip.has_per_sector_crc() {
            phase(FlashPhase::Verify);
            let padding_len = if firmware.len() & 0xff != 0 {
                0x100 - (firmware.len() & 0xff)
            } else {
                0
            };
            let mut padded = firmware.to_vec();
            padded.extend(std::iter::repeat_n(0xFFu8, padding_len));
            let expected_crc = ops::crc32_ver2(&padded);
            ops::crc_check(transport, base_addr, padded.len() as u32, expected_crc)
                .map_err(to_flash_err)?;
            pct(100); // explicitly mark Verify complete before transitioning
        }
    }

    // Protect
    phase(FlashPhase::Protect);
    ops::protect_flash(transport, flash_params, chip).map_err(to_flash_err)?;

    // Reboot
    phase(FlashPhase::Reboot);
    ops::reboot(transport).map_err(to_flash_err)?;
    // ⚠ Semantics: "reboot command sent", NOT "device confirmed rebooted".
    // `ops::reboot` fires three BKRegDoReboot frames and returns Ok
    // unconditionally — the bootrom sends no acknowledgement, and nothing here
    // re-opens the port to check the device came back up. This is the first
    // plugin in the tree to emit `Rebooted` at all (ESP and LN882H deliberately
    // don't), so it sets the precedent: consumers must word it as "command
    // sent". cobuilder-web already does; the CLI's "Device rebooted"
    // (tyutool-cli/src/reporter.rs, `milestone_text`) is pre-existing wording
    // that overstates it and should be revisited separately.
    progress(FlashEvent::Milestone {
        milestone: FlashMilestone::Rebooted,
    });

    Ok(())
}

/// Erase mode: unprotect → erase → protect → reboot.
fn run_erase_mode<T: super::beken::transport::IoTransport>(
    job: &FlashJob,
    transport: &mut Transport<'_, T>,
    chip: &dyn super::beken::chip::ChipSpec,
    flash_params: &super::beken::flash_table::FlashParams,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    let pct = |v: u8| progress(FlashEvent::Percent { value: v });
    let phase = |p: FlashPhase| progress(FlashEvent::Phase { phase: p });

    let start = ops::parse_hex_addr(job.erase_start_hex.as_deref()).map_err(to_flash_err)?;
    let end = ops::parse_hex_addr(job.erase_end_hex.as_deref()).map_err(to_flash_err)?;

    if start >= end {
        return Err(FlashError::InvalidJob(format!(
            "erase start ({start:#010x}) >= end ({end:#010x})"
        )));
    }

    // Half-open [start, end) must use 4 KiB-aligned bounds (matches UI / ESP path).
    const SECTOR: u32 = super::beken::ops::SECTOR_SIZE_PUB;
    let aligned_start = start & !(SECTOR - 1);
    let aligned_end = (end + SECTOR - 1) & !(SECTOR - 1);
    if aligned_end <= aligned_start {
        return Err(FlashError::InvalidJob(
            "aligned erase range is empty; check erase_start_hex / erase_end_hex".into(),
        ));
    }

    // Unprotect
    phase(FlashPhase::Unprotect);
    ops::unprotect_flash(transport, flash_params, chip).map_err(to_flash_err)?;

    // Erase
    phase(FlashPhase::Erase);
    ops::erase(
        transport,
        flash_params,
        chip,
        aligned_start,
        aligned_end,
        &|done, total| {
            pct((done as u64 * 100 / total.max(1) as u64) as u8);
        },
    )
    .map_err(to_flash_err)?;
    pct(100);

    // Protect
    phase(FlashPhase::Protect);
    ops::protect_flash(transport, flash_params, chip).map_err(to_flash_err)?;

    // Reboot
    phase(FlashPhase::Reboot);
    ops::reboot(transport).map_err(to_flash_err)?;

    Ok(())
}

/// Read mode: read flash → save to file → CRC check (BK7231N) → reboot.
fn run_read_mode<T: super::beken::transport::IoTransport>(
    job: &FlashJob,
    transport: &mut Transport<'_, T>,
    chip: &dyn super::beken::chip::ChipSpec,
    flash_params: &super::beken::flash_table::FlashParams,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    let pct = |v: u8| progress(FlashEvent::Percent { value: v });
    let phase = |p: FlashPhase| progress(FlashEvent::Phase { phase: p });

    let start = ops::parse_hex_addr(job.read_start_hex.as_deref()).unwrap_or(0);
    let end = ops::parse_hex_addr(job.read_end_hex.as_deref()).map_err(to_flash_err)?;

    if end <= start {
        return Err(FlashError::InvalidJob(format!(
            "read start ({start:#010x}) >= end ({end:#010x})"
        )));
    }
    let length = end - start;

    let file_path = job
        .read_file_path
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| FlashError::InvalidJob("missing read_file_path".into()))?;

    // Read
    phase(FlashPhase::Read);
    log::info!(
        "Reading {:#010x}..{:#010x} ({} KiB)",
        start,
        end,
        length / 1024
    );
    let data = ops::read(
        transport,
        flash_params,
        chip,
        start,
        length,
        &|done, total| {
            pct((done as u64 * 100 / total.max(1) as u64) as u8);
        },
    )
    .map_err(to_flash_err)?;

    // CRC check (BK7231N only — T5AI already verified per-sector CRC during read)
    // BK7231N bootrom uses crc32_ver2 (no final XOR).
    // For read, we use the raw data length (already aligned from sector reads).
    if !chip.has_per_sector_crc() {
        phase(FlashPhase::Verify);
        let expected_crc = ops::crc32_ver2(&data);
        ops::crc_check(transport, start, length, expected_crc).map_err(to_flash_err)?;
        pct(100);
    }

    // Save to file
    phase(FlashPhase::Save);
    log::info!("Saving {} bytes to {}", data.len(), file_path);
    std::fs::write(file_path, &data)
        .map_err(|e| FlashError::Plugin(format!("cannot write file '{}': {e}", file_path)))?;

    // Reboot
    phase(FlashPhase::Reboot);
    ops::reboot(transport).map_err(to_flash_err)?;

    Ok(())
}

/// Convert `ProtocolError` → `FlashError`.
fn to_flash_err(e: super::beken::frame::ProtocolError) -> FlashError {
    match e {
        super::beken::frame::ProtocolError::Cancelled => FlashError::Cancelled,
        other => FlashError::Plugin(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::beken::command;
    use super::super::beken::transport::mock::MockIo;
    use super::*;
    use crate::flash_event::FlashMilestone;
    use crate::job::FlashSegment;
    use std::cell::RefCell;
    use std::sync::atomic::Ordering;

    #[test]
    fn plugin_id_is_uppercase() {
        assert_eq!(Bk7231nPlugin.id(), "BK7231N");
    }

    // ── Flash-flow scaffolding (frame builders mirror `beken::ops` tests) ──

    /// Standard RX frame `[04 0e LEN 01 e0 fc CMD STATUS DATA…]`.
    fn std_resp(cmd: u8, status: u8, data: &[u8]) -> Vec<u8> {
        let len = (5 + data.len()) as u8;
        let mut v = vec![0x04, 0x0e, len, 0x01, 0xe0, 0xfc, cmd, status];
        v.extend_from_slice(data);
        v
    }

    /// Extended RX frame `[04 0e ff 01 e0 fc f4 LEN_L LEN_H CMD STATUS DATA…]`.
    fn ext_resp(cmd: u8, status: u8, data: &[u8]) -> Vec<u8> {
        let len = (2 + data.len()) as u16;
        let mut v = vec![0x04, 0x0e, 0xff, 0x01, 0xe0, 0xfc, 0xf4];
        v.push((len & 0xff) as u8);
        v.push((len >> 8) as u8);
        v.push(cmd);
        v.push(status);
        v.extend_from_slice(data);
        v
    }

    fn mid_ext_resp(mid: u32) -> Vec<u8> {
        let b = mid.to_le_bytes();
        ext_resp(command::CMD_FLASH_GET_MID, 0x00, &[0x00, b[0], b[1], b[2]])
    }

    fn crc_resp(crc: u32) -> Vec<u8> {
        let b = crc.to_le_bytes();
        std_resp(command::CMD_CHECK_CRC, b[0], &[b[1], b[2], b[3]])
    }

    fn flash_job(firmware_path: &str) -> FlashJob {
        FlashJob {
            mode: FlashMode::Flash,
            chip_id: "BK7231N".into(),
            port: "/dev/mock".into(),
            // Same as the chip's initial baud → no baud-switch round trip.
            baud_rate: 115200,
            segments: Some(vec![FlashSegment {
                firmware_path: firmware_path.into(),
                start_addr: "0x11000".into(),
                end_addr: "0x12000".into(),
            }]),
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

    /// The Beken flash flow must report its progress on the user-visible
    /// channel: handshake, flash chip identified, erase done, write done and
    /// reboot are all things an operator watching the log panel needs — and per
    /// the Logging Contract they belong on `FlashEvent::Milestone`, not
    /// `log::info!` (which only reaches the developer log file).
    #[test]
    fn flash_mode_emits_user_visible_milestones() {
        let firmware = vec![0xAAu8; 4096];
        let path = std::env::temp_dir().join("tyutool_bk7231n_milestone_flow.bin");
        std::fs::write(&path, &firmware).unwrap();

        let mut mock = MockIo::new();
        // Handshake: LinkCheck ack.
        mock.add_response(vec![0x04, 0x0e, 0x05, 0x01, 0xe0, 0xfc, 0x01, 0x00]);
        // FlashGetMID: MID absent from the table → fallback params (no WP), so
        // unprotect/protect are no-ops and need no frames.
        mock.add_response(mid_ext_resp(0xABCDEF));
        // Erase: one 4 KiB sector ack.
        mock.add_response(ext_resp(command::CMD_SET_BAUD_RATE, 0x00, &[]));
        // Write: one 4 KiB chunk ack.
        mock.add_response(ext_resp(command::CMD_FLASH_WRITE_4K, 0x00, &[0, 0, 0, 0]));
        // Whole-image CRC (BK7231N has no per-sector CRC).
        mock.add_response(crc_resp(ops::crc32_ver2(&firmware)));

        static CANCEL: AtomicBool = AtomicBool::new(false);
        CANCEL.store(false, Ordering::Relaxed);
        let log = |_: &str| {};
        let mut transport = Transport::new(mock, "/dev/mock", 115200, &CANCEL, &log);

        let job = flash_job(&path.to_string_lossy());
        let events: RefCell<Vec<FlashEvent>> = RefCell::new(Vec::new());
        let result = run_beken_on_transport(
            &job,
            &mut transport,
            &|e| events.borrow_mut().push(e),
            &Bk7231nSpec,
            false,
        );
        let _ = std::fs::remove_file(&path);
        result.unwrap();

        let milestones: Vec<FlashMilestone> = events
            .borrow()
            .iter()
            .filter_map(|e| match e {
                FlashEvent::Milestone { milestone } => Some(milestone.clone()),
                _ => None,
            })
            .collect();

        let has = |pred: &dyn Fn(&FlashMilestone) -> bool| milestones.iter().any(pred);
        assert!(
            has(&|m| matches!(m, FlashMilestone::HandshakeComplete)),
            "missing HandshakeComplete; got {milestones:?}"
        );
        assert!(
            has(&|m| matches!(
                m,
                FlashMilestone::FlashIdRead {
                    mid: Some(0xABCDEF)
                }
            )),
            "missing FlashIdRead with the MID read from the device; got {milestones:?}"
        );
        assert!(
            has(&|m| matches!(m, FlashMilestone::EraseComplete)),
            "missing EraseComplete; got {milestones:?}"
        );
        assert!(
            has(&|m| matches!(m, FlashMilestone::WriteComplete)),
            "missing WriteComplete; got {milestones:?}"
        );
        assert!(
            has(&|m| matches!(m, FlashMilestone::Rebooted)),
            "missing Rebooted; got {milestones:?}"
        );
    }
}
