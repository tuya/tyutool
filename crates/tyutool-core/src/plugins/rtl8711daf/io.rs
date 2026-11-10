//! Serial I/O for the RTL8711DAF (AmebaDplus) plugin.
//!
//! [`RtlIo`] abstracts the raw port so the protocol layers can be tested with a
//! mock (same pattern as `beken::transport::IoTransport`). [`Link`] adds the
//! timeout / cancellation helpers every protocol step needs.
//!
//! The port is opened exactly once per job and the baud rate is changed in
//! place: on Linux, re-opening a tty raises DTR/RTS, and boards that wire RTS
//! to CHIP_EN would reset the chip out of download mode.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::error::FlashError;

/// Byte-level serial access.
pub trait RtlIo: Send {
    fn write_all(&mut self, data: &[u8]) -> io::Result<()>;
    /// Returns `Ok(0)` when nothing arrived within the port's short read timeout.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    fn set_baud_rate(&mut self, baud: u32) -> io::Result<()>;
    fn set_rts(&mut self, level: bool) -> io::Result<()>;
    fn clear_input(&mut self) -> io::Result<()>;
}

/// Read timeout of the underlying port; bounds how long a cancel takes to be noticed.
const PORT_READ_TIMEOUT: Duration = Duration::from_millis(20);

pub struct SerialRtlIo {
    port: Box<dyn serialport::SerialPort>,
}

impl SerialRtlIo {
    pub fn open(port_name: &str, baud: u32) -> Result<Self, FlashError> {
        let mut port = serialport::new(port_name, baud)
            .timeout(PORT_READ_TIMEOUT)
            .dtr_on_open(false)
            .open()?;
        // Deassert both lines: RTS may drive CHIP_EN on download boards.
        port.write_request_to_send(false)?;
        port.write_data_terminal_ready(false)?;
        Ok(Self { port })
    }
}

fn to_io(e: serialport::Error) -> io::Error {
    io::Error::other(e.to_string())
}

impl RtlIo for SerialRtlIo {
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        io::Write::write_all(&mut self.port, data)?;
        io::Write::flush(&mut self.port)
    }

    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match io::Read::read(&mut self.port, buf) {
            Ok(n) => Ok(n),
            Err(e) if e.kind() == io::ErrorKind::TimedOut => Ok(0),
            Err(e) => Err(e),
        }
    }

    fn set_baud_rate(&mut self, baud: u32) -> io::Result<()> {
        self.port.set_baud_rate(baud).map_err(to_io)
    }

    fn set_rts(&mut self, level: bool) -> io::Result<()> {
        self.port.write_request_to_send(level).map_err(to_io)
    }

    fn clear_input(&mut self) -> io::Result<()> {
        self.port
            .clear(serialport::ClearBuffer::Input)
            .map_err(to_io)
    }
}

/// Serial link with cancellation-aware timed reads.
pub struct Link<'a> {
    io: Box<dyn RtlIo + 'a>,
    cancel: &'a AtomicBool,
    baud: u32,
}

impl<'a> Link<'a> {
    pub fn new(io: Box<dyn RtlIo + 'a>, cancel: &'a AtomicBool, baud: u32) -> Self {
        Self { io, cancel, baud }
    }

    pub fn baud(&self) -> u32 {
        self.baud
    }

    pub fn check_cancel(&self) -> Result<(), FlashError> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(FlashError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Cancellable sleep.
    pub fn sleep(&self, dur: Duration) -> Result<(), FlashError> {
        let end = Instant::now() + dur;
        loop {
            self.check_cancel()?;
            let now = Instant::now();
            if now >= end {
                return Ok(());
            }
            std::thread::sleep((end - now).min(Duration::from_millis(20)));
        }
    }

    pub fn set_baud(&mut self, baud: u32) -> Result<(), FlashError> {
        self.io.set_baud_rate(baud)?;
        self.baud = baud;
        Ok(())
    }

    pub fn set_rts(&mut self, level: bool) -> Result<(), FlashError> {
        Ok(self.io.set_rts(level)?)
    }

    pub fn clear(&mut self) -> Result<(), FlashError> {
        Ok(self.io.clear_input()?)
    }

    pub fn write(&mut self, data: &[u8]) -> Result<(), FlashError> {
        self.check_cancel()?;
        Ok(self.io.write_all(data)?)
    }

    /// Next byte, or `None` on timeout.
    pub fn read_byte(&mut self, timeout: Duration) -> Result<Option<u8>, FlashError> {
        let end = Instant::now() + timeout;
        let mut b = [0u8; 1];
        loop {
            self.check_cancel()?;
            if self.io.read(&mut b)? == 1 {
                return Ok(Some(b[0]));
            }
            if Instant::now() >= end {
                return Ok(None);
            }
        }
    }

    /// Exactly `n` bytes, or `None` if they did not all arrive within `timeout`.
    pub fn read_exact(
        &mut self,
        n: usize,
        timeout: Duration,
    ) -> Result<Option<Vec<u8>>, FlashError> {
        let end = Instant::now() + timeout;
        let mut buf = vec![0u8; n];
        let mut got = 0;
        while got < n {
            self.check_cancel()?;
            got += self.io.read(&mut buf[got..])?;
            if got < n && Instant::now() >= end {
                return Ok(None);
            }
        }
        Ok(Some(buf))
    }

    /// Everything that arrives during `dur`.
    pub fn drain(&mut self, dur: Duration) -> Result<Vec<u8>, FlashError> {
        let end = Instant::now() + dur;
        let mut out = Vec::new();
        let mut buf = [0u8; 1024];
        while Instant::now() < end {
            self.check_cancel()?;
            let n = self.io.read(&mut buf)?;
            out.extend_from_slice(&buf[..n]);
        }
        Ok(out)
    }
}

#[cfg(test)]
pub mod mock {
    //! Scripted serial port: each write may queue a canned reply.
    use super::RtlIo;
    use std::collections::VecDeque;
    use std::io;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    pub struct State {
        pub rx: VecDeque<u8>,
        pub writes: Vec<Vec<u8>>,
        /// Replies queued in order, one per `write_all` call (empty = no reply).
        pub replies: VecDeque<Vec<u8>>,
        pub bauds: Vec<u32>,
        pub rts: Vec<bool>,
    }

    #[derive(Clone, Default)]
    pub struct MockIo(pub Arc<Mutex<State>>);

    impl MockIo {
        pub fn with_replies(replies: Vec<Vec<u8>>) -> Self {
            let m = MockIo::default();
            m.0.lock().unwrap().replies = replies.into();
            m
        }
        pub fn push_rx(&self, bytes: &[u8]) {
            self.0.lock().unwrap().rx.extend(bytes);
        }
    }

    impl RtlIo for MockIo {
        fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
            let mut s = self.0.lock().unwrap();
            s.writes.push(data.to_vec());
            if let Some(r) = s.replies.pop_front() {
                s.rx.extend(r);
            }
            Ok(())
        }
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let mut s = self.0.lock().unwrap();
            let n = buf.len().min(s.rx.len());
            for b in buf.iter_mut().take(n) {
                *b = s.rx.pop_front().unwrap_or(0);
            }
            Ok(n)
        }
        fn set_baud_rate(&mut self, baud: u32) -> io::Result<()> {
            self.0.lock().unwrap().bauds.push(baud);
            Ok(())
        }
        fn set_rts(&mut self, level: bool) -> io::Result<()> {
            self.0.lock().unwrap().rts.push(level);
            Ok(())
        }
        fn clear_input(&mut self) -> io::Result<()> {
            self.0.lock().unwrap().rx.clear();
            Ok(())
        }
    }
}
