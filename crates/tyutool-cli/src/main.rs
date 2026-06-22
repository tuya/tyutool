use std::io::{IsTerminal as _, Write as _};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::{builder::PossibleValuesParser, CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use tyutool_core::{
    device_reset_dtr_rts, list_serial_ports, run_job, usb_port_survey, FlashJob, FlashMode,
};

mod reporter;
use reporter::CliReporter;
mod serve;
mod update;

#[derive(Parser)]
#[command(name = "tyutool", version, about = "Tuya Uart Tool.")]
struct Cli {
    /// Also write developer logs to stderr (always writes to log file)
    #[arg(long, global = true)]
    verbose: bool,

    /// Force plain text output (ASCII, no spinner/progress bar)
    #[arg(long, global = true)]
    plain: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Flash firmware to device
    Write {
        /// Soc name
        #[arg(short = 'd', long = "device", value_parser = chip_value_parser())]
        device: String,
        /// Target port
        #[arg(short = 'p', long = "port")]
        port: Option<String>,
        /// Uart baud rate
        #[arg(short = 'b', long = "baud")]
        baud: Option<u32>,
        /// Flash address of start (hex, e.g. 0x0)
        #[arg(short = 's', long = "start")]
        start: Option<String>,
        /// Flash address of end (hex, optional; defaults to start + file size)
        #[arg(long = "end")]
        end: Option<String>,
        /// Firmware BIN file
        #[arg(short = 'f', long = "file")]
        file: String,
    },
    /// Read flash from device
    Read {
        /// Soc name
        #[arg(short = 'd', long = "device", value_parser = chip_value_parser())]
        device: String,
        /// Target port
        #[arg(short = 'p', long = "port")]
        port: Option<String>,
        /// Uart baud rate
        #[arg(short = 'b', long = "baud")]
        baud: Option<u32>,
        /// Flash address of start (hex, e.g. 0x0)
        #[arg(short = 's', long = "start")]
        start: Option<String>,
        /// Flash read length (hex, default 0x200000)
        #[arg(short = 'l', long = "length", default_value = "0x200000")]
        length: String,
        /// Output BIN file
        #[arg(short = 'f', long = "file")]
        file: String,
    },
    /// Erase flash on device
    Erase {
        /// Soc name
        #[arg(short = 'd', long = "device", value_parser = chip_value_parser())]
        device: String,
        /// Target port
        #[arg(short = 'p', long = "port")]
        port: Option<String>,
        /// Uart baud rate
        #[arg(short = 'b', long = "baud")]
        baud: Option<u32>,
        /// Erase address of start (hex, e.g. 0x0)
        #[arg(short = 's', long = "start")]
        start: Option<String>,
        /// Erase length (hex, default 0x200000)
        #[arg(short = 'l', long = "length", default_value = "0x200000")]
        length: String,
    },
    /// List serial ports
    ListPorts {
        /// Output as JSON (array of port objects) instead of tab-separated columns
        #[arg(long)]
        json: bool,
    },
    /// Dump raw USB/serial metadata for cross-OS survey (JSON). See `tmp/usb-port-survey.md`.
    UsbPortSurvey,
    /// Hardware-reset the device via DTR/RTS (UART)
    Reset {
        /// Serial port (default: first available)
        #[arg(short = 'p', long = "port")]
        port: Option<String>,
        /// Chip id: Beken uses the same DTR/RTS pulse as flash handshake (bk7231n/t2 vs t5/t3/t1); ESP32* uses espflash hard_reset
        #[arg(short = 'd', long = "device", default_value = "bk7231n")]
        device: String,
    },
    /// Check for updates and self-update the binary
    Update {
        /// Only check version, do not download
        #[arg(long)]
        check: bool,
        /// Update source: github (default) or gitee
        #[arg(long)]
        source: Option<String>,
    },
    /// Start a local WebSocket server for browser-mode flashing (dev only).
    Serve {
        /// WebSocket port to listen on
        #[arg(long, default_value_t = 9527)]
        port: u16,
    },
    /// TuyaOpen device authorization via UART shell (auth-read / auth write)
    Authorize {
        /// Serial port (default: first available)
        #[arg(short = 'p', long = "port")]
        port: Option<String>,
        /// UUID to write (omit to only read current auth state)
        #[arg(long)]
        uuid: Option<String>,
        /// AuthKey to write (omit to only read current auth state)
        #[arg(long)]
        authkey: Option<String>,
    },
    /// Generate a shell completion script and print it to stdout
    Completions {
        /// Target shell
        #[arg(value_enum)]
        shell: Shell,
    },
}

// Single list of accepted `--device` values for write/read/erase. clap needs
// `'static` strings, so we can't build this straight from the core registry at
// runtime — instead `device_list_matches_registry` asserts the two agree, so a
// chip added to the registry can't silently drift out of the CLI.
const SUPPORTED_DEVICES: &[&str] = &[
    "bk7231n", "t2", "t3", "t1", "t5", "ln882h", "esp32", "esp32c3", "esp32c6", "esp32s3",
];

fn chip_value_parser() -> PossibleValuesParser {
    PossibleValuesParser::new(SUPPORTED_DEVICES)
}

// Must stay in sync with `defaultBaudRate` in src/features/firmware-flash/chip-manifests.ts.
// When adding or modifying a chip, update both.
fn default_baud(device: &str) -> u32 {
    match device.to_ascii_lowercase().as_str() {
        "ln882h" => 115200,
        "esp32" | "esp32c3" | "esp32c6" | "esp32s3" => 460800,
        _ => 921600,
    }
}

fn default_start(_device: &str) -> String {
    "0x00000000".to_string()
}

fn choose_port() -> Result<String, Box<dyn std::error::Error>> {
    let ports = list_serial_ports()?;
    if ports.is_empty() {
        return Err("No serial ports found.".into());
    }
    if ports.len() == 1 {
        eprintln!("Using port: {}", ports[0].path);
        return Ok(ports[0].path.clone());
    }
    eprintln!("Available ports:");
    for (i, p) in ports.iter().enumerate() {
        if let Some(ref name) = p.name {
            eprintln!("  [{}] {} ({})", i, p.path, name);
        } else {
            eprintln!("  [{}] {}", i, p.path);
        }
    }
    // Without an interactive terminal (CI, pipes) there is no one to answer the
    // prompt; reading stdin would just hit EOF and surface a confusing parse
    // error. Tell the caller to pick a port explicitly instead.
    if !std::io::stdin().is_terminal() {
        return Err(format!(
            "multiple serial ports found; specify one with -p/--port (e.g. -p {})",
            ports[0].path
        )
        .into());
    }
    eprint!("Select port [0-{}]: ", ports.len() - 1);
    let _ = std::io::stderr().flush();
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let idx: usize = input.trim().parse().map_err(|_| "Invalid selection")?;
    if idx >= ports.len() {
        return Err(format!("Selection out of range: {}", idx).into());
    }
    Ok(ports[idx].path.clone())
}

fn compute_end_from_file(
    start_hex: &str,
    file_path: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let start = parse_hex_addr(start_hex)?;
    let metadata = std::fs::metadata(file_path)?;
    let file_size = metadata.len();
    let end = start + file_size;
    Ok(format!("0x{:08X}", end))
}

fn parse_hex_addr(s: &str) -> Result<u64, Box<dyn std::error::Error>> {
    let trimmed = s.trim();
    let raw = if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
        &trimmed[2..]
    } else {
        trimmed
    };
    u64::from_str_radix(raw, 16).map_err(|e| format!("invalid hex address '{}': {}", s, e).into())
}

const LOG_MAX_BYTES: u64 = 5 * 1024 * 1024; // 5 MB, matches the GUI cap
const LOG_KEEP: usize = 3;

/// `tyutool.log` -> `tyutool.log.1`, `.1` -> `.2`, ... up to `keep`.
fn with_ext_num(log_path: &std::path::Path, n: usize) -> std::path::PathBuf {
    let mut s = log_path.as_os_str().to_os_string();
    s.push(format!(".{n}"));
    std::path::PathBuf::from(s)
}

/// Ordered (from, to) renames to rotate `log_path`, oldest shifted out last.
fn rotation_plan(
    log_path: &std::path::Path,
    keep: usize,
) -> Vec<(std::path::PathBuf, std::path::PathBuf)> {
    let mut moves = Vec::new();
    for i in (1..keep).rev() {
        moves.push((with_ext_num(log_path, i), with_ext_num(log_path, i + 1)));
    }
    moves.push((log_path.to_path_buf(), with_ext_num(log_path, 1)));
    moves
}

/// Rotate the log if it exceeds `LOG_MAX_BYTES`. Best-effort: rotation failures
/// are ignored so logging still proceeds.
fn rotate_if_needed(log_path: &std::path::Path) {
    let too_big = std::fs::metadata(log_path)
        .map(|m| m.len() > LOG_MAX_BYTES)
        .unwrap_or(false);
    if !too_big {
        return;
    }
    let _ = std::fs::remove_file(with_ext_num(log_path, LOG_KEEP));
    for (from, to) in rotation_plan(log_path, LOG_KEEP) {
        let _ = std::fs::rename(&from, &to);
    }
}

fn init_logging(verbose: bool) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let log_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("tyutool");
    std::fs::create_dir_all(&log_dir)?;
    let log_path = log_dir.join("tyutool.log");

    rotate_if_needed(&log_path);

    let fmt = |out: fern::FormatCallback<'_>,
               message: &std::fmt::Arguments<'_>,
               record: &log::Record<'_>| {
        out.finish(format_args!(
            "[{} {} {}] {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            record.level(),
            record.target(),
            message
        ))
    };

    let mut dispatch = fern::Dispatch::new()
        .format(fmt)
        .level(log::LevelFilter::Info)
        .chain(fern::log_file(&log_path)?);

    if verbose {
        dispatch = dispatch.chain(
            fern::Dispatch::new()
                .format(|out, message, record| {
                    out.finish(format_args!(
                        "[{} {}] {}",
                        record.level(),
                        record.target(),
                        message
                    ))
                })
                .chain(std::io::stderr()),
        );
        eprintln!("[log] Writing to: {}", log_path.display());
    }

    dispatch.apply()?;
    Ok(log_path)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let force_plain = cli.plain;

    // Commands whose stdout is machine-consumed (survey JSON, completion
    // scripts): suppress the banner and log-file setup so nothing pollutes the
    // surrounding `eval`/pipe.
    let quiet = matches!(
        cli.command,
        Commands::UsbPortSurvey | Commands::Completions { .. }
    );
    let log_path = if !quiet {
        Some(init_logging(cli.verbose)?)
    } else {
        None
    };

    // User-facing startup banner (not a log::info! call)
    if !quiet {
        eprintln!(
            "tyutool v{}  {}/{}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        if let Some(ref p) = log_path {
            eprintln!("log: {}", p.display());
        }
        eprintln!();
    }

    // Developer diagnostics → log file (not shown to user)
    if !quiet {
        tyutool_core::diagnostics::log_session_banner(
            "tyutool-cli",
            "CLI",
            env!("CARGO_PKG_VERSION"),
            None,
        );
    }

    // Shared cancellation flag wired to Ctrl+C, so a flash/read/erase/authorize
    // job can unwind gracefully (close the port, emit Cancelled) instead of the
    // process being killed mid-transfer.
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let cancel = Arc::clone(&cancel);
        if let Err(e) = ctrlc::set_handler(move || {
            cancel.store(true, Ordering::SeqCst);
        }) {
            log::warn!("failed to install Ctrl+C handler: {}", e);
        }
    }

    match cli.command {
        Commands::ListPorts { json } => {
            let ports = list_serial_ports()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&ports)?);
            } else {
                // Tab-separated: path, vid:pid, usb_if, port_role, display_name
                for p in ports {
                    let vidpid = match (p.usb_vid, p.usb_pid) {
                        (Some(v), Some(pid)) => format!("{:04x}:{:04x}", v, pid),
                        _ => "-".to_string(),
                    };
                    let ifs = p
                        .usb_interface
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "-".to_string());
                    let role = p.port_role.as_deref().unwrap_or("-");
                    let name = p.name.as_deref().unwrap_or("");
                    println!("{}\t{}\t{}\t{}\t{}", p.path, vidpid, ifs, role, name);
                }
            }
        }
        Commands::UsbPortSurvey => {
            let rows = usb_port_survey()?;
            println!("{}", serde_json::to_string_pretty(&rows)?);
        }
        Commands::Reset { port, device } => {
            let port = match port {
                Some(p) => p,
                None => choose_port()?,
            };
            let chip_id = device.to_ascii_uppercase();
            device_reset_dtr_rts(&port, &chip_id)
                .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;
            log::info!("Device reset (DTR/RTS) completed on {}", port);
        }
        Commands::Update { check, source } => {
            update::run_update(check, source)
                .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;
        }
        Commands::Serve { port } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(serve::run_serve(port))?;
        }
        Commands::Authorize {
            port,
            uuid,
            authkey,
        } => {
            // Writing requires both halves; one without the other would silently
            // fall back to read-only in core, so reject the ambiguous case here.
            if uuid.is_some() != authkey.is_some() {
                return Err(
                    "authorize: provide both --uuid and --authkey to write, or neither to read"
                        .into(),
                );
            }
            let port = match port {
                Some(p) => p,
                None => choose_port()?,
            };
            let job = FlashJob {
                mode: FlashMode::Authorize,
                chip_id: String::new(),
                port,
                baud_rate: 115_200,
                segments: None,
                flash_start_hex: None,
                flash_end_hex: None,
                erase_start_hex: None,
                erase_end_hex: None,
                read_start_hex: None,
                read_end_hex: None,
                read_file_path: None,
                firmware_path: None,
                authorize_uuid: uuid,
                authorize_key: authkey,
            };
            let reporter = CliReporter::new(force_plain);
            run_job(&job, &cancel, reporter.callback())
                .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;
        }
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            let bin_name = cmd.get_name().to_string();
            generate(shell, &mut cmd, bin_name, &mut std::io::stdout());
        }
        Commands::Write {
            device,
            port,
            baud,
            start,
            end,
            file,
        } => {
            let baud = baud.unwrap_or_else(|| default_baud(&device));
            let start = start.unwrap_or_else(|| default_start(&device));
            let port = match port {
                Some(p) => p,
                None => choose_port()?,
            };
            let end = match end {
                Some(e) => e,
                None => compute_end_from_file(&start, &file)?,
            };
            let chip_id = device.to_ascii_uppercase();

            let reporter = CliReporter::new(force_plain);

            let job = FlashJob {
                mode: FlashMode::Flash,
                chip_id,
                port,
                baud_rate: baud,
                segments: None,
                flash_start_hex: Some(start),
                flash_end_hex: Some(end),
                erase_start_hex: None,
                erase_end_hex: None,
                read_start_hex: None,
                read_end_hex: None,
                read_file_path: None,
                firmware_path: Some(file),
                authorize_uuid: None,
                authorize_key: None,
            };
            run_job(&job, &cancel, reporter.callback())
                .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;
        }
        Commands::Read {
            device,
            port,
            baud,
            start,
            length,
            file,
        } => {
            let baud = baud.unwrap_or_else(|| default_baud(&device));
            let start = start.unwrap_or_else(|| default_start(&device));
            let port = match port {
                Some(p) => p,
                None => choose_port()?,
            };
            let start_val = parse_hex_addr(&start)?;
            let length_val = parse_hex_addr(&length)?;
            let end = format!("0x{:08X}", start_val + length_val);
            let chip_id = device.to_ascii_uppercase();

            let reporter = CliReporter::new(force_plain);

            let job = FlashJob {
                mode: FlashMode::Read,
                chip_id,
                port,
                baud_rate: baud,
                segments: None,
                flash_start_hex: None,
                flash_end_hex: None,
                erase_start_hex: None,
                erase_end_hex: None,
                read_start_hex: Some(start),
                read_end_hex: Some(end),
                read_file_path: Some(file),
                firmware_path: None,
                authorize_uuid: None,
                authorize_key: None,
            };
            run_job(&job, &cancel, reporter.callback())
                .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;
        }
        Commands::Erase {
            device,
            port,
            baud,
            start,
            length,
        } => {
            let baud = baud.unwrap_or_else(|| default_baud(&device));
            let start = start.unwrap_or_else(|| default_start(&device));
            let port = match port {
                Some(p) => p,
                None => choose_port()?,
            };
            let start_val = parse_hex_addr(&start)?;
            let length_val = parse_hex_addr(&length)?;
            let end = format!("0x{:08X}", start_val + length_val);
            let chip_id = device.to_ascii_uppercase();

            let reporter = CliReporter::new(force_plain);

            let job = FlashJob {
                mode: FlashMode::Erase,
                chip_id,
                port,
                baud_rate: baud,
                segments: None,
                flash_start_hex: None,
                flash_end_hex: None,
                erase_start_hex: Some(start),
                erase_end_hex: Some(end),
                read_start_hex: None,
                read_end_hex: None,
                read_file_path: None,
                firmware_path: None,
                authorize_uuid: None,
                authorize_key: None,
            };
            run_job(&job, &cancel, reporter.callback())
                .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tyutool_core::default_registry;

    #[test]
    fn device_list_matches_registry() {
        let mut from_cli: Vec<String> = SUPPORTED_DEVICES.iter().map(|s| s.to_string()).collect();
        from_cli.sort();
        let mut from_registry: Vec<String> = default_registry()
            .list_chip_ids()
            .into_iter()
            .map(|s| s.to_ascii_lowercase())
            .collect();
        from_registry.sort();
        assert_eq!(
            from_cli, from_registry,
            "SUPPORTED_DEVICES drifted from the core FlashPluginRegistry"
        );
    }

    #[test]
    fn parse_hex_addr_accepts_prefixes_and_trims() {
        assert_eq!(parse_hex_addr("0x200000").unwrap(), 0x200000);
        assert_eq!(parse_hex_addr("0X1CE400").unwrap(), 0x1CE400);
        assert_eq!(parse_hex_addr("ff").unwrap(), 0xff);
        assert_eq!(parse_hex_addr("  0x10  ").unwrap(), 0x10);
        assert_eq!(parse_hex_addr("0").unwrap(), 0);
    }

    #[test]
    fn parse_hex_addr_rejects_invalid() {
        assert!(parse_hex_addr("0xZZ").is_err());
        assert!(parse_hex_addr("not_hex").is_err());
        assert!(parse_hex_addr("").is_err());
    }

    #[test]
    fn default_baud_per_chip_family() {
        assert_eq!(default_baud("ln882h"), 115200);
        assert_eq!(default_baud("LN882H"), 115200);
        assert_eq!(default_baud("esp32"), 460800);
        assert_eq!(default_baud("esp32c3"), 460800);
        assert_eq!(default_baud("esp32c6"), 460800);
        assert_eq!(default_baud("esp32s3"), 460800);
        assert_eq!(default_baud("bk7231n"), 921600);
        assert_eq!(default_baud("t5"), 921600);
    }

    #[test]
    fn default_start_is_zero() {
        assert_eq!(default_start("bk7231n"), "0x00000000");
    }

    #[test]
    fn compute_end_from_file_adds_size_to_start() {
        let path = std::env::temp_dir().join("tyutool_compute_end_test.bin");
        std::fs::write(&path, vec![0u8; 16]).unwrap();
        let end = compute_end_from_file("0x100", path.to_str().unwrap()).unwrap();
        std::fs::remove_file(&path).unwrap();
        // 0x100 + 16 = 0x110, formatted as 8-wide upper hex.
        assert_eq!(end, "0x00000110");
    }
}

#[cfg(test)]
mod rotation_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn with_ext_num_appends_numeric_suffix() {
        let base = Path::new("/var/log/tyutool.log");
        assert_eq!(
            with_ext_num(base, 1).display().to_string(),
            "/var/log/tyutool.log.1"
        );
        assert_eq!(
            with_ext_num(base, 3).display().to_string(),
            "/var/log/tyutool.log.3"
        );
    }

    #[test]
    fn rotation_plan_keep_1_only_moves_base() {
        let base = Path::new("/tmp/tyutool.log");
        let moves: Vec<(String, String)> = rotation_plan(base, 1)
            .iter()
            .map(|(a, b): &(std::path::PathBuf, std::path::PathBuf)| {
                (a.display().to_string(), b.display().to_string())
            })
            .collect();
        assert_eq!(
            moves,
            vec![(
                "/tmp/tyutool.log".to_string(),
                "/tmp/tyutool.log.1".to_string()
            )]
        );
    }

    #[test]
    fn rotation_plan_keep_3_shifts_oldest_last() {
        let base = Path::new("/tmp/tyutool.log");
        let moves: Vec<(String, String)> = rotation_plan(base, 3)
            .iter()
            .map(|(a, b): &(std::path::PathBuf, std::path::PathBuf)| {
                (a.display().to_string(), b.display().to_string())
            })
            .collect();
        assert_eq!(
            moves,
            vec![
                (
                    "/tmp/tyutool.log.2".to_string(),
                    "/tmp/tyutool.log.3".to_string()
                ),
                (
                    "/tmp/tyutool.log.1".to_string(),
                    "/tmp/tyutool.log.2".to_string()
                ),
                (
                    "/tmp/tyutool.log".to_string(),
                    "/tmp/tyutool.log.1".to_string()
                ),
            ]
        );
    }
}
