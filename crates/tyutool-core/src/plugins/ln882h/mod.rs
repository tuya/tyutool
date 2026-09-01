mod protocol;

use serialport::SerialPort;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::error::FlashError;
use crate::flash_event::{FlashEvent, FlashMilestone, FlashPhase};
use crate::job::{FlashJob, FlashMode, FlashSegment};
use crate::plugin::FlashPlugin;
use crate::ram_loader::RamLoaderRef;

use protocol::{
    flush_buffers, read_flash_chunk, read_response, send_command, wait_for_response_containing,
    XmodemSend,
};

/// The vendor RAM code XMODEM-uploaded to 0x20000000 before anything else can happen.
///
/// Not compiled in: [`crate::ram_loader`] downloads and caches it on first use, verifying
/// it against the digest pinned here. The vendor ships no version information in the
/// image at all, so `version` is tyutool's own asset version — provenance lives in the
/// asset's `.txt` notes. See `assets/ram-loader/README.md` on publishing a new one (both
/// halves are needed: the asset, then this constant).
const RAM_LOADER: RamLoaderRef = RamLoaderRef {
    chip: "ln882h",
    version: "1.0.0",
    size: 37_872,
    sha256: "6bd437c6f8366b9cca0fb8de0c80c70788516e3681c6c43d654512feb7a0c723",
};

pub struct Ln882hPlugin;

impl FlashPlugin for Ln882hPlugin {
    fn id(&self) -> &'static str {
        "LN882H"
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
                "LN882H: authorize mode not supported".into(),
            )),
        }
    }
}

fn open_port(port_name: &str, baud: u32) -> Result<Box<dyn SerialPort>, FlashError> {
    // Retry up to 10 times with 1 s delay: CH340 briefly disconnects after device reboot,
    // and the kernel ioctl inside serialport::open() can block for several seconds on the
    // transition.  Retrying lets us ride out the reconnect window gracefully.
    let mut last_err = None;
    for attempt in 0..10 {
        match serialport::new(port_name, baud)
            .timeout(Duration::from_millis(100))
            .open()
        {
            Ok(port) => return Ok(port),
            Err(e) => {
                log::warn!("open_port attempt {attempt}: {e}");
                last_err = Some(FlashError::Serial(e));
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
    Err(last_err.unwrap())
}

/// Send "version\r\n" and return true if response contains "RAMCODE" (device in RAM mode).
fn check_ram_mode(port: &mut Box<dyn SerialPort>) -> Result<bool, FlashError> {
    let _ = flush_buffers(port);
    send_command(port, "version")?;
    let resp = read_response(port, 256, 1)?;
    Ok(resp.windows(7).any(|w| w == b"RAMCODE"))
}

/// Full boot sequence: wait for device ready, optionally load RAM code, switch to flash baud.
/// `switch_baud`: true for flash/erase (921600), false for read (stays at 115200).
/// The user must reset the device with the BOOT/A9 pin held LOW before invoking the tool.
fn boot(
    port: &mut Box<dyn SerialPort>,
    ram_bin: &[u8],
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
    switch_baud: bool,
) -> Result<(), FlashError> {
    // Steps 1+2: connect and load RAM code.  Retried up to 3 times when XMODEM fails —
    // this happens when the device boots into firmware mode instead of ROM download mode
    // (BOOT/A9 pin was not held LOW).  After each failure we ask the user to retry with
    // the pin held, then wait for the device to respond again.
    let mut ram_ready = false;
    for boot_attempt in 0..3u8 {
        if boot_attempt > 0 {
            progress(FlashEvent::Warning {
                message: "Device not in ROM download mode — hold BOOT/A9 pin LOW, then power-cycle the device".into(),
            });
        }

        // Step 1: show_version — wait up to 20 s for device to respond.
        // flush/write/read errors are ignored: CH340 briefly disconnects during device reset
        // (EIO from tcflush), so we keep retrying until the device responds or time runs out.
        progress(FlashEvent::Phase {
            phase: FlashPhase::Connect,
        });
        let mut connected = false;
        for _ in 0..20 {
            if cancel.load(Ordering::Relaxed) {
                return Err(FlashError::Cancelled);
            }
            let _ = flush_buffers(port);
            if send_command(port, "version").is_err() {
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
            let resp = match read_response(port, 256, 1) {
                Ok(r) => r,
                Err(_) => continue,
            };
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

        // Step 2: load RAM code if not already in RAM mode.
        if check_ram_mode(port)? {
            ram_ready = true;
            break;
        }

        progress(FlashEvent::Phase {
            phase: FlashPhase::LoadRam,
        });
        let cmd = format!("download [rambin] [0x20000000] [{}]", ram_bin.len());
        send_command(port, &cmd)?;

        match XmodemSend::new(port, ram_bin, 1024).send("ram.bin", cancel, &|_, _| {}) {
            Ok(()) => {}
            Err(FlashError::Cancelled) => return Err(FlashError::Cancelled),
            Err(_) => continue, // XMODEM failed: retry outer loop with BOOT pin message
        }

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
        ram_ready = true;
        break;
    }

    if !ram_ready {
        return Err(FlashError::Plugin(
            "LN882H: could not enter download mode after 3 attempts — ensure BOOT/A9 pin is held LOW during power-on".into(),
        ));
    }

    // Step 3: set_baudrate — switch to 921600 for flash/erase (not needed for read)
    if !switch_baud {
        return Ok(());
    }
    progress(FlashEvent::Phase {
        phase: FlashPhase::SwitchBaud,
    });
    for _ in 0..3 {
        if cancel.load(Ordering::Relaxed) {
            return Err(FlashError::Cancelled);
        }
        let _ = flush_buffers(port);
        send_command(port, "baudrate 921600")?;
        let _ = read_response(port, 128, 1);
        port.set_baud_rate(921600)?;
        // Reference implementation uses 5 s to let the device stabilize after baud change
        std::thread::sleep(Duration::from_secs(5));
        if check_ram_mode(port)? {
            return Ok(());
        }
    }
    Err(FlashError::Plugin(
        "LN882H: baud rate switch to 921600 failed".into(),
    ))
}

fn run_read(
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    const CHUNK: u32 = 0x200; // 512 bytes; matches protocol.rs CHUNK_SIZE and keeps reads 4 KB-sector-aligned

    let start = parse_hex_addr(job.read_start_hex.as_deref().unwrap_or("0x00000000"))
        .map_err(|_| FlashError::InvalidJob("invalid read_start_hex".into()))?;
    let end = parse_hex_addr(job.read_end_hex.as_deref().unwrap_or("0x00200000"))
        .map_err(|_| FlashError::InvalidJob("invalid read_end_hex".into()))?;

    if end <= start {
        return Err(FlashError::InvalidJob(
            "read_end_hex must be greater than read_start_hex".into(),
        ));
    }
    let length = end - start;

    let file_path = job
        .read_file_path
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| FlashError::InvalidJob("missing read_file_path".into()))?;

    // Before the port: a loader we cannot get hold of should fail the job while the
    // device is still untouched.
    let ram_bin = crate::ram_loader::resolve(&RAM_LOADER, progress)?;

    let mut port = open_port(&job.port, 115200)?;
    boot(&mut port, &ram_bin, cancel, progress, false)?;

    progress(FlashEvent::Phase {
        phase: FlashPhase::Read,
    });
    log::info!("Reading 0x{start:08x}..0x{end:08x} ({length} bytes)");

    // Read in CHUNK-aligned passes; trim to [start, end) at byte level
    let aligned_start = (start / CHUNK) * CHUNK;
    let aligned_end = end.div_ceil(CHUNK) * CHUNK;
    let mut buf: Vec<u8> = Vec::with_capacity(length as usize);
    let mut addr = aligned_start;

    while addr < aligned_end {
        if cancel.load(Ordering::Relaxed) {
            return Err(FlashError::Cancelled);
        }
        // Retry up to 5 times with a short 2 s per-attempt timeout: the RAM code
        // occasionally pauses mid-response; flush serial buffers between retries.
        let chunk = {
            let mut last_err = None;
            let mut data = None;
            for attempt in 0..5u8 {
                if attempt > 0 {
                    let _ = flush_buffers(&mut port);
                    std::thread::sleep(Duration::from_millis(200));
                }
                match read_flash_chunk(&mut port, addr) {
                    Ok(d) => {
                        data = Some(d);
                        break;
                    }
                    Err(FlashError::Cancelled) => return Err(FlashError::Cancelled),
                    Err(e) => {
                        log::warn!("flash_read retry {attempt} at 0x{addr:x}: {e}");
                        last_err = Some(e);
                    }
                }
            }
            match data {
                Some(d) => d,
                None => return Err(last_err.unwrap()),
            }
        };

        // Trim to the requested [start, end) window
        let chunk_start = addr;
        let chunk_end = addr + CHUNK;
        let keep_start = start.max(chunk_start) - chunk_start;
        let keep_end = end.min(chunk_end) - chunk_start;
        buf.extend_from_slice(&chunk[keep_start as usize..keep_end as usize]);

        addr += CHUNK;
        let done = (addr.min(aligned_end) - aligned_start) as u64;
        let total = (aligned_end - aligned_start) as u64;
        progress(FlashEvent::Percent {
            value: (done * 100 / total) as u8,
        });
    }

    // Switch back to 115200 so device is in a predictable state
    send_command(&mut port, "baudrate 115200")?;
    let _ = read_response(&mut port, 64, 1);
    port.set_baud_rate(115200)?;

    progress(FlashEvent::Phase {
        phase: FlashPhase::Save,
    });
    std::fs::write(file_path, &buf)
        .map_err(|e| FlashError::Plugin(format!("cannot write '{file_path}': {e}")))?;

    log::info!("Read complete: {} bytes saved to {}", buf.len(), file_path);
    Ok(())
}

fn run_erase(
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
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

    let ram_bin = crate::ram_loader::resolve(&RAM_LOADER, progress)?;

    // LN882H always boots at 115200 then switches to 921600; job.baud_rate is intentionally ignored.
    let mut port = open_port(&job.port, 115200)?;
    boot(&mut port, &ram_bin, cancel, progress, true)?;

    progress(FlashEvent::Phase {
        phase: FlashPhase::Erase,
    });
    log::info!("Erasing 0x{start:08x}..0x{end:08x} ({length} bytes)");

    send_command(&mut port, &format!("ferase 0x{start:x} 0x{length:x}"))?;
    wait_for_response_containing(&mut port, b"pppp", 120)?;

    progress(FlashEvent::Milestone {
        milestone: FlashMilestone::EraseComplete,
    });

    send_command(&mut port, "reboot")?;
    let _ = read_response(&mut port, 128, 1);

    Ok(())
}

fn run_flash(
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    let segments = resolve_segments(job)?;
    let ram_bin = crate::ram_loader::resolve(&RAM_LOADER, progress)?;

    // LN882H always boots at 115200 then switches to 921600; job.baud_rate is intentionally ignored.
    let mut port = open_port(&job.port, 115200)?;
    boot(&mut port, &ram_bin, cancel, progress, true)?;

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
        let erase_len = (data.len() as u32).div_ceil(SECTOR) * SECTOR;

        progress(FlashEvent::Phase {
            phase: FlashPhase::WriteSegment {
                current: (idx + 1) as u32,
                total: total_segs as u32,
            },
        });
        log::info!(
            "Segment {}/{}: erasing 0x{start:08x}..0x{:08x}",
            idx + 1,
            total_segs,
            start + erase_len
        );

        send_command(&mut port, &format!("ferase 0x{start:x} 0x{erase_len:x}"))?;
        wait_for_response_containing(&mut port, b"pppp", 120)?;

        send_command(&mut port, &format!("startaddr 0x{start:x}"))?;
        wait_for_response_containing(&mut port, b"pppp", 5)?;

        send_command(&mut port, "upgrade")?;
        let _ = read_response(&mut port, 100, 1);
        port.clear(serialport::ClearBuffer::All)?;

        log::info!("Writing {} bytes at 0x{:08x}", data.len(), start);

        progress(FlashEvent::Phase {
            phase: FlashPhase::Write,
        });

        XmodemSend::new(&mut port, &data, 16 * 1024).send("qio.bin", cancel, &|sent, total| {
            let pct = (sent as u64 * 100 / total.max(1) as u64) as u8;
            progress(FlashEvent::Percent { value: pct });
        })?;
        // Emit 100% explicitly — XMODEM callback may not land exactly on 100
        progress(FlashEvent::Percent { value: 100 });

        progress(FlashEvent::Milestone {
            milestone: FlashMilestone::SegmentWritten {
                current: (idx + 1) as u32,
                total: total_segs as u32,
            },
        });
    }

    progress(FlashEvent::Phase {
        phase: FlashPhase::Reboot,
    });
    send_command(&mut port, "reboot")?;
    let _ = read_response(&mut port, 128, 1);

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
    let start = job
        .flash_start_hex
        .clone()
        .unwrap_or_else(|| "0x00000000".into());
    let end = job
        .flash_end_hex
        .clone()
        .unwrap_or_else(|| "0x00200000".into());
    Ok(vec![FlashSegment {
        firmware_path: fw.to_string(),
        start_addr: start,
        end_addr: end,
    }])
}

fn parse_hex_addr(s: &str) -> Result<u32, ()> {
    u32::from_str_radix(s.trim_start_matches("0x").trim_start_matches("0X"), 16).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal `FlashJob` for unit tests - only `mode` varies; inert defaults elsewhere.
    fn job(mode: FlashMode) -> FlashJob {
        FlashJob {
            mode,
            chip_id: "LN882H".to_string(),
            port: String::new(),
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
            authorize_uuid: None,
            authorize_key: None,
            authorize_storage: None,
            confirm_overwrite: None,
        }
    }

    #[test]
    fn plugin_id_is_ln882h() {
        assert_eq!(Ln882hPlugin.id(), "LN882H");
    }

    /// The image is no longer compiled in, so this is the only thing standing between a
    /// wrong `RAM_LOADER` constant and a published asset nothing can verify.
    #[test]
    fn the_pinned_ram_loader_matches_the_published_asset() {
        crate::ram_loader::repo_asset_bytes(&RAM_LOADER);
    }

    #[test]
    fn plugin_rejects_authorize_mode_without_opening_a_port() {
        // LN882H has no auth flow: Authorize must short-circuit before `open_port`.
        let cancel = AtomicBool::new(false);
        let res = Ln882hPlugin.run(&job(FlashMode::Authorize), &cancel, &|_| {});
        assert!(matches!(
            res,
            Err(FlashError::Plugin(ref msg)) if msg.contains("authorize mode not supported")
        ));
    }

    #[test]
    fn parse_hex_addr_strips_prefix_and_parses_hex() {
        assert_eq!(parse_hex_addr("0x1000"), Ok(0x1000));
        assert_eq!(parse_hex_addr("0X1000"), Ok(0x1000)); // uppercase prefix
        assert_eq!(parse_hex_addr("1000"), Ok(0x1000)); // no prefix
        assert_eq!(parse_hex_addr("0xDEADBEEF"), Ok(0xDEAD_BEEF));
        assert_eq!(parse_hex_addr("deadbeef"), Ok(0xDEAD_BEEF)); // lowercase, no prefix
    }

    #[test]
    fn parse_hex_addr_rejects_invalid_input() {
        assert!(parse_hex_addr("").is_err());
        assert!(parse_hex_addr("0x").is_err()); // no digits after prefix
        assert!(parse_hex_addr("xyz").is_err());
        assert!(parse_hex_addr("0xGGGG").is_err());
    }

    #[test]
    fn resolve_segments_passes_through_provided_segments() {
        let mut j = job(FlashMode::Flash);
        j.segments = Some(vec![
            FlashSegment {
                firmware_path: "a.bin".into(),
                start_addr: "0x0000".into(),
                end_addr: "0x1000".into(),
            },
            FlashSegment {
                firmware_path: "b.bin".into(),
                start_addr: "0x1000".into(),
                end_addr: "0x2000".into(),
            },
        ]);
        let segs = resolve_segments(&j).expect("provided segments should pass through");
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].firmware_path, "a.bin");
        assert_eq!(segs[1].end_addr, "0x2000");
    }

    #[test]
    fn resolve_segments_rejects_empty_segment_list() {
        let mut j = job(FlashMode::Flash);
        j.segments = Some(vec![]);
        assert!(matches!(
            resolve_segments(&j),
            Err(FlashError::InvalidJob(_))
        ));
    }

    #[test]
    fn resolve_segments_defaults_single_segment_from_firmware_path() {
        let mut j = job(FlashMode::Flash);
        j.firmware_path = Some("fw.bin".into());
        let segs = resolve_segments(&j).expect("firmware_path should yield one segment");
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].firmware_path, "fw.bin");
        assert_eq!(segs[0].start_addr, "0x00000000"); // default start
        assert_eq!(segs[0].end_addr, "0x00200000"); // default end (2 MiB)
    }

    #[test]
    fn resolve_segments_honours_explicit_flash_range() {
        let mut j = job(FlashMode::Flash);
        j.firmware_path = Some("fw.bin".into());
        j.flash_start_hex = Some("0x1000".into());
        j.flash_end_hex = Some("0x2000".into());
        let segs = resolve_segments(&j).unwrap();
        assert_eq!(segs[0].start_addr, "0x1000");
        assert_eq!(segs[0].end_addr, "0x2000");
    }

    #[test]
    fn resolve_segments_requires_firmware_path_when_no_segments() {
        let j = job(FlashMode::Flash); // segments=None, firmware_path=None
        assert!(matches!(
            resolve_segments(&j),
            Err(FlashError::InvalidJob(_))
        ));
    }
}
