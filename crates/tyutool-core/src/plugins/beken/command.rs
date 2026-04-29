//! Beken UART command codes, payload builders, and response parsers.
//!
//! Reference: Python `tyutool/flash/bk7231n/protocol.py` CMD class.
//!
//! Several command codes are overloaded — the device disambiguates by payload
//! content and context (e.g. `0x0e` is both `FlashGetMID` and `Reboot`,
//! `0x0f` is both `SetBaudRate` and `FlashErase`).

// ─────────────────────────────────────────────────────────────────────────
// Command opcodes
// ─────────────────────────────────────────────────────────────────────────

/// Link-check / heartbeat.
pub const CMD_LINK_CHECK: u8 = 0x00;

/// Write 32-bit register.
pub const CMD_WRITE_REG: u8 = 0x01;

/// Read 32-bit register (also used for T5 `GetChipID`).
pub const CMD_READ_REG: u8 = 0x03;

/// Write up to 256 bytes to flash (legacy, not used in 4K path).
#[allow(dead_code)]
pub const CMD_FLASH_WRITE: u8 = 0x06;

/// Write 4 KiB to flash (extended frame).
pub const CMD_FLASH_WRITE_4K: u8 = 0x07;

/// Read up to 256 bytes from flash (legacy).
#[allow(dead_code)]
pub const CMD_FLASH_READ: u8 = 0x08;

/// Read 4 KiB from flash (extended frame).
pub const CMD_FLASH_READ_4K: u8 = 0x09;

/// Erase entire flash chip.
#[allow(dead_code)]
pub const CMD_FLASH_ERASE_ALL: u8 = 0x0a;

/// Erase a single 4 KiB sector (by sector command code embedded in payload).
pub const CMD_FLASH_ERASE_SECTOR: u8 = 0x0b;

/// Read flash Status Register.
pub const CMD_FLASH_READ_SR: u8 = 0x0c;

/// Write flash Status Register.
pub const CMD_FLASH_WRITE_SR: u8 = 0x0d;

/// Get flash JEDEC MID **or** Reboot (disambiguated by payload).
///
/// - `FlashGetMID`: payload = `[]` (empty)
/// - `Reboot`:      payload = `[0xa5]`
pub const CMD_FLASH_GET_MID: u8 = 0x0e;

/// Set UART baud rate **or** Flash erase block (disambiguated by payload).
///
/// - `SetBaudRate`:  payload = `[baud_le(4), delay_ms(1)]`
/// - `FlashErase`:   payload = `[erase_cmd, addr_le(4)]` (used for 4K/64K erase)
pub const CMD_SET_BAUD_RATE: u8 = 0x0f;

/// CRC-32 check over a flash region.
pub const CMD_CHECK_CRC: u8 = 0x10;

/// Software reset (BKRegDoReboot).
pub const CMD_RESET: u8 = 0xfe;

// ─────────────────────────────────────────────────────────────────────────
// Flash erase sub-commands (sent as first byte of CMD_SET_BAUD_RATE payload)
// ─────────────────────────────────────────────────────────────────────────

/// Erase 4 KiB sector.
pub const ERASE_CMD_4K: u8 = 0x20;
/// Erase 32 KiB block.
#[allow(dead_code)]
pub const ERASE_CMD_32K: u8 = 0x52;
/// Erase 64 KiB block.
pub const ERASE_CMD_64K: u8 = 0xd8;

// T5 extended erase commands (for >16 MiB flash)
/// Extended erase 4 KiB sector.
pub const ERASE_CMD_4K_EXT: u8 = 0x21;
/// Extended erase 64 KiB block.
pub const ERASE_CMD_64K_EXT: u8 = 0xdc;

/// Reboot marker payload byte for CMD 0x0e.
pub const REBOOT_PAYLOAD: u8 = 0xa5;

// ─────────────────────────────────────────────────────────────────────────
// Payload builders
// ─────────────────────────────────────────────────────────────────────────

/// Namespace for payload-building functions.
///
/// Each function returns only the **payload** bytes; the caller wraps them
/// in a standard or extended frame via [`super::frame::encode_standard`] /
/// [`encode_extended`].
pub mod build {
    use super::*;

    /// LinkCheck — empty payload.
    pub fn link_check() -> Vec<u8> {
        Vec::new()
    }

    /// SetBaudRate — `[baud_le(4), delay_ms(1)]`.
    pub fn set_baud_rate(baud: u32, delay_ms: u8) -> Vec<u8> {
        let mut v = baud.to_le_bytes().to_vec();
        v.push(delay_ms);
        v
    }

    /// FlashGetMID — empty payload (cmd 0x0e with no 0xa5).
    pub fn flash_get_mid() -> Vec<u8> {
        Vec::new()
    }

    /// FlashGetMID (extended, T5) — `[reg_addr, 0, 0, 0]`.
    /// `reg_addr` is typically `0x9f` (JEDEC Read-ID SPI command).
    pub fn flash_get_mid_ext(reg_addr: u8) -> Vec<u8> {
        vec![reg_addr, 0x00, 0x00, 0x00]
    }

    /// FlashReadSR — `[sr_cmd]` where `sr_cmd` is the SPI read-SR command
    /// (e.g. `0x05` for SR1, `0x35` for SR2).
    pub fn flash_read_sr(sr_cmd: u8) -> Vec<u8> {
        vec![sr_cmd]
    }

    /// FlashWriteSR — `[sr_cmd, value(s)…]`.
    /// For single-byte SR: `values = &[val]`.
    /// For two-byte SR:     `values = &[val_lo, val_hi]`.
    pub fn flash_write_sr(sr_cmd: u8, values: &[u8]) -> Vec<u8> {
        let mut v = vec![sr_cmd];
        v.extend_from_slice(values);
        v
    }

    /// Flash erase (4K or 64K) — sent via CMD 0x0f with erase sub-command.
    /// `erase_cmd` is one of `ERASE_CMD_4K`, `ERASE_CMD_64K`, etc.
    pub fn flash_erase(erase_cmd: u8, addr: u32) -> Vec<u8> {
        let mut v = vec![erase_cmd];
        v.extend_from_slice(&addr.to_le_bytes());
        v
    }

    /// FlashWrite4K — `[addr_le(4), data(4096)]`.
    /// Payload for an **extended** frame.
    pub fn flash_write_4k(addr: u32, data: &[u8]) -> Vec<u8> {
        let mut v = Vec::with_capacity(4 + data.len());
        v.extend_from_slice(&addr.to_le_bytes());
        v.extend_from_slice(data);
        v
    }

    /// FlashRead4K — `[addr_le(4)]`.
    /// Payload for an **extended** frame.
    pub fn flash_read_4k(addr: u32) -> Vec<u8> {
        addr.to_le_bytes().to_vec()
    }

    /// CheckCRC — `[start_addr_le(4), end_addr_le(4)]`.
    /// Note: `end_addr` is **inclusive** (last byte address).
    pub fn check_crc(start_addr: u32, end_addr_inclusive: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity(8);
        v.extend_from_slice(&start_addr.to_le_bytes());
        v.extend_from_slice(&end_addr_inclusive.to_le_bytes());
        v
    }

    /// Reboot (via CMD 0x0e with marker 0xa5).
    pub fn reboot() -> Vec<u8> {
        vec![REBOOT_PAYLOAD]
    }

    /// BKRegDoReboot (software reset via CMD 0xfe).
    ///
    /// Payload is `[0x95, 0x27, 0x95, 0x27]` — the magic reboot key
    /// expected by the BK7231N/T5 bootrom.
    pub fn bk_reg_do_reboot() -> Vec<u8> {
        vec![0x95, 0x27, 0x95, 0x27]
    }

    /// ReadReg — `[addr_le(4)]` (used for T5 GetChipID).
    pub fn read_reg(addr: u32) -> Vec<u8> {
        addr.to_le_bytes().to_vec()
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Response parsers
// ─────────────────────────────────────────────────────────────────────────

/// Namespace for parsing device response payloads.
pub mod parse {
    use super::super::frame::ProtocolError;

    /// Parse a FlashGetMID response (extended frame, T5).
    ///
    /// Extended frame: `[04 0e ff 01 e0 fc f4 LEN_L LEN_H CMD STATUS DATA…]`
    /// In our `RxFrame`: cmd=0x0e, status=0x00, data=[mid0, mid1, mid2, mid3].
    /// MID = data[0] | (data[1] << 8) | (data[2] << 16).
    /// (data[3] is typically 0x00; the Python version does `>> 8` on a u32.)
    pub fn flash_mid_from_extended(data: &[u8]) -> Result<u32, ProtocolError> {
        if data.len() < 4 {
            return Err(ProtocolError::Protocol(format!(
                "FlashGetMID extended response too short: data len = {}",
                data.len()
            )));
        }
        // Python: struct.unpack("<I", content[11:])[0] >> 8
        // That reads 4 bytes as LE u32 then shifts right 8 bits
        let raw = (data[0] as u32)
            | ((data[1] as u32) << 8)
            | ((data[2] as u32) << 16)
            | ((data[3] as u32) << 24);
        Ok(raw >> 8)
    }
    ///
    /// Standard frame layout: `[04 0e LEN 01 e0 fc CMD MID0 MID1 MID2]`
    /// where CMD = 0x0e, and `data` = raw bytes after the echo header.
    ///
    /// In our `RxFrame`, status = MID byte 0, data = [MID1, MID2].
    /// So MID (LE) = status | (data[0] << 8) | (data[1] << 16).
    pub fn flash_mid_from_standard(status: u8, data: &[u8]) -> Result<u32, ProtocolError> {
        if data.len() < 2 {
            return Err(ProtocolError::Protocol(format!(
                "FlashGetMID response too short: data len = {}",
                data.len()
            )));
        }
        let mid = (status as u32) | ((data[0] as u32) << 8) | ((data[1] as u32) << 16);
        Ok(mid)
    }

    /// Parse a FlashReadSR response (standard frame).
    ///
    /// Standard frame: `CMD=0x0c`, after echo header → status, then SR
    /// value byte(s).
    ///
    /// In our `RxFrame`: `status` = reg_addr echo, `data[0]` = SR value.
    pub fn flash_sr_from_standard(data: &[u8]) -> Result<u8, ProtocolError> {
        if data.is_empty() {
            return Err(ProtocolError::Protocol(
                "FlashReadSR response has no SR value".into(),
            ));
        }
        Ok(data[0])
    }

    /// Parse a FlashReadSR response (extended frame, T5).
    ///
    /// Extended frame response: `[04 0e ff 01 e0 fc f4 LEN_L LEN_H CMD STATUS DATA…]`
    /// In our `RxFrame`: cmd=0x0c, status=0x00, data=[reg_cmd_echo, sr_value].
    pub fn flash_sr_from_extended(data: &[u8]) -> Result<u8, ProtocolError> {
        // data should contain at least [reg_cmd_echo, sr_value]
        if data.len() < 2 {
            return Err(ProtocolError::Protocol(format!(
                "FlashReadSR extended response too short: data len = {}",
                data.len()
            )));
        }
        Ok(data[1])
    }

    /// Parse a CheckCRC response — extract the CRC-32 value.
    ///
    /// Standard frame: `[04 0e 08 01 e0 fc 10 CRC0 CRC1 CRC2 CRC3]`
    /// In our `RxFrame`: cmd=0x10, status=CRC0, data=[CRC1, CRC2, CRC3].
    pub fn crc32_from_standard(status: u8, data: &[u8]) -> Result<u32, ProtocolError> {
        if data.len() < 3 {
            return Err(ProtocolError::Protocol(format!(
                "CheckCRC response too short: data len = {}",
                data.len()
            )));
        }
        let crc = (status as u32)
            | ((data[0] as u32) << 8)
            | ((data[1] as u32) << 16)
            | ((data[2] as u32) << 24);
        Ok(crc)
    }

    /// Parse a ReadReg (T5 GetChipID) response.
    ///
    /// Standard frame: `[04 0e LEN 01 e0 fc 03 regB0..B3(4) chipB0..B3(4)]`
    /// In our `RxFrame`: cmd=0x03, status=regB0, data=[regB1..B3, chipB0..B3].
    pub fn chip_id_from_read_reg(data: &[u8]) -> Result<u32, ProtocolError> {
        // data should have at least 7 bytes: regB1..B3(3) + chipB0..B3(4)
        if data.len() < 7 {
            return Err(ProtocolError::Protocol(format!(
                "ReadReg response too short for ChipID: data len = {}",
                data.len()
            )));
        }
        // ChipID is at data[3..7] (after the remaining 3 reg addr bytes)
        let chip_id = (data[3] as u32)
            | ((data[4] as u32) << 8)
            | ((data[5] as u32) << 16)
            | ((data[6] as u32) << 24);
        Ok(chip_id)
    }

    /// Parse a FlashRead4K response (extended frame).
    ///
    /// Extended frame response: `[04 0e ff 01 e0 fc f4 LEN_L LEN_H CMD STATUS ADDR(4) DATA(4096)]`
    /// In our `RxFrame`: cmd=0x09, status=byte, data=[addr(4), flash_data(4096)].
    /// Returns the 4K flash data (skipping the 4-byte address echo).
    ///
    /// Python reference: `CheckRespond_FlashRead4K` returns `buf[15:]` which is
    /// the flash data after the 4 address bytes + 1 status byte.
    pub fn flash_read_4k_data(data: &[u8]) -> Result<&[u8], ProtocolError> {
        // data should have at least 4 (addr) + some flash bytes
        if data.len() < 4 {
            return Err(ProtocolError::Protocol(format!(
                "FlashRead4K response too short: data len = {}",
                data.len()
            )));
        }
        // Skip the 4-byte address echo, return the flash data
        Ok(&data[4..])
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_link_check_is_empty() {
        assert!(build::link_check().is_empty());
    }

    #[test]
    fn build_set_baud_rate_format() {
        let payload = build::set_baud_rate(921600, 100);
        assert_eq!(payload.len(), 5);
        let baud = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        assert_eq!(baud, 921600);
        assert_eq!(payload[4], 100);
    }

    #[test]
    fn build_flash_erase_4k() {
        let payload = build::flash_erase(ERASE_CMD_4K, 0x0001_0000);
        assert_eq!(payload[0], ERASE_CMD_4K);
        let addr = u32::from_le_bytes([payload[1], payload[2], payload[3], payload[4]]);
        assert_eq!(addr, 0x0001_0000);
    }

    #[test]
    fn build_flash_write_4k_format() {
        let data = [0x55u8; 4096];
        let payload = build::flash_write_4k(0x0002_0000, &data);
        assert_eq!(payload.len(), 4 + 4096);
        let addr = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        assert_eq!(addr, 0x0002_0000);
        assert_eq!(payload[4], 0x55);
    }

    #[test]
    fn build_check_crc_format() {
        let payload = build::check_crc(0x0000_0000, 0x0000_0FFF);
        assert_eq!(payload.len(), 8);
        let start = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        let end = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
        assert_eq!(start, 0);
        assert_eq!(end, 0x0FFF);
    }

    #[test]
    fn build_reboot_has_marker() {
        let payload = build::reboot();
        assert_eq!(payload, vec![REBOOT_PAYLOAD]);
    }

    #[test]
    fn parse_flash_mid() {
        // MID = 0x1440C8 (GD25D80, LE in response: C8 40 14)
        // RxFrame: status=0xC8, data=[0x40, 0x14]
        let mid = parse::flash_mid_from_standard(0xC8, &[0x40, 0x14]).unwrap();
        assert_eq!(mid, 0x1440C8);
    }

    #[test]
    fn parse_crc32_value() {
        // CRC = 0xDEADBEEF → LE: EF BE AD DE
        // RxFrame: status=0xEF, data=[0xBE, 0xAD, 0xDE]
        let crc = parse::crc32_from_standard(0xEF, &[0xBE, 0xAD, 0xDE]).unwrap();
        assert_eq!(crc, 0xDEAD_BEEF);
    }

    #[test]
    fn parse_flash_sr() {
        // data[0] = SR value
        let sr = parse::flash_sr_from_standard(&[0x3C]).unwrap();
        assert_eq!(sr, 0x3C);
    }

    #[test]
    fn parse_chip_id_from_read_reg() {
        // RxFrame data = [regB1, regB2, regB3, chipB0, chipB1, chipB2, chipB3]
        // Chip ID = 0x12345678 → LE: 78 56 34 12
        let data = [0x00, 0x00, 0x44, 0x78, 0x56, 0x34, 0x12]; // reg bytes then chip
        let chip_id = parse::chip_id_from_read_reg(&data).unwrap();
        assert_eq!(chip_id, 0x12345678);
    }
}
