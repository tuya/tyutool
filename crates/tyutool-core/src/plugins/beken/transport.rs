//! Serial-port transport layer for the Beken protocol.
//!
//! Provides frame-level send/receive, handshake, baud-rate switching,
//! and hardware reset (DTR/RTS) on top of the raw byte I/O abstracted
//! by [`IoTransport`].

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::frame::{self, ProtocolError, RxFrame};

// ─────────────────────────────────────────────────────────────────────────
// IoTransport trait — abstraction over serial port
// ─────────────────────────────────────────────────────────────────────────

/// Byte-level I/O abstraction over a serial port.
///
/// Implemented by the real `serialport::SerialPort` adapter and by
/// test mocks.
pub trait IoTransport: Send {
    fn write_all(&mut self, data: &[u8]) -> io::Result<()>;
    fn write(&mut self, data: &[u8]) -> io::Result<usize>;
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    fn set_baud_rate(&mut self, baud: u32) -> io::Result<()>;
    fn set_dtr(&mut self, level: bool) -> io::Result<()>;
    fn set_rts(&mut self, level: bool) -> io::Result<()>;
    fn clear_buffers(&mut self) -> io::Result<()>;
    fn flush(&mut self) -> io::Result<()>;
    /// Set the read timeout on the underlying serial port.
    fn set_timeout(&mut self, timeout: Duration) -> io::Result<()>;
}

// ── Real serial port adapter ────────────────────────────────────────────

/// Wrapper around `serialport::SerialPort` implementing [`IoTransport`].
pub struct SerialIo {
    port: Box<dyn serialport::SerialPort>,
}

impl SerialIo {
    pub fn open(port_name: &str, baud: u32) -> Result<Self, ProtocolError> {
        let port = serialport::new(port_name, baud)
            .timeout(Duration::from_millis(100))
            .open()
            .map_err(|e| ProtocolError::Io(io::Error::new(io::ErrorKind::Other, e.to_string())))?;
        Ok(Self { port })
    }

    /// Re-open the serial port at a different baud rate.
    pub fn reopen(&mut self, port_name: &str, baud: u32) -> Result<(), ProtocolError> {
        // Drop old port, open new one
        let port = serialport::new(port_name, baud)
            .timeout(Duration::from_millis(100))
            .open()
            .map_err(|e| ProtocolError::Io(io::Error::new(io::ErrorKind::Other, e.to_string())))?;
        self.port = port;
        Ok(())
    }
}

impl IoTransport for SerialIo {
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        io::Write::write_all(&mut self.port, data)
    }

    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        io::Write::write(&mut self.port, data)
    }

    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        io::Read::read(&mut self.port, buf)
    }

    fn set_baud_rate(&mut self, baud: u32) -> io::Result<()> {
        self.port
            .set_baud_rate(baud)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
    }

    fn set_dtr(&mut self, level: bool) -> io::Result<()> {
        self.port
            .write_data_terminal_ready(level)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
    }

    fn set_rts(&mut self, level: bool) -> io::Result<()> {
        self.port
            .write_request_to_send(level)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
    }

    fn clear_buffers(&mut self) -> io::Result<()> {
        // Only clear input (RX) buffer; clearing output (TX) can discard
        // data that hasn't been sent yet.
        self.port
            .clear(serialport::ClearBuffer::Input)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
    }

    fn flush(&mut self) -> io::Result<()> {
        io::Write::flush(&mut self.port)
    }

    fn set_timeout(&mut self, timeout: Duration) -> io::Result<()> {
        self.port
            .set_timeout(timeout)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Transport
// ─────────────────────────────────────────────────────────────────────────

/// Frame-level transport over an [`IoTransport`].
///
/// Manages:
/// - RX buffering and frame decoding
/// - Cancel signal checking
/// - Log forwarding to the progress callback
pub struct Transport<'a, T: IoTransport> {
    pub(crate) io: T,
    rx_buf: Vec<u8>,
    cancel: &'a AtomicBool,
    log_fn: &'a dyn Fn(&str),
    /// Port name (needed for re-opening at different baud rate).
    port_name: String,
    /// Current baud rate — used to pace large writes so the USB-to-UART
    /// bridge FIFO does not overflow.
    baud_rate: u32,
}

impl<'a, T: IoTransport> Transport<'a, T> {
    /// Create a new transport wrapping the given I/O.
    pub fn new(
        io: T,
        port_name: &str,
        initial_baud: u32,
        cancel: &'a AtomicBool,
        log_fn: &'a dyn Fn(&str),
    ) -> Self {
        log::debug!("Transport created for port: {}", port_name);
        Self {
            io,
            rx_buf: Vec::with_capacity(8192),
            cancel,
            log_fn,
            port_name: port_name.to_string(),
            baud_rate: initial_baud,
        }
    }

    /// Emit a log line through the progress callback.
    pub fn log(&self, msg: &str) {
        (self.log_fn)(msg);
    }

    /// Check the cancel signal; return `Err(Cancelled)` if set.
    #[inline]
    pub fn check_cancel(&self) -> Result<(), ProtocolError> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(ProtocolError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Send raw bytes (already encoded frame).
    ///
    /// On macOS, large frames (e.g. FlashWrite4K = 4108 bytes) are paced
    /// in small chunks to prevent USB-to-UART bridge FIFO overflow.  At
    /// high baud rates the USB host delivers data much faster than the UART
    /// can drain, so the bridge chip's internal FIFO (~320 bytes) overflows
    /// and silently corrupts data.
    ///
    /// Each chunk is flushed then followed by a sleep calibrated to the
    /// current baud rate (with safety margin) so the UART has time to
    /// drain before the next chunk is queued.
    pub fn send_raw(&mut self, data: &[u8]) -> Result<(), ProtocolError> {
        const TARGET_OS: &str = std::env::consts::OS;
        log::trace!("target_os={}, TX raw: {} bytes", TARGET_OS, data.len());
        self.rx_buf.clear();

        const FIFO_SAFE: usize = 256;
        if TARGET_OS == "macos" && data.len() > FIFO_SAFE {
            // UART drain time for one chunk (10 bits/byte for 8N1) plus a
            // fixed 500 µs margin for USB round-trip and OS scheduling.
            let drain_us = (FIFO_SAFE as u64) * 10_000_000 / (self.baud_rate as u64) + 500;
            let drain = Duration::from_micros(drain_us);

            for chunk in data.chunks(FIFO_SAFE) {
                self.io.write_all(chunk)?;
                self.io.flush()?;
                std::thread::sleep(drain);
            }
        } else {
            self.io.write_all(data)?;
        }
        Ok(())
    }

    /// Read bytes from the serial port into the internal RX buffer.
    ///
    /// Returns the number of new bytes read (0 on timeout, which is normal).
    fn fill_rx_buf(&mut self) -> Result<usize, ProtocolError> {
        let mut tmp = [0u8; 4096];
        match self.io.read(&mut tmp) {
            Ok(n) => {
                self.rx_buf.extend_from_slice(&tmp[..n]);
                Ok(n)
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut => Ok(0),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(0),
            Err(e) => Err(ProtocolError::Io(e)),
        }
    }

    /// Receive and decode one RX frame, with timeout.
    pub fn recv_frame(&mut self, timeout_ms: u64) -> Result<RxFrame, ProtocolError> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            self.check_cancel()?;

            // Try to decode a frame from buffered data
            while let Some((frame, consumed)) = frame::decode(&self.rx_buf) {
                self.rx_buf.drain(..consumed);
                // Skip garbage/synthetic skip frames
                if frame.status != 0xff || frame.cmd != 0 || !frame.data.is_empty() {
                    return Ok(frame);
                }
                // It was a skip-garbage frame, try again
            }

            if Instant::now() >= deadline {
                return Err(ProtocolError::Timeout { attempts: 1 });
            }

            // Read more data
            self.fill_rx_buf()?;
        }
    }

    /// Send a standard-frame command and receive the response.
    pub fn send_recv_standard(
        &mut self,
        cmd: u8,
        payload: &[u8],
        timeout_ms: u64,
    ) -> Result<RxFrame, ProtocolError> {
        log::debug!(
            "TX standard frame: cmd=0x{:02x}, payload={} bytes",
            cmd,
            payload.len()
        );
        let tx = frame::encode_standard(cmd, payload);
        self.send_raw(&tx)?;
        let result = self.recv_frame(timeout_ms);
        if result.is_ok() {
            log::debug!("RX standard frame: response received");
        }
        result
    }

    /// Send an extended-frame command and receive the response.
    pub fn send_recv_extended(
        &mut self,
        cmd: u8,
        payload: &[u8],
        timeout_ms: u64,
    ) -> Result<RxFrame, ProtocolError> {
        log::debug!(
            "TX extended frame: cmd=0x{:02x}, payload={} bytes",
            cmd,
            payload.len()
        );
        let tx = frame::encode_extended(cmd, payload);
        self.send_raw(&tx)?;
        let result = self.recv_frame(timeout_ms);
        if result.is_ok() {
            log::debug!("RX extended frame: response received");
        }
        result
    }

    /// Send a command (standard or extended) and receive with retry.
    pub fn send_recv_retry(
        &mut self,
        cmd: u8,
        payload: &[u8],
        extended: bool,
        timeout_ms: u64,
        max_retries: u32,
    ) -> Result<RxFrame, ProtocolError> {
        let mut last_err = None;
        for attempt in 0..=max_retries {
            self.check_cancel()?;
            let result = if extended {
                self.send_recv_extended(cmd, payload, timeout_ms)
            } else {
                self.send_recv_standard(cmd, payload, timeout_ms)
            };
            match result {
                Ok(frame) => return Ok(frame),
                Err(e) => {
                    if attempt < max_retries {
                        self.log(&format!("retry {}/{}: {}", attempt + 1, max_retries, e));
                        log::warn!("Retry {}/{}", attempt + 1, max_retries);
                        // Clear buffers for clean retry
                        self.rx_buf.clear();
                        let _ = self.io.clear_buffers();
                    }
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.unwrap())
    }

    /// Perform hardware reset via DTR/RTS lines to enter download mode.
    ///
    /// BK7231N sequence:
    /// 1. DTR=0, RTS=1 (assert reset)
    /// 2. Wait 200ms
    /// 3. RTS=0 (release reset)
    pub fn reset_into_download_mode_bk(&mut self) -> Result<(), ProtocolError> {
        self.io.set_dtr(false)?;
        self.io.set_rts(true)?;
        std::thread::sleep(Duration::from_millis(200));
        self.io.set_rts(false)?;
        Ok(())
    }

    /// T5 reset sequence:
    /// 1. DTR=0, RTS=1
    /// 2. Wait 300ms
    /// 3. RTS=0
    pub fn reset_into_download_mode_t5(&mut self) -> Result<(), ProtocolError> {
        self.io.set_dtr(false)?;
        self.io.set_rts(true)?;
        std::thread::sleep(Duration::from_millis(300));
        self.io.set_rts(false)?;
        Ok(())
    }

    /// Handshake: send `LinkCheck` repeatedly until the device responds.
    ///
    /// `interval_ms` controls the serial-port read timeout per attempt.
    /// For BK-family chips (BK7231N, BK7231X, T2) Python uses 0.001s (1ms).
    /// We temporarily lower the serial timeout to `interval_ms` for fast
    /// link-check spinning, then restore it after handshake completes.
    pub fn handshake(&mut self, max_retries: u32, interval_ms: u64) -> Result<(), ProtocolError> {
        use super::command::{build, CMD_LINK_CHECK};

        let link_payload = build::link_check();
        let link_frame = frame::encode_standard(CMD_LINK_CHECK, &link_payload);

        // Temporarily set serial timeout to match the requested interval.
        // Python BK7231N handler uses serial.Serial(port, baudrate, timeout=0.001)
        // which means each read() returns within 1ms if no data.
        // Our default 100ms timeout makes the loop ~100× slower than Python.
        let short_timeout = Duration::from_millis(interval_ms.max(1));
        let _ = self.io.set_timeout(short_timeout);

        // recv_frame timeout should be just slightly longer than one read cycle
        let recv_timeout_ms = interval_ms.max(1) + 5;

        let result = (|| {
            for attempt in 0..max_retries {
                self.check_cancel()?;

                self.send_raw(&link_frame)?;

                match self.recv_frame(recv_timeout_ms) {
                    Ok(rx) => {
                        // LinkCheck response: cmd should be 0x01 (CMD+1), status 0x00
                        if rx.cmd == 0x01 && rx.status == 0x00 {
                            self.log(&format!(
                                "handshake OK (attempt {}/{})",
                                attempt + 1,
                                max_retries
                            ));
                            log::info!("Handshake successful");
                            return Ok(());
                        }
                    }
                    Err(ProtocolError::Timeout { .. }) => {
                        // Expected — try again
                        continue;
                    }
                    Err(ProtocolError::Cancelled) => return Err(ProtocolError::Cancelled),
                    Err(_) => continue,
                }
            }

            Err(ProtocolError::Timeout {
                attempts: max_retries,
            })
        })();

        // Restore default serial timeout (100ms)
        let _ = self.io.set_timeout(Duration::from_millis(100));

        result
    }

    /// Switch baud rate: send `SetBaudRate` command, then re-configure
    /// the serial port to the new rate.
    pub fn switch_baud_rate(&mut self, new_baud: u32, delay_ms: u8) -> Result<(), ProtocolError> {
        use super::command::{build, CMD_SET_BAUD_RATE};

        log::info!("Switching baud rate to {}", new_baud);

        let payload = build::set_baud_rate(new_baud, delay_ms);
        let tx = frame::encode_standard(CMD_SET_BAUD_RATE, &payload);
        self.send_raw(&tx)?;

        // Wait for device to process the baud-rate change
        let wait = (delay_ms as u64) / 2;
        std::thread::sleep(Duration::from_millis(wait.max(10)));

        // Read any response at old baud rate (ignore errors)
        let _ = self.recv_frame(50);

        // Clear buffers and switch
        self.rx_buf.clear();
        let _ = self.io.clear_buffers();
        self.io.set_baud_rate(new_baud)?;
        self.baud_rate = new_baud;

        // Small delay for port to settle
        std::thread::sleep(Duration::from_millis(10));

        self.log(&format!("baud rate switched to {new_baud}"));
        Ok(())
    }

    /// Get the port name (needed for SerialIo::reopen).
    pub fn port_name(&self) -> &str {
        &self.port_name
    }

    /// Clear the RX buffer.
    pub fn clear_rx(&mut self) {
        self.rx_buf.clear();
        let _ = self.io.clear_buffers();
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Mock I/O for testing
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::VecDeque;

    /// Mock serial I/O for unit testing.
    ///
    /// Pre-load responses with `add_response()`; reads return them in order.
    /// Writes are recorded in `sent` for assertion.
    pub struct MockIo {
        responses: VecDeque<Vec<u8>>,
        current: Vec<u8>,
        pub sent: Vec<Vec<u8>>,
    }

    impl MockIo {
        pub fn new() -> Self {
            Self {
                responses: VecDeque::new(),
                current: Vec::new(),
                sent: Vec::new(),
            }
        }

        /// Enqueue a response that will be returned on the next `read()`.
        pub fn add_response(&mut self, data: Vec<u8>) {
            self.responses.push_back(data);
        }

        /// Add N empty responses (simulate silence / timeout).
        #[allow(dead_code)]
        pub fn add_silence(&mut self, count: usize) {
            for _ in 0..count {
                self.responses.push_back(Vec::new());
            }
        }
    }

    impl IoTransport for MockIo {
        fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
            self.sent.push(data.to_vec());
            Ok(())
        }

        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            self.sent.push(data.to_vec());
            Ok(data.len())
        }

        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            // If we have remaining bytes from a previous partial read
            if !self.current.is_empty() {
                let n = self.current.len().min(buf.len());
                buf[..n].copy_from_slice(&self.current[..n]);
                self.current.drain(..n);
                return Ok(n);
            }
            // Get next response
            if let Some(resp) = self.responses.pop_front() {
                if resp.is_empty() {
                    // Simulate timeout
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "mock timeout"));
                }
                let n = resp.len().min(buf.len());
                buf[..n].copy_from_slice(&resp[..n]);
                if n < resp.len() {
                    self.current = resp[n..].to_vec();
                }
                Ok(n)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "mock: no more responses",
                ))
            }
        }

        fn set_baud_rate(&mut self, _baud: u32) -> io::Result<()> {
            Ok(())
        }
        fn set_dtr(&mut self, _level: bool) -> io::Result<()> {
            Ok(())
        }
        fn set_rts(&mut self, _level: bool) -> io::Result<()> {
            Ok(())
        }
        fn clear_buffers(&mut self) -> io::Result<()> {
            self.current.clear();
            Ok(())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn set_timeout(&mut self, _timeout: Duration) -> io::Result<()> {
            Ok(())
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use mock::MockIo;

    fn make_transport(mock: MockIo) -> Transport<'static, MockIo> {
        static CANCEL: AtomicBool = AtomicBool::new(false);
        CANCEL.store(false, Ordering::Relaxed);
        Transport::new(mock, "/dev/mock", 115200, &CANCEL, &|_msg| {})
    }

    #[test]
    fn handshake_succeeds_on_first_try() {
        let mut mock = MockIo::new();
        // Valid LinkCheck response: [04 0e 05 01 e0 fc 01 00]
        mock.add_response(vec![0x04, 0x0e, 0x05, 0x01, 0xe0, 0xfc, 0x01, 0x00]);

        let mut transport = make_transport(mock);
        let result = transport.handshake(5, 20);
        assert!(result.is_ok());
    }

    #[test]
    fn handshake_succeeds_after_retries() {
        let mut mock = MockIo::new();
        // 2 timeouts, then success
        mock.add_silence(2);
        mock.add_response(vec![0x04, 0x0e, 0x05, 0x01, 0xe0, 0xfc, 0x01, 0x00]);

        let mut transport = make_transport(mock);
        let result = transport.handshake(5, 20);
        assert!(result.is_ok());
    }

    #[test]
    fn handshake_fails_after_max_retries() {
        let mut mock = MockIo::new();
        mock.add_silence(10);

        let mut transport = make_transport(mock);
        let result = transport.handshake(3, 20);
        assert!(matches!(result, Err(ProtocolError::Timeout { .. })));
    }

    #[test]
    fn send_recv_standard_works() {
        let mut mock = MockIo::new();
        // FlashGetMID response: [04 0e 07 01 e0 fc 0e C8 40 14]
        // LEN=7: 01 e0 fc(3) + CMD(1) + MID(3) = 7
        mock.add_response(vec![
            0x04, 0x0e, 0x07, 0x01, 0xe0, 0xfc, 0x0e, 0xC8, 0x40, 0x14,
        ]);

        let mut transport = make_transport(mock);
        let rx = transport.send_recv_standard(0x0e, &[], 1000).unwrap();
        assert_eq!(rx.cmd, 0x0e);
        // status = 0xC8 (first MID byte), data = [0x40, 0x14]
        assert_eq!(rx.status, 0xC8);
        assert_eq!(rx.data, vec![0x40, 0x14]);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn send_raw_prefers_single_write_on_non_macos() {
        let mock = MockIo::new();
        let data = vec![0x5a; 257];
        let mut transport = make_transport(mock);

        transport.send_raw(&data).unwrap();

        assert_eq!(transport.io.sent, vec![data]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn send_raw_chunks_large_writes_on_macos() {
        let mock = MockIo::new();
        let data = vec![0x5a; 257];
        let mut transport = make_transport(mock);

        transport.send_raw(&data).unwrap();

        assert_eq!(transport.io.sent, vec![vec![0x5a; 256], vec![0x5a]]);
    }

    #[test]
    fn cancel_signal_stops_handshake() {
        use std::sync::atomic::AtomicBool;

        let cancel = AtomicBool::new(true); // pre-cancelled
        let mock = MockIo::new();
        let mut transport = Transport::new(mock, "/dev/mock", 115200, &cancel, &|_| {});

        let result = transport.handshake(10, 20);
        assert!(matches!(result, Err(ProtocolError::Cancelled)));
    }
}
