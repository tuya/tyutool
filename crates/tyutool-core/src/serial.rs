use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

use serde::Serialize;

use crate::error::FlashError;
use crate::plugins::beken::frame::ProtocolError;
use crate::plugins::beken::transport::{SerialIo, Transport};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialPortEntry {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usb_vid: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usb_pid: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usb_serial: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usb_interface: Option<u8>,
    /// Machine key for i18n: `flash_auth`, `log`, etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_role: Option<String>,
}

/// Produce a human-readable label for the serial port type.
fn port_type_label(pt: &serialport::SerialPortType) -> Option<String> {
    match pt {
        serialport::SerialPortType::UsbPort(info) => {
            let mut parts: Vec<String> = Vec::new();
            if let Some(ref m) = info.manufacturer {
                parts.push(m.clone());
            }
            if let Some(ref p) = info.product {
                parts.push(p.clone());
            }
            if parts.is_empty() {
                Some(format!("USB [{:04x}:{:04x}]", info.vid, info.pid))
            } else {
                Some(parts.join(" "))
            }
        }
        serialport::SerialPortType::PciPort => Some("PCI".into()),
        serialport::SerialPortType::BluetoothPort => Some("Bluetooth".into()),
        serialport::SerialPortType::Unknown => None,
    }
}

/// Returns `true` for built-in/legacy serial ports that are almost certainly
/// not real hardware (e.g. `/dev/ttyS0`–`/dev/ttyS31` on most Linux boxes).
fn is_phantom_port(path: &str, pt: &serialport::SerialPortType) -> bool {
    // On Linux, /dev/ttyS* ports with type Unknown are legacy 8250/16550 UARTs
    // that exist in every kernel config but are rarely wired to anything.
    // Real USB/ACM/PCI serial devices have a known type.
    if cfg!(target_os = "linux") {
        if matches!(pt, serialport::SerialPortType::Unknown) && path.starts_with("/dev/ttyS") {
            return true;
        }
    }
    false
}

/// macOS lists every device twice as `/dev/tty.*` (dial-in) and `/dev/cu.*` (call-out).
/// Outbound flashing should use `cu` only; `tty` duplicates confuse users.
/// Also drop built-in Bluetooth RFCOMM and PCI UART stubs (e.g. `URT0`) that are
/// rarely used for embedded flashing.
#[cfg(target_os = "macos")]
fn should_list_serial_port_macos(path: &str, pt: &serialport::SerialPortType) -> bool {
    if !path.starts_with("/dev/cu.") {
        return false;
    }
    if path.to_ascii_lowercase().contains("bluetooth") {
        return false;
    }
    if matches!(
        pt,
        serialport::SerialPortType::BluetoothPort | serialport::SerialPortType::PciPort
    ) {
        return false;
    }
    true
}

#[cfg(not(target_os = "macos"))]
#[inline]
fn should_list_serial_port_macos(_path: &str, _pt: &serialport::SerialPortType) -> bool {
    true
}

#[cfg(any(target_os = "macos", test))]
fn extract_ioreg_quoted_value(line: &str, key: &str) -> Option<String> {
    let marker = format!("\"{}\" = \"", key);
    let start = line.find(&marker)? + marker.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

#[cfg(any(target_os = "macos", test))]
fn extract_ioreg_u8_value(line: &str, key: &str) -> Option<u8> {
    let marker = format!("\"{}\" = ", key);
    let start = line.find(&marker)? + marker.len();
    let digits: String = line[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

#[cfg(any(target_os = "macos", test))]
fn parse_macos_ioreg_usb_interfaces(ioreg: &str) -> HashMap<String, u8> {
    let mut out = HashMap::new();
    let mut interface_number: Option<u8> = None;
    let mut is_cdc_data_interface = false;

    for line in ioreg.lines() {
        if line.contains("+-o IOUSBHostInterface@") {
            interface_number = None;
            is_cdc_data_interface = false;
            continue;
        }
        if let Some(n) = extract_ioreg_u8_value(line, "bInterfaceNumber") {
            interface_number = Some(n);
            continue;
        }
        if extract_ioreg_u8_value(line, "bInterfaceClass") == Some(10) {
            is_cdc_data_interface = true;
            continue;
        }
        if let Some(path) = extract_ioreg_quoted_value(line, "IOCalloutDevice") {
            if is_cdc_data_interface {
                if let Some(n) = interface_number {
                    out.insert(path, n);
                }
            }
        }
    }

    out
}

#[cfg(target_os = "macos")]
fn macos_usb_interface_overrides() -> HashMap<String, u8> {
    let output = std::process::Command::new("ioreg")
        .args([
            "-p",
            "IOService",
            "-r",
            "-c",
            "IOUSBHostInterface",
            "-l",
            "-w",
            "0",
        ])
        .output();
    let Ok(output) = output else {
        return HashMap::new();
    };
    if !output.status.success() {
        return HashMap::new();
    }
    parse_macos_ioreg_usb_interfaces(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(target_os = "macos"))]
fn macos_usb_interface_overrides() -> HashMap<String, u8> {
    HashMap::new()
}

/// List serial ports (Python `list_ports.comports()` equivalent).
///
/// Filters out phantom legacy ports (e.g. `/dev/ttyS*` with Unknown type) that
/// are not backed by real hardware.
pub fn list_serial_ports() -> Result<Vec<SerialPortEntry>, FlashError> {
    let ports = serialport::available_ports()?;
    let macos_interfaces = macos_usb_interface_overrides();
    log::debug!("Raw serial ports found: {}", ports.len());
    let mut out: Vec<SerialPortEntry> = ports
        .into_iter()
        .filter(|p| {
            !is_phantom_port(&p.port_name, &p.port_type)
                && should_list_serial_port_macos(&p.port_name, &p.port_type)
        })
        .map(|p| {
            let (usb_vid, usb_pid, usb_serial, usb_interface) = match &p.port_type {
                serialport::SerialPortType::UsbPort(info) => (
                    Some(info.vid),
                    Some(info.pid),
                    info.serial_number.clone(),
                    info.interface
                        .or_else(|| macos_interfaces.get(&p.port_name).copied()),
                ),
                _ => (None, None, None, None),
            };
            let port_role = match &p.port_type {
                serialport::SerialPortType::UsbPort(info) => {
                    crate::tuya_dev_usb::infer_usb_port_role(info.vid, info.pid, usb_interface)
                        .map(str::to_string)
                }
                _ => None,
            };
            let entry = SerialPortEntry {
                path: p.port_name.clone(),
                name: port_type_label(&p.port_type),
                usb_vid,
                usb_pid,
                usb_serial,
                usb_interface,
                port_role,
            };
            log::trace!(
                "[Serial][USB] path={} name={:?} vid={:?} pid={:?} serial={:?} interface={:?} role={:?}",
                entry.path,
                entry.name,
                entry.usb_vid,
                entry.usb_pid,
                entry.usb_serial,
                entry.usb_interface,
                entry.port_role
            );
            entry
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    log::info!("Serial ports after filtering: {} device(s)", out.len());
    for entry in &out {
        log::debug!(
            "  Port: {} ({})",
            entry.path,
            entry.name.as_deref().unwrap_or("unknown")
        );
    }
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────
// Port availability check — detect busy ports and identify processes
// ─────────────────────────────────────────────────────────────────────────

/// Result of checking whether a serial port is available.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortCheckResult {
    pub available: bool,
    pub error_message: Option<String>,
    pub process_info: Option<String>,
    pub kill_hint: Option<String>,
}

/// Check if a serial port can be opened. If not, attempt to identify
/// which process is using it and suggest a platform-specific kill command.
pub fn check_port_available(port: &str) -> PortCheckResult {
    match serialport::new(port, 9600)
        .timeout(std::time::Duration::from_millis(100))
        .open()
    {
        Ok(_port) => {
            // Port opened successfully — it's available
            // (port is dropped here, releasing the handle)
            log::info!("Port {} is available", port);
            PortCheckResult {
                available: true,
                error_message: None,
                process_info: None,
                kill_hint: None,
            }
        }
        Err(e) => {
            log::warn!("Port {} unavailable: {}", port, e);
            let error_message = format!("{}", e);
            let (process_info, kill_hint) = detect_port_usage(port);
            PortCheckResult {
                available: false,
                error_message: Some(error_message),
                process_info,
                kill_hint,
            }
        }
    }
}

/// Detect which process is using a serial port (platform-specific).
fn detect_port_usage(port: &str) -> (Option<String>, Option<String>) {
    #[cfg(target_os = "linux")]
    {
        // Try fuser first
        if let Ok(output) = std::process::Command::new("fuser").arg(port).output() {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let combined = if stdout.is_empty() { stderr } else { stdout };
            if !combined.is_empty() {
                return (Some(combined), Some(format!("sudo fuser -k {}", port)));
            }
        }
        // Fallback to lsof
        if let Ok(output) = std::process::Command::new("lsof").arg(port).output() {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !stdout.is_empty() {
                return (Some(stdout), Some(format!("sudo fuser -k {}", port)));
            }
        }
        (None, None)
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("lsof").arg(port).output() {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !stdout.is_empty() {
                return (
                    Some(stdout),
                    Some(format!(
                        "lsof {} | awk 'NR>1 {{print $2}}' | xargs kill",
                        port
                    )),
                );
            }
        }
        (None, None)
    }
    #[cfg(target_os = "windows")]
    {
        let _ = port; // suppress unused warning
        (
            Some("Another program may be using this port.".into()),
            Some("Close the other program or check Device Manager.".into()),
        )
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = port;
        (None, None)
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Hardware reset — aligned with flash auto-reset per platform
// ─────────────────────────────────────────────────────────────────────────

fn protocol_err_to_flash(e: ProtocolError) -> FlashError {
    match e {
        ProtocolError::Cancelled => FlashError::Cancelled,
        ProtocolError::Io(err) => FlashError::Io(err),
        _ => FlashError::Plugin(e.to_string()),
    }
}

/// Same Beken DTR/RTS pulse as [`crate::plugins::beken::ops::shake`] uses to enter download mode:
/// `Transport::reset_into_download_mode_bk` (BK7231N, T2) vs `reset_into_download_mode_t5` (T5, T3, T1).
fn device_reset_beken_match_flash_shake(port: &str, chip_upper: &str) -> Result<(), FlashError> {
    let io = SerialIo::open(port, 115_200).map_err(protocol_err_to_flash)?;
    let cancel = AtomicBool::new(false);
    let noop: &dyn Fn(&str) = &|_| {};
    let mut transport = Transport::new(io, port, 115_200, &cancel, noop);

    let t5_style = matches!(chip_upper, "T5" | "T3" | "T1");
    let r = if t5_style {
        transport.reset_into_download_mode_t5()
    } else {
        transport.reset_into_download_mode_bk()
    };
    r.map_err(protocol_err_to_flash)
}

/// Pulse UART control lines to reset the device — **same strategies as automatic reset during flash**:
///
/// - **Beken (BK7231N, T2, T3, T5, T1)**: `Transport::reset_into_download_mode_bk` (BK/T2) or
///   `reset_into_download_mode_t5` (T5/T3/T1), matching `plugins::beken::ops::shake`.
/// - **ESP32 (all variants)**: espflash `hard_reset` / `reset_after_flash` (USB PID–aware), matching
///   post-flash reset in `plugins::esp::common::run_esp`.
pub fn device_reset_dtr_rts(port: &str, chip_id: &str) -> Result<(), FlashError> {
    let key = chip_id.trim().to_ascii_uppercase();
    if key.starts_with("ESP32") {
        return crate::plugins::esp::common::esp_uart_hard_reset(port);
    }
    device_reset_beken_match_flash_shake(port, &key)
}

#[cfg(test)]
mod hw_reset_tests {
    #[test]
    fn beken_pulse_variant_matches_shake_routing() {
        let t5_style = |chip: &str| matches!(chip, "T5" | "T3" | "T1");
        assert!(t5_style("T5"));
        assert!(t5_style("T3"));
        assert!(t5_style("T1"));
        assert!(!t5_style("T2"));
        assert!(!t5_style("BK7231N"));
    }
}

#[cfg(test)]
mod macos_ioreg_parse_tests {
    use super::parse_macos_ioreg_usb_interfaces;

    #[test]
    fn maps_cdc_data_interface_to_callout_device() {
        let ioreg = r#"
+-o IOUSBHostInterface@1  <class IOUSBHostInterface>
  | {
  |   "bInterfaceClass" = 10
  |   "bInterfaceNumber" = 1
  | }
  +-o AppleUSBACMData  <class AppleUSBACMData>
    | {
    |   "IOTTYSuffix" = "56D70351251"
    | }
    +-o IOSerialBSDClient  <class IOSerialBSDClient>
        {
          "IOCalloutDevice" = "/dev/cu.usbmodem56D70351251"
        }
+-o IOUSBHostInterface@3  <class IOUSBHostInterface>
  | {
  |   "bInterfaceClass" = 10
  |   "bInterfaceNumber" = 3
  | }
  +-o AppleUSBACMData  <class AppleUSBACMData>
    +-o IOSerialBSDClient  <class IOSerialBSDClient>
        {
          "IOCalloutDevice" = "/dev/cu.usbmodem56D70351253"
        }
"#;
        let parsed = parse_macos_ioreg_usb_interfaces(ioreg);

        assert_eq!(parsed.get("/dev/cu.usbmodem56D70351251"), Some(&1));
        assert_eq!(parsed.get("/dev/cu.usbmodem56D70351253"), Some(&3));
    }

    #[test]
    fn ignores_control_interface_without_cdc_data_class() {
        let ioreg = r#"
+-o IOUSBHostInterface@0  <class IOUSBHostInterface>
  | {
  |   "bInterfaceClass" = 2
  |   "bInterfaceNumber" = 0
  | }
  +-o IOSerialBSDClient  <class IOSerialBSDClient>
      {
        "IOCalloutDevice" = "/dev/cu.should-not-map"
      }
"#;
        let parsed = parse_macos_ioreg_usb_interfaces(ioreg);

        assert!(!parsed.contains_key("/dev/cu.should-not-map"));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod macos_serial_list_tests {
    use super::should_list_serial_port_macos;
    use serialport::{SerialPortType, UsbPortInfo};

    fn usb_dual_serial() -> SerialPortType {
        SerialPortType::UsbPort(UsbPortInfo {
            vid: 0x303a,
            pid: 0x1001,
            serial_number: None,
            manufacturer: None,
            product: Some("Dual_Serial".into()),
            interface: None,
        })
    }

    #[test]
    fn keeps_cu_usb_and_drops_tty_duplicate() {
        assert!(should_list_serial_port_macos(
            "/dev/cu.usbmodem56D70351251",
            &usb_dual_serial()
        ));
        assert!(!should_list_serial_port_macos(
            "/dev/tty.usbmodem56D70351251",
            &usb_dual_serial()
        ));
    }

    #[test]
    fn drops_bluetooth_incoming_and_pci_uart() {
        assert!(!should_list_serial_port_macos(
            "/dev/cu.Bluetooth-Incoming-Port",
            &SerialPortType::BluetoothPort
        ));
        assert!(!should_list_serial_port_macos(
            "/dev/cu.URT0",
            &SerialPortType::PciPort
        ));
    }
}
