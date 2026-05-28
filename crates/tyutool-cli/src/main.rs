use std::io::Write as _;
use std::sync::atomic::AtomicBool;

use clap::{builder::PossibleValuesParser, Parser, Subcommand};
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
        #[arg(short = 'd', long = "device", value_parser = PossibleValuesParser::new(["bk7231n", "t2", "t3", "t1", "t5", "ln882h", "esp32", "esp32c3", "esp32c6", "esp32s3"]))]
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
        #[arg(short = 'd', long = "device", value_parser = PossibleValuesParser::new(["bk7231n", "t2", "t3", "t1", "t5", "ln882h", "esp32", "esp32c3", "esp32c6", "esp32s3"]))]
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
    /// List serial ports
    ListPorts,
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

fn init_logging(verbose: bool) -> Result<(), Box<dyn std::error::Error>> {
    let log_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("tyutool");
    std::fs::create_dir_all(&log_dir)?;
    let log_path = log_dir.join("tyutool.log");

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
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let force_plain = cli.plain;

    // Init file logging (+ stderr if --verbose)
    let survey_json_only = matches!(cli.command, Commands::UsbPortSurvey);
    if !survey_json_only {
        init_logging(cli.verbose)?;
    }

    // User-facing startup banner (not a log::info! call)
    if !survey_json_only {
        eprintln!(
            "tyutool v{}  {}/{}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        eprintln!();
    }

    // Developer diagnostics → log file (not shown to user)
    if !survey_json_only {
        log::info!("========================================");
        log::info!("[App] tyutool-cli v{} starting", env!("CARGO_PKG_VERSION"));
        log::info!("[App] Type: CLI");
        log::info!(
            "[App] OS: {}, Arch: {}, Family: {}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::env::consts::FAMILY
        );
        if let Ok(exe) = std::env::current_exe() {
            log::info!("[App] Exe: {}", exe.display());
        }
        log::info!("========================================");
    }

    match cli.command {
        Commands::ListPorts => {
            let ports = list_serial_ports()?;
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
        Commands::Authorize { port, uuid, authkey } => {
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
            let cancel = AtomicBool::new(false);
            let reporter = CliReporter::new(force_plain);
            run_job(&job, &cancel, reporter.callback())
                .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;
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
            let cancel = AtomicBool::new(false);
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
            let cancel = AtomicBool::new(false);
            run_job(&job, &cancel, reporter.callback())
                .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;
        }
    }
    Ok(())
}
