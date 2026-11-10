//! RTL8711DAF (Realtek AmebaDplus) flash plugin: flash / erase / read over the log UART.
//!
//! Flow (verified on hardware, protocol from Realtek's AmebaImageTool):
//! 1. enter ROM UART download mode ([`rom::enter_download_mode`]: `reboot uartburn`,
//!    then RTS→CHIP_EN reset, then a manual prompt);
//! 2. ROM handshake + switch to the transfer baud, upload the flash loader into RAM;
//! 3. talk to the flash loader ([`floader`]) to erase / write / read / checksum NOR flash;
//! 4. `NEXTOP Reset` so the chip boots the firmware.
//!
//! Addresses in jobs are flash offsets (0x0 = start of flash); the loader works with
//! CPU addresses (`0x0800_0000 + offset`).

mod floader;
mod io;
mod rom;

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::error::FlashError;
use crate::flash_event::{FlashEvent, FlashMilestone, FlashPhase};
use crate::job::{FlashJob, FlashMode, FlashSegment};
use crate::plugin::FlashPlugin;
use crate::ram_loader::RamLoaderRef;

use floader::{DeviceInfo, Floader};
use io::{Link, SerialRtlIo};

/// Realtek `floader_amebadplus.bin` (AmebaImageTool `Devices/Floaders`), published under
/// `assets/ram-loader/rtl8711daf/` and uploaded to RAM at run time.
const FLOADER: RamLoaderRef = RamLoaderRef {
    chip: "rtl8711daf",
    version: "1.0.0",
    size: 29_024,
    sha256: "3e40733a85c64ba77b474c2eb2242e565f5bba5dd5b0eff6bc1cb07cfe4ac807",
};

const DEVICE_ID: u16 = 0x6845;
const FLASH_BASE: u32 = 0x0800_0000;
const SECTOR: u32 = 0x1000;
/// SENSE after this many WRITE pages (vendor `SensePacketCount`).
const SENSE_PACKET_COUNT: u32 = 32;
const DEFAULT_XFER_BAUD: u32 = 1_500_000;
/// NOR status register (cmd 0x05) block-protect bits; vendor `CheckFlashLock`.
const PROTECT_MASK: u8 = 0x3C;

pub struct Rtl8711dafPlugin;

impl FlashPlugin for Rtl8711dafPlugin {
    fn id(&self) -> &'static str {
        "RTL8711DAF"
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
                "RTL8711DAF: authorize mode not supported".into(),
            )),
        }
    }
}

// ───────────────────────────────────────────────────────── helpers

fn phase(progress: &dyn Fn(FlashEvent), phase: FlashPhase) {
    progress(FlashEvent::Phase { phase });
}

fn milestone(progress: &dyn Fn(FlashEvent), milestone: FlashMilestone) {
    progress(FlashEvent::Milestone { milestone });
}

fn warning(progress: &dyn Fn(FlashEvent), message: String) {
    progress(FlashEvent::Warning { message });
}

/// Parse a hex address; CPU addresses (`0x08xx_xxxx`) are accepted and turned into offsets.
fn parse_offset(s: Option<&str>, what: &str) -> Result<u32, FlashError> {
    let s = s
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| FlashError::InvalidJob(format!("missing {what}")))?;
    let digits = s.trim_start_matches("0x").trim_start_matches("0X");
    let v = u32::from_str_radix(digits, 16)
        .map_err(|_| FlashError::InvalidJob(format!("invalid {what}: {s}")))?;
    Ok(if v >= FLASH_BASE { v - FLASH_BASE } else { v })
}

/// Transfer baud: the job's rate if the ROM table has it, else 1.5 Mbaud.
fn xfer_baud(job: &FlashJob, progress: &dyn Fn(FlashEvent)) -> u32 {
    if job.baud_rate >= rom::HANDSHAKE_BAUD && rom::rom_baud_index(job.baud_rate).is_some() {
        job.baud_rate
    } else {
        warning(
            progress,
            format!(
                "RTL8711DAF: baud rate {} is not supported by the boot ROM, using {DEFAULT_XFER_BAUD}",
                job.baud_rate
            ),
        );
        DEFAULT_XFER_BAUD
    }
}

fn percent(progress: &dyn Fn(FlashEvent), done: u64, total: u64, last: &mut u8) {
    let v = (done * 100)
        .checked_div(total)
        .map_or(100, |p| p.min(100) as u8);
    if v != *last {
        *last = v;
        progress(FlashEvent::Percent { value: v });
    }
}

/// Erased-flash checksum for `len` bytes (all 0xFF words).
fn erased_word_sum(len: u32) -> u32 {
    (len / 4).wrapping_mul(0xFFFF_FFFF)
}

/// JEDEC id as the other plugins report it: manufacturer, memory type, capacity.
fn jedec_id(info: &DeviceInfo) -> u32 {
    let [ty, cap] = info.flash_did.to_le_bytes();
    (info.flash_mid as u32) << 16 | (ty as u32) << 8 | cap as u32
}

// ───────────────────────────────────────────────────────── session

/// Open the port, get into the flash loader and identify the chip, then run `body`.
/// The chip is reset afterwards (best effort on failure) so it boots its firmware.
fn with_floader<T>(
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
    body: impl FnOnce(&mut Floader, &DeviceInfo) -> Result<T, FlashError>,
) -> Result<T, FlashError> {
    let loader = crate::ram_loader::resolve(&FLOADER, progress)?;
    let baud = xfer_baud(job, progress);

    phase(progress, FlashPhase::Connect);
    log::info!("RTL8711DAF: opening {}", job.port);
    let io = SerialRtlIo::open(&job.port, rom::HANDSHAKE_BAUD)?;
    let mut link = Link::new(Box::new(io), cancel, rom::HANDSHAKE_BAUD);

    phase(progress, FlashPhase::Handshake);
    rom::enter_download_mode(&mut link, baud, progress)?;
    phase(progress, FlashPhase::LoadRam);
    log::info!("RTL8711DAF: uploading flash loader @ {baud} baud");
    rom::download_floader(&mut link, &loader)?;

    let mut fl = Floader::new(&mut link);
    let mut sensed = Err(FlashError::Plugin(
        "RTL8711DAF: the flash loader did not answer after upload".into(),
    ));
    for _ in 0..3 {
        sensed = fl.sense();
        if sensed.is_ok() || matches!(sensed, Err(FlashError::Cancelled)) {
            break;
        }
    }
    sensed?;
    milestone(progress, FlashMilestone::HandshakeComplete);

    phase(progress, FlashPhase::ReadFlashId);
    let info = fl.query()?;
    log::info!(
        "RTL8711DAF: DID 0x{:04x}, NOR flash MID 0x{:02x} DID 0x{:04x}, {} KiB, page {} B",
        info.did,
        info.flash_mid,
        info.flash_did,
        info.capacity / 1024,
        info.page_size
    );
    if info.did != DEVICE_ID {
        return Err(FlashError::Plugin(format!(
            "RTL8711DAF: unexpected device id 0x{:04x} (expected 0x{DEVICE_ID:04x})",
            info.did
        )));
    }
    if info.memory_type != floader::MEM_NOR || info.page_size == 0 || info.capacity == 0 {
        return Err(FlashError::Plugin(format!(
            "RTL8711DAF: unsupported flash (type {}, page {}, capacity {})",
            info.memory_type, info.page_size, info.capacity
        )));
    }
    milestone(
        progress,
        FlashMilestone::FlashIdRead {
            mid: Some(jedec_id(&info)),
        },
    );
    fl.config_default()?;

    let result = body(&mut fl, &info);
    match &result {
        Ok(_) => {
            phase(progress, FlashPhase::Reboot);
            fl.reset()?;
            milestone(progress, FlashMilestone::Rebooted);
        }
        Err(FlashError::Cancelled) => {}
        Err(_) => {
            let _ = fl.reset();
        }
    }
    result
}

fn ensure_unprotected(fl: &mut Floader) -> Result<(), FlashError> {
    let status = fl.read_status_register(0x05, 0x00)?;
    if status & PROTECT_MASK != 0 {
        return Err(FlashError::Plugin(format!(
            "RTL8711DAF: flash is write-protected (status register 0x{status:02x}); not modifying flash"
        )));
    }
    Ok(())
}

/// Device checksum over `[start, start + len)` must equal `want`.
fn verify_checksum(
    fl: &mut Floader,
    start: u32,
    len: u32,
    want: u32,
    what: &str,
) -> Result<(), FlashError> {
    let got = fl.checksum(FLASH_BASE + start, FLASH_BASE + start + len, len)?;
    if got != want {
        return Err(FlashError::Plugin(format!(
            "RTL8711DAF: {what} verify failed at 0x{start:08x}: checksum 0x{got:08x}, expected 0x{want:08x}"
        )));
    }
    Ok(())
}

// ───────────────────────────────────────────────────────── flash

struct Image {
    /// Flash offset.
    start: u32,
    /// Page-aligned, 0xFF padded.
    data: Vec<u8>,
}

fn job_segments(job: &FlashJob) -> Result<Vec<FlashSegment>, FlashError> {
    if let Some(segs) = job.segments.as_ref().filter(|s| !s.is_empty()) {
        return Ok(segs.clone());
    }
    let firmware_path = job
        .firmware_path
        .clone()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| FlashError::InvalidJob("missing firmware_path".into()))?;
    Ok(vec![FlashSegment {
        firmware_path,
        start_addr: job.flash_start_hex.clone().unwrap_or_else(|| "0x0".into()),
        end_addr: job.flash_end_hex.clone().unwrap_or_default(),
    }])
}

/// Load one segment: data is cut to `[start, end)` (when `end` is given) and padded to a page.
fn load_image(
    seg: &FlashSegment,
    page: u32,
    capacity: u32,
    progress: &dyn Fn(FlashEvent),
) -> Result<Image, FlashError> {
    let start = parse_offset(Some(&seg.start_addr), "flash start address")?;
    if !start.is_multiple_of(SECTOR) {
        return Err(FlashError::InvalidJob(format!(
            "RTL8711DAF: start address 0x{start:08x} must be 4 KiB aligned"
        )));
    }
    let mut data = std::fs::read(&seg.firmware_path)?;
    let limit = if seg.end_addr.trim().is_empty() {
        capacity
    } else {
        parse_offset(Some(&seg.end_addr), "flash end address")?.min(capacity)
    };
    if limit <= start {
        return Err(FlashError::InvalidJob(format!(
            "RTL8711DAF: empty range 0x{start:08x}..0x{limit:08x} for {}",
            seg.firmware_path
        )));
    }
    let room = (limit - start) as usize;
    if data.len() > room {
        warning(
            progress,
            format!(
                "RTL8711DAF: {} is {} bytes; writing only 0x{start:08x}..0x{limit:08x} ({room} bytes)",
                seg.firmware_path,
                data.len()
            ),
        );
        data.truncate(room);
    }
    if data.is_empty() {
        return Err(FlashError::InvalidJob(format!(
            "RTL8711DAF: {} is empty",
            seg.firmware_path
        )));
    }
    let padded = data.len().div_ceil(page as usize) * page as usize;
    data.resize(padded, 0xFF);
    if start as u64 + data.len() as u64 > capacity as u64 {
        return Err(FlashError::InvalidJob(format!(
            "RTL8711DAF: {} does not fit in flash at 0x{start:08x}",
            seg.firmware_path
        )));
    }
    Ok(Image { start, data })
}

/// Erase + program one image the way the vendor tool does: erase a block, write its pages,
/// SENSE every 32 pages / at the end of a block / at the end of the image.
fn program_image(
    fl: &mut Floader,
    img: &Image,
    page: u32,
    on_bytes: &mut dyn FnMut(u64),
) -> Result<(), FlashError> {
    let total = img.data.len() as u32;
    let mut off = 0u32;
    while off < total {
        let addr = FLASH_BASE + img.start + off;
        let block = floader::erase_block_size(img.start + off, (total - off) as u64);
        log::debug!("RTL8711DAF: erase 0x{addr:08x} +0x{block:x}");
        fl.erase(addr, block)?;
        let pages_in_block = (block / page).max(1);
        let sense_timeout =
            Duration::from_millis(5 * pages_in_block.max(SENSE_PACKET_COUNT) as u64)
                + floader::erase_timeout(block);
        let block_end = (off + block).min(total);
        let mut i = 0u32;
        while off < block_end {
            let p = &img.data[off as usize..(off + page) as usize];
            fl.write(FLASH_BASE + img.start + off, p)?;
            off += page;
            i += 1;
            if i.is_multiple_of(SENSE_PACKET_COUNT) || off >= block_end || off >= total {
                fl.sense_expect(floader::OP_WRITE, sense_timeout)?;
            }
            on_bytes(page as u64);
        }
    }
    Ok(())
}

fn run_flash(
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    let segments = job_segments(job)?;
    with_floader(job, cancel, progress, |fl, info| {
        let images = segments
            .iter()
            .map(|s| load_image(s, info.page_size, info.capacity, progress))
            .collect::<Result<Vec<_>, _>>()?;
        ensure_unprotected(fl)?;

        let total: u64 = images.iter().map(|i| i.data.len() as u64).sum();
        let count = images.len() as u32;
        let mut done = 0u64;
        let mut last = u8::MAX;
        percent(progress, 0, total, &mut last);
        for (idx, img) in images.iter().enumerate() {
            phase(
                progress,
                FlashPhase::WriteSegment {
                    current: idx as u32 + 1,
                    total: count,
                },
            );
            log::info!(
                "RTL8711DAF: writing {} bytes at 0x{:08x}",
                img.data.len(),
                img.start
            );
            program_image(fl, img, info.page_size, &mut |n| {
                done += n;
                percent(progress, done, total, &mut last);
            })?;
            phase(progress, FlashPhase::Verify);
            verify_checksum(
                fl,
                img.start,
                img.data.len() as u32,
                floader::word_sum(&img.data),
                "write",
            )?;
        }
        milestone(progress, FlashMilestone::WriteComplete);
        Ok(())
    })
}

// ───────────────────────────────────────────────────────── erase

/// Half-open erase range aligned to 4 KiB (floor start, ceil end), like the ESP / Beken plugins.
fn erase_range(job: &FlashJob) -> Result<(u32, u32), FlashError> {
    let start = parse_offset(job.erase_start_hex.as_deref(), "erase_start_hex")?;
    let end = parse_offset(job.erase_end_hex.as_deref(), "erase_end_hex")?;
    let start = start & !(SECTOR - 1);
    let end = end.saturating_add(SECTOR - 1) & !(SECTOR - 1);
    if end <= start {
        return Err(FlashError::InvalidJob(
            "aligned erase range is empty; check erase_start_hex / erase_end_hex".into(),
        ));
    }
    Ok((start, end))
}

fn run_erase(
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    let (start, end) = erase_range(job)?;
    with_floader(job, cancel, progress, |fl, info| {
        let end = end.min(info.capacity);
        if end <= start {
            return Err(FlashError::InvalidJob(format!(
                "erase start 0x{start:08x} is beyond flash size 0x{:08x}",
                info.capacity
            )));
        }
        ensure_unprotected(fl)?;
        phase(progress, FlashPhase::Erase);
        log::info!("RTL8711DAF: erasing 0x{start:08x}..0x{end:08x}");
        let total = (end - start) as u64;
        let mut last = u8::MAX;
        let mut off = start;
        while off < end {
            let block = floader::erase_block_size(off, (end - off) as u64);
            fl.erase(FLASH_BASE + off, block)?;
            fl.sense_expect(floader::OP_FS_ERASE, floader::erase_timeout(block))?;
            off += block;
            percent(progress, (off - start) as u64, total, &mut last);
        }
        phase(progress, FlashPhase::Verify);
        verify_checksum(
            fl,
            start,
            end - start,
            erased_word_sum(end - start),
            "erase",
        )?;
        milestone(progress, FlashMilestone::EraseComplete);
        Ok(())
    })
}

// ───────────────────────────────────────────────────────── read

fn run_read(
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    let start = parse_offset(job.read_start_hex.as_deref(), "read_start_hex")?;
    let end = parse_offset(job.read_end_hex.as_deref(), "read_end_hex")?;
    if end <= start {
        return Err(FlashError::InvalidJob(
            "read_end_hex must be greater than read_start_hex".into(),
        ));
    }
    let file_path = job
        .read_file_path
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| FlashError::InvalidJob("missing read_file_path".into()))?
        .to_string();

    with_floader(job, cancel, progress, |fl, info| {
        let end = end.min(info.capacity);
        if end <= start {
            return Err(FlashError::InvalidJob(format!(
                "read start 0x{start:08x} is beyond flash size 0x{:08x}",
                info.capacity
            )));
        }
        phase(progress, FlashPhase::Read);
        log::info!("RTL8711DAF: reading 0x{start:08x}..0x{end:08x}");
        let total = (end - start) as u64;
        let mut buf = Vec::with_capacity(total as usize);
        let mut last = u8::MAX;
        let mut off = start;
        while off < end {
            let n = info.page_size.min(end - off);
            buf.extend_from_slice(&fl.read(FLASH_BASE + off, n)?);
            off += n;
            percent(progress, (off - start) as u64, total, &mut last);
        }

        phase(progress, FlashPhase::Verify);
        if buf.len().is_multiple_of(4) {
            verify_checksum(fl, start, buf.len() as u32, floader::word_sum(&buf), "read")?;
        } else {
            log::info!("RTL8711DAF: read length not a multiple of 4, checksum verify skipped");
        }

        phase(progress, FlashPhase::Save);
        std::fs::write(&file_path, &buf)?;
        log::info!("RTL8711DAF: saved {} bytes to {file_path}", buf.len());
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job() -> FlashJob {
        FlashJob {
            mode: FlashMode::Flash,
            chip_id: "RTL8711DAF".into(),
            port: "/dev/null".into(),
            baud_rate: 1_500_000,
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
    fn plugin_id_is_rtl8711daf() {
        assert_eq!(Rtl8711dafPlugin.id(), "RTL8711DAF");
    }

    /// The image is not compiled in, so this is the only thing standing between a wrong
    /// `FLOADER` constant and a published asset nothing can verify.
    #[test]
    fn the_pinned_floader_matches_the_published_asset() {
        let bytes = crate::ram_loader::repo_asset_bytes(&FLOADER);
        assert_eq!(
            &bytes[..8],
            &[0x99, 0x99, 0x96, 0x96, 0x3F, 0xCC, 0x66, 0xFC]
        );
    }

    #[test]
    fn plugin_rejects_authorize_mode_without_opening_a_port() {
        let cancel = AtomicBool::new(false);
        let mut j = job();
        j.mode = FlashMode::Authorize;
        let res = Rtl8711dafPlugin.run(&j, &cancel, &|_| {});
        assert!(matches!(
            res,
            Err(FlashError::Plugin(ref msg)) if msg.contains("authorize mode not supported")
        ));
    }

    #[test]
    fn parse_offset_accepts_offsets_and_cpu_addresses() {
        assert_eq!(parse_offset(Some("0x14000"), "x").unwrap(), 0x14000);
        assert_eq!(parse_offset(Some("0x08014000"), "x").unwrap(), 0x14000);
        assert_eq!(parse_offset(Some("25D000"), "x").unwrap(), 0x25D000);
        assert!(parse_offset(Some("zz"), "x").is_err());
        assert!(parse_offset(None, "x").is_err());
    }

    #[test]
    fn erase_range_is_half_open_and_4k_aligned() {
        let mut j = job();
        j.erase_start_hex = Some("0x00001234".into());
        j.erase_end_hex = Some("0x003FFFFF".into());
        assert_eq!(erase_range(&j).unwrap(), (0x1000, 0x400000));
        j.erase_end_hex = Some("0x00001000".into());
        assert!(erase_range(&j).is_err());
    }

    #[test]
    fn xfer_baud_falls_back_to_1m5() {
        let quiet = |_p: FlashEvent| {};
        let mut j = job();
        assert_eq!(xfer_baud(&j, &quiet), 1_500_000);
        j.baud_rate = 921_600;
        assert_eq!(xfer_baud(&j, &quiet), 921_600);
        j.baud_rate = 460_801;
        assert_eq!(xfer_baud(&j, &quiet), 1_500_000);
        j.baud_rate = 9600;
        assert_eq!(xfer_baud(&j, &quiet), 1_500_000);
    }

    #[test]
    fn load_image_truncates_to_end_and_pads_page() {
        let dir = std::env::temp_dir().join(format!("rtl8711daf-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("qio.bin");
        std::fs::write(&path, vec![0x5Au8; 0x30_0000]).unwrap();
        let warned = std::cell::Cell::new(false);
        let watch = |e: FlashEvent| {
            if matches!(e, FlashEvent::Warning { .. }) {
                warned.set(true);
            }
        };
        let seg = FlashSegment {
            firmware_path: path.to_string_lossy().into_owned(),
            start_addr: "0x0".into(),
            end_addr: "0x25D000".into(),
        };
        let img = load_image(&seg, 1024, 0x40_0000, &watch).unwrap();
        assert_eq!(img.data.len(), 0x25D000);
        assert!(warned.get(), "truncation must be reported to the user");

        std::fs::write(&path, vec![0x5Au8; 1000]).unwrap();
        let seg = FlashSegment {
            end_addr: String::new(),
            start_addr: "0x14000".into(),
            ..seg
        };
        let img = load_image(&seg, 1024, 0x40_0000, &watch).unwrap();
        assert_eq!((img.start, img.data.len()), (0x14000, 1024));
        assert!(img.data[1000..].iter().all(|&b| b == 0xFF));

        let seg = FlashSegment {
            start_addr: "0x14100".into(),
            ..seg
        };
        assert!(load_image(&seg, 1024, 0x40_0000, &watch).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn erased_checksum_matches_device() {
        // measured on hardware: CHKSM of an erased 4 KiB sector
        assert_eq!(erased_word_sum(0x1000), 0xffff_fc00);
        assert_eq!(erased_word_sum(0x1000), floader::word_sum(&[0xFF; 0x1000]));
    }

    #[test]
    fn jedec_id_from_query() {
        let info = DeviceInfo {
            did: DEVICE_ID,
            memory_type: 1,
            flash_mid: 0x85,
            flash_did: 0x1620,
            page_size: 1024,
            capacity: 0x40_0000,
        };
        assert_eq!(jedec_id(&info), 0x0085_2016);
    }
}
