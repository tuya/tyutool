//! BK7231N flash plugin — real hardware implementation.

use std::sync::atomic::AtomicBool;

use crate::error::FlashError;
use crate::job::{FlashJob, FlashMode};
use crate::plugin::FlashPlugin;
use crate::progress::FlashProgress;

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
        progress: &dyn Fn(FlashProgress),
    ) -> Result<(), FlashError> {
        let chip = Bk7231nSpec;
        run_beken(job, cancel, progress, &chip, false)
    }
}

/// Shared implementation for BK7231N and T5.
///
/// The `chip` trait object controls the behavioural differences;
/// `is_t5` selects the appropriate reset sequence.
pub(crate) fn run_beken(
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashProgress),
    chip: &dyn super::beken::chip::ChipSpec,
    is_t5: bool,
) -> Result<(), FlashError> {
    log::info!("Plugin starting: port={}, mode={:?}", job.port, job.mode);

    // ── Helper closures ─────────────────────────────────────────────
    let log = |msg: &str| {
        progress(FlashProgress::LogLine {
            line: msg.to_string(),
        });
    };
    let pct = |v: u8| {
        progress(FlashProgress::Percent { value: v });
    };
    let phase = |name: &str| {
        progress(FlashProgress::Phase {
            name: name.to_string(),
        });
    };

    // ── Open serial port ────────────────────────────────────────────
    let serial_io = SerialIo::open(&job.port, chip.initial_baud()).map_err(to_flash_err)?;
    let mut transport = Transport::new(serial_io, &job.port, chip.initial_baud(), cancel, &log);

    // ── Phase: Handshake ────────────────────────────────────────────
    phase("Handshake");
    ops::shake(&mut transport, job.baud_rate, chip, is_t5).map_err(to_flash_err)?;
    pct(3);

    // ── Phase: Read flash parameters ────────────────────────────────
    phase("ReadFlashID");
    let flash_params = ops::get_flash_params(&mut transport, chip).map_err(to_flash_err)?;
    pct(5);

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
    progress: &dyn Fn(FlashProgress),
) -> Result<(), FlashError> {
    let pct = |v: u8| progress(FlashProgress::Percent { value: v });
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
    phase("Unprotect");
    ops::unprotect_flash(transport, flash_params, chip).map_err(to_flash_err)?;
    pct(8);

    let total_segments = segments.len();

    for (i, seg) in segments.iter().enumerate() {
        let seg_start_pct = 8 + (i as u64 * 87 / total_segments as u64) as u8;
        let seg_end_pct = 8 + ((i + 1) as u64 * 87 / total_segments as u64) as u8;
        let seg_range = (seg_end_pct - seg_start_pct) as u64;

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

        let base_addr = ops::parse_hex_addr(Some(&seg.start_addr)).map_err(to_flash_err)?;
        let end_addr = base_addr + firmware.len() as u32;

        // Align erase range to sector boundaries
        let erase_start = base_addr & !(super::beken::ops::SECTOR_SIZE_PUB - 1);
        let erase_end = (end_addr + super::beken::ops::SECTOR_SIZE_PUB - 1)
            & !(super::beken::ops::SECTOR_SIZE_PUB - 1);

        // Erase
        phase("Erase");
        ops::erase(
            transport,
            flash_params,
            chip,
            erase_start,
            erase_end,
            &|done, total| {
                let p = seg_start_pct
                    + (done as u64 * (seg_range * 30 / 100) / total.max(1) as u64) as u8;
                pct(p);
            },
        )
        .map_err(to_flash_err)?;

        // Write
        phase("Write");
        ops::write(
            transport,
            flash_params,
            chip,
            &firmware,
            base_addr,
            &|done, total| {
                // Write takes 30% -> 90% of segment range
                let offset = (seg_range * 30 / 100) as u8;
                let p = seg_start_pct
                    + offset
                    + (done as u64 * (seg_range * 60 / 100) / total.max(1) as u64) as u8;
                pct(p);
            },
        )
        .map_err(to_flash_err)?;

        // CRC check
        if !chip.has_per_sector_crc() {
            phase("Verify");
            let padding_len = if firmware.len() & 0xff != 0 {
                0x100 - (firmware.len() & 0xff)
            } else {
                0
            };
            let mut padded = firmware.to_vec();
            padded.extend(std::iter::repeat(0xFFu8).take(padding_len));
            let expected_crc = ops::crc32_ver2(&padded);
            ops::crc_check(transport, base_addr, padded.len() as u32, expected_crc)
                .map_err(to_flash_err)?;
        }
        pct(seg_end_pct);
    }

    // Protect
    phase("Protect");
    ops::protect_flash(transport, flash_params, chip).map_err(to_flash_err)?;

    // Reboot
    phase("Reboot");
    ops::reboot(transport).map_err(to_flash_err)?;
    pct(100);

    Ok(())
}

/// Erase mode: unprotect → erase → protect → reboot.
fn run_erase_mode<T: super::beken::transport::IoTransport>(
    job: &FlashJob,
    transport: &mut Transport<'_, T>,
    chip: &dyn super::beken::chip::ChipSpec,
    flash_params: &super::beken::flash_table::FlashParams,
    progress: &dyn Fn(FlashProgress),
) -> Result<(), FlashError> {
    let pct = |v: u8| progress(FlashProgress::Percent { value: v });
    let phase = |name: &str| {
        progress(FlashProgress::Phase {
            name: name.to_string(),
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
    phase("Unprotect");
    ops::unprotect_flash(transport, flash_params, chip).map_err(to_flash_err)?;
    pct(8);

    // Erase
    phase("Erase");
    ops::erase(
        transport,
        flash_params,
        chip,
        aligned_start,
        aligned_end,
        &|done, total| {
            let p = 8 + (done as u64 * 87 / total.max(1) as u64) as u8; // 8→95%
            pct(p);
        },
    )
    .map_err(to_flash_err)?;
    pct(95);

    // Protect
    phase("Protect");
    ops::protect_flash(transport, flash_params, chip).map_err(to_flash_err)?;

    // Reboot
    phase("Reboot");
    ops::reboot(transport).map_err(to_flash_err)?;
    pct(100);

    Ok(())
}

/// Read mode: read flash → save to file → CRC check (BK7231N) → reboot.
fn run_read_mode<T: super::beken::transport::IoTransport>(
    job: &FlashJob,
    transport: &mut Transport<'_, T>,
    chip: &dyn super::beken::chip::ChipSpec,
    flash_params: &super::beken::flash_table::FlashParams,
    progress: &dyn Fn(FlashProgress),
) -> Result<(), FlashError> {
    let pct = |v: u8| progress(FlashProgress::Percent { value: v });
    let phase = |name: &str| {
        progress(FlashProgress::Phase {
            name: name.to_string(),
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
    phase("Read");
    {
        use std::collections::HashMap;
        let mut p = HashMap::new();
        p.insert("start".to_string(), format!("{:#010x}", start));
        p.insert("end".to_string(), format!("{:#010x}", end));
        p.insert("kib".to_string(), (length / 1024).to_string());
        progress(FlashProgress::LogKey {
            key: "flash.log.beken.readRange".to_string(),
            params: p,
        });
    }
    let data = ops::read(
        transport,
        flash_params,
        chip,
        start,
        length,
        &|done, total| {
            let p = 5 + (done as u64 * 80 / total.max(1) as u64) as u8; // 5→85%
            pct(p);
        },
    )
    .map_err(to_flash_err)?;
    pct(85);

    // CRC check (BK7231N only — T5 already verified per-sector CRC during read)
    // BK7231N bootrom uses crc32_ver2 (no final XOR).
    // For read, we use the raw data length (already aligned from sector reads).
    if !chip.has_per_sector_crc() {
        phase("Verify");
        let expected_crc = ops::crc32_ver2(&data);
        ops::crc_check(transport, start, length, expected_crc).map_err(to_flash_err)?;
    }
    pct(90);

    // Save to file
    phase("Save");
    {
        use std::collections::HashMap;
        let mut p = HashMap::new();
        p.insert("size".to_string(), data.len().to_string());
        p.insert("path".to_string(), file_path.to_string());
        progress(FlashProgress::LogKey {
            key: "flash.log.beken.savingBytes".to_string(),
            params: p,
        });
    }
    std::fs::write(file_path, &data)
        .map_err(|e| FlashError::Plugin(format!("cannot write file '{}': {e}", file_path)))?;
    pct(95);

    // Reboot
    phase("Reboot");
    ops::reboot(transport).map_err(to_flash_err)?;
    pct(100);

    Ok(())
}

/// Convert `ProtocolError` → `FlashError`.
fn to_flash_err(e: super::beken::frame::ProtocolError) -> FlashError {
    match e {
        super::beken::frame::ProtocolError::Cancelled => FlashError::Cancelled,
        other => FlashError::Plugin(other.to_string()),
    }
}
