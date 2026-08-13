use std::collections::HashMap;
use std::thread::sleep;
use std::time::Duration;

use serde::Serialize;

use crate::error::FlashError;

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
    if cfg!(target_os = "linux")
        && matches!(pt, serialport::SerialPortType::Unknown)
        && path.starts_with("/dev/ttyS")
    {
        return true;
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
    /// 人类可读的占用者，如 `"ModemManager (812)"`；认不出来时 None（不许编）。
    /// 与 `process_info`（原始 fuser/lsof 输出，只进日志）分开：这一条是要给
    /// 用户看的——Ubuntu 上占口的往往是 ModemManager 这类系统服务，
    /// 只说「被其他程序占用」用户根本无从下手。
    pub holders: Option<String>,
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
                holders: None,
            }
        }
        Err(e) => {
            log::warn!("Port {} unavailable: {}", port, e);
            let error_message = format!("{}", e);
            let (usage, kill_hint) = detect_port_usage(port);
            let holders =
                describe_port_holders(usage.as_ref().map(|(src, raw)| (*src, raw.as_str())));
            PortCheckResult {
                available: false,
                error_message: Some(error_message),
                // 原始输出仍原样进日志（诊断用），只是现在带着来源标签
                process_info: usage.map(|(_, raw)| raw),
                kill_hint,
                holders,
            }
        }
    }
}

/// 占用信息是哪个工具产出的——**解析方式必须由来源决定，不能靠猜**。
///
/// ⚠ 二次 CR 抓到的真缺陷：早先版本不记来源，对任何 raw 都先按 fuser 解析。
/// 而 lsof 表格里的 FD(`9u`) / DEVICE(`166,0`) / NODE(`221`) 全是数字列，
/// 会被当成 PID 去读 `/proc/<n>/comm`——在真机上有些号真能解析出**无关的真实进程名**。
/// 报错名字比不报名字更坏（用户会去关一个无辜的进程），直接违背本模块「绝不编造」的承诺。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// 各变体的构造点分散在 cfg 平台分支里（Fuser 只在 Linux、Opaque 只在 Windows），
// 单平台构建下必有变体「未被构造」——测试里三个都用得到
#[allow(dead_code)]
pub(crate) enum PortUsageSource {
    /// `fuser <port>`：只给 PID，名字要再读 `/proc/<pid>/comm`
    Fuser,
    /// `lsof <port>`：表格首列就是进程名
    Lsof,
    /// 平台拿不到真实占用者（Windows 的静态提示语）——不许当名字用
    Opaque,
}

/// 按来源决定「这串文本里能读出什么」。纯函数，便于单测钉住上面那条 CR 缺陷。
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
pub(crate) fn holder_hint(source: PortUsageSource, raw: &str) -> HolderHint {
    match source {
        PortUsageSource::Fuser => HolderHint::Pids(parse_fuser_pids(raw)),
        PortUsageSource::Lsof => HolderHint::Names(parse_lsof_commands(raw)),
        PortUsageSource::Opaque => HolderHint::Names(Vec::new()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
pub(crate) enum HolderHint {
    Pids(Vec<u32>),
    Names(Vec<String>),
}

/// 把 `detect_port_usage` 的原始输出提炼成用户看得懂的占用者名字。
/// Linux：fuser 只给 PID，名字要再读 `/proc/<pid>/comm`（纯文件读，无额外依赖）；
/// 进程已退出 / 无权限读时该条静默丢弃（宁可少报，绝不编）。
/// macOS：lsof 首列就是进程名。Windows：拿不到，返回 None 让上层用通用文案。
#[allow(unused_variables)]
fn describe_port_holders(usage: Option<(PortUsageSource, &str)>) -> Option<String> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let (source, raw) = usage?;
        let entries: Vec<(String, Option<u32>)> = match holder_hint(source, raw) {
            HolderHint::Pids(pids) => pids
                .into_iter()
                .filter_map(|pid| {
                    std::fs::read_to_string(format!("/proc/{pid}/comm"))
                        .ok()
                        .map(|name| (name.trim().to_string(), Some(pid)))
                        .filter(|(name, _)| !name.is_empty())
                })
                .collect(),
            HolderHint::Names(names) => names.into_iter().map(|name| (name, None)).collect(),
        };
        format_holders(&entries)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// Detect which process is using a serial port (platform-specific).
fn detect_port_usage(port: &str) -> (Option<(PortUsageSource, String)>, Option<String>) {
    #[cfg(target_os = "linux")]
    {
        // Try fuser first
        if let Ok(output) = std::process::Command::new("fuser").arg(port).output() {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let combined = if stdout.is_empty() { stderr } else { stdout };
            if !combined.is_empty() {
                return (
                    Some((PortUsageSource::Fuser, combined)),
                    Some(format!("sudo fuser -k {}", port)),
                );
            }
        }
        // Fallback to lsof
        if let Ok(output) = std::process::Command::new("lsof").arg(port).output() {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !stdout.is_empty() {
                return (
                    Some((PortUsageSource::Lsof, stdout)),
                    Some(format!("sudo fuser -k {}", port)),
                );
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
                    Some((PortUsageSource::Lsof, stdout)),
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
            // Opaque：这是一句提示语，不是占用者名字——holder_hint 会拒绝拿它当名字用
            Some((
                PortUsageSource::Opaque,
                "Another program may be using this port.".to_string(),
            )),
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

const ESP_USB_SERIAL_JTAG_PID: u16 = 0x1001;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeviceResetStrategy {
    Esp32HardReset { usb_pid: u16 },
    BekenBk,
    BekenT5Ai,
}

fn device_reset_strategy_for_chip(chip_id: &str, usb_pid: u16) -> DeviceResetStrategy {
    let key = crate::registry::normalize_chip_id(chip_id);
    if key.starts_with("ESP32") {
        return DeviceResetStrategy::Esp32HardReset { usb_pid };
    }
    if matches!(key.as_str(), "T5AI" | "T3" | "T1") {
        return DeviceResetStrategy::BekenT5Ai;
    }
    DeviceResetStrategy::BekenBk
}

fn usb_pid_for_port(port_name: &str) -> u16 {
    serialport::available_ports()
        .ok()
        .and_then(|ports| {
            ports.into_iter().find_map(|port| {
                if port.port_name != port_name {
                    return None;
                }
                match port.port_type {
                    serialport::SerialPortType::UsbPort(info) => Some(info.pid),
                    _ => None,
                }
            })
        })
        .unwrap_or(0)
}

fn write_dtr(port: &mut dyn serialport::SerialPort, level: bool) -> Result<(), FlashError> {
    port.write_data_terminal_ready(level)
        .map_err(|e| FlashError::Plugin(e.to_string()))
}

fn write_rts(port: &mut dyn serialport::SerialPort, level: bool) -> Result<(), FlashError> {
    port.write_request_to_send(level)
        .map_err(|e| FlashError::Plugin(e.to_string()))
}

fn device_reset_with_strategy(
    port: &mut dyn serialport::SerialPort,
    strategy: DeviceResetStrategy,
) -> Result<(), FlashError> {
    log::info!("[serial] device reset via {:?}", strategy);
    match strategy {
        DeviceResetStrategy::BekenBk => {
            write_dtr(port, false)?;
            write_rts(port, true)?;
            sleep(Duration::from_millis(200));
            write_rts(port, false)?;
            Ok(())
        }
        DeviceResetStrategy::BekenT5Ai => {
            write_dtr(port, false)?;
            write_rts(port, true)?;
            sleep(Duration::from_millis(300));
            write_rts(port, false)?;
            Ok(())
        }
        DeviceResetStrategy::Esp32HardReset { usb_pid } => {
            sleep(Duration::from_millis(100));
            if usb_pid == ESP_USB_SERIAL_JTAG_PID {
                write_dtr(port, false)?;
                sleep(Duration::from_millis(100));
                write_rts(port, true)?;
                write_dtr(port, false)?;
                write_rts(port, true)?;
                sleep(Duration::from_millis(100));
                write_rts(port, false)?;
            } else {
                write_rts(port, true)?;
                sleep(Duration::from_millis(100));
                write_rts(port, false)?;
            }
            Ok(())
        }
    }
}

pub(crate) fn device_reset_serial_port(
    port_name: &str,
    port: &mut dyn serialport::SerialPort,
    chip_id: &str,
) -> Result<(), FlashError> {
    let strategy = device_reset_strategy_for_chip(chip_id, usb_pid_for_port(port_name));
    device_reset_with_strategy(port, strategy)
}

/// Pulse UART control lines to reset the device — **same strategies as automatic reset during flash**:
///
/// - **Beken (BK7231N, T2, T3, T5AI, T1)**: `Transport::reset_into_download_mode_bk` (BK/T2) or
///   `reset_into_download_mode_t5ai` (T5AI/T3/T1), matching `plugins::beken::ops::shake`.
/// - **ESP32 (all variants)**: espflash `hard_reset` / `reset_after_flash` (USB PID–aware), matching
///   post-flash reset in `plugins::esp::common::run_esp`.
pub fn device_reset_dtr_rts(port: &str, chip_id: &str) -> Result<(), FlashError> {
    let mut handle = serialport::new(port, 115_200)
        .timeout(Duration::from_millis(500))
        .open()
        .map_err(|e| FlashError::Plugin(format!("cannot open port {port}: {e}")))?;
    device_reset_serial_port(port, &mut *handle, chip_id)
}

#[cfg(test)]
mod hw_reset_tests {
    use super::{device_reset_strategy_for_chip, DeviceResetStrategy};

    #[test]
    fn beken_pulse_variant_matches_shake_routing() {
        assert_eq!(
            device_reset_strategy_for_chip("T5AI", 0),
            DeviceResetStrategy::BekenT5Ai
        );
        assert_eq!(
            device_reset_strategy_for_chip("T3", 0),
            DeviceResetStrategy::BekenT5Ai
        );
        assert_eq!(
            device_reset_strategy_for_chip("T1", 0),
            DeviceResetStrategy::BekenT5Ai
        );
        assert_eq!(
            device_reset_strategy_for_chip("T2", 0),
            DeviceResetStrategy::BekenBk
        );
        assert_eq!(
            device_reset_strategy_for_chip("BK7231N", 0),
            DeviceResetStrategy::BekenBk
        );
    }

    #[test]
    fn esp32_variants_route_to_hard_reset_strategy() {
        assert_eq!(
            device_reset_strategy_for_chip("ESP32", 0x1001),
            DeviceResetStrategy::Esp32HardReset { usb_pid: 0x1001 }
        );
        assert_eq!(
            device_reset_strategy_for_chip("ESP32C3", 0),
            DeviceResetStrategy::Esp32HardReset { usb_pid: 0 }
        );
    }
}

#[cfg(test)]
mod phantom_port_tests {
    use super::is_phantom_port;
    use serialport::{SerialPortType, UsbPortInfo};

    fn usb() -> SerialPortType {
        SerialPortType::UsbPort(UsbPortInfo {
            vid: 0x1a86,
            pid: 0x55d2,
            serial_number: None,
            manufacturer: None,
            product: None,
            interface: None,
        })
    }

    #[test]
    fn legacy_ttys_with_unknown_type_is_phantom_only_on_linux() {
        let phantom = is_phantom_port("/dev/ttyS0", &SerialPortType::Unknown);
        // The classification is Linux-specific; on other targets it is never phantom.
        assert_eq!(phantom, cfg!(target_os = "linux"));
        assert_eq!(
            is_phantom_port("/dev/ttyS31", &SerialPortType::Unknown),
            cfg!(target_os = "linux")
        );
    }

    #[test]
    fn usb_and_acm_ports_are_never_phantom() {
        // Real USB device with a known type — never phantom on any platform.
        assert!(!is_phantom_port("/dev/ttyUSB0", &usb()));
        // ACM / non-ttyS paths with Unknown type — not phantom.
        assert!(!is_phantom_port("/dev/ttyACM0", &SerialPortType::Unknown));
        // A ttyS path but with a known (non-Unknown) type — not phantom.
        assert!(!is_phantom_port("/dev/ttyS0", &usb()));
    }
}

#[cfg(test)]
mod ioreg_extract_tests {
    use super::{extract_ioreg_quoted_value, extract_ioreg_u8_value};

    #[test]
    fn quoted_value_extracts_callout_device_path() {
        let line = r#"        "IOCalloutDevice" = "/dev/cu.usbmodem56D70351251""#;
        assert_eq!(
            extract_ioreg_quoted_value(line, "IOCalloutDevice").as_deref(),
            Some("/dev/cu.usbmodem56D70351251")
        );
    }

    #[test]
    fn quoted_value_returns_none_for_absent_or_malformed_key() {
        // Key not present.
        assert_eq!(
            extract_ioreg_quoted_value("nothing here", "IOCalloutDevice"),
            None
        );
        // Opening quote present but no closing quote.
        let line = r#"   "IOCalloutDevice" = "/dev/cu.unterminated"#;
        assert_eq!(extract_ioreg_quoted_value(line, "IOCalloutDevice"), None);
    }

    #[test]
    fn u8_value_parses_interface_fields() {
        let line = r#"      "bInterfaceNumber" = 3"#;
        assert_eq!(extract_ioreg_u8_value(line, "bInterfaceNumber"), Some(3));
        let cls = r#"      "bInterfaceClass" = 10"#;
        assert_eq!(extract_ioreg_u8_value(cls, "bInterfaceClass"), Some(10));
    }

    #[test]
    fn u8_value_returns_none_when_missing_or_non_numeric() {
        // Key absent.
        assert_eq!(
            extract_ioreg_u8_value("unrelated line", "bInterfaceNumber"),
            None
        );
        // Value present but not a digit immediately after the marker.
        let line = r#"      "bInterfaceNumber" = Yes"#;
        assert_eq!(extract_ioreg_u8_value(line, "bInterfaceNumber"), None);
    }

    #[test]
    fn u8_value_overflow_returns_none() {
        // 256 does not fit in u8; parse must fail and return None.
        let line = r#"      "bInterfaceNumber" = 256"#;
        assert_eq!(extract_ioreg_u8_value(line, "bInterfaceNumber"), None);
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

    #[test]
    fn cdc_data_flag_resets_at_each_new_host_interface() {
        // The first interface is CDC data (class 10); the second is a control
        // interface (class 2). The new `+-o IOUSBHostInterface@` boundary must
        // reset the cdc-data flag so the second callout device is NOT mapped.
        let ioreg = r#"
+-o IOUSBHostInterface@1  <class IOUSBHostInterface>
  | {
  |   "bInterfaceClass" = 10
  |   "bInterfaceNumber" = 1
  | }
  +-o IOSerialBSDClient  <class IOSerialBSDClient>
      {
        "IOCalloutDevice" = "/dev/cu.mapped"
      }
+-o IOUSBHostInterface@0  <class IOUSBHostInterface>
  | {
  |   "bInterfaceClass" = 2
  |   "bInterfaceNumber" = 0
  | }
  +-o IOSerialBSDClient  <class IOSerialBSDClient>
      {
        "IOCalloutDevice" = "/dev/cu.unmapped"
      }
"#;
        let parsed = parse_macos_ioreg_usb_interfaces(ioreg);
        assert_eq!(parsed.get("/dev/cu.mapped"), Some(&1));
        assert!(!parsed.contains_key("/dev/cu.unmapped"));
    }

    #[test]
    fn empty_input_yields_empty_map() {
        assert!(parse_macos_ioreg_usb_interfaces("").is_empty());
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

// ─────────────────────────────────────────────────────────────────────────
// Port holder naming — turn `fuser` / `lsof` raw output into names a user
// can act on (Ubuntu's ModemManager being the case that motivated this).
// ─────────────────────────────────────────────────────────────────────────

/// PIDs out of `fuser <port>` output (`"/dev/ttyACM0: 1234 5678"` or bare
/// `"1234 5678"`; fuser writes the port label to stderr and PIDs to stdout,
/// and callers may hand us either stream — so tolerate both shapes).
// 只在 Linux 分支有调用方（测试里两平台都跑），macOS/Windows 构建下豁免 dead_code
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn parse_fuser_pids(raw: &str) -> Vec<u32> {
    raw.split(|c: char| c == ':' || c.is_whitespace())
        .filter_map(|tok| {
            // fuser appends access-type letters to PIDs (`1234c`, `5678f`)
            let digits: String = tok.chars().take_while(|c| c.is_ascii_digit()).collect();
            if digits.is_empty() {
                None
            } else {
                digits.parse::<u32>().ok()
            }
        })
        .collect()
}

/// Process names out of `lsof <port>` table output (first column, header row
/// and blank lines dropped, de-duplicated, order preserved).
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
pub(crate) fn parse_lsof_commands(raw: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for line in raw.lines() {
        let Some(first) = line.split_whitespace().next() else {
            continue;
        };
        if first == "COMMAND" {
            continue;
        }
        if !names.iter().any(|n| n == first) {
            names.push(first.to_string());
        }
    }
    names
}

/// Human-readable holder list: `"ModemManager (1234)"`, joined by `, `.
/// `None` when nothing could be named — callers must not invent a name.
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
pub(crate) fn format_holders(entries: &[(String, Option<u32>)]) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let text = entries
        .iter()
        .map(|(name, pid)| match pid {
            Some(pid) => format!("{name} ({pid})"),
            None => name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    Some(text)
}

#[cfg(test)]
mod port_holder_tests {
    use super::{format_holders, parse_fuser_pids, parse_lsof_commands};

    #[test]
    fn fuser_pids_parse_from_labelled_and_bare_output() {
        // 真实 fuser 输出：端口标签在 stderr、PID 在 stdout，调用方两股都可能递过来
        assert_eq!(
            parse_fuser_pids("/dev/ttyACM0: 1234 5678"),
            vec![1234, 5678]
        );
        assert_eq!(parse_fuser_pids(" 1234\n"), vec![1234]);
        // 访问类型后缀（c=cwd, f=open file…）必须剥掉，否则 parse 失败整条丢
        assert_eq!(
            parse_fuser_pids("/dev/ttyACM0: 1234c 5678f"),
            vec![1234, 5678]
        );
        assert!(parse_fuser_pids("").is_empty());
        assert!(parse_fuser_pids("/dev/ttyACM0:").is_empty());
    }

    #[test]
    fn lsof_commands_drop_header_and_dedupe() {
        let raw = "COMMAND     PID  USER   FD   TYPE DEVICE SIZE/OFF NODE NAME\n\
                   ModemManager 812  root    9u   CHR 166,0      0t0  221 /dev/ttyACM0\n\
                   ModemManager 812  root   10u   CHR 166,0      0t0  221 /dev/ttyACM0\n\
                   picocom     4021 user    3u   CHR 166,0      0t0  221 /dev/ttyACM0";
        assert_eq!(
            parse_lsof_commands(raw),
            vec!["ModemManager".to_string(), "picocom".to_string()]
        );
        assert!(parse_lsof_commands("").is_empty());
    }

    /// 二次 CR 抓到的真缺陷的回归护栏：lsof 表格**绝不能**按 fuser 解析。
    /// 它的 FD(`9u`) / DEVICE(`166,0`) / NODE(`221`) 都是数字列，按 PID 解析会去读
    /// `/proc/9/comm` 之类，真机上有些号能读出**无关的真实进程名**——
    /// 报错名字比不报名字更坏（用户会去关一个无辜的进程）。
    #[test]
    fn holder_hint_is_decided_by_source_not_guessed() {
        use super::{holder_hint, HolderHint, PortUsageSource};
        let lsof = "COMMAND     PID  USER   FD   TYPE DEVICE SIZE/OFF NODE NAME\n\
                    ModemManager 812  root    9u   CHR 166,0      0t0  221 /dev/ttyACM0";
        // lsof 来源 → 只读首列名字，一个数字都不许当 PID
        assert_eq!(
            holder_hint(PortUsageSource::Lsof, lsof),
            HolderHint::Names(vec!["ModemManager".to_string()])
        );
        // fuser 来源 → 才按 PID 解析
        assert_eq!(
            holder_hint(PortUsageSource::Fuser, "/dev/ttyACM0: 812"),
            HolderHint::Pids(vec![812])
        );
        // Windows 的静态提示语不是名字，不许拿去当占用者
        assert_eq!(
            holder_hint(
                PortUsageSource::Opaque,
                "Another program may be using this port."
            ),
            HolderHint::Names(Vec::new())
        );
    }

    #[test]
    fn holders_format_with_and_without_pid() {
        assert_eq!(
            format_holders(&[("ModemManager".into(), Some(812))]).as_deref(),
            Some("ModemManager (812)")
        );
        assert_eq!(
            format_holders(&[("ModemManager".into(), Some(812)), ("brltty".into(), None)])
                .as_deref(),
            Some("ModemManager (812), brltty")
        );
        // 一个都没认出来时不许编：宁可回退到通用文案
        assert_eq!(format_holders(&[]), None);
    }
}
