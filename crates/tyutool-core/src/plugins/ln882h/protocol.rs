use crate::error::FlashError;
use serialport::SerialPort;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const CHUNK_SIZE: usize = 0x200; // 512 bytes per flash_read (matches reference tool; aligned to 4 KB sectors)

// XMODEM control bytes
pub const SOH: u8 = 0x01; // 128-byte packet header
pub const STX: u8 = 0x02; // 1024+ byte packet header
pub const EOT: u8 = 0x04; // end of transmission
pub const ACK: u8 = 0x06; // acknowledge
pub const CAN: u8 = 0x18; // cancel
pub const CRC_BYTE: u8 = b'C'; // device requests CRC mode

// CRC16-CCITT (XModem variant): poly=0x1021, init=0
const CRC_TABLE: [u16; 256] = {
    let mut table = [0u16; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = (i as u16) << 8;
        let mut j = 0;
        while j < 8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        let idx = (((crc >> 8) as u8) ^ byte) as usize;
        crc = (crc << 8) ^ CRC_TABLE[idx];
    }
    crc
}

/// Flush serial RX/TX buffers.
pub fn flush_buffers(port: &mut Box<dyn SerialPort>) -> Result<(), FlashError> {
    port.clear(serialport::ClearBuffer::All)?;
    Ok(())
}

/// Write ASCII command followed by `\r\n`.
pub fn send_command(port: &mut Box<dyn SerialPort>, cmd: &str) -> Result<(), FlashError> {
    let msg = format!("{cmd}\r\n");
    port.write_all(msg.as_bytes())?;
    Ok(())
}

/// Read up to `max_bytes` for up to `timeout_secs`, return collected bytes.
/// Sets port read timeout to 100 ms. Stops at two `\n` bytes or `max_bytes`.
/// Caller is responsible for resetting the port timeout afterward if needed.
pub fn read_response(
    port: &mut Box<dyn SerialPort>,
    max_bytes: usize,
    timeout_secs: u64,
) -> Result<Vec<u8>, FlashError> {
    port.set_timeout(Duration::from_millis(100))?;
    let mut result = Vec::with_capacity(max_bytes);
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let mut buf = [0u8; 256];
    loop {
        if Instant::now() >= deadline {
            break;
        }
        match port.read(&mut buf) {
            Ok(n) if n > 0 => {
                result.extend_from_slice(&buf[..n]);
                if result.len() >= max_bytes {
                    break;
                }
                // Stop at two newlines (two lines received)
                if result.iter().filter(|&&b| b == b'\n').count() >= 2 {
                    break;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {}
            Err(e) => return Err(FlashError::Io(e)),
            _ => {}
        }
    }
    Ok(result)
}

/// Returns `Err(FlashError::Plugin)` on timeout.
pub fn wait_for_response_containing(
    port: &mut Box<dyn SerialPort>,
    needle: &[u8],
    timeout_secs: u64,
) -> Result<(), FlashError> {
    let data = read_response(port, 512, timeout_secs)?;
    if data.windows(needle.len()).any(|w| w == needle) {
        Ok(())
    } else {
        Err(FlashError::Plugin(format!(
            "expected {:?} but got: {:?}",
            std::str::from_utf8(needle).unwrap_or("(binary)"),
            std::str::from_utf8(&data).unwrap_or("(binary)")
        )))
    }
}

fn hex_nibble(b: u8) -> Result<u8, FlashError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        _ => Err(FlashError::Plugin(format!(
            "flash_read: invalid hex byte 0x{b:02x}"
        ))),
    }
}

/// Read one CHUNK_SIZE-byte chunk from flash at `addr` using the RAM code `flash_read` command.
///
/// Protocol: send `flash_read 0x{addr:x} 0x{CHUNK_SIZE:x}\r\n`, receive:
///   - Echo: the command string
///   - Data: CHUNK_SIZE bytes each as `"HH "` (2 uppercase hex + space)
///   - CRC:  CRC16 high byte as `"HH "` then low byte as `"HH "` (6 chars total)
pub fn read_flash_chunk(
    port: &mut Box<dyn SerialPort>,
    addr: u32,
) -> Result<[u8; CHUNK_SIZE], FlashError> {
    let cmd = format!("flash_read 0x{addr:x} 0x{CHUNK_SIZE:x}\r\n");
    port.write_all(cmd.as_bytes())?;

    // echo (cmd.len()) + (CHUNK_SIZE + 2 CRC bytes) × 3 chars each ("HH ")
    let expected = cmd.len() + (CHUNK_SIZE + 2) * 3;
    let mut buf = vec![0u8; expected];
    let mut received = 0usize;
    let deadline = Instant::now() + Duration::from_secs(2);

    port.set_timeout(Duration::from_millis(500))?;
    while received < expected {
        if Instant::now() >= deadline {
            return Err(FlashError::Plugin(format!(
                "flash_read 0x{addr:x}: timeout ({received}/{expected} bytes)"
            )));
        }
        match port.read(&mut buf[received..]) {
            Ok(n) if n > 0 => received += n,
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {}
            Err(e) => return Err(FlashError::Io(e)),
            _ => {}
        }
    }

    // Parse (CHUNK_SIZE + 2) hex pairs starting after the echo
    let data_crc = &buf[cmd.len()..];
    let mut parsed = [0u8; CHUNK_SIZE + 2];
    for (i, out) in parsed.iter_mut().enumerate() {
        let off = i * 3;
        *out = (hex_nibble(data_crc[off])? << 4) | hex_nibble(data_crc[off + 1])?;
        // data_crc[off + 2] == b' '
    }

    let expected_crc = ((parsed[CHUNK_SIZE] as u16) << 8) | (parsed[CHUNK_SIZE + 1] as u16);
    let actual_crc = crc16(&parsed[..CHUNK_SIZE]);
    if actual_crc != expected_crc {
        return Err(FlashError::Plugin(format!(
            "flash_read 0x{addr:x}: CRC mismatch (expected 0x{expected_crc:04x}, got 0x{actual_crc:04x})"
        )));
    }

    let mut result = [0u8; CHUNK_SIZE];
    result.copy_from_slice(&parsed[..CHUNK_SIZE]);
    Ok(result)
}

/// XMODEM-CRC16 sender. Implements the YModem-style protocol used by the LN882H bootloader.
/// Packet sizes: 1024 bytes for RAM binary download, 16384 bytes for firmware write.
pub struct XmodemSend<'a> {
    port: &'a mut Box<dyn SerialPort>,
    data: &'a [u8],
    packet_size: usize,
}

impl<'a> XmodemSend<'a> {
    pub fn new(port: &'a mut Box<dyn SerialPort>, data: &'a [u8], packet_size: usize) -> Self {
        Self {
            port,
            data,
            packet_size,
        }
    }

    /// Run the full XMODEM session for `data`.
    /// `file_name` is sent in the YModem filename header (e.g. "ram.bin", "qio.bin").
    /// `progress` is called with (bytes_sent, total_bytes) after each packet.
    pub fn send(
        &mut self,
        file_name: &str,
        cancel: &AtomicBool,
        progress: &dyn Fn(usize, usize),
    ) -> Result<(), FlashError> {
        // Set 5 s read timeout for XMODEM operations
        self.port.set_timeout(Duration::from_secs(5))?;
        self.receive_header(cancel)?;
        self.receive_response(file_name, self.data.len())?;
        self.send_file(cancel, progress)?;
        self.send_eot()?;
        Ok(())
    }

    fn read_byte(&mut self) -> Result<u8, FlashError> {
        let mut buf = [0u8; 1];
        self.port.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    fn abort(&mut self) {
        let _ = self.port.write_all(&[CAN, CAN]);
    }

    /// Wait for device to send `CRC` (0x43) to start transfer.
    fn receive_header(&mut self, cancel: &AtomicBool) -> Result<(), FlashError> {
        const MAX_ERRORS: usize = 20;
        let mut errors = 0usize;
        let mut cancels = 0usize;
        loop {
            if cancel.load(Ordering::Relaxed) {
                self.abort();
                return Err(FlashError::Cancelled);
            }
            match self.read_byte() {
                Ok(CRC_BYTE) => return Ok(()),
                Ok(CAN) => {
                    cancels += 1;
                    if cancels >= 2 {
                        return Err(FlashError::Plugin("xmodem: cancelled by device".into()));
                    }
                }
                Ok(_) | Err(_) => {
                    errors += 1;
                    if errors > MAX_ERRORS {
                        self.abort();
                        return Err(FlashError::Plugin(
                            "xmodem: device did not send CRC request".into(),
                        ));
                    }
                }
            }
        }
    }

    /// Send filename/size header packet (SOH seq=0), wait for ACK+CRC from device.
    fn receive_response(&mut self, file_name: &str, file_size: usize) -> Result<(), FlashError> {
        // Build filename/size payload: name\x00size_digits\x00\x00...\x00 (128 bytes total)
        // Matches Python reference: data_name + '\x00' + data_size + '\x00', ljust(128, '\x00')
        let mut payload = vec![0u8; 128];
        let name_bytes = file_name.as_bytes();
        let name_len = name_bytes.len().min(116); // cap to leave room for \0 + size + trailing \0
        payload[..name_len].copy_from_slice(&name_bytes[..name_len]);
        // payload[name_len] = 0x00 (null terminator for name — already zero-initialized)
        let size_str = file_size.to_string();
        let sz_bytes = size_str.as_bytes();
        let sz_start = name_len + 1;
        let sz_end = (sz_start + sz_bytes.len()).min(127);
        payload[sz_start..sz_end].copy_from_slice(&sz_bytes[..sz_end - sz_start]);
        // payload[sz_end] = 0x00 (null terminator for size — already zero-initialized)

        let crc = crc16(&payload);
        let packet = {
            let mut p = vec![SOH, 0x00, 0xFF];
            p.extend_from_slice(&payload);
            p.push((crc >> 8) as u8);
            p.push((crc & 0xFF) as u8);
            p
        };
        self.port.write_all(&packet)?;

        const MAX_ERRORS: usize = 20;
        let mut errors = 0usize;
        let mut cancels = 0usize;
        loop {
            match self.read_byte() {
                Ok(ACK) => {
                    match self.read_byte() {
                        Ok(CRC_BYTE) => {}
                        Ok(b) => log::warn!("xmodem: expected CRC_BYTE after ACK, got 0x{b:02x}"),
                        Err(_) => {}
                    }
                    return Ok(());
                }
                Ok(CAN) => {
                    cancels += 1;
                    if cancels >= 2 {
                        return Err(FlashError::Plugin(
                            "xmodem: cancelled during header exchange".into(),
                        ));
                    }
                }
                Ok(_) | Err(_) => {
                    errors += 1;
                    if errors > MAX_ERRORS {
                        self.abort();
                        return Err(FlashError::Plugin(
                            "xmodem: no ACK for filename packet".into(),
                        ));
                    }
                }
            }
        }
    }

    /// Send all data in chunks of `packet_size`. Calls `progress(sent, total)` after each packet.
    fn send_file(
        &mut self,
        cancel: &AtomicBool,
        progress: &dyn Fn(usize, usize),
    ) -> Result<(), FlashError> {
        let total = self.data.len();
        let mut offset = 0usize;
        let mut sequence: u8 = 1;
        let header_byte = if self.packet_size >= 1024 { STX } else { SOH };

        while offset < total {
            if cancel.load(Ordering::Relaxed) {
                self.abort();
                return Err(FlashError::Cancelled);
            }

            let end = (offset + self.packet_size).min(total);
            let chunk = &self.data[offset..end];

            // Pad to full packet size with 0x1A (standard XMODEM fill byte / CTRL-Z)
            let mut data = vec![0x1au8; self.packet_size];
            data[..chunk.len()].copy_from_slice(chunk);

            let crc = crc16(&data);
            let packet = {
                let mut p = vec![header_byte, sequence, 0xFF - sequence];
                p.extend_from_slice(&data);
                p.push((crc >> 8) as u8);
                p.push((crc & 0xFF) as u8);
                p
            };

            self.port.write_all(&packet)?;

            // Wait for ACK; tolerate up to 20 unexpected bytes before giving up
            const MAX_ERRORS: usize = 20;
            let mut errors = 0usize;
            loop {
                match self.read_byte() {
                    Ok(ACK) => break,
                    Ok(_) | Err(_) => {
                        if cancel.load(Ordering::Relaxed) {
                            self.abort();
                            return Err(FlashError::Cancelled);
                        }
                        errors += 1;
                        if errors > MAX_ERRORS {
                            self.abort();
                            return Err(FlashError::Plugin("xmodem: max retries exceeded".into()));
                        }
                    }
                }
            }

            offset = end;
            sequence = sequence.wrapping_add(1);
            progress(offset.min(total), total);
        }
        Ok(())
    }

    /// Send EOT, handle NAK/ACK/CRC handshake, then send empty YModem terminator packet.
    fn send_eot(&mut self) -> Result<(), FlashError> {
        const MAX_ERRORS: usize = 20;
        const NAK: u8 = 0x15;

        // YModem EOT handshake:
        //   sender → EOT  |  device → NAK  (first EOT)
        //   sender → EOT  |  device → ACK + CRC  (second EOT; CRC requests null terminator)
        let mut errors = 0usize;
        loop {
            self.port.write_all(&[EOT])?;
            match self.read_byte() {
                Ok(ACK) => {
                    // Consume the CRC byte the device sends after ACKing the second EOT
                    let _ = self.read_byte();
                    break;
                }
                Ok(NAK) => {} // Expected for the first EOT; send another EOT
                Ok(_) | Err(_) => {
                    errors += 1;
                    if errors > MAX_ERRORS {
                        self.abort();
                        return Err(FlashError::Plugin("xmodem: no ACK for EOT".into()));
                    }
                }
            }
        }

        // YModem batch terminator: null SOH packet (seq=0, 128 zero bytes)
        let data = [0u8; 128];
        let crc = crc16(&data);
        let packet = {
            let mut p = vec![SOH, 0x00, 0xFF];
            p.extend_from_slice(&data);
            p.push((crc >> 8) as u8);
            p.push((crc & 0xFF) as u8);
            p
        };
        self.port.write_all(&packet)?;

        let mut errors = 0usize;
        loop {
            match self.read_byte() {
                Ok(ACK) => return Ok(()),
                Ok(_) | Err(_) => {
                    errors += 1;
                    if errors > MAX_ERRORS {
                        self.abort();
                        return Err(FlashError::Plugin(
                            "xmodem: no ACK for terminator packet".into(),
                        ));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_known_values() {
        // "123456789" → 0x31C3 for XModem CRC16
        assert_eq!(crc16(b"123456789"), 0x31C3);
    }

    #[test]
    fn crc16_empty() {
        assert_eq!(crc16(b""), 0x0000);
    }

    #[test]
    fn crc16_table_spot_check() {
        // Verify compile-time table generation is correct
        assert_eq!(CRC_TABLE[0], 0x0000);
        assert_eq!(CRC_TABLE[1], 0x1021);
        // Verify function uses the table correctly for a single byte
        assert_eq!(crc16(&[0x01]), 0x1021);
    }
}
