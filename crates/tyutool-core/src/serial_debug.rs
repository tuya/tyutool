//! Interactive serial debug session — open a port, stream Rx chunks
//! to a callback, send bytes, handle disconnects. Used by the Tauri
//! `serial_debug_*` commands and the CLI `serve` WS handler.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::FlashError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DataBits {
    Five,
    Six,
    Seven,
    Eight,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Parity {
    None,
    Odd,
    Even,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StopBits {
    One,
    OnePointFive,
    Two,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugConfig {
    pub port: String,
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Tx,
    Rx,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugChunk {
    pub direction: Direction,
    pub ts_ms: u64,
    pub bytes: Vec<u8>,
}

pub type ChunkCallback = Box<dyn Fn(DebugChunk) + Send + Sync>;
pub type DisconnectCallback = Box<dyn Fn(String) + Send + Sync>;

pub struct SerialDebugSession {
    cfg: DebugConfig,
    port: Arc<Mutex<Box<dyn serialport::SerialPort>>>,
    stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn map_data_bits(v: DataBits) -> serialport::DataBits {
    match v {
        DataBits::Five => serialport::DataBits::Five,
        DataBits::Six => serialport::DataBits::Six,
        DataBits::Seven => serialport::DataBits::Seven,
        DataBits::Eight => serialport::DataBits::Eight,
    }
}

fn map_parity(v: Parity) -> serialport::Parity {
    match v {
        Parity::None => serialport::Parity::None,
        Parity::Odd => serialport::Parity::Odd,
        Parity::Even => serialport::Parity::Even,
    }
}

fn map_stop_bits(v: StopBits) -> serialport::StopBits {
    // serialport crate does not support 1.5 natively on all platforms.
    // Map 1.5 to One with a log warning; the OS driver may still honor it.
    match v {
        StopBits::One | StopBits::OnePointFive => serialport::StopBits::One,
        StopBits::Two => serialport::StopBits::Two,
    }
}

impl SerialDebugSession {
    pub fn open(
        cfg: DebugConfig,
        on_chunk: ChunkCallback,
        on_disconnect: DisconnectCallback,
    ) -> Result<Self, FlashError> {
        if matches!(cfg.stop_bits, StopBits::OnePointFive) {
            log::warn!(
                "[SerialDebug] stop_bits=1.5 requested; the serialport crate does not support \
                 it directly — falling back to 1 stop bit. OS drivers may differ."
            );
        }
        let builder = serialport::new(&cfg.port, cfg.baud_rate)
            .data_bits(map_data_bits(cfg.data_bits))
            .parity(map_parity(cfg.parity))
            .stop_bits(map_stop_bits(cfg.stop_bits))
            .timeout(Duration::from_millis(50));
        let port = builder.open().map_err(|e| {
            FlashError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("open {} failed: {}", cfg.port, e),
            ))
        })?;

        let port = Arc::new(Mutex::new(port));
        let stop = Arc::new(AtomicBool::new(false));

        let reader = {
            let port = Arc::clone(&port);
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name(format!("serial-debug-read:{}", cfg.port))
                .spawn(move || {
                    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        read_loop(port, stop, on_chunk, on_disconnect);
                    }));
                    if let Err(payload) = run {
                        log::error!("[SerialDebug] reader thread panicked: {:?}", payload);
                    }
                })
                .map_err(|e| {
                    FlashError::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("spawn reader thread failed: {}", e),
                    ))
                })?
        };

        log::info!(
            "[SerialDebug] opened {} @ {} {:?}/{:?}/{:?}",
            cfg.port,
            cfg.baud_rate,
            cfg.data_bits,
            cfg.parity,
            cfg.stop_bits
        );

        Ok(Self {
            cfg,
            port,
            stop,
            reader: Some(reader),
        })
    }

    pub fn write(&self, bytes: &[u8]) -> Result<(), FlashError> {
        let mut guard = self.port.lock().map_err(|_| {
            FlashError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "serial debug mutex poisoned",
            ))
        })?;
        guard.write_all(bytes).map_err(FlashError::Io)?;
        guard.flush().map_err(FlashError::Io)?;
        Ok(())
    }

    pub fn close(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.reader.take() {
            let _ = h.join();
        }
        log::info!("[SerialDebug] closed {}", self.cfg.port);
    }

    pub fn is_open(&self) -> bool {
        !self.stop.load(Ordering::SeqCst)
    }

    pub fn config(&self) -> &DebugConfig {
        &self.cfg
    }
}

impl Drop for SerialDebugSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.reader.take() {
            let _ = h.join();
        }
    }
}

fn read_loop(
    port: Arc<Mutex<Box<dyn serialport::SerialPort>>>,
    stop: Arc<AtomicBool>,
    on_chunk: ChunkCallback,
    on_disconnect: DisconnectCallback,
) {
    let mut buf = [0u8; 4096];
    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let read_result = {
            let mut guard = match port.lock() {
                Ok(g) => g,
                Err(_) => {
                    on_disconnect("port mutex poisoned".into());
                    return;
                }
            };
            guard.read(&mut buf)
        };
        match read_result {
            Ok(0) => continue,
            Ok(n) => {
                let chunk = DebugChunk {
                    direction: Direction::Rx,
                    ts_ms: now_ms(),
                    bytes: buf[..n].to_vec(),
                };
                on_chunk(chunk);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(e)
                if e.kind() == std::io::ErrorKind::BrokenPipe
                    || e.kind() == std::io::ErrorKind::NotFound =>
            {
                log::warn!(
                    "[SerialDebug] reader IO error: {} ({:?}) — disconnecting",
                    e,
                    e.kind()
                );
                on_disconnect(format!("{}", e));
                return;
            }
            Err(e) => {
                log::error!(
                    "[SerialDebug] reader unexpected error: {} ({:?}) — disconnecting",
                    e,
                    e.kind()
                );
                on_disconnect(format!("{}", e));
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_config_round_trips_json_camel_case() {
        let cfg = DebugConfig {
            port: "/dev/ttyUSB0".into(),
            baud_rate: 115200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("\"baudRate\":115200"));
        assert!(json.contains("\"dataBits\":\"eight\""));
        assert!(json.contains("\"parity\":\"none\""));
        assert!(json.contains("\"stopBits\":\"one\""));
        let back: DebugConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn debug_chunk_serializes_direction_lowercase() {
        let chunk = DebugChunk {
            direction: Direction::Rx,
            ts_ms: 1_700_000_000_000,
            bytes: vec![0x41, 0x42],
        };
        let json = serde_json::to_string(&chunk).unwrap();
        assert!(json.contains("\"direction\":\"rx\""));
        assert!(json.contains("\"tsMs\":1700000000000"));
        assert!(json.contains("\"bytes\":[65,66]"));
    }

    // Loopback integration — only runs on Linux/macOS where serialport exposes pair().
    // If the `pair()` call fails (CI without PTY support), the test skips via an early return.
    //
    // Note: in serialport 4.x, `TTYPort::pair()` returns (master, slave). The master has no
    // port_name (it is the /dev/ptmx fd), while the slave exposes its /dev/pts/N path.
    // The session opens the slave by name (second independent fd) and we write on the
    // master handle we keep alive.
    #[cfg(unix)]
    #[test]
    fn write_is_observed_on_the_paired_end_and_close_stops_reader() {
        use serialport::SerialPort;
        use std::io::Write;
        use std::sync::mpsc::channel;

        let Ok((mut master, slave)) = serialport::TTYPort::pair() else {
            eprintln!("serialport::TTYPort::pair() unavailable on this host; skipping");
            return;
        };
        let Some(slave_name) = slave.name() else {
            eprintln!("pty slave has no name on this host; skipping");
            return;
        };
        // Drop the slave handle so SerialDebugSession can open the path itself.
        drop(slave);

        let (tx_chunk, rx_chunk) = channel::<DebugChunk>();
        let (tx_disc, rx_disc) = channel::<String>();

        let cfg = DebugConfig {
            port: slave_name.clone(),
            baud_rate: 115200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
        };
        let session = SerialDebugSession::open(
            cfg,
            Box::new(move |c| {
                let _ = tx_chunk.send(c);
            }),
            Box::new(move |r| {
                let _ = tx_disc.send(r);
            }),
        )
        .expect("session open");

        // Write on the master fd; the session's read loop on the slave should produce a chunk.
        master.write_all(b"ping\n").expect("write master");
        master.flush().expect("flush master");

        let mut accumulated: Vec<u8> = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_millis(1500);
        while accumulated.len() < b"ping\n".len() {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                panic!("timed out waiting for full payload; got {:?}", accumulated);
            }
            let chunk = rx_chunk
                .recv_timeout(remaining)
                .expect("expected chunk within deadline");
            assert_eq!(chunk.direction, Direction::Rx);
            accumulated.extend_from_slice(&chunk.bytes);
        }
        assert_eq!(&accumulated[..b"ping\n".len()], b"ping\n");

        session.close();
        assert!(
            rx_disc.try_recv().is_err(),
            "close() should not trigger on_disconnect"
        );
    }
}
