use std::io::{IsTerminal as _, Write as _};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::{
    builder::{StringValueParser, TypedValueParser},
    CommandFactory, Parser, Subcommand,
};
use clap_complete::{generate, Shell};
use tyutool_core::{
    device_reset_dtr_rts, list_serial_ports, normalize_chip_id, run_job, usb_port_survey, FlashJob,
    FlashMode,
};

mod monitor;
mod reporter;
use reporter::CliReporter;
mod serve;
mod update;

#[derive(Parser)]
#[command(name = "tyutool", version, about = "Tuya Uart Tool.")]
struct Cli {
    /// Also write developer diagnostic logs to stderr (always written to log file)
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
        /// Chip id: Beken uses the same DTR/RTS pulse as flash handshake (bk7231n/t2 vs t5ai/t3/t1); ESP32* uses espflash hard_reset
        #[arg(short = 'd', long = "device", default_value = "bk7231n", value_parser = chip_value_parser())]
        device: String,
    },
    /// Check for updates and self-update the binary
    Update {
        /// Only check version, do not download
        #[arg(long)]
        check: bool,
        /// Update source: github (default) or tuya (Tuya OSS, mainland China)
        #[arg(long)]
        source: Option<String>,
    },
    /// Start a local WebSocket server for browser-mode flashing (dev only).
    Serve {
        /// WebSocket port to listen on
        #[arg(long, default_value_t = 9527)]
        port: u16,
    },
    /// Live serial monitor — stream device output to the terminal (Ctrl+] or Ctrl+C to quit)
    Monitor {
        /// Serial port (default: first available)
        #[arg(short = 'p', long = "port")]
        port: Option<String>,
        /// Uart baud rate (default: chip-specific monitor baud; 115200 without -d)
        #[arg(short = 'b', long = "baud")]
        baud: Option<u32>,
        /// Chip type — selects the default monitor baud (t5ai/t3: 460800, others: 115200)
        #[arg(short = 'd', long = "device", value_parser = chip_value_parser())]
        device: Option<String>,
        /// Append received data to this file
        #[arg(short = 'l', long = "log")]
        log: Option<String>,
    },
    /// TuyaOpen device authorization via UART shell (auth-read / auth write, KV storage only)
    #[command(visible_alias = "auth")]
    Authorize {
        /// Serial port (default: first available)
        #[arg(short = 'p', long = "port")]
        port: Option<String>,
        /// Chip type — selects per-chip auth timing (default: generic)
        #[arg(short = 'd', long = "device", value_parser = chip_value_parser())]
        device: Option<String>,
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
    "bk7231n", "t2", "t3", "t1", "t5ai", "ln882h", "esp32", "esp32c3", "esp32c6", "esp32p4",
    "esp32s3",
];

fn chip_value_parser() -> impl TypedValueParser<Value = String> {
    // Lowercase first so --device T5AI / T5 / t5AI etc. all work.
    // Legacy `t5` (any case) maps to `t5ai` for backwards compatibility.
    StringValueParser::new().try_map(|s| {
        let lower = s.to_ascii_lowercase();
        let canonical = if lower == "t5" {
            "t5ai".to_string()
        } else {
            lower
        };
        if SUPPORTED_DEVICES.contains(&canonical.as_str()) {
            Ok(canonical)
        } else {
            Err(format!(
                "invalid device '{}', valid values: {}",
                s,
                SUPPORTED_DEVICES.join(", ")
            ))
        }
    })
}

// Must stay in sync with `defaultBaudRate` in src/features/firmware-flash/chip-manifests.ts.
// When adding or modifying a chip, update both.
fn default_baud(device: &str) -> u32 {
    match device.to_ascii_lowercase().as_str() {
        "ln882h" => 115200,
        "esp32" | "esp32c3" | "esp32c6" | "esp32p4" | "esp32s3" => 460800,
        _ => 921600,
    }
}

// Must stay in sync with `defaultLogBaudRate` in
// src/features/firmware-flash/chip-manifests.ts: T5AI and T3 log at 460800,
// every other chip (and no `-d`) at 115200. The GUI reads the same port at the
// same rate, so a chip listed at 460800 there and 115200 here would show the
// user solid garbage in one of the two.
fn monitor_default_baud(device: Option<&str>) -> u32 {
    match device.map(|d| d.to_ascii_lowercase()).as_deref() {
        Some("t5ai") | Some("t3") => 460800,
        _ => 115200,
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

const MAX_LOG_FILES: usize = 100;
const MAX_LOG_BYTES_TOTAL: u64 = 100 * 1024 * 1024; // 100 MB
const MAX_LOG_BYTES_PER_FILE: u64 = 10 * 1024 * 1024; // 10 MB per session file

/// Size-capped log sink for one CLI session. Writes to `tyutool-<stem>.log`
/// until it reaches `MAX_LOG_BYTES_PER_FILE`, then rolls over to
/// `tyutool-<stem>-1.log`, `-2.log`, … so a long-running session can never grow
/// a single file without bound. fern serializes writes, so no extra locking is
/// needed here. All produced files share the `tyutool-` prefix, so
/// `prune_log_files` reclaims them across sessions like any other session log.
struct SessionLogWriter {
    dir: std::path::PathBuf,
    stem: String, // e.g. "tyutool-20240101-120000" (no extension)
    index: u32,
    file: std::fs::File,
    size: u64,
}

impl SessionLogWriter {
    fn path_for(dir: &std::path::Path, stem: &str, index: u32) -> std::path::PathBuf {
        if index == 0 {
            dir.join(format!("{stem}.log"))
        } else {
            dir.join(format!("{stem}-{index}.log"))
        }
    }

    fn open(dir: &std::path::Path, stem: &str) -> std::io::Result<Self> {
        let path = Self::path_for(dir, stem, 0);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            dir: dir.to_path_buf(),
            stem: stem.to_string(),
            index: 0,
            file,
            size,
        })
    }

    fn roll(&mut self) -> std::io::Result<()> {
        self.index += 1;
        let path = Self::path_for(&self.dir, &self.stem, self.index);
        self.file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        self.size = self.file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(())
    }
}

impl std::io::Write for SessionLogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // Roll before a write that would push past the cap, but never roll an
        // empty file — a single record larger than the cap still lands in one
        // file rather than spinning forever.
        if self.size > 0 && self.size + buf.len() as u64 > MAX_LOG_BYTES_PER_FILE {
            self.roll()?;
        }
        let n = self.file.write(buf)?;
        self.size += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// Delete the oldest per-session log files until the collection is within both
/// the file-count and total-size limits. Only manages files whose stem starts
/// with "tyutool-"; always retains at least one file.
fn prune_log_files(log_dir: &std::path::Path) {
    let mut files: Vec<(std::path::PathBuf, u64)> = match std::fs::read_dir(log_dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().map(|x| x == "log").unwrap_or(false)
                    && p.file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.starts_with("tyutool-"))
                        .unwrap_or(false)
            })
            .map(|p| {
                let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                (p, size)
            })
            .collect(),
        Err(_) => return,
    };

    files.sort_by(|a, b| a.0.file_name().cmp(&b.0.file_name()));

    let mut count = files.len();
    let mut total: u64 = files.iter().map(|(_, s)| s).sum();

    for (path, size) in &files {
        if count <= 1 || (count <= MAX_LOG_FILES && total <= MAX_LOG_BYTES_TOTAL) {
            break;
        }
        let _ = std::fs::remove_file(path);
        count -= 1;
        total = total.saturating_sub(*size);
    }
}

fn init_logging(verbose: bool) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let log_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("tyutool");
    std::fs::create_dir_all(&log_dir)?;
    let stem = format!("tyutool-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    let log_path = log_dir.join(format!("{stem}.log"));
    let session_writer = SessionLogWriter::open(&log_dir, &stem)?;

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

    // File sink captures full developer diagnostics (Trace+) so debug/trace
    // logs persist per the Logging Contract; the top-level filter stays Info so
    // stderr (when enabled) is not flooded by trace frames.
    let file_dispatch = fern::Dispatch::new()
        .level(log::LevelFilter::Trace)
        .chain(Box::new(session_writer) as Box<dyn std::io::Write + Send>);

    let mut dispatch = fern::Dispatch::new().format(fmt).chain(file_dispatch);

    if verbose {
        dispatch = dispatch.chain(
            fern::Dispatch::new()
                .level(log::LevelFilter::Info)
                .chain(std::io::stderr()),
        );
        eprintln!("[log] Writing to: {}", log_path.display());
    }

    dispatch.apply()?;
    prune_log_files(&log_dir);
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
            let chip_id = normalize_chip_id(&device);
            log::info!("[cli] reset port={} chip={}", port, chip_id);
            device_reset_dtr_rts(&port, &chip_id)
                .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;
            log::info!("Device reset (DTR/RTS) completed on {}", port);
        }
        Commands::Update { check, source } => {
            log::info!("[cli] update check={} source={:?}", check, source);
            update::run_update(check, source)
                .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;
        }
        Commands::Monitor {
            port,
            baud,
            device,
            log,
        } => {
            let port = match port {
                Some(p) => p,
                None => choose_port()?,
            };
            let baud = baud.unwrap_or_else(|| monitor_default_baud(device.as_deref()));
            log::info!("[cli] monitor port={} baud={} log={:?}", port, baud, log);
            monitor::run_monitor(&port, baud, log.as_deref(), &cancel)?;
        }
        Commands::Serve { port } => {
            log::info!("[cli] serve on port {}", port);
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(serve::run_serve(port))?;
        }
        Commands::Authorize {
            port,
            device,
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
            let chip_id = device.as_deref().map(normalize_chip_id).unwrap_or_default();
            let mode = if uuid.is_some() {
                "read+write"
            } else {
                "read-only"
            };
            log::info!(
                "[cli] authorize chip={} port={} mode={}",
                chip_id,
                port,
                mode
            );
            let job = FlashJob {
                mode: FlashMode::Authorize,
                chip_id,
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
                authorize_storage: None,
                confirm_overwrite: None,
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
            let chip_id = normalize_chip_id(&device);
            log::info!(
                "[cli] write chip={} port={} baud={} start={} end={} file={}",
                chip_id,
                port,
                baud,
                start,
                end,
                file
            );

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
                authorize_storage: None,
                confirm_overwrite: None,
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
            let chip_id = normalize_chip_id(&device);
            log::info!(
                "[cli] read chip={} port={} baud={} start={} end={} file={}",
                chip_id,
                port,
                baud,
                start,
                end,
                file
            );

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
                authorize_storage: None,
                confirm_overwrite: None,
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
            let chip_id = normalize_chip_id(&device);
            log::info!(
                "[cli] erase chip={} port={} baud={} start={} end={}",
                chip_id,
                port,
                baud,
                start,
                end
            );

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
                authorize_storage: None,
                confirm_overwrite: None,
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
    fn cli_accepts_legacy_t5_device_and_resolves_to_t5ai() {
        // Input is lowercased before matching, so all case variants are accepted.
        // Legacy `t5` (any case) resolves to the canonical `t5ai`.
        for variant in ["t5", "T5", "t5ai", "T5AI", "T5aI"] {
            let cli = Cli::try_parse_from([
                "tyutool",
                "write",
                "--device",
                variant,
                "--port",
                "/dev/null",
                "--file",
                "x.bin",
            ])
            .unwrap_or_else(|e| panic!("clap rejected --device {variant}: {e}"));
            match cli.command {
                Commands::Write { device, .. } => assert_eq!(device, "t5ai"),
                _ => panic!("expected Commands::Write"),
            }
        }
    }

    #[test]
    fn cli_device_arg_is_case_insensitive() {
        // chip_value_parser lowercases input before matching, so mixed-case
        // spellings must be accepted and resolved to the canonical lowercase id.
        let cases: &[(&str, &str)] = &[
            ("BK7231N", "bk7231n"),
            ("Bk7231N", "bk7231n"),
            ("T2", "t2"),
            ("T3", "t3"),
            ("T1", "t1"),
            ("LN882H", "ln882h"),
            ("ESP32", "esp32"),
            ("ESP32C3", "esp32c3"),
            ("ESP32C6", "esp32c6"),
            ("ESP32P4", "esp32p4"),
            ("ESP32S3", "esp32s3"),
        ];
        for (input, canonical) in cases {
            let cli = Cli::try_parse_from([
                "tyutool",
                "write",
                "--device",
                input,
                "--port",
                "/dev/null",
                "--file",
                "x.bin",
            ])
            .unwrap_or_else(|e| panic!("clap rejected --device {input}: {e}"));
            match cli.command {
                Commands::Write { device, .. } => assert_eq!(device, *canonical),
                _ => panic!("expected Commands::Write"),
            }
        }
    }

    #[test]
    fn default_baud_per_chip_family() {
        assert_eq!(default_baud("ln882h"), 115200);
        assert_eq!(default_baud("LN882H"), 115200);
        assert_eq!(default_baud("esp32"), 460800);
        assert_eq!(default_baud("esp32c3"), 460800);
        assert_eq!(default_baud("esp32c6"), 460800);
        assert_eq!(default_baud("esp32p4"), 460800);
        assert_eq!(default_baud("esp32s3"), 460800);
        assert_eq!(default_baud("bk7231n"), 921600);
        assert_eq!(default_baud("t5ai"), 921600);
    }

    #[test]
    fn monitor_default_baud_per_chip() {
        // 460800 chips — the ones whose `defaultLogBaudRate` is 460800 in
        // chip-manifests.ts.
        assert_eq!(monitor_default_baud(Some("t5ai")), 460800);
        assert_eq!(monitor_default_baud(Some("T5AI")), 460800);
        assert_eq!(monitor_default_baud(Some("t3")), 460800);
        assert_eq!(monitor_default_baud(Some("T3")), 460800);
        assert_eq!(monitor_default_baud(Some("bk7231n")), 115200);
        assert_eq!(monitor_default_baud(Some("t1")), 115200);
        assert_eq!(monitor_default_baud(Some("t2")), 115200);
        assert_eq!(monitor_default_baud(Some("ln882h")), 115200);
        assert_eq!(monitor_default_baud(Some("esp32")), 115200);
        assert_eq!(monitor_default_baud(None), 115200);
    }

    #[test]
    fn monitor_command_parses_flags() {
        let cli = Cli::try_parse_from([
            "tyutool", "monitor", "-p", "COM3", "-b", "115200", "-l", "dev.log",
        ])
        .unwrap();
        match cli.command {
            Commands::Monitor {
                port, baud, log, ..
            } => {
                assert_eq!(port.as_deref(), Some("COM3"));
                assert_eq!(baud, Some(115200));
                assert_eq!(log.as_deref(), Some("dev.log"));
            }
            _ => panic!("expected Commands::Monitor"),
        }
    }

    #[test]
    fn auth_is_an_alias_for_authorize() {
        let cli = Cli::try_parse_from([
            "tyutool",
            "auth",
            "-p",
            "COM3",
            "--uuid",
            "u",
            "--authkey",
            "k",
        ])
        .unwrap();
        match cli.command {
            Commands::Authorize { uuid, authkey, .. } => {
                assert_eq!(uuid.as_deref(), Some("u"));
                assert_eq!(authkey.as_deref(), Some("k"));
            }
            _ => panic!("expected Commands::Authorize via alias"),
        }
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

    #[test]
    fn reset_rejects_unknown_device() {
        // `reset -d <unknown>` must be rejected at parse time, like every other
        // subcommand — previously `reset` accepted any string and only failed
        // (or used the wrong reset pulse) at runtime.
        let result = Cli::try_parse_from(["tyutool", "reset", "--device", "nope"]);
        assert!(
            result.is_err(),
            "reset should reject an unknown --device value"
        );
    }

    #[test]
    fn reset_accepts_known_device_and_default() {
        // Explicit known device.
        let cli = Cli::try_parse_from(["tyutool", "reset", "--device", "t5ai"]).unwrap();
        match cli.command {
            Commands::Reset { device, .. } => assert_eq!(device, "t5ai"),
            _ => panic!("expected Commands::Reset"),
        }
        // Default value ("bk7231n") also passes the parser.
        let cli = Cli::try_parse_from(["tyutool", "reset"]).unwrap();
        match cli.command {
            Commands::Reset { device, .. } => assert_eq!(device, "bk7231n"),
            _ => panic!("expected Commands::Reset"),
        }
    }
}

#[cfg(test)]
mod prune_tests {
    use super::*;

    fn touch(dir: &std::path::Path, name: &str, size: u64) {
        let path = dir.join(name);
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(size).unwrap();
    }

    fn names(dir: &std::path::Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn prune_removes_oldest_when_over_count_limit() {
        let dir = tempfile::tempdir().unwrap();
        for i in 1..=(MAX_LOG_FILES + 2) {
            touch(dir.path(), &format!("tyutool-20240101-{i:06}.log"), 1024);
        }
        prune_log_files(dir.path());
        assert_eq!(names(dir.path()).len(), MAX_LOG_FILES);
        // The oldest two should be gone.
        assert!(!dir.path().join("tyutool-20240101-000001.log").exists());
        assert!(!dir.path().join("tyutool-20240101-000002.log").exists());
    }

    #[test]
    fn prune_removes_oldest_when_over_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        // 5 files each 15 MB → total 75 MB > 50 MB limit
        for i in 1..=5 {
            touch(
                dir.path(),
                &format!("tyutool-20240101-{i:06}.log"),
                15 * 1024 * 1024,
            );
        }
        prune_log_files(dir.path());
        let remaining = names(dir.path());
        let total: u64 = remaining
            .iter()
            .map(|n| std::fs::metadata(dir.path().join(n)).unwrap().len())
            .sum();
        assert!(total <= MAX_LOG_BYTES_TOTAL);
    }

    #[test]
    fn prune_always_keeps_at_least_one_file() {
        let dir = tempfile::tempdir().unwrap();
        // One enormous file that exceeds both limits by itself.
        touch(
            dir.path(),
            "tyutool-20240101-000001.log",
            MAX_LOG_BYTES_TOTAL + 1,
        );
        prune_log_files(dir.path());
        assert_eq!(names(dir.path()).len(), 1);
    }

    #[test]
    fn prune_ignores_legacy_tyutool_log() {
        let dir = tempfile::tempdir().unwrap();
        // Legacy file should not be touched.
        touch(dir.path(), "tyutool.log", 1024);
        prune_log_files(dir.path());
        assert!(dir.path().join("tyutool.log").exists());
    }

    #[test]
    fn session_writer_rolls_over_at_size_cap() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let stem = "tyutool-20240101-000000";
        let mut w = SessionLogWriter::open(dir.path(), stem).unwrap();
        // 11 MB in 1 MB chunks → base fills to 10 MB, the 11th rolls to `-1`.
        let chunk = vec![b'x'; 1024 * 1024];
        for _ in 0..11 {
            w.write_all(&chunk).unwrap();
        }
        w.flush().unwrap();

        let base = dir.path().join(format!("{stem}.log"));
        let rolled = dir.path().join(format!("{stem}-1.log"));
        assert!(base.exists());
        assert!(rolled.exists());
        assert!(std::fs::metadata(&base).unwrap().len() <= MAX_LOG_BYTES_PER_FILE);
        // No bytes lost across the rollover.
        let total =
            std::fs::metadata(&base).unwrap().len() + std::fs::metadata(&rolled).unwrap().len();
        assert_eq!(total, 11 * 1024 * 1024);
    }

    #[test]
    fn session_writer_single_oversized_record_stays_in_one_file() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let stem = "tyutool-20240101-000001";
        let mut w = SessionLogWriter::open(dir.path(), stem).unwrap();
        // A first write larger than the cap must not roll (empty-file guard).
        // Use a single write() call (not write_all) to exercise the guard; the
        // returned count is asserted to satisfy unused_io_amount.
        let big = vec![b'x'; (MAX_LOG_BYTES_PER_FILE + 4096) as usize];
        let n = w.write(&big).unwrap();
        assert!(n > 0);
        w.flush().unwrap();
        assert!(dir.path().join(format!("{stem}.log")).exists());
        assert!(!dir.path().join(format!("{stem}-1.log")).exists());
    }
}
