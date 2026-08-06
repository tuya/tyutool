//! BK7231N flash plugin — real hardware implementation.

use std::sync::atomic::AtomicBool;

use crate::error::FlashError;
use crate::flash_event::{FlashEvent, FlashPhase};
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

    // ── Helper closures ─────────────────────────────────────────────
    let log = |msg: &str| log::info!("{}", msg);
    let phase = |p: FlashPhase| {
        progress(FlashEvent::Phase { phase: p });
    };

    // ── Open serial port ────────────────────────────────────────────
    let serial_io = SerialIo::open(&job.port, chip.initial_baud()).map_err(to_flash_err)?;
    let mut transport = Transport::new(serial_io, &job.port, chip.initial_baud(), cancel, &log);

    // ── Phase: Handshake ────────────────────────────────────────────
    phase(FlashPhase::Handshake);
    ops::shake(&mut transport, job.baud_rate, chip, is_t5ai).map_err(to_flash_err)?;

    // ── Phase: Read flash parameters ────────────────────────────────
    phase(FlashPhase::ReadFlashId);
    let flash_params = ops::get_flash_params(&mut transport, chip).map_err(to_flash_err)?;

    // ── Dispatch by mode ────────────────────────────────────────────
    match job.mode {
        FlashMode::Flash => {
            run_flash_mode(job, &mut transport, chip, &flash_params, progress)?;
        }
        FlashMode::Erase => {
            run_erase_mode(job, &mut transport, chip, &flash_params, progress)?;
        }
        FlashMode::Read => {
            run_read_mode(job, &mut transport, chip, &flash_params, progress)?;
        }
        FlashMode::Authorize => unreachable!("Authorize is handled in run_job before plugin.run"),
    }

    log::info!("Plugin completed successfully");
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
    let warn = |msg: &str| {
        progress(FlashEvent::Warning {
            message: msg.to_string(),
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
            Some(&warn),
        )
        .map_err(to_flash_err)?;
        pct(100); // explicitly mark Erase complete before transitioning

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
    let warn = |msg: &str| {
        progress(FlashEvent::Warning {
            message: msg.to_string(),
        })
    };

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
        Some(&warn),
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
    let warn = |msg: &str| {
        progress(FlashEvent::Warning {
            message: msg.to_string(),
        })
    };

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
        Some(&warn),
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
    use super::*;

    #[test]
    fn plugin_id_is_uppercase() {
        assert_eq!(Bk7231nPlugin.id(), "BK7231N");
    }
}
