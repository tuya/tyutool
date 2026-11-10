//! Flash-loader (floader) protocol, as spoken by Realtek's `floader_amebadplus.bin`
//! once it runs from RAM (mirrors `FloaderHandler.cs` in the vendor ImageTool).
//!
//! Request frame: `[0xA5][len_lo][len_hi][len_lo ^ len_hi][payload..][sum8(payload)]`.
//! Reply: one status byte (`0xB0` ACK, `0xB1` ACK + RX buffer full, `>= 0xE0` error code),
//! or for data-returning commands a frame with the same layout whose payload starts
//! with the echoed opcode.

use std::time::Duration;

use super::io::Link;
use crate::error::FlashError;

pub const SOF: u8 = 0xA5;
pub const ACK: u8 = 0xB0;
pub const ACK_BUF_FULL: u8 = 0xB1;
pub const NEGATIVE_MIN: u8 = 0xE0;

pub const OP_QUERY: u8 = 0x02;
pub const OP_READ: u8 = 0x05;
pub const OP_CHKSM: u8 = 0x06;
pub const OP_SENSE: u8 = 0x07;
pub const OP_NEXTOP: u8 = 0x08;
pub const OP_FS_RDSTS: u8 = 0x21;
pub const OP_CONFIG: u8 = 0x83;
pub const OP_WRITE: u8 = 0x84;
pub const OP_FS_ERASE: u8 = 0xA0;

/// Memory type: NOR flash.
pub const MEM_NOR: u8 = 1;
/// `NEXTOP` type: reset the chip (boots the flashed firmware).
pub const NEXTOP_RESET: u8 = 2;

const RETRIES: usize = 3;
const SYNC_TIMEOUT: Duration = Duration::from_millis(1000);
const WRITE_TIMEOUT: Duration = Duration::from_millis(2000);
const RETRY_INTERVAL: Duration = Duration::from_millis(1);

pub fn sum8(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |a, &b| a.wrapping_add(b))
}

pub fn encode_frame(payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u16;
    let (lo, hi) = ((len & 0xFF) as u8, (len >> 8) as u8);
    let mut f = Vec::with_capacity(payload.len() + 5);
    f.extend_from_slice(&[SOF, lo, hi, lo ^ hi]);
    f.extend_from_slice(payload);
    f.push(sum8(payload));
    f
}

/// Validate a reply frame body (bytes after `0xA5`): 3-byte length header, payload, sum8.
pub fn parse_reply_body(header: &[u8; 3], body: &[u8]) -> Result<Vec<u8>, String> {
    if header[2] != header[0] ^ header[1] {
        return Err(format!("bad length header {:02x?}", header));
    }
    let len = u16::from_le_bytes([header[0], header[1]]) as usize;
    if body.len() != len + 1 {
        return Err(format!("length {len} but got {} bytes", body.len()));
    }
    if sum8(&body[..len]) != body[len] {
        return Err("reply checksum error".into());
    }
    Ok(body[..len].to_vec())
}

/// Sum of little-endian u32 words, wrapping (vendor checksum for CHKSM verification).
/// A trailing partial word contributes `byte[i] << (8 * i)`.
pub fn word_sum(data: &[u8]) -> u32 {
    let mut chunks = data.chunks_exact(4);
    let mut s = 0u32;
    for c in &mut chunks {
        s = s.wrapping_add(u32::from_le_bytes([c[0], c[1], c[2], c[3]]));
    }
    for (i, &b) in chunks.remainder().iter().enumerate() {
        s = s.wrapping_add((b as u32) << (8 * i));
    }
    s
}

/// NOR erase block for the next chunk (vendor `DownloadImage`): 64 KiB when
/// 64K-aligned with >= 64 KiB left, else 32 KiB likewise, else 4 KiB.
pub fn erase_block_size(addr: u32, remaining: u64) -> u32 {
    if addr.is_multiple_of(0x10000) && remaining >= 0x10000 {
        0x10000
    } else if addr.is_multiple_of(0x8000) && remaining >= 0x8000 {
        0x8000
    } else {
        0x1000
    }
}

/// NOR erase time budget per block (FlashUtils.cs constants).
pub fn erase_timeout(block: u32) -> Duration {
    Duration::from_millis(match block {
        0x10000 => 2500,
        0x8000 => 2000,
        _ => 800,
    })
}

fn le32(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SenseStatus {
    pub opcode: u8,
    pub status: u8,
    pub data: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub did: u16,
    pub memory_type: u8,
    pub flash_mid: u8,
    pub flash_did: u16,
    pub page_size: u32,
    pub capacity: u32,
}

/// Parse the 129-byte QUERY reply payload (including the echoed opcode at [0]).
pub fn parse_query(d: &[u8]) -> Result<DeviceInfo, String> {
    if d.len() < 66 || d[0] != OP_QUERY {
        return Err(format!("unexpected QUERY reply ({} bytes)", d.len()));
    }
    let u16_at = |o: usize| u16::from_le_bytes([d[o], d[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
    Ok(DeviceInfo {
        did: u16_at(1),
        memory_type: d[7],
        flash_mid: d[8],
        flash_did: u16_at(9),
        page_size: u32_at(43),
        capacity: u32_at(62),
    })
}

pub struct Floader<'l, 'a> {
    link: &'l mut Link<'a>,
}

impl<'l, 'a> Floader<'l, 'a> {
    pub fn new(link: &'l mut Link<'a>) -> Self {
        Self { link }
    }

    /// Send one request; returns the reply payload (`resp_len` > 0) or an empty vec on ACK.
    fn request(
        &mut self,
        payload: &[u8],
        timeout: Duration,
        resp_len: usize,
    ) -> Result<Vec<u8>, FlashError> {
        let frame = encode_frame(payload);
        let mut last = String::from("no reply");
        for _ in 0..RETRIES {
            self.link.clear()?;
            self.link.write(&frame)?;
            let Some(ch) = self.link.read_byte(timeout)? else {
                last = "timeout".into();
                continue;
            };
            if resp_len == 0 {
                match ch {
                    ACK => return Ok(Vec::new()),
                    ACK_BUF_FULL => {
                        self.link.sleep(RETRY_INTERVAL)?;
                        return Ok(Vec::new());
                    }
                    c if c >= NEGATIVE_MIN => last = format!("error code 0x{c:02x}"),
                    c => last = format!("unexpected reply 0x{c:02x}"),
                }
                self.link.sleep(RETRY_INTERVAL)?;
                continue;
            }
            if ch != SOF {
                last = if ch >= NEGATIVE_MIN {
                    format!("error code 0x{ch:02x}")
                } else {
                    format!("unexpected reply 0x{ch:02x}")
                };
                self.link.sleep(RETRY_INTERVAL)?;
                continue;
            }
            let Some(hdr) = self.link.read_exact(3, SYNC_TIMEOUT)? else {
                last = "reply header timeout".into();
                continue;
            };
            let hdr = [hdr[0], hdr[1], hdr[2]];
            let len = u16::from_le_bytes([hdr[0], hdr[1]]) as usize;
            if len != resp_len {
                last = format!("reply length {len}, expected {resp_len}");
                continue;
            }
            let Some(body) = self.link.read_exact(len + 1, timeout.max(SYNC_TIMEOUT))? else {
                last = "reply payload timeout".into();
                continue;
            };
            match parse_reply_body(&hdr, &body) {
                Ok(p) if p.first() == payload.first() => return Ok(p),
                Ok(p) => last = format!("reply opcode 0x{:02x}", p.first().copied().unwrap_or(0)),
                Err(e) => last = e,
            }
        }
        Err(FlashError::Plugin(format!(
            "RTL8711DAF floader: command 0x{:02x} failed: {last}",
            payload.first().copied().unwrap_or(0)
        )))
    }

    pub fn sense(&mut self) -> Result<SenseStatus, FlashError> {
        let d = self.request(&[OP_SENSE], SYNC_TIMEOUT, 7)?;
        Ok(SenseStatus {
            opcode: d[1],
            status: d[2],
            data: u32::from_le_bytes([d[3], d[4], d[5], d[6]]),
        })
    }

    /// SENSE after an asynchronous WRITE / FS_ERASE: the reported operation must match
    /// and its status must be zero.
    pub fn sense_expect(&mut self, opcode: u8, timeout: Duration) -> Result<(), FlashError> {
        let end = std::time::Instant::now() + timeout;
        loop {
            match self.sense() {
                Ok(s) if s.status != 0 => {
                    return Err(FlashError::Plugin(format!(
                        "RTL8711DAF: operation 0x{:02x} @0x{:08x} failed, status 0x{:02x}",
                        s.opcode, s.data, s.status
                    )))
                }
                Ok(s) if s.opcode != opcode => {
                    return Err(FlashError::Plugin(format!(
                        "RTL8711DAF: SENSE reports op 0x{:02x}, expected 0x{opcode:02x}",
                        s.opcode
                    )))
                }
                Ok(_) => return Ok(()),
                Err(FlashError::Cancelled) => return Err(FlashError::Cancelled),
                Err(e) if std::time::Instant::now() >= end => return Err(e),
                Err(_) => {}
            }
        }
    }

    pub fn query(&mut self) -> Result<DeviceInfo, FlashError> {
        let d = self.request(&[OP_QUERY], SYNC_TIMEOUT, 129)?;
        parse_query(&d).map_err(|e| FlashError::Plugin(format!("RTL8711DAF: {e}")))
    }

    /// CONFIG with the vendor default (all zero).
    pub fn config_default(&mut self) -> Result<(), FlashError> {
        let mut p = vec![OP_CONFIG];
        p.extend_from_slice(&[0u8; 16]);
        self.request(&p, SYNC_TIMEOUT, 0).map(|_| ())
    }

    pub fn read_status_register(&mut self, cmd: u8, addr: u8) -> Result<u8, FlashError> {
        let d = self.request(&[OP_FS_RDSTS, cmd, addr], SYNC_TIMEOUT, 2)?;
        Ok(d[1])
    }

    pub fn read(&mut self, addr: u32, size: u32) -> Result<Vec<u8>, FlashError> {
        let mut p = vec![OP_READ, MEM_NOR];
        p.extend_from_slice(&le32(addr));
        p.extend_from_slice(&le32(size));
        let timeout = Duration::from_millis((size as u64 / 1024).max(1000));
        let d = self.request(&p, timeout, size as usize + 1)?;
        Ok(d[1..].to_vec())
    }

    /// One page write; the device acknowledges reception, completion is checked via SENSE.
    pub fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        let mut p = Vec::with_capacity(6 + data.len());
        p.extend_from_slice(&[OP_WRITE, MEM_NOR]);
        p.extend_from_slice(&le32(addr));
        p.extend_from_slice(data);
        self.request(&p, WRITE_TIMEOUT, 0).map(|_| ())
    }

    pub fn erase(&mut self, start: u32, size: u32) -> Result<(), FlashError> {
        let mut p = vec![OP_FS_ERASE, MEM_NOR, 0];
        p.extend_from_slice(&le32(start));
        p.extend_from_slice(&le32(start.wrapping_add(size)));
        p.extend_from_slice(&le32(size));
        self.request(&p, SYNC_TIMEOUT, 0).map(|_| ())
    }

    pub fn checksum(&mut self, start: u32, end: u32, size: u32) -> Result<u32, FlashError> {
        let mut p = vec![OP_CHKSM, MEM_NOR];
        p.extend_from_slice(&le32(start));
        p.extend_from_slice(&le32(end));
        p.extend_from_slice(&le32(size));
        let timeout = Duration::from_millis((size as u64 / 1024).max(1000) + 1000);
        let d = self.request(&p, timeout, 5)?;
        Ok(u32::from_le_bytes([d[1], d[2], d[3], d[4]]))
    }

    pub fn reset(&mut self) -> Result<(), FlashError> {
        self.request(&[OP_NEXTOP, NEXTOP_RESET, 0, 0, 0, 0], SYNC_TIMEOUT, 0)
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::rtl8711daf::io::mock::MockIo;
    use std::sync::atomic::AtomicBool;

    fn reply_frame(payload: &[u8]) -> Vec<u8> {
        encode_frame(payload)
    }

    #[test]
    fn encode_frame_layout() {
        let f = encode_frame(&[OP_SENSE]);
        assert_eq!(f, vec![0xA5, 0x01, 0x00, 0x01, 0x07, 0x07]);
        let big = vec![0x11u8; 0x406];
        let f = encode_frame(&big);
        assert_eq!(&f[..4], &[0xA5, 0x06, 0x04, 0x02]);
        assert_eq!(f.len(), 0x406 + 5);
        assert_eq!(*f.last().unwrap(), sum8(&big));
    }

    #[test]
    fn parse_reply_rejects_bad_header_and_checksum() {
        assert!(parse_reply_body(&[0x02, 0x00, 0x03], &[1, 2, 3]).is_err());
        assert!(parse_reply_body(&[0x02, 0x00, 0x02], &[1, 2, 0]).is_err());
        assert!(parse_reply_body(&[0x02, 0x00, 0x02], &[1, 2]).is_err());
        assert_eq!(
            parse_reply_body(&[0x02, 0x00, 0x02], &[1, 2, 3]).unwrap(),
            vec![1, 2]
        );
    }

    #[test]
    fn word_sum_matches_vendor() {
        assert_eq!(
            word_sum(&[0x99, 0x99, 0x96, 0x96, 0x3f, 0xcc, 0x66, 0xfc]),
            0x9696_9999u32.wrapping_add(0xfc66_cc3f)
        );
        assert_eq!(word_sum(&[0xff; 4096]), 0xffff_fc00);
        assert_eq!(word_sum(&[1, 2, 3]), 1 + (2 << 8) + (3 << 16));
    }

    #[test]
    fn erase_block_selection() {
        assert_eq!(erase_block_size(0x0, 0x20000), 0x10000);
        assert_eq!(erase_block_size(0x0, 0xFFFF), 0x8000);
        assert_eq!(erase_block_size(0x8000, 0x20000), 0x8000);
        assert_eq!(erase_block_size(0x14000, 0x100000), 0x1000);
        assert_eq!(erase_block_size(0x10000, 0x7FFF), 0x1000);
    }

    #[test]
    fn query_reply_parsed() {
        let mut d = vec![0u8; 129];
        d[0] = OP_QUERY;
        d[1..3].copy_from_slice(&0x6845u16.to_le_bytes());
        d[7] = 1;
        d[8] = 0x85;
        d[9..11].copy_from_slice(&0x1620u16.to_le_bytes());
        d[43..47].copy_from_slice(&1024u32.to_le_bytes());
        d[62..66].copy_from_slice(&0x40_0000u32.to_le_bytes());
        let i = parse_query(&d).unwrap();
        assert_eq!(
            (
                i.did,
                i.memory_type,
                i.flash_mid,
                i.flash_did,
                i.page_size,
                i.capacity
            ),
            (0x6845, 1, 0x85, 0x1620, 1024, 0x40_0000)
        );
    }

    #[test]
    fn request_ack_and_data_reply() {
        let cancel = AtomicBool::new(false);
        let mock = MockIo::with_replies(vec![
            vec![ACK],
            reply_frame(&[OP_CHKSM, 0x5b, 0x2c, 0x3a, 0xbc]),
        ]);
        let mut link = Link::new(Box::new(mock.clone()), &cancel, 1_500_000);
        let mut f = Floader::new(&mut link);
        f.erase(0x0820_0000, 0x1000).unwrap();
        assert_eq!(
            f.checksum(0x0800_0000, 0x0800_2000, 0x2000).unwrap(),
            0xbc3a_2c5b
        );
        let w = &mock.0.lock().unwrap().writes;
        assert_eq!(w[0][4], OP_FS_ERASE);
        assert_eq!(&w[0][7..11], &0x0820_0000u32.to_le_bytes());
        assert_eq!(&w[0][11..15], &0x0820_1000u32.to_le_bytes());
    }

    #[test]
    fn request_retries_on_error_code_then_fails() {
        let cancel = AtomicBool::new(false);
        let mock = MockIo::with_replies(vec![vec![0xE4], vec![0xE4], vec![0xE4]]);
        let mut link = Link::new(Box::new(mock.clone()), &cancel, 1_500_000);
        let err = Floader::new(&mut link).reset().unwrap_err();
        assert!(err.to_string().contains("0xe4"), "{err}");
        assert_eq!(mock.0.lock().unwrap().writes.len(), 3);
    }

    #[test]
    fn sense_expect_checks_status_and_opcode() {
        let cancel = AtomicBool::new(false);
        let ok = reply_frame(&[OP_SENSE, OP_WRITE, 0, 0, 0, 0, 0]);
        let bad = reply_frame(&[OP_SENSE, OP_WRITE, 0x33, 0, 0, 0, 0]);
        let mock = MockIo::with_replies(vec![ok, bad]);
        let mut link = Link::new(Box::new(mock), &cancel, 1_500_000);
        let mut f = Floader::new(&mut link);
        f.sense_expect(OP_WRITE, Duration::from_millis(100))
            .unwrap();
        assert!(f
            .sense_expect(OP_WRITE, Duration::from_millis(100))
            .is_err());
    }
}
