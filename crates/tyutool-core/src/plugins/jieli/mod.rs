//! JieLi (杰理) USB Mass Storage (SCSI BOT) flash plugin.
//!
//! Flashes JieLi WL82, WL83, AC791N, and AC792N devices over USB Mass Storage
//! Bulk-Only Transport (BOT, 0xFC SCSI CDB) without requiring a serial COM port.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::FlashError;
use crate::flash_event::{FlashEvent, FlashMilestone, FlashPhase};
use crate::job::{FlashJob, FlashMode};
use crate::plugin::FlashPlugin;

pub const JIELI_CHIP_ID: &str = "JIELI";

// JieLi SCSI 0xFC Subcommands
pub const SC_BLOCK_ERASE_64K: u8 = 0x00;
pub const SC_SECTOR_ERASE_4K: u8 = 0x01;
pub const SC_CHIP_ERASE: u8 = 0x02;
pub const SC_WRITE_FLASH: u8 = 0x04;
pub const SC_READ_FLASH: u8 = 0x05;
pub const SC_RESET: u8 = 0x0C;
pub const SC_HARDWARE_CRC: u8 = 0x13;
pub const SC_PROBE_CHIP: u8 = 0x83;
pub const SC_PROBE_FLASH_ID: u8 = 0x84;

/// Calculate CRC16-CCITT (poly 0x1021, init 0x0000)
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0x0000;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Builds a 16-byte SCSI CDB frame for JieLi 0xFC protocol.
pub fn build_jieli_cdb(subcmd: u8, addr: u32, len: u32, extra: u16) -> [u8; 16] {
    let mut cdb = [0u8; 16];
    cdb[0] = 0xFC;
    cdb[1] = subcmd;
    cdb[2] = ((addr >> 24) & 0xFF) as u8;
    cdb[3] = ((addr >> 16) & 0xFF) as u8;
    cdb[4] = ((addr >> 8) & 0xFF) as u8;
    cdb[5] = (addr & 0xFF) as u8;
    cdb[6] = ((len >> 8) & 0xFF) as u8;
    cdb[7] = (len & 0xFF) as u8;
    cdb[8] = ((extra >> 8) & 0xFF) as u8;
    cdb[9] = (extra & 0xFF) as u8;
    cdb
}

pub struct JieliPlugin;

impl FlashPlugin for JieliPlugin {
    fn id(&self) -> &'static str {
        JIELI_CHIP_ID
    }

    fn run(
        &self,
        job: &FlashJob,
        cancel: &AtomicBool,
        progress: &dyn Fn(FlashEvent),
    ) -> Result<(), FlashError> {
        match job.mode {
            FlashMode::Flash => run_flash(job, cancel, progress),
            FlashMode::Erase => run_erase(job, cancel, progress),
            FlashMode::Read => run_read(job, cancel, progress),
            FlashMode::Authorize => Err(FlashError::Plugin(
                "JIELI: authorize mode not supported".into(),
            )),
        }
    }
}

fn parse_hex_u32(s: Option<&str>, default_val: u32) -> Result<u32, FlashError> {
    match s {
        Some(raw) => {
            let trimmed = raw.trim();
            let hex_str = if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
                &trimmed[2..]
            } else {
                trimmed
            };
            u32::from_str_radix(hex_str, 16)
                .map_err(|e| FlashError::Plugin(format!("invalid hex address '{}': {}", raw, e)))
        }
        None => Ok(default_val),
    }
}

fn locate_python_flasher() -> Option<PathBuf> {
    // 1. Check environment variable
    if let Ok(env_path) = std::env::var("JIELI_FLASHER_PY") {
        let p = PathBuf::from(env_path);
        if p.is_file() {
            return Some(p);
        }
    }

    // 2. Check TuyaOpen platform paths relative to current directory
    let search_roots = [
        ".",
        "platforms/JIELI",
        "platforms/jieli",
        "../platforms/JIELI",
        "../platforms/jieli",
        "TuyaOpen-JieLi",
        "../TuyaOpen-JieLi",
        "/home/share/samba/ePanda/TuyaOpen-JieLi",
    ];

    for root in &search_roots {
        let candidate = Path::new(root).join("tools").join("jieli_flasher.py");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn run_via_python_flasher(
    args: &[String],
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    let flasher_script = locate_python_flasher().ok_or_else(|| {
        FlashError::Plugin(
            "JieLi flasher engine not found. Ensure TuyaOpen-JieLi tools are present or set JIELI_FLASHER_PY."
                .into(),
        )
    })?;

    log::info!("Invoking JieLi Python flasher at {:?}", flasher_script);

    let python_bin = if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_ok()
    {
        "python3"
    } else {
        "python"
    };

    let mut cmd = std::process::Command::new(python_bin);
    cmd.arg(flasher_script);
    cmd.args(args);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| FlashError::Plugin(format!("failed to spawn JieLi python flasher: {}", e)))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| FlashError::Plugin("failed to capture JieLi flasher stdout".into()))?;

    use std::io::{BufRead, BufReader};
    let reader = BufReader::new(stdout);

    for line_res in reader.lines() {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            return Err(FlashError::Cancelled);
        }

        let line = match line_res {
            Ok(l) => l,
            Err(_) => break,
        };

        log::debug!("[jieli-engine] {}", line);

        if line.starts_with("[phase]") {
            let phase_str = line.trim_start_matches("[phase]").trim();
            match phase_str {
                "Connect" => progress(FlashEvent::Phase {
                    phase: FlashPhase::Connect,
                }),
                "Read Flash ID" => progress(FlashEvent::Phase {
                    phase: FlashPhase::ReadFlashId,
                }),
                "Erase Flash" => progress(FlashEvent::Phase {
                    phase: FlashPhase::Erase,
                }),
                "Write Flash" => progress(FlashEvent::Phase {
                    phase: FlashPhase::Write,
                }),
                "Verify Flash" => progress(FlashEvent::Phase {
                    phase: FlashPhase::Verify,
                }),
                "Reset Device" => progress(FlashEvent::Phase {
                    phase: FlashPhase::Reboot,
                }),
                other => progress(FlashEvent::Phase {
                    phase: FlashPhase::Other(other.to_string()),
                }),
            }
        } else if line.starts_with("[progress]") {
            let pct_str = line
                .trim_start_matches("[progress]")
                .trim()
                .trim_end_matches('%');
            if let Ok(pct) = pct_str.parse::<u8>() {
                progress(FlashEvent::Percent { value: pct });
            }
        }
    }

    let status = child.wait().map_err(|e| {
        FlashError::Plugin(format!("failed to wait for JieLi python flasher: {}", e))
    })?;

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        return Err(FlashError::Plugin(format!(
            "JieLi flasher exited with failure code {}",
            code
        )));
    }

    progress(FlashEvent::Milestone {
        milestone: FlashMilestone::WriteComplete,
    });

    Ok(())
}

fn run_flash(
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    let firmware_path = job
        .firmware_path
        .as_deref()
        .ok_or_else(|| FlashError::Plugin("JieLi flash requires firmware_path (-f)".into()))?;

    let start_addr = parse_hex_u32(job.flash_start_hex.as_deref(), 0x00000000)?;
    let mut args = vec![
        "-m".to_string(),
        "usb".to_string(),
        "-wflash".to_string(),
        format!("0x{:08X}", start_addr),
        firmware_path.to_string(),
        "-reboot".to_string(),
        "500".to_string(),
    ];

    if !job.port.is_empty() && job.port != "auto" {
        args.push("-p".to_string());
        args.push(job.port.clone());
    }

    run_via_python_flasher(&args, cancel, progress)
}

fn run_erase(
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    let start_addr = parse_hex_u32(job.erase_start_hex.as_deref(), 0x00000000)?;
    let end_addr = parse_hex_u32(job.erase_end_hex.as_deref(), start_addr + 0x00200000)?;
    let len = end_addr.saturating_sub(start_addr);

    let mut args = vec![
        "-m".to_string(),
        "usb".to_string(),
        "--erase-flash".to_string(),
        format!("0x{:08X}", start_addr),
        format!("0x{:08X}", len),
    ];

    if !job.port.is_empty() && job.port != "auto" {
        args.push("-p".to_string());
        args.push(job.port.clone());
    }

    run_via_python_flasher(&args, cancel, progress)
}

fn run_read(
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    let read_path = job
        .read_file_path
        .as_deref()
        .ok_or_else(|| FlashError::Plugin("JieLi read requires output file (-f)".into()))?;

    let start_addr = parse_hex_u32(job.read_start_hex.as_deref(), 0x00000000)?;
    let end_addr = parse_hex_u32(job.read_end_hex.as_deref(), start_addr + 0x00200000)?;
    let len = end_addr.saturating_sub(start_addr);

    let mut args = vec![
        "-m".to_string(),
        "usb".to_string(),
        "--read-flash".to_string(),
        format!("0x{:08X}", start_addr),
        format!("0x{:08X}", len),
        read_path.to_string(),
    ];

    if !job.port.is_empty() && job.port != "auto" {
        args.push("-p".to_string());
        args.push(job.port.clone());
    }

    run_via_python_flasher(&args, cancel, progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_id_is_uppercase() {
        assert_eq!(JieliPlugin.id(), "JIELI");
    }

    #[test]
    fn crc16_ccitt_standard_vectors() {
        assert_eq!(crc16_ccitt(b"123456789"), 0x31C3);
        assert_eq!(crc16_ccitt(&[]), 0x0000);
    }

    #[test]
    fn cdb_frame_builder() {
        let cdb = build_jieli_cdb(SC_WRITE_FLASH, 0x12345678, 0x1000, 0xABCD);
        assert_eq!(cdb[0], 0xFC);
        assert_eq!(cdb[1], SC_WRITE_FLASH);
        assert_eq!(cdb[2], 0x12);
        assert_eq!(cdb[3], 0x34);
        assert_eq!(cdb[4], 0x56);
        assert_eq!(cdb[5], 0x78);
        assert_eq!(cdb[6], 0x10);
        assert_eq!(cdb[7], 0x00);
        assert_eq!(cdb[8], 0xAB);
        assert_eq!(cdb[9], 0xCD);
    }
}
