mod protocol;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use serialport::SerialPort;

use crate::error::FlashError;
use crate::job::{FlashJob, FlashMode, FlashSegment};
use crate::plugin::FlashPlugin;
use crate::progress::FlashProgress;

use protocol::{
    flush_buffers, send_command, read_response, wait_for_response_containing, XmodemSend,
};

const RAM_BIN: &[u8] = include_bytes!("ram.bin");

pub struct Ln882hPlugin;

impl FlashPlugin for Ln882hPlugin {
    fn id(&self) -> &'static str {
        "LN882H"
    }

    fn run(
        &self,
        job: &FlashJob,
        cancel: &AtomicBool,
        progress: &dyn Fn(FlashProgress),
    ) -> Result<(), FlashError> {
        match job.mode {
            FlashMode::Flash => run_flash(job, cancel, progress),
            FlashMode::Erase => run_erase(job, cancel, progress),
            FlashMode::Read => Err(FlashError::Plugin(
                "LN882H: read is not yet supported".into(),
            )),
            FlashMode::Authorize => Err(FlashError::Plugin(
                "LN882H: authorize mode not supported".into(),
            )),
        }
    }
}

fn open_port(port_name: &str, baud: u32) -> Result<Box<dyn SerialPort>, FlashError> {
    serialport::new(port_name, baud)
        .timeout(Duration::from_millis(100))
        .open()
        .map_err(FlashError::Serial)
}

/// Send "version\r\n" and return true if response contains "RAMCODE" (device in RAM mode).
fn check_ram_mode(port: &mut Box<dyn SerialPort>) -> Result<bool, FlashError> {
    flush_buffers(port)?;
    send_command(port, "version")?;
    let resp = read_response(port, 256, 1)?;
    Ok(resp.windows(7).any(|w| w == b"RAMCODE"))
}

/// Full boot sequence: wait for device ready, optionally load RAM code, switch to flash baud.
/// The user must reset the device before invoking the tool.
fn boot(
    port: &mut Box<dyn SerialPort>,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashProgress),
) -> Result<(), FlashError> {
    // Step 1: show_version — wait up to 20 s for device to respond
    progress(FlashProgress::Phase { name: "connecting".into() });
    let mut connected = false;
    for _ in 0..20 {
        if cancel.load(Ordering::Relaxed) {
            return Err(FlashError::Cancelled);
        }
        flush_buffers(port)?;
        send_command(port, "version")?;
        let resp = read_response(port, 256, 1)?;
        if resp.windows(7).any(|w| w == b"RAMCODE") || resp.len() > 5 {
            connected = true;
            break;
        }
    }
    if !connected {
        return Err(FlashError::Plugin(
            "LN882H: device did not respond — reset the device and retry".into(),
        ));
    }

    // Step 2: check_boot_version — load RAM code if not already in RAM mode
    if !check_ram_mode(port)? {
        progress(FlashProgress::Phase { name: "loading_ram".into() });
        let cmd = format!("download [rambin] [0x20000000] [{}]", RAM_BIN.len());
        send_command(port, &cmd)?;
        XmodemSend::new(port, RAM_BIN, 1024).send("ram.bin", cancel, &|_, _| {})?;
        // Drain post-transfer noise, then wait for RAM code to boot up (reference uses 5 s)
        let _ = read_response(port, 300, 1);
        std::thread::sleep(Duration::from_secs(5));
        // Retry: RAM code takes a few seconds to start responding
        let mut in_ram = false;
        for _ in 0..10 {
            if check_ram_mode(port)? {
                in_ram = true;
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        if !in_ram {
            return Err(FlashError::Plugin(
                "LN882H: RAM code upload failed — device did not enter RAM mode".into(),
            ));
        }
    }

    // Step 3: set_baudrate — switch to 921600 for faster flash operations
    progress(FlashProgress::Phase { name: "switching_baud".into() });
    for _ in 0..3 {
        if cancel.load(Ordering::Relaxed) {
            return Err(FlashError::Cancelled);
        }
        flush_buffers(port)?;
        send_command(port, "baudrate 921600")?;
        let _ = read_response(port, 128, 1);
        port.set_baud_rate(921600)?;
        // Reference implementation uses 5 s to let the device stabilize after baud change
        std::thread::sleep(Duration::from_secs(5));
        if check_ram_mode(port)? {
            return Ok(());
        }
    }
    Err(FlashError::Plugin("LN882H: baud rate switch to 921600 failed".into()))
}

fn run_erase(
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashProgress),
) -> Result<(), FlashError> {
    let start_hex = job.erase_start_hex.as_deref().unwrap_or("0x00000000");
    let end_hex = job.erase_end_hex.as_deref().unwrap_or("0x00200000");

    let start = parse_hex_addr(start_hex)
        .map_err(|_| FlashError::InvalidJob(format!("invalid erase_start_hex: {start_hex}")))?;
    let end = parse_hex_addr(end_hex)
        .map_err(|_| FlashError::InvalidJob(format!("invalid erase_end_hex: {end_hex}")))?;

    if end <= start {
        return Err(FlashError::InvalidJob(
            "erase_end_hex must be greater than erase_start_hex".into(),
        ));
    }
    let length = end - start;

    // LN882H requires 4 KiB-aligned erase regions
    const SECTOR: u32 = 0x1000;
    if start % SECTOR != 0 || length % SECTOR != 0 {
        return Err(FlashError::InvalidJob(
            "LN882H: erase start and length must be 4 KiB aligned".into(),
        ));
    }

    // LN882H always boots at 115200 then switches to 921600; job.baud_rate is intentionally ignored.
    let mut port = open_port(&job.port, 115200)?;
    boot(&mut port, cancel, progress)?;

    progress(FlashProgress::Phase { name: "erasing".into() });
    progress(FlashProgress::LogLine {
        line: format!("Erasing 0x{start:08x}..0x{end:08x} ({length} bytes)"),
    });

    send_command(&mut port, &format!("ferase 0x{start:x} 0x{length:x}"))?;
    wait_for_response_containing(&mut port, b"pppp", 120)?;

    progress(FlashProgress::LogLine { line: "Erase complete.".into() });

    send_command(&mut port, "reboot")?;
    let _ = read_response(&mut port, 128, 1);

    Ok(())
}

fn run_flash(
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashProgress),
) -> Result<(), FlashError> {
    let segments = resolve_segments(job)?;

    // LN882H always boots at 115200 then switches to 921600; job.baud_rate is intentionally ignored.
    let mut port = open_port(&job.port, 115200)?;
    boot(&mut port, cancel, progress)?;

    let total_segs = segments.len();
    for (idx, seg) in segments.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(FlashError::Cancelled);
        }

        let start = parse_hex_addr(&seg.start_addr).map_err(|_| {
            FlashError::InvalidJob(format!("invalid start_addr: {}", seg.start_addr))
        })?;

        let data = std::fs::read(&seg.firmware_path).map_err(|e| {
            FlashError::Plugin(format!("cannot read firmware '{}': {e}", seg.firmware_path))
        })?;

        if data.is_empty() {
            return Err(FlashError::InvalidJob(format!(
                "firmware file '{}' is empty",
                seg.firmware_path
            )));
        }

        // Align erase length to 4 KiB sector
        const SECTOR: u32 = 0x1000;
        let erase_len = ((data.len() as u32 + SECTOR - 1) / SECTOR) * SECTOR;

        progress(FlashProgress::Phase {
            name: format!("segment_{}_of_{}", idx + 1, total_segs),
        });
        progress(FlashProgress::LogLine {
            line: format!(
                "Segment {}/{}: erasing 0x{start:08x}..0x{:08x}",
                idx + 1,
                total_segs,
                start + erase_len
            ),
        });

        send_command(&mut port, &format!("ferase 0x{start:x} 0x{erase_len:x}"))?;
        wait_for_response_containing(&mut port, b"pppp", 120)?;

        send_command(&mut port, &format!("startaddr 0x{start:x}"))?;
        wait_for_response_containing(&mut port, b"pppp", 5)?;

        send_command(&mut port, "upgrade")?;
        let _ = read_response(&mut port, 100, 1);
        port.clear(serialport::ClearBuffer::All)?;

        progress(FlashProgress::LogLine {
            line: format!("Writing {} bytes...", data.len()),
        });

        let seg_start_pct = (idx as u64 * 90 / total_segs as u64) as u8;
        let seg_end_pct = ((idx + 1) as u64 * 90 / total_segs as u64) as u8;
        let total_bytes = data.len();

        XmodemSend::new(&mut port, &data, 16 * 1024).send(
            "qio.bin",
            cancel,
            &|sent, total| {
                let range = (seg_end_pct - seg_start_pct) as u64;
                let pct = seg_start_pct + (sent as u64 * range / total as u64) as u8;
                progress(FlashProgress::Percent { value: pct });
            },
        )?;

        progress(FlashProgress::LogLine {
            line: format!("Segment {}/{} written ({total_bytes} bytes).", idx + 1, total_segs),
        });
    }

    progress(FlashProgress::Phase { name: "rebooting".into() });
    send_command(&mut port, "reboot")?;
    let _ = read_response(&mut port, 128, 1);

    progress(FlashProgress::Percent { value: 100 });
    Ok(())
}

fn resolve_segments(job: &FlashJob) -> Result<Vec<FlashSegment>, FlashError> {
    if let Some(ref s) = job.segments {
        if s.is_empty() {
            return Err(FlashError::InvalidJob("no flash segments provided".into()));
        }
        return Ok(s.clone());
    }
    let fw = job
        .firmware_path
        .as_deref()
        .ok_or_else(|| FlashError::InvalidJob("missing firmware_path".into()))?;
    let start = job.flash_start_hex.clone().unwrap_or_else(|| "0x00000000".into());
    let end = job.flash_end_hex.clone().unwrap_or_else(|| "0x00200000".into());
    Ok(vec![FlashSegment {
        firmware_path: fw.to_string(),
        start_addr: start,
        end_addr: end,
    }])
}

fn parse_hex_addr(s: &str) -> Result<u32, ()> {
    u32::from_str_radix(s.trim_start_matches("0x").trim_start_matches("0X"), 16).map_err(|_| ())
}
