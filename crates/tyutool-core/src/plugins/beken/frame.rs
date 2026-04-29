//! Beken UART frame encoding / decoding.
//!
//! Two frame types exist on the wire:
//!
//! **Standard (short) frame** — used for most control commands:
//! ```text
//! TX: [01] [e0] [fc] [LEN] [CMD] [PAYLOAD…]
//!      LEN = 1 + payload.len()   (counts CMD byte + payload)
//!
//! RX: [04] [0e] [LEN] [01] [e0] [fc] [CMD+1] [STATUS] [DATA…]
//!      LEN = 3 + 1 + 1 + data.len()   (counts echo header + CMD + STATUS + data)
//! ```
//!
//! **Extended (long) frame** — used for 4 KiB flash read/write and some T5 ops:
//! ```text
//! TX: [01] [e0] [fc] [ff] [f4] [LEN_L] [LEN_H] [CMD] [PAYLOAD…]
//!      LEN = 1 + payload.len()   (LE u16, counts CMD byte + payload)
//!
//! RX: [04] [0e] [ff] [01] [e0] [fc] [f4] [LEN_L] [LEN_H] [CMD] [STATUS] [DATA…]
//!      LEN = 1 + 1 + data.len()   (LE u16, counts CMD + STATUS + data)
//! ```

use std::fmt;

// ── TX magic bytes ──────────────────────────────────────────────────────
const TX_MAGIC: [u8; 3] = [0x01, 0xe0, 0xfc];
const TX_EXT_MARKER: [u8; 2] = [0xff, 0xf4]; // appended after TX_MAGIC for extended

// ── RX magic bytes ──────────────────────────────────────────────────────
const RX_MAGIC: [u8; 2] = [0x04, 0x0e];
const RX_ECHO: [u8; 3] = [0x01, 0xe0, 0xfc]; // echo of TX header inside RX
const RX_EXT_TAG: u8 = 0xf4;

// ── Minimum frame sizes ─────────────────────────────────────────────────
/// Minimum standard RX frame: 04 0e LEN 01 e0 fc CMD STATUS  →  8 bytes
const RX_STD_MIN: usize = 8;
/// Minimum extended RX frame: 04 0e ff 01 e0 fc f4 LEN_L LEN_H CMD STATUS  →  11 bytes
const RX_EXT_MIN: usize = 11;

// ─────────────────────────────────────────────────────────────────────────
// Protocol error
// ─────────────────────────────────────────────────────────────────────────

/// Low-level protocol error (internal to beken layer; converted to [`FlashError`] at plugin boundary).
#[derive(Debug)]
pub enum ProtocolError {
    /// Serial / filesystem I/O.
    Io(std::io::Error),
    /// Timeout waiting for device response.
    Timeout { attempts: u32 },
    /// Received frame has wrong magic bytes.
    BadMagic(u8),
    /// Device returned non-zero status byte.
    DeviceError(u8),
    /// CRC mismatch between host and device.
    CrcMismatch { expected: u32, got: u32 },
    /// Flash MID not found in the built-in table.
    UnknownFlashMid(u32),
    /// User pressed cancel.
    Cancelled,
    /// Generic protocol-level error message.
    Protocol(String),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "serial I/O: {e}"),
            Self::Timeout { attempts } => write!(f, "timeout after {attempts} attempts"),
            Self::BadMagic(b) => write!(f, "bad frame magic: {b:#04x}"),
            Self::DeviceError(s) => write!(f, "device returned error status: {s:#04x}"),
            Self::CrcMismatch { expected, got } => {
                write!(f, "CRC mismatch: expected={expected:#010x} got={got:#010x}")
            }
            Self::UnknownFlashMid(mid) => write!(f, "flash MID {mid:#08x} not in table"),
            Self::Cancelled => write!(f, "operation cancelled"),
            Self::Protocol(msg) => write!(f, "protocol: {msg}"),
        }
    }
}

impl std::error::Error for ProtocolError {}

impl From<std::io::Error> for ProtocolError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serialport::Error> for ProtocolError {
    fn from(e: serialport::Error) -> Self {
        Self::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            e.to_string(),
        ))
    }
}

// ─────────────────────────────────────────────────────────────────────────
// RxFrame — decoded response
// ─────────────────────────────────────────────────────────────────────────

/// A decoded RX frame from the device.
#[derive(Debug, Clone)]
pub struct RxFrame {
    /// Command byte from the response (usually CMD+1 relative to the request).
    pub cmd: u8,
    /// Status byte (0x00 = success).
    pub status: u8,
    /// Payload data after the status byte.
    pub data: Vec<u8>,
    /// `true` if this was an extended (long) frame.
    pub extended: bool,
}

// ─────────────────────────────────────────────────────────────────────────
// Encoding (TX)
// ─────────────────────────────────────────────────────────────────────────

/// Encode a **standard** (short) TX frame.
///
/// ```text
/// [01 e0 fc] [LEN] [CMD] [PAYLOAD…]
/// LEN = 1 + payload.len()
/// ```
pub fn encode_standard(cmd: u8, payload: &[u8]) -> Vec<u8> {
    let len = 1u8.wrapping_add(payload.len() as u8); // CMD + payload
    let mut buf = Vec::with_capacity(4 + payload.len());
    buf.extend_from_slice(&TX_MAGIC);
    buf.push(len);
    buf.push(cmd);
    buf.extend_from_slice(payload);
    buf
}

/// Encode an **extended** (long) TX frame.
///
/// ```text
/// [01 e0 fc] [ff f4] [LEN_L LEN_H] [CMD] [PAYLOAD…]
/// LEN = 1 + payload.len()   (LE u16)
/// ```
pub fn encode_extended(cmd: u8, payload: &[u8]) -> Vec<u8> {
    let len = (1 + payload.len()) as u16; // CMD + payload
    let mut buf = Vec::with_capacity(7 + payload.len());
    buf.extend_from_slice(&TX_MAGIC);
    buf.extend_from_slice(&TX_EXT_MARKER);
    buf.push((len & 0xff) as u8);
    buf.push(((len >> 8) & 0xff) as u8);
    buf.push(cmd);
    buf.extend_from_slice(payload);
    buf
}

// ─────────────────────────────────────────────────────────────────────────
// Decoding (RX)
// ─────────────────────────────────────────────────────────────────────────

/// Try to decode one RX frame from the front of `buf`.
///
/// Returns `Some((frame, consumed_bytes))` on success, or `None` if the
/// buffer does not yet contain a complete frame.
///
/// The caller must manage the receive buffer: on `Some` drain `consumed`
/// bytes; on `None` read more bytes from the serial port and retry.
pub fn decode(buf: &[u8]) -> Option<(RxFrame, usize)> {
    if buf.len() < 2 {
        return None;
    }

    // Both standard and extended start with [04 0e].
    if buf[0] != RX_MAGIC[0] || buf[1] != RX_MAGIC[1] {
        // Try to skip garbage and find the next magic
        if let Some(pos) = find_rx_magic(buf) {
            // Return a synthetic "skip" — caller should drain `pos` bytes then retry
            return Some((
                RxFrame {
                    cmd: 0,
                    status: 0xff,
                    data: Vec::new(),
                    extended: false,
                },
                pos,
            ));
        }
        return None;
    }

    // ── Extended frame? ─────────────────────────────────────────────
    // [04 0e ff 01 e0 fc f4 LEN_L LEN_H CMD STATUS DATA…]
    if buf.len() >= 7 && buf[2] == 0xff && buf[3..6] == RX_ECHO && buf[6] == RX_EXT_TAG {
        if buf.len() < RX_EXT_MIN {
            return None; // need more data
        }
        let payload_len = (buf[7] as u16) | ((buf[8] as u16) << 8);
        // Total frame = 9 (fixed header) + payload_len
        let total = 9 + payload_len as usize;
        if buf.len() < total {
            return None;
        }
        let cmd = buf[9];
        let status = buf[10];
        let data = buf[11..total].to_vec();
        return Some((
            RxFrame {
                cmd,
                status,
                data,
                extended: true,
            },
            total,
        ));
    }

    // ── Standard frame ──────────────────────────────────────────────
    // [04 0e LEN 01 e0 fc CMD STATUS DATA…]
    if buf.len() < 3 {
        return None;
    }
    let len = buf[2] as usize;
    // Total frame = 3 + len  (header [04 0e LEN] + len bytes)
    let total = 3 + len;
    if buf.len() < total {
        return None;
    }
    // Verify echo header [01 e0 fc] at positions [3..6]
    if total < RX_STD_MIN || buf[3..6] != RX_ECHO {
        // Malformed — skip this byte
        return Some((
            RxFrame {
                cmd: 0,
                status: 0xff,
                data: Vec::new(),
                extended: false,
            },
            1,
        ));
    }
    let cmd = buf[6];
    let status = if total > 7 { buf[7] } else { 0 };
    let data = if total > 8 {
        buf[8..total].to_vec()
    } else {
        Vec::new()
    };
    Some((
        RxFrame {
            cmd,
            status,
            data,
            extended: false,
        },
        total,
    ))
}

/// Find the byte offset of the next `[04 0e]` magic in `buf`, skipping position 0.
fn find_rx_magic(buf: &[u8]) -> Option<usize> {
    for i in 1..buf.len().saturating_sub(1) {
        if buf[i] == RX_MAGIC[0] && buf[i + 1] == RX_MAGIC[1] {
            return Some(i);
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_standard_link_check() {
        // LinkCheck: CMD=0x00, no payload → [01 e0 fc 01 00]
        let frame = encode_standard(0x00, &[]);
        assert_eq!(frame, vec![0x01, 0xe0, 0xfc, 0x01, 0x00]);
    }

    #[test]
    fn encode_standard_set_baud() {
        // SetBaudRate 921600=0x000e1000: CMD=0x0f, payload=[00 10 0e 00 64]
        let baud: u32 = 921600;
        let dly: u8 = 100;
        let mut payload = baud.to_le_bytes().to_vec();
        payload.push(dly);
        let frame = encode_standard(0x0f, &payload);
        assert_eq!(frame.len(), 4 + 1 + 5); // header(4) + cmd already in len
        assert_eq!(frame[0..3], TX_MAGIC);
        assert_eq!(frame[3], 6); // LEN = 1(CMD) + 5(payload)
        assert_eq!(frame[4], 0x0f);
        assert_eq!(&frame[5..9], &baud.to_le_bytes());
        assert_eq!(frame[9], dly);
    }

    #[test]
    fn encode_extended_write_4k() {
        let data = [0xABu8; 4096];
        let addr: u32 = 0x0001_0000;
        let mut payload = addr.to_le_bytes().to_vec();
        payload.extend_from_slice(&data);
        let frame = encode_extended(0x07, &payload);
        // header: 3(magic) + 2(ext marker) + 2(len) + 1(cmd) = 8
        assert_eq!(frame.len(), 8 + payload.len());
        assert_eq!(frame[0..3], TX_MAGIC);
        assert_eq!(frame[3..5], TX_EXT_MARKER);
        let len = (frame[5] as u16) | ((frame[6] as u16) << 8);
        assert_eq!(len as usize, 1 + payload.len()); // CMD + payload
        assert_eq!(frame[7], 0x07); // CMD
    }

    #[test]
    fn decode_standard_link_check_response() {
        // Expected LinkCheck response: [04 0e 05 01 e0 fc 01 00]
        let buf = [0x04, 0x0e, 0x05, 0x01, 0xe0, 0xfc, 0x01, 0x00];
        let (frame, consumed) = decode(&buf).unwrap();
        assert_eq!(consumed, 8); // 3 + 5 = 8
        assert_eq!(frame.cmd, 0x01); // LinkCheck reply = CMD+1
        assert_eq!(frame.status, 0x00);
        assert!(frame.data.is_empty());
        assert!(!frame.extended);
    }

    #[test]
    fn decode_standard_crc_response() {
        // CheckCRC response: [04 0e 08 01 e0 fc 10 CRC0 CRC1 CRC2 CRC3]
        // CRC = 0xDEADBEEF → LE = [EF BE AD DE]
        let buf = [
            0x04, 0x0e, 0x08, 0x01, 0xe0, 0xfc, 0x10, 0xEF, 0xBE, 0xAD, 0xDE,
        ];
        let (frame, consumed) = decode(&buf).unwrap();
        assert_eq!(consumed, 11);
        assert_eq!(frame.cmd, 0x10);
        // status is at buf[7] = 0xEF — but for CRC response the layout is:
        // [04 0e 08 01 e0 fc CMD data...]  where data starts right after CMD
        // In our standard decode: cmd=buf[6], status=buf[7], data=buf[8..total]
        assert_eq!(frame.status, 0xEF); // first byte of CRC treated as status
        assert_eq!(frame.data, vec![0xBE, 0xAD, 0xDE]);
    }

    #[test]
    fn decode_extended_write_response() {
        // Extended response for FlashWrite4K:
        // [04 0e ff 01 e0 fc f4 06 00 07 00 00 00 01 00]
        // LEN=6 → CMD(07) + STATUS(00) + DATA(4 bytes addr echo)
        let buf = [
            0x04, 0x0e, 0xff, 0x01, 0xe0, 0xfc, 0xf4, 0x06, 0x00, 0x07, 0x00, 0x00, 0x00, 0x01,
            0x00,
        ];
        let (frame, consumed) = decode(&buf).unwrap();
        assert_eq!(consumed, 15); // 9 + 6 = 15
        assert!(frame.extended);
        assert_eq!(frame.cmd, 0x07);
        assert_eq!(frame.status, 0x00);
        assert_eq!(frame.data, vec![0x00, 0x00, 0x01, 0x00]);
    }

    #[test]
    fn decode_partial_returns_none() {
        let partial = [0x04, 0x0e, 0x05];
        assert!(decode(&partial).is_none());
    }

    #[test]
    fn decode_empty_returns_none() {
        assert!(decode(&[]).is_none());
    }

    #[test]
    fn decode_handles_garbage_prefix() {
        // Some garbage then a valid frame
        let mut buf = vec![0xAA, 0xBB];
        buf.extend_from_slice(&[0x04, 0x0e, 0x05, 0x01, 0xe0, 0xfc, 0x01, 0x00]);
        let (frame, consumed) = decode(&buf).unwrap();
        // Should skip to position 2 (the garbage)
        assert_eq!(consumed, 2);
        assert_eq!(frame.status, 0xff); // synthetic skip frame

        // Now decode the remaining valid frame
        let (frame2, consumed2) = decode(&buf[consumed..]).unwrap();
        assert_eq!(consumed2, 8);
        assert_eq!(frame2.cmd, 0x01);
        assert_eq!(frame2.status, 0x00);
    }
}
