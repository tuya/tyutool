//! Boot ROM side: entering UART download mode, the ROM handshake and loading the
//! flash loader into RAM (mirrors `RomHandler.cs` / `DownloadHandler.CheckDownloadMode`
//! in the vendor ImageTool).

use std::time::Duration;

use super::io::Link;
use crate::error::FlashError;
use crate::flash_event::FlashEvent;

pub const NAK: u8 = 0x15;
pub const ACK: u8 = 0x06;
pub const CAN: u8 = 0x18;
pub const STX: u8 = 0x02;
pub const EOT: u8 = 0x04;
pub const ESC: u8 = 0x1B;
pub const BAUDSET: u8 = 0x05;
pub const BAUDCHK: u8 = 0x07;

/// ROM handshake baud rate.
pub const HANDSHAKE_BAUD: u32 = 115_200;
/// Baud rate of the running firmware's log UART (where `reboot uartburn` is typed).
pub const LOG_BAUD: u32 = 1_500_000;
/// RAM address the flash loader is loaded to and run from.
pub const FLOADER_ADDR: u32 = 0x3000_A000;
const XMODEM_DATA_LEN: usize = 1024;

/// ROM `BAUDSET` index table (35 entries; AmebaDplus differs from older Ameba chips).
pub const ROM_BAUD_TABLE: [u32; 35] = [
    110, 300, 600, 1200, 2400, 4800, 9600, 14400, 19200, 28800, 38400, 57600, 76800, 115200,
    128000, 153600, 230400, 380400, 460800, 500000, 921600, 1000000, 1382400, 1444400, 1500000,
    1843200, 2000000, 2100000, 2764800, 3000000, 3250000, 3692300, 3750000, 4000000, 6000000,
];

// Vendor RtSettings defaults (profile has SupportUsbDownload=true, so the Usb* boot delays apply).
const BAUD_SWITCH_DELAY: Duration = Duration::from_millis(200);
const ROM_BOOT_DELAY: Duration = Duration::from_millis(500);
pub const FLOADER_BOOT_DELAY: Duration = Duration::from_millis(500);
const ROM_REPLY_TIMEOUT: Duration = Duration::from_millis(500);
const STX_REPLY_TIMEOUT: Duration = Duration::from_millis(1000);
/// Time for the firmware to boot far enough that its shell accepts commands.
const FIRMWARE_BOOT_WAIT: Duration = Duration::from_millis(1500);
/// How long the user gets to enter download mode by hand.
const MANUAL_WAIT: Duration = Duration::from_secs(60);

pub fn rom_baud_index(baud: u32) -> Option<u8> {
    ROM_BAUD_TABLE
        .iter()
        .position(|&b| b == baud)
        .map(|i| i as u8)
}

/// True when `bytes` look like the ROM's idle NAK stream rather than firmware log noise
/// (a 1.5 Mbaud log read at 115200 baud is random bytes that occasionally include 0x15).
pub fn is_nak_stream(bytes: &[u8]) -> bool {
    let naks = bytes.iter().filter(|&&b| b == NAK).count();
    naks >= 4 && naks * 2 >= bytes.len()
}

/// One xmodem-style STX packet: `[STX][seq][!seq][addr32 LE][1024 data][sum8(addr+data)]`.
pub fn build_stx_packet(seq: u8, addr: u32, data: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(8 + XMODEM_DATA_LEN);
    p.extend_from_slice(&[STX, seq, !seq]);
    p.extend_from_slice(&addr.to_le_bytes());
    p.extend_from_slice(data);
    p.resize(7 + XMODEM_DATA_LEN, 0x00);
    let sum = p[3..].iter().fold(0u8, |a, &b| a.wrapping_add(b));
    p.push(sum);
    p
}

/// Loader image as sent to the ROM: loader + baud rate (u32 LE, read by the loader at
/// start-up) zero-padded to a multiple of 1024 bytes.
pub fn build_floader_image(loader: &[u8], baud: u32) -> Vec<u8> {
    let mut img = loader.to_vec();
    img.extend_from_slice(&baud.to_le_bytes());
    let padded = img.len().div_ceil(XMODEM_DATA_LEN) * XMODEM_DATA_LEN;
    img.resize(padded, 0x00);
    img
}

/// ROM request: clear RX, send, read up to 3 bytes looking for ACK (NAKs are skipped);
/// the whole thing is tried twice.
fn rom_request(
    link: &mut Link,
    req: &[u8],
    timeout: Duration,
) -> Result<Result<(), String>, FlashError> {
    let mut last = String::from("no reply");
    for _ in 0..2 {
        link.clear()?;
        link.write(req)?;
        for _ in 0..3 {
            match link.read_byte(timeout)? {
                None => {
                    last = "timeout".into();
                    break;
                }
                Some(ACK) => return Ok(Ok(())),
                Some(CAN) => return Ok(Err("CAN".into())),
                Some(NAK) => last = "NAK".into(),
                Some(b) => last = format!("unexpected 0x{b:02x}"),
            }
        }
        link.sleep(Duration::from_millis(1))?;
    }
    Ok(Err(last))
}

/// ROM handshake at 115200: expect NAK, `BAUDSET idx` → ACK, switch baud, `BAUDCHK` → ACK.
/// Leaves the link at `xfer_baud` on success.
pub fn rom_handshake(link: &mut Link, xfer_baud: u32) -> Result<Result<(), String>, FlashError> {
    let Some(idx) = rom_baud_index(xfer_baud) else {
        return Ok(Err(format!("baud {xfer_baud} not supported by ROM")));
    };
    match link.read_byte(ROM_REPLY_TIMEOUT)? {
        Some(NAK) => {}
        other => return Ok(Err(format!("not in download mode (got {other:02x?})"))),
    }
    if xfer_baud != link.baud() {
        if let Err(e) = rom_request(link, &[BAUDSET, idx], ROM_REPLY_TIMEOUT)? {
            return Ok(Err(format!("BAUDSET: {e}")));
        }
        link.sleep(BAUD_SWITCH_DELAY)?;
        link.set_baud(xfer_baud)?;
    }
    if let Err(e) = rom_request(link, &[BAUDCHK], ROM_REPLY_TIMEOUT)? {
        return Ok(Err(format!("BAUDCHK @ {xfer_baud}: {e}")));
    }
    Ok(Ok(()))
}

/// Listen at 115200 for the ROM's NAK stream.
fn rom_is_listening(link: &mut Link, window: Duration) -> Result<bool, FlashError> {
    link.set_baud(HANDSHAKE_BAUD)?;
    link.clear()?;
    let bytes = link.drain(window)?;
    Ok(is_nak_stream(&bytes))
}

/// Type `reboot uartburn` into the firmware shell on the log UART.
fn software_reboot_to_download(link: &mut Link) -> Result<bool, FlashError> {
    link.set_baud(LOG_BAUD)?;
    link.drain(Duration::from_millis(300))?;
    link.clear()?;
    link.write(&[ESC, b'\r', b'\n'])?;
    link.sleep(Duration::from_millis(20))?;
    link.write(b"reboot uartburn\r\n")?;
    link.sleep(ROM_BOOT_DELAY)?;
    rom_is_listening(link, Duration::from_millis(500))
}

/// Pulse RTS (wired to CHIP_EN on download boards), like the vendor `Reset.cfg`.
pub fn rts_reset(link: &mut Link) -> Result<(), FlashError> {
    link.set_rts(true)?;
    link.sleep(Duration::from_millis(200))?;
    link.set_rts(false)?;
    Ok(())
}

/// Bring the chip into ROM UART download mode and complete the ROM handshake at
/// `xfer_baud`. Three attempts, in order:
/// 1. `reboot uartburn` typed into the running firmware;
/// 2. RTS reset (CHIP_EN) then `reboot uartburn` again (firmware shell was stuck);
/// 3. ask the user to enter download mode by hand and wait for the NAK stream.
pub fn enter_download_mode(
    link: &mut Link,
    xfer_baud: u32,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    // Opening the port may have pulsed RTS and reset the chip; give the firmware time,
    // and catch a chip that is already sitting in download mode.
    if rom_is_listening(link, FIRMWARE_BOOT_WAIT)? {
        log::info!("RTL8711DAF: already in download mode");
        if let Ok(()) = rom_handshake(link, xfer_baud)? {
            return Ok(());
        }
    }

    log::info!("RTL8711DAF: entering download mode via 'reboot uartburn'");
    if software_reboot_to_download(link)? {
        match rom_handshake(link, xfer_baud)? {
            Ok(()) => return Ok(()),
            Err(e) => log::info!("RTL8711DAF: ROM handshake failed: {e}"),
        }
    } else {
        log::info!("RTL8711DAF: firmware did not respond to 'reboot uartburn'");
    }

    log::info!("RTL8711DAF: resetting via RTS (CHIP_EN) and retrying");
    rts_reset(link)?;
    link.set_baud(LOG_BAUD)?;
    link.drain(FIRMWARE_BOOT_WAIT)?;
    if rom_is_listening(link, Duration::from_millis(300))? || software_reboot_to_download(link)? {
        match rom_handshake(link, xfer_baud)? {
            Ok(()) => return Ok(()),
            Err(e) => log::info!("RTL8711DAF: ROM handshake failed: {e}"),
        }
    } else {
        log::info!("RTL8711DAF: still no download mode after RTS reset");
    }

    progress(FlashEvent::Warning {
        message: format!(
            "RTL8711DAF did not enter download mode automatically: pull Log_Tx to GND, \
             power-cycle the board, then reconnect Log_Tx (waiting up to {} s)",
            MANUAL_WAIT.as_secs()
        ),
    });
    let deadline = std::time::Instant::now() + MANUAL_WAIT;
    while std::time::Instant::now() < deadline {
        if rom_is_listening(link, Duration::from_millis(300))? {
            match rom_handshake(link, xfer_baud)? {
                Ok(()) => return Ok(()),
                Err(e) => log::info!("RTL8711DAF: ROM handshake failed: {e}"),
            }
        }
    }
    Err(FlashError::Plugin(
        "RTL8711DAF: could not enter UART download mode (pull Log_Tx low, power-cycle, then release Log_Tx)".into(),
    ))
}

/// Send the flash loader to RAM through the ROM (link already at `xfer_baud`), then wait
/// for it to start.
pub fn download_floader(link: &mut Link, loader: &[u8]) -> Result<(), FlashError> {
    let image = build_floader_image(loader, link.baud());
    let mut seq: u8 = 1;
    for (i, chunk) in image.chunks(XMODEM_DATA_LEN).enumerate() {
        let addr = FLOADER_ADDR + (i * XMODEM_DATA_LEN) as u32;
        let pkt = build_stx_packet(seq, addr, chunk);
        if let Err(e) = rom_request(link, &pkt, STX_REPLY_TIMEOUT)? {
            let _ = link.write(&[ESC]);
            return Err(FlashError::Plugin(format!(
                "RTL8711DAF: flash loader packet {} @0x{addr:08x} failed: {e}",
                i + 1
            )));
        }
        seq = seq.wrapping_add(1);
    }
    if let Err(e) = rom_request(link, &[EOT], ROM_REPLY_TIMEOUT)? {
        return Err(FlashError::Plugin(format!(
            "RTL8711DAF: flash loader EOT failed: {e}"
        )));
    }
    link.sleep(FLOADER_BOOT_DELAY)?;
    link.clear()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::rtl8711daf::io::mock::MockIo;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn baud_index_uses_35_entry_table() {
        assert_eq!(rom_baud_index(115_200), Some(13));
        assert_eq!(rom_baud_index(921_600), Some(20));
        assert_eq!(rom_baud_index(1_500_000), Some(24));
        assert_eq!(rom_baud_index(2_000_000), Some(26));
        assert_eq!(rom_baud_index(1_234_567), None);
    }

    #[test]
    fn nak_stream_detection() {
        assert!(is_nak_stream(&[NAK; 20]));
        assert!(!is_nak_stream(&[NAK]));
        let mut noise = vec![0x51u8; 300];
        noise[7] = NAK;
        noise[90] = NAK;
        assert!(!is_nak_stream(&noise));
    }

    #[test]
    fn stx_packet_layout() {
        let data = [0xABu8; 1024];
        let p = build_stx_packet(1, 0x3000_A000, &data);
        assert_eq!(p.len(), 1032);
        assert_eq!(&p[..7], &[STX, 1, 0xFE, 0x00, 0xA0, 0x00, 0x30]);
        let sum = p[3..1031].iter().fold(0u8, |a, &b| a.wrapping_add(b));
        assert_eq!(p[1031], sum);
        // short last chunk is zero padded
        let p = build_stx_packet(2, 0, &[1, 2, 3]);
        assert_eq!(p.len(), 1032);
        assert_eq!(&p[7..11], &[1, 2, 3, 0]);
    }

    #[test]
    fn floader_image_has_baud_tail_and_padding() {
        let img = build_floader_image(&[0x11; 29024], 1_500_000);
        assert_eq!(img.len(), 29 * 1024);
        assert_eq!(&img[29024..29028], &1_500_000u32.to_le_bytes());
        assert!(img[29028..].iter().all(|&b| b == 0));
        // exact multiple still gets room for the baud word
        assert_eq!(build_floader_image(&[0; 1024], 115_200).len(), 2048);
    }

    #[test]
    fn rom_handshake_switches_baud_after_ack() {
        let cancel = AtomicBool::new(false);
        let mock = MockIo::with_replies(vec![vec![ACK], vec![ACK]]);
        mock.push_rx(&[NAK]);
        let mut link = Link::new(Box::new(mock.clone()), &cancel, HANDSHAKE_BAUD);
        rom_handshake(&mut link, 1_500_000).unwrap().unwrap();
        let s = mock.0.lock().unwrap();
        assert_eq!(s.writes[0], vec![BAUDSET, 24]);
        assert_eq!(s.writes[1], vec![BAUDCHK]);
        assert_eq!(s.bauds, vec![1_500_000]);
        assert_eq!(link.baud(), 1_500_000);
    }

    #[test]
    fn rom_handshake_requires_nak_first() {
        let cancel = AtomicBool::new(false);
        let mock = MockIo::default();
        mock.push_rx(&[0x51]);
        let mut link = Link::new(Box::new(mock), &cancel, HANDSHAKE_BAUD);
        assert!(rom_handshake(&mut link, 1_500_000).unwrap().is_err());
    }

    #[test]
    fn download_floader_sends_all_packets_then_eot() {
        let cancel = AtomicBool::new(false);
        let mock = MockIo::with_replies(vec![vec![ACK]; 4]);
        let mut link = Link::new(Box::new(mock.clone()), &cancel, 1_500_000);
        download_floader(&mut link, &[0x22; 2048]).unwrap();
        let s = mock.0.lock().unwrap();
        assert_eq!(s.writes.len(), 4); // 3 packets (2048 + 4 bytes) + EOT
        assert_eq!(&s.writes[1][3..7], &(FLOADER_ADDR + 1024).to_le_bytes());
        assert_eq!(s.writes[3], vec![EOT]);
    }
}
