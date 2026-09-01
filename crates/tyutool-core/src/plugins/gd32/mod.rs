//! GD32VW553 flash plugin.
//!
//! The chip has no download protocol of its own: its mask ROM speaks the AN3155-style
//! USART bootloader, which can do little more than write SRAM and jump to it, so
//! flashing means uploading a RAM loader first and letting *that* program the SiP
//! flash. [`protocol`] holds the wire format for both stages and the per-step calls;
//! this module is the job-level flow and the user-visible event stream.
//!
//! **[`LOADER_BIN`] is a vendor binary** — the same downloader GigaDevice's and Tuya's
//! tools upload, lifted byte for byte out of a USB capture of one of them (see
//! `protocol`'s module docs). It reports `SDK build revision 94fb25571b15fbea`,
//! `2025/07/04`. Treat it as opaque: it is 15 600 bytes, loads and enters at
//! [`protocol::LOADER_LOAD_ADDR`], and is the only thing that knows how to talk to this
//! part's flash controller. The precedent is `plugins/ln882h/ram.bin`.
//!
//! Not supported: [`FlashMode::Read`] — the loader exposes erase, program, verify and
//! reset, and no way to read flash back.

mod protocol;

use sha2::{Digest, Sha256};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::error::FlashError;
use crate::flash_event::{FlashEvent, FlashMilestone, FlashPhase};
use crate::job::{FlashJob, FlashMode};
use crate::plugin::FlashPlugin;

use protocol::{
    Gd32Io, FLASH_BASE, FRAME_DATA_LEN, FRAME_UNITS, ISP_BAUD, ISP_PARITY, LOADER_LOAD_ADDR,
    SECTOR_SIZE,
};

/// The RAM loader uploaded over the ROM bootloader. See the module docs on provenance.
const LOADER_BIN: &[u8] = include_bytes!("loader.bin");

/// Registry key.
pub const CHIP_ID: &str = "GD32VW553";

pub struct Gd32vw553Plugin;

impl FlashPlugin for Gd32vw553Plugin {
    fn id(&self) -> &'static str {
        CHIP_ID
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
            FlashMode::Read => Err(FlashError::Plugin(
                "GD32VW553: reading flash is not supported — the download loader has no \
                 read command"
                    .into(),
            )),
            FlashMode::Authorize => Err(FlashError::Plugin(
                "GD32VW553: authorize mode not supported".into(),
            )),
        }
    }
}

// ── Job planning (pure) ─────────────────────────────────────────────────────

/// Parses a `0x`-prefixed or bare hex address.
fn parse_hex_addr(s: &str) -> Result<u32, ()> {
    u32::from_str_radix(
        s.trim().trim_start_matches("0x").trim_start_matches("0X"),
        16,
    )
    .map_err(|_| ())
}

/// Flash offset for a user-supplied address.
///
/// Both spellings are accepted: an offset (`0x00000000`, what the GUI defaults to) and
/// the mapped address the GD32 datasheet and linker scripts use (`0x08000000`). Erase
/// and verify take offsets on the wire; only the image frames carry mapped addresses.
fn flash_offset(addr: u32) -> u32 {
    addr.checked_sub(FLASH_BASE).unwrap_or(addr)
}

/// The single (offset, firmware path) pair a flash job comes down to.
///
/// Multi-segment jobs are rejected rather than half-supported: the loader programs one
/// stream of consecutively addressed frames per session, and nothing in the capture
/// shows a second stream being opened after EOT.
fn plan_flash(job: &FlashJob) -> Result<(u32, String), FlashError> {
    let (path, start_hex) = match job.segments.as_deref() {
        Some([]) | None => {
            let path = job
                .firmware_path
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| FlashError::InvalidJob("missing firmware_path".into()))?;
            (
                path.to_string(),
                job.flash_start_hex
                    .clone()
                    .unwrap_or_else(|| "0x00000000".into()),
            )
        }
        Some([seg]) => (seg.firmware_path.clone(), seg.start_addr.clone()),
        Some(segs) => {
            return Err(FlashError::InvalidJob(format!(
                "GD32VW553: one firmware image per job, got {}",
                segs.len()
            )))
        }
    };

    let start = parse_hex_addr(&start_hex)
        .map_err(|_| FlashError::InvalidJob(format!("invalid flash start address: {start_hex}")))?;
    let offset = flash_offset(start);
    if !offset.is_multiple_of(SECTOR_SIZE) {
        return Err(FlashError::InvalidJob(format!(
            "GD32VW553: flash start address {start_hex} must be 4 KiB aligned"
        )));
    }
    Ok((offset, path))
}

/// The `[offset, offset + length)` window an erase job asks for.
fn plan_erase(job: &FlashJob) -> Result<(u32, u32), FlashError> {
    let start_hex = job.erase_start_hex.as_deref().unwrap_or("0x00000000");
    let end_hex = job.erase_end_hex.as_deref().unwrap_or("0x00400000");
    let start = parse_hex_addr(start_hex)
        .map_err(|_| FlashError::InvalidJob(format!("invalid erase_start_hex: {start_hex}")))?;
    let end = parse_hex_addr(end_hex)
        .map_err(|_| FlashError::InvalidJob(format!("invalid erase_end_hex: {end_hex}")))?;
    if end <= start {
        return Err(FlashError::InvalidJob(
            "erase_end_hex must be greater than erase_start_hex".into(),
        ));
    }
    let offset = flash_offset(start);
    let length = end - start;
    if !offset.is_multiple_of(SECTOR_SIZE) || !length.is_multiple_of(SECTOR_SIZE) {
        return Err(FlashError::InvalidJob(
            "GD32VW553: erase start and length must be 4 KiB aligned".into(),
        ));
    }
    Ok((offset, length))
}

/// Sector count as the erase command's `u16` field, refusing a range that overflows it.
fn erase_sectors(length: u32) -> Result<u16, FlashError> {
    u16::try_from(protocol::sector_count(length)).map_err(|_| {
        FlashError::InvalidJob(format!(
            "GD32VW553: 0x{length:X} bytes is more than one erase command can cover"
        ))
    })
}

/// How long to wait for an erase of `sectors` sectors.
///
/// The vendor tool's 317-sector erase took 15 s (~48 ms/sector); this budgets four
/// times that plus a fixed floor, so a slow part cannot trip the timeout while a dead
/// one still fails in bounded time.
fn erase_timeout(sectors: u16) -> Duration {
    Duration::from_millis(30_000 + 200 * u64::from(sectors))
}

// ── Job flows ───────────────────────────────────────────────────────────────

fn run_flash(
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    let (offset, path) = plan_flash(job)?;
    let image = std::fs::read(&path)
        .map_err(|e| FlashError::Plugin(format!("cannot read firmware '{path}': {e}")))?;
    if image.is_empty() {
        return Err(FlashError::InvalidJob(format!(
            "firmware file '{path}' is empty"
        )));
    }

    let mut io = protocol::open_port(&job.port, ISP_BAUD, ISP_PARITY)?;
    flash_with(&mut io, offset, &image, job.baud_rate, cancel, progress)
}

fn run_erase(
    job: &FlashJob,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    let (offset, length) = plan_erase(job)?;
    let mut io = protocol::open_port(&job.port, ISP_BAUD, ISP_PARITY)?;
    erase_with(&mut io, offset, length, cancel, progress)
}

/// Reset into the ROM bootloader, upload the RAM loader and hand control to it.
fn bring_up_loader(
    io: &mut dyn Gd32Io,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    progress(FlashEvent::Phase {
        phase: FlashPhase::Handshake,
    });
    protocol::isp_sync(io, cancel, progress)?;

    progress(FlashEvent::Phase {
        phase: FlashPhase::LoadRam,
    });
    log::info!(
        "uploading {} byte RAM loader to 0x{LOADER_LOAD_ADDR:08X}",
        LOADER_BIN.len()
    );
    protocol::isp_write_memory(io, LOADER_LOAD_ADDR, LOADER_BIN, cancel)?;
    protocol::isp_go(io, LOADER_LOAD_ADDR, cancel)?;

    let chip_id = protocol::loader_handshake(io, cancel)?;
    let chip_id = u32::from_le_bytes(chip_id);
    log::info!("RAM loader up, chip id 0x{chip_id:08X}");
    progress(FlashEvent::Milestone {
        milestone: FlashMilestone::Connected {
            chip_info: Some(format!("GD32VW553 (chip id 0x{chip_id:08X})")),
        },
    });
    Ok(())
}

/// Erase, program and verify `image` at flash `offset`, then reboot the device.
fn flash_with(
    io: &mut dyn Gd32Io,
    offset: u32,
    image: &[u8],
    transfer_baud: u32,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    let length = u32::try_from(image.len())
        .map_err(|_| FlashError::InvalidJob("GD32VW553: firmware image is too large".into()))?;
    // Erase what will actually be written, not what the image measures: the last frame
    // is padded out to a full [`FRAME_DATA_LEN`], and for some sizes that padding
    // reaches into the sector after the image's own last one (8 KiB of firmware is
    // 2 sectors but 4 frames). Programming a sector that was never erased is what the
    // loader reports as `fmc write fail`.
    let written = image.len().div_ceil(FRAME_DATA_LEN) * FRAME_DATA_LEN;
    let sectors =
        erase_sectors(u32::try_from(written).map_err(|_| {
            FlashError::InvalidJob("GD32VW553: firmware image is too large".into())
        })?)?;
    // Refuse before touching the device, not after erasing it, when the image cannot be
    // verified afterwards.
    protocol::loader_verify_command(offset, length)?;

    bring_up_loader(io, cancel, progress)?;

    progress(FlashEvent::Phase {
        phase: FlashPhase::Erase,
    });
    log::info!("erasing {sectors} sector(s) from offset 0x{offset:08X}");
    protocol::loader_erase(io, offset, sectors, erase_timeout(sectors), cancel)?;
    progress(FlashEvent::Milestone {
        milestone: FlashMilestone::EraseComplete,
    });

    progress(FlashEvent::Phase {
        phase: FlashPhase::SwitchBaud,
    });
    protocol::loader_set_baud(io, transfer_baud, cancel)?;
    protocol::loader_set_frame_size(io, FRAME_UNITS, cancel)?;

    progress(FlashEvent::Phase {
        phase: FlashPhase::Write,
    });
    log::info!(
        "writing {length} byte(s) to offset 0x{offset:08X} in {}-byte frames",
        FRAME_DATA_LEN
    );
    protocol::send_image(io, offset, image, cancel, &|sent| {
        progress(FlashEvent::Percent {
            value: (sent as u64 * 100 / length.max(1) as u64) as u8,
        });
    })?;
    progress(FlashEvent::Percent { value: 100 });
    progress(FlashEvent::Milestone {
        milestone: FlashMilestone::WriteComplete,
    });

    progress(FlashEvent::Phase {
        phase: FlashPhase::Verify,
    });
    let reported = protocol::loader_verify(io, offset, length, cancel)?;
    let expected: [u8; 32] = Sha256::digest(image).into();
    if reported != expected {
        return Err(FlashError::Plugin(format!(
            "GD32VW553: verification failed — the device reports SHA-256 {} for what it \
             stored, the firmware is {}",
            hex(&reported),
            hex(&expected)
        )));
    }
    progress(FlashEvent::Milestone {
        milestone: FlashMilestone::VerifyPassed,
    });

    progress(FlashEvent::Phase {
        phase: FlashPhase::Reboot,
    });
    protocol::loader_reset(io)?;
    progress(FlashEvent::Milestone {
        milestone: FlashMilestone::Rebooted,
    });
    Ok(())
}

/// Erase `[offset, offset + length)` and reboot the device.
fn erase_with(
    io: &mut dyn Gd32Io,
    offset: u32,
    length: u32,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    let sectors = erase_sectors(length)?;
    bring_up_loader(io, cancel, progress)?;

    progress(FlashEvent::Phase {
        phase: FlashPhase::Erase,
    });
    log::info!("erasing {sectors} sector(s) from offset 0x{offset:08X}");
    protocol::loader_erase(io, offset, sectors, erase_timeout(sectors), cancel)?;
    progress(FlashEvent::Milestone {
        milestone: FlashMilestone::EraseComplete,
    });

    progress(FlashEvent::Phase {
        phase: FlashPhase::Reboot,
    });
    protocol::loader_reset(io)?;
    progress(FlashEvent::Milestone {
        milestone: FlashMilestone::Rebooted,
    });
    Ok(())
}

/// Lower-case hex for a digest in an error message.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::FlashSegment;
    use serialport::Parity;
    use std::collections::VecDeque;
    use std::io;
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    fn job(mode: FlashMode) -> FlashJob {
        FlashJob::new(mode, CHIP_ID, "", 2_000_000)
    }

    // ── A scripted GD32VW553 ────────────────────────────────────────────────
    //
    // Answers both protocol stages the way the captured device did, and keeps what it
    // was told to store, so a flow test can assert on the flash contents rather than on
    // a transcript of bytes.

    const FAKE_FLASH_LEN: usize = 0x0040_0000;
    const FAKE_CHIP_ID: [u8; 4] = [0x36, 0x48, 0x4D, 0x50]; // as captured

    #[derive(PartialEq)]
    enum Stage {
        Isp,
        Loader,
    }

    #[derive(PartialEq)]
    enum IspState {
        Idle,
        WriteAddress,
        WriteBlock,
        GoAddress,
    }

    struct FakeDevice {
        stage: Stage,
        isp: IspState,
        synced: bool,
        inbuf: Vec<u8>,
        out: VecDeque<u8>,
        write_addr: u32,
        ram: Vec<(u32, Vec<u8>)>,
        flash: Vec<u8>,
        frame_data_len: usize,
        erased: Vec<(u32, u16)>,
        baud: u32,
        parity: Parity,
        link: Vec<(u32, Parity)>,
        lines: Vec<(bool, bool)>,
        dtr: bool,
        rts: bool,
        rebooted: bool,
        /// Flip one bit of the reported digest, to exercise the verify failure path.
        corrupt_digest: bool,
        /// Answer the first image frame with a NAK, to exercise the resend path.
        nak_first_frame: bool,
        naks_sent: u32,
        /// Stay silent for this many sync bytes first, standing in for a board that is
        /// not in boot mode yet.
        deaf_syncs: u8,
    }

    impl FakeDevice {
        fn new() -> Self {
            Self {
                stage: Stage::Isp,
                isp: IspState::Idle,
                synced: false,
                inbuf: Vec::new(),
                out: VecDeque::new(),
                write_addr: 0,
                ram: Vec::new(),
                flash: vec![0x00; FAKE_FLASH_LEN],
                frame_data_len: FRAME_DATA_LEN,
                erased: Vec::new(),
                baud: ISP_BAUD,
                parity: ISP_PARITY,
                link: Vec::new(),
                lines: Vec::new(),
                dtr: false,
                rts: false,
                rebooted: false,
                corrupt_digest: false,
                nak_first_frame: false,
                naks_sent: 0,
                deaf_syncs: 0,
            }
        }

        /// The loader image as the device received it, in address order.
        fn ram_image(&self) -> Vec<u8> {
            let mut chunks = self.ram.clone();
            chunks.sort_by_key(|(addr, _)| *addr);
            chunks.into_iter().flat_map(|(_, data)| data).collect()
        }

        fn take(&mut self, n: usize) -> Option<Vec<u8>> {
            if self.inbuf.len() < n {
                return None;
            }
            Some(self.inbuf.drain(..n).collect())
        }

        fn pump(&mut self) {
            while self.step() {}
        }

        /// Consume one complete command; `false` when more bytes are needed.
        fn step(&mut self) -> bool {
            match self.stage {
                Stage::Isp => self.step_isp(),
                Stage::Loader => self.step_loader(),
            }
        }

        fn step_isp(&mut self) -> bool {
            match self.isp {
                IspState::Idle => {
                    let Some(&first) = self.inbuf.first() else {
                        return false;
                    };
                    match first {
                        protocol::ISP_SYNC => {
                            self.inbuf.remove(0);
                            if self.deaf_syncs > 0 {
                                self.deaf_syncs -= 1;
                                return true;
                            }
                            self.out.push_back(if self.synced {
                                protocol::ISP_NACK
                            } else {
                                protocol::ISP_ACK
                            });
                            self.synced = true;
                        }
                        protocol::ISP_CMD_WRITE_MEMORY | protocol::ISP_CMD_GO => {
                            let Some(cmd) = self.take(2) else {
                                return false;
                            };
                            assert_eq!(cmd[1], !cmd[0], "command complement");
                            self.isp = if first == protocol::ISP_CMD_WRITE_MEMORY {
                                IspState::WriteAddress
                            } else {
                                IspState::GoAddress
                            };
                            self.out.push_back(protocol::ISP_ACK);
                        }
                        other => panic!("unexpected bootloader byte 0x{other:02X}"),
                    }
                    true
                }
                IspState::WriteAddress | IspState::GoAddress => {
                    let Some(frame) = self.take(5) else {
                        return false;
                    };
                    let xor = frame[..4].iter().fold(0u8, |a, b| a ^ b);
                    assert_eq!(xor, frame[4], "address checksum");
                    let addr = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]);
                    self.out.push_back(protocol::ISP_ACK);
                    if self.isp == IspState::GoAddress {
                        assert_eq!(addr, LOADER_LOAD_ADDR, "jump target");
                        // The captured ROM emits a second ACK as it hands over.
                        self.out.push_back(protocol::ISP_ACK);
                        self.stage = Stage::Loader;
                        self.isp = IspState::Idle;
                    } else {
                        self.write_addr = addr;
                        self.isp = IspState::WriteBlock;
                    }
                    true
                }
                IspState::WriteBlock => {
                    let Some(&n) = self.inbuf.first() else {
                        return false;
                    };
                    let total = 1 + n as usize + 1 + 1;
                    let Some(block) = self.take(total) else {
                        return false;
                    };
                    let cks = block[..total - 1].iter().fold(0u8, |a, b| a ^ b);
                    assert_eq!(cks, block[total - 1], "write block checksum");
                    self.ram
                        .push((self.write_addr, block[1..total - 1].to_vec()));
                    self.out.push_back(protocol::ISP_ACK);
                    self.isp = IspState::Idle;
                    true
                }
            }
        }

        fn step_loader(&mut self) -> bool {
            let Some(&first) = self.inbuf.first() else {
                return false;
            };
            match first {
                protocol::LOADER_CMD_PING => {
                    self.inbuf.remove(0);
                    self.out.push_back(protocol::ISP_ACK);
                }
                protocol::LOADER_CMD_CHIP_ID => {
                    self.inbuf.remove(0);
                    self.out.extend(FAKE_CHIP_ID);
                }
                protocol::LOADER_CMD_ERASE => {
                    let Some(cmd) = self.take(7) else {
                        return false;
                    };
                    let offset = u32::from_le_bytes([cmd[1], cmd[2], cmd[3], cmd[4]]);
                    let sectors = u16::from_le_bytes([cmd[5], cmd[6]]);
                    let end = offset as usize + sectors as usize * SECTOR_SIZE as usize;
                    self.flash[offset as usize..end].fill(0xFF);
                    self.erased.push((offset, sectors));
                    self.out.push_back(protocol::LOADER_ACK);
                }
                protocol::LOADER_CMD_SET_BAUD => {
                    let Some(cmd) = self.take(5) else {
                        return false;
                    };
                    let _ = u32::from_le_bytes([cmd[1], cmd[2], cmd[3], cmd[4]]);
                    self.out.push_back(protocol::LOADER_ACK);
                }
                protocol::LOADER_CMD_FRAME_SIZE => {
                    let Some(cmd) = self.take(2) else {
                        return false;
                    };
                    self.frame_data_len = cmd[1] as usize * protocol::FRAME_UNIT;
                    self.out.push_back(protocol::LOADER_ACK);
                }
                protocol::STX => {
                    let total = 3 + 4 + self.frame_data_len + 1;
                    let Some(frame) = self.take(total) else {
                        return false;
                    };
                    assert_eq!(frame[2], !frame[1], "frame sequence complement");
                    let sum = frame[3..total - 1]
                        .iter()
                        .fold(0u8, |a, b| a.wrapping_add(*b));
                    assert_eq!(sum, frame[total - 1], "frame checksum");
                    if self.nak_first_frame && self.naks_sent == 0 {
                        self.naks_sent += 1;
                        self.out.push_back(protocol::LOADER_NAK);
                        return true;
                    }
                    let addr = u32::from_le_bytes([frame[3], frame[4], frame[5], frame[6]]);
                    let at = (addr - FLASH_BASE) as usize;
                    self.flash[at..at + self.frame_data_len].copy_from_slice(&frame[7..total - 1]);
                    self.out.push_back(protocol::LOADER_ACK);
                }
                protocol::EOT => {
                    self.inbuf.remove(0);
                    self.out.push_back(protocol::LOADER_ACK);
                }
                protocol::LOADER_CMD_VERIFY => {
                    let Some(cmd) = self.take(8) else {
                        return false;
                    };
                    let offset = u32::from_le_bytes([cmd[1], cmd[2], cmd[3], 0]) as usize;
                    let len = u32::from_le_bytes([cmd[4], cmd[5], cmd[6], 0]) as usize;
                    assert_eq!(cmd[7], protocol::LOADER_VERIFY_SHA256);
                    let mut digest: [u8; 32] =
                        Sha256::digest(&self.flash[offset..offset + len]).into();
                    if self.corrupt_digest {
                        digest[0] ^= 0xFF;
                    }
                    self.out.extend(digest);
                }
                protocol::LOADER_CMD_RESET => {
                    let Some(cmd) = self.take(2) else {
                        return false;
                    };
                    assert_eq!(cmd[1], 0x01);
                    self.rebooted = true;
                }
                other => panic!("unexpected loader byte 0x{other:02X}"),
            }
            true
        }
    }

    impl Gd32Io for FakeDevice {
        fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
            self.inbuf.extend_from_slice(data);
            self.pump();
            Ok(())
        }

        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.out.is_empty() {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "no data"));
            }
            let n = buf.len().min(self.out.len());
            for slot in buf.iter_mut().take(n) {
                *slot = self.out.pop_front().unwrap();
            }
            Ok(n)
        }

        fn reconfigure(&mut self, baud: u32, parity: Parity) -> io::Result<()> {
            self.baud = baud;
            self.parity = parity;
            self.link.push((baud, parity));
            Ok(())
        }

        fn set_dtr(&mut self, level: bool) -> io::Result<()> {
            self.dtr = level;
            self.lines.push((self.dtr, self.rts));
            Ok(())
        }

        fn set_rts(&mut self, level: bool) -> io::Result<()> {
            self.rts = level;
            self.lines.push((self.dtr, self.rts));
            Ok(())
        }

        fn clear_input(&mut self) -> io::Result<()> {
            self.out.clear();
            Ok(())
        }
    }

    /// Collects the events a flow emits.
    #[derive(Default)]
    struct Events(Mutex<Vec<FlashEvent>>);

    impl Events {
        fn sink(&self) -> impl Fn(FlashEvent) + '_ {
            move |e| self.0.lock().unwrap().push(e)
        }

        fn phases(&self) -> Vec<String> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .filter_map(|e| match e {
                    FlashEvent::Phase { phase } => Some(format!("{phase:?}")),
                    _ => None,
                })
                .collect()
        }

        fn milestones(&self) -> Vec<String> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .filter_map(|e| match e {
                    FlashEvent::Milestone { milestone } => Some(format!("{milestone:?}")),
                    _ => None,
                })
                .collect()
        }

        fn percents(&self) -> Vec<u8> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .filter_map(|e| match e {
                    FlashEvent::Percent { value } => Some(*value),
                    _ => None,
                })
                .collect()
        }
    }

    /// An image that is not a whole number of frames, so padding is exercised too.
    fn test_image(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 7 + 3) as u8).collect()
    }

    #[test]
    fn plugin_id_is_the_registry_key() {
        assert_eq!(Gd32vw553Plugin.id(), "GD32VW553");
    }

    #[test]
    fn read_and_authorize_are_refused_without_opening_a_port() {
        let cancel = AtomicBool::new(false);
        for mode in [FlashMode::Read, FlashMode::Authorize] {
            let err = Gd32vw553Plugin
                .run(&job(mode), &cancel, &|_| {})
                .expect_err("mode is unsupported");
            assert!(
                matches!(err, FlashError::Plugin(ref m) if m.contains("not supported")),
                "unexpected error for {mode:?}: {err}"
            );
        }
    }

    #[test]
    fn the_bundled_loader_is_the_captured_one() {
        assert_eq!(LOADER_BIN.len(), 15_600);
        assert_eq!(LOADER_BIN.len() % protocol::ISP_CHUNK, 0);
        assert_eq!(
            hex(&Sha256::digest(LOADER_BIN)),
            "2559d822553f2af8f9f4ff26201fffac151f8e13ec29b6b1c0241215445d373e"
        );
    }

    #[test]
    fn flash_writes_verifies_and_reboots() {
        let mut dev = FakeDevice::new();
        let image = test_image(FRAME_DATA_LEN * 2 + 17);
        let events = Events::default();
        let cancel = AtomicBool::new(false);

        flash_with(&mut dev, 0x1000, &image, 2_000_000, &cancel, &events.sink())
            .expect("flash succeeds against a scripted device");

        // The loader arrived intact, at the address the ROM was told to jump to.
        assert_eq!(dev.ram_image(), LOADER_BIN);
        assert_eq!(dev.ram[0].0, LOADER_LOAD_ADDR);
        // Exactly the image footprint was erased, and the image is in flash.
        assert_eq!(dev.erased, vec![(0x1000, 2)]);
        assert_eq!(&dev.flash[0x1000..0x1000 + image.len()], &image[..]);
        // Padding stopped at the frame boundary, leaving the rest erased.
        let padded_end = 0x1000 + 3 * FRAME_DATA_LEN;
        assert!(dev.flash[0x1000 + image.len()..padded_end]
            .iter()
            .all(|&b| b == protocol::PAD_BYTE));
        assert!(dev.rebooted);

        // The link was re-framed twice: 8N1 for the loader, then the transfer baud.
        assert_eq!(
            dev.link,
            vec![
                (protocol::LOADER_BAUD, Parity::None),
                (2_000_000, Parity::None)
            ]
        );
        // Reset dance: reset asserted, BOOT0 raised under reset, reset released, BOOT0
        // dropped — the order the chip needs to sample BOOT0.
        assert_eq!(
            dev.lines,
            vec![
                (true, false),
                (true, false),
                (true, true),
                (false, true),
                (false, false)
            ]
        );

        assert_eq!(
            events.phases(),
            vec![
                "Handshake",
                "LoadRam",
                "Erase",
                "SwitchBaud",
                "Write",
                "Verify",
                "Reboot"
            ]
        );
        assert_eq!(
            events.milestones(),
            vec![
                "Connected { chip_info: Some(\"GD32VW553 (chip id 0x504D4836)\") }",
                "EraseComplete",
                "WriteComplete",
                "VerifyPassed",
                "Rebooted",
            ]
        );
        let percents = events.percents();
        assert_eq!(*percents.last().unwrap(), 100);
        assert!(percents.windows(2).all(|w| w[0] <= w[1]), "{percents:?}");
    }

    #[test]
    fn erase_covers_the_frame_padding_not_just_the_image() {
        // 8 KiB is exactly 2 sectors but 4 frames (10 KiB written), so erasing the
        // image's own footprint would leave the last frame programming live flash.
        let mut dev = FakeDevice::new();
        let image = test_image(8192);
        let cancel = AtomicBool::new(false);

        flash_with(&mut dev, 0, &image, 2_000_000, &cancel, &|_| {})
            .expect("flash succeeds against a scripted device");

        assert_eq!(dev.erased, vec![(0, 3)]);
        assert_eq!(&dev.flash[..image.len()], &image[..]);
    }

    #[test]
    fn a_board_not_in_boot_mode_is_told_what_to_do_and_then_retried() {
        // The board that ignores the first sync byte stands in for one whose BOOT is a
        // button: the operator gets a warning saying to hold it, and the retry that
        // follows (which pulses reset for them) is what gets in.
        let mut dev = FakeDevice::new();
        dev.deaf_syncs = 1;
        let events = Events::default();
        let cancel = AtomicBool::new(false);

        flash_with(
            &mut dev,
            0,
            &test_image(64),
            2_000_000,
            &cancel,
            &events.sink(),
        )
        .expect("the second reset gets in");

        let warnings: Vec<String> = events
            .0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                FlashEvent::Warning { message } => Some(message.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(warnings.len(), 1, "one warning, not one per retry");
        assert!(warnings[0].contains("boot mode"), "{}", warnings[0]);
        // Two reset dances: five line writes each.
        assert_eq!(dev.lines.len(), 10);
    }

    #[test]
    fn a_naked_frame_is_resent() {
        let mut dev = FakeDevice::new();
        dev.nak_first_frame = true;
        let image = test_image(FRAME_DATA_LEN + 5);
        let cancel = AtomicBool::new(false);

        flash_with(&mut dev, 0, &image, 2_000_000, &cancel, &|_| {})
            .expect("a NAKed frame is resent, not fatal");

        assert_eq!(dev.naks_sent, 1);
        assert_eq!(&dev.flash[..image.len()], &image[..]);
    }

    #[test]
    fn a_digest_mismatch_fails_the_job() {
        let mut dev = FakeDevice::new();
        dev.corrupt_digest = true;
        let cancel = AtomicBool::new(false);

        let err = flash_with(&mut dev, 0, &test_image(64), 2_000_000, &cancel, &|_| {})
            .expect_err("a wrong digest must not pass");
        assert!(
            matches!(err, FlashError::Plugin(ref m) if m.contains("verification failed")),
            "{err}"
        );
        // The device is left alone rather than rebooted into a bad image.
        assert!(!dev.rebooted);
    }

    #[test]
    fn cancelling_stops_the_job() {
        let mut dev = FakeDevice::new();
        let cancel = AtomicBool::new(true);
        let err = flash_with(&mut dev, 0, &test_image(64), 2_000_000, &cancel, &|_| {})
            .expect_err("a cancelled job must not run");
        assert!(matches!(err, FlashError::Cancelled), "{err}");
    }

    #[test]
    fn erase_only_erases_and_reboots() {
        let mut dev = FakeDevice::new();
        dev.flash[0x2000] = 0x42;
        let events = Events::default();
        let cancel = AtomicBool::new(false);

        erase_with(&mut dev, 0x2000, 0x4000, &cancel, &events.sink())
            .expect("erase succeeds against a scripted device");

        assert_eq!(dev.erased, vec![(0x2000, 4)]);
        assert!(dev.flash[0x2000..0x6000].iter().all(|&b| b == 0xFF));
        assert!(dev.rebooted);
        assert_eq!(
            events.phases(),
            vec!["Handshake", "LoadRam", "Erase", "Reboot"]
        );
        // No frames are sent, so the transfer baud is never negotiated.
        assert_eq!(dev.link, vec![(protocol::LOADER_BAUD, Parity::None)]);
    }

    // ── Planning ────────────────────────────────────────────────────────────

    #[test]
    fn plan_flash_uses_firmware_path_and_default_start() {
        let mut j = job(FlashMode::Flash);
        j.firmware_path = Some("fw.bin".into());
        assert_eq!(plan_flash(&j).unwrap(), (0, "fw.bin".to_string()));
    }

    #[test]
    fn plan_flash_accepts_a_single_segment() {
        let mut j = job(FlashMode::Flash);
        j.segments = Some(vec![FlashSegment {
            firmware_path: "app.bin".into(),
            start_addr: "0x00010000".into(),
            end_addr: "0x00020000".into(),
        }]);
        assert_eq!(plan_flash(&j).unwrap(), (0x1_0000, "app.bin".to_string()));
    }

    #[test]
    fn plan_flash_rejects_multiple_segments() {
        let mut j = job(FlashMode::Flash);
        let seg = FlashSegment {
            firmware_path: "a.bin".into(),
            start_addr: "0x0".into(),
            end_addr: "0x1000".into(),
        };
        j.segments = Some(vec![seg.clone(), seg]);
        assert!(matches!(plan_flash(&j), Err(FlashError::InvalidJob(_))));
    }

    #[test]
    fn plan_flash_takes_mapped_addresses_as_well_as_offsets() {
        let mut j = job(FlashMode::Flash);
        j.firmware_path = Some("fw.bin".into());
        j.flash_start_hex = Some("0x08010000".into());
        assert_eq!(plan_flash(&j).unwrap().0, 0x1_0000);
        j.flash_start_hex = Some("0x00010000".into());
        assert_eq!(plan_flash(&j).unwrap().0, 0x1_0000);
    }

    #[test]
    fn plan_flash_requires_a_sector_aligned_start() {
        let mut j = job(FlashMode::Flash);
        j.firmware_path = Some("fw.bin".into());
        j.flash_start_hex = Some("0x00000800".into());
        assert!(matches!(plan_flash(&j), Err(FlashError::InvalidJob(_))));
    }

    #[test]
    fn plan_flash_requires_a_firmware_path() {
        assert!(matches!(
            plan_flash(&job(FlashMode::Flash)),
            Err(FlashError::InvalidJob(_))
        ));
    }

    #[test]
    fn plan_erase_defaults_to_the_whole_4_mib_part() {
        assert_eq!(
            plan_erase(&job(FlashMode::Erase)).unwrap(),
            (0, 0x0040_0000)
        );
    }

    #[test]
    fn plan_erase_measures_a_half_open_range() {
        let mut j = job(FlashMode::Erase);
        j.erase_start_hex = Some("0x00001000".into());
        j.erase_end_hex = Some("0x00003000".into());
        assert_eq!(plan_erase(&j).unwrap(), (0x1000, 0x2000));
    }

    #[test]
    fn plan_erase_rejects_unaligned_or_inverted_ranges() {
        let mut j = job(FlashMode::Erase);
        j.erase_start_hex = Some("0x00002000".into());
        j.erase_end_hex = Some("0x00001000".into());
        assert!(matches!(plan_erase(&j), Err(FlashError::InvalidJob(_))));

        j.erase_start_hex = Some("0x00000000".into());
        j.erase_end_hex = Some("0x00000800".into());
        assert!(matches!(plan_erase(&j), Err(FlashError::InvalidJob(_))));
    }

    #[test]
    fn erase_sectors_refuses_a_range_past_the_u16_field() {
        assert_eq!(erase_sectors(0x0040_0000).unwrap(), 1024);
        assert!(erase_sectors(0x1000_0000).is_err()); // 65 536 sectors
    }

    #[test]
    fn erase_timeout_grows_with_the_range() {
        // The captured 317-sector erase took 15 s; the budget has to sit above it.
        assert!(erase_timeout(317) > Duration::from_secs(15));
        assert!(erase_timeout(1024) > erase_timeout(317));
    }

    #[test]
    fn parse_hex_addr_accepts_the_spellings_the_ui_produces() {
        assert_eq!(parse_hex_addr("0x1000"), Ok(0x1000));
        assert_eq!(parse_hex_addr("0X1000"), Ok(0x1000));
        assert_eq!(parse_hex_addr("1000"), Ok(0x1000));
        assert!(parse_hex_addr("").is_err());
        assert!(parse_hex_addr("nope").is_err());
    }
}
