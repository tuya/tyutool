//! Interactive serial debug session — open a port, stream Rx chunks
//! to a callback, send bytes, handle disconnects. Used by the Tauri
//! `serial_debug_*` commands and the CLI `serve` WS handler.

use serde::{Deserialize, Serialize};

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
}
