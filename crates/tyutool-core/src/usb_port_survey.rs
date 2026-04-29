//! Raw serial/USB enumeration for cross-OS surveys (see `tmp/usb-port-survey.md`).
//! Uses the same `usbportinfo-interface` data as the rest of the crate; independent of
//! `list_serial_ports` filtering except for the `wouldListInTyutool` hint.

use std::collections::HashSet;

use serde::Serialize;
use serialport::{SerialPortInfo, SerialPortType};

use crate::error::FlashError;
use crate::serial::list_serial_ports;

fn port_type_tag(pt: &SerialPortType) -> &'static str {
    match pt {
        SerialPortType::UsbPort(_) => "UsbPort",
        SerialPortType::BluetoothPort => "BluetoothPort",
        SerialPortType::PciPort => "PciPort",
        SerialPortType::Unknown => "Unknown",
    }
}

/// USB fields exposed by `serialport` for one CDC/virtual serial device.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsbPortSurveyUsb {
    pub vid: u16,
    pub pid: u16,
    /// Lowercase hex `vvvv:pppp` for quick grep / `lsusb` comparison.
    pub vid_pid_hex: String,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial_number: Option<String>,
    /// Interface number when the OS/driver exposes it (`usbportinfo-interface` feature).
    pub usb_interface: Option<u8>,
}

/// One row: OS-native port path + `SerialPortType` summary + optional USB details.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsbPortSurveyEntry {
    pub port_path: String,
    pub port_type: &'static str,
    /// `true` if this path appears in [`crate::list_serial_ports`] (after phantom/macOS filters).
    pub would_list_in_tyutool: bool,
    pub usb: Option<UsbPortSurveyUsb>,
}

/// All entries from `serialport::available_ports()` on this machine, with USB metadata when applicable.
pub fn usb_port_survey() -> Result<Vec<UsbPortSurveyEntry>, FlashError> {
    let listed: HashSet<String> = list_serial_ports()?.into_iter().map(|e| e.path).collect();
    let raw: Vec<SerialPortInfo> = serialport::available_ports()?;

    let mut out: Vec<UsbPortSurveyEntry> = raw
        .into_iter()
        .map(|p| {
            let usb = match &p.port_type {
                SerialPortType::UsbPort(info) => Some(UsbPortSurveyUsb {
                    vid: info.vid,
                    pid: info.pid,
                    vid_pid_hex: format!("{:04x}:{:04x}", info.vid, info.pid),
                    manufacturer: info.manufacturer.clone(),
                    product: info.product.clone(),
                    serial_number: info.serial_number.clone(),
                    usb_interface: info.interface,
                }),
                _ => None,
            };
            UsbPortSurveyEntry {
                would_list_in_tyutool: listed.contains(&p.port_name),
                port_path: p.port_name,
                port_type: port_type_tag(&p.port_type),
                usb,
            }
        })
        .collect();

    out.sort_by(|a, b| a.port_path.cmp(&b.port_path));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::usb_port_survey;

    #[test]
    fn survey_runs_without_panic() {
        let rows = usb_port_survey().expect("usb_port_survey");
        for e in &rows {
            assert!(!e.port_path.is_empty());
            if let Some(u) = &e.usb {
                assert_eq!(u.vid_pid_hex.len(), 9);
                assert!(u.vid_pid_hex.contains(':'));
            }
        }
    }
}
