//! tyutool-bridge binary entry — two modes over the same WS server:
//!
//! * default: resident tray shell (menu bar status item) with the server on a
//!   background tokio runtime, so the user can see the bridge is alive and quit
//!   it deliberately;
//! * `--headless`: the pre-B6 behaviour (serve until killed, stderr logging,
//!   `exit(1)` when the port is taken) — what CI and the smoke scripts drive.
//!
//! Headless is also where the human-in-the-loop story changes: there is nobody
//! in front of a CI runner or an ssh session, so it refuses every dangerous
//! operation unless started with [`UNATTENDED_FLAG`] (see [`prompt_choice`]).
//!
//! A hand-rolled flag check instead of clap: the binary has three flags and no
//! subcommands, so an argument parser would be the larger surface.

// No console window on Windows: this is a resident tray app that a user starts
// from a Start-menu shortcut or an autostart entry, and a console-subsystem
// binary would put a black window on their screen for as long as it runs.
//
// Two consequences are handled elsewhere rather than left to chance:
//  * children no longer inherit a console either, so `CreateProcess` would give
//    each one its own — every dialog, every `open <url>`, and (worst) the
//    `cmd /c ver` behind **every** WebSocket hello frame. See [`tyutool_bridge::proc`].
//  * with no console the standard handles are NULL, and `std` silently discards
//    writes to those, so `--help` / `--headless` output would disappear without
//    an error. See [`tyutool_bridge::proc::attach_parent_console`].
//
// `not(debug_assertions)` so `cargo run` during development keeps its console,
// matching the GUI shell's own entry point (`src-tauri/src/main.rs`).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tyutool_bridge::autostart;
use tyutool_bridge::lang::{detect_lang, Lang};
use tyutool_bridge::proc::hidden_command;
use tyutool_bridge::status::{self, StatsSnapshot};
use tyutool_bridge::tray_glyph;
use tyutool_bridge::{
    bind, AuthPrompt, Authority, ConfirmDecision, ConfirmRequest, ConfirmResponder, DangerousOp,
    FileTokenStore, GrantPolicy, MemoryTokenStore, TokenStore, DEFAULT_PORT,
};

/// Own version, shown in the tray status line.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Login-item / LaunchAgent label for the autostart registration.
const AUTOSTART_APP_NAME: &str = "tyutool-bridge";

/// Tray menu targets.
///
/// TODO(联调期确认): both are placeholders — swap for the real Cobuilder entry
/// point and the bridge download/landing page once product confirms them.
const COBUILDER_URL: &str = "https://iot.tuya.com";
const LATEST_VERSION_URL: &str = "https://iot.tuya.com";

/// Session log retention for this binary, mirroring `prune_log_files` in
/// tyutool-cli (same "delete oldest until inside the limits" rule, smaller
/// budget: the bridge is a resident process, not an interactive tool).
const MAX_LOG_FILES: usize = 20;
const MAX_LOG_BYTES_TOTAL: u64 = 50 * 1024 * 1024; // 50 MB
/// Log file prefix; `prune_log_files` only ever touches files matching it.
const LOG_FILE_PREFIX: &str = "tyutool-bridge-";

/// The opt-in that lets `--headless` write devices with no human in the loop.
///
/// A flag and deliberately **not** an environment variable: one discoverable
/// switch that is visible in the process table, whereas an env var is far too
/// easy to inherit unnoticed into a process that never meant to opt in (a shell
/// profile, a CI job's global env, a parent supervisor) — and this is the single
/// setting that removes the whole B7 confirmation gate.
const UNATTENDED_FLAG: &str = "--allow-unattended-writes";

fn main() {
    // First statement, before anything can write: see the module doc on
    // `proc::attach_parent_console` for why a later call would be a no-op that
    // still looks like it worked.
    tyutool_bridge::proc::attach_parent_console();

    // Read once, here, and passed down by value from now on: the tray shell has
    // no settings UI to change it from, and re-reading it per string would only
    // let one dialog disagree with the next.
    //
    // `sys_locale` rather than `LANG`: the shipped bridge is started by a
    // LaunchAgent, and that process inherits no shell environment at all — the
    // env-var route would be dead on the one path that matters most.
    let lang = detect_lang(&sys_locale::get_locale().unwrap_or_default());

    // Hand-rolled, order-independent, and it *rejects* what it does not know:
    // a typo'd `--allow-unattended-write` must not look like it worked.
    let mut headless = false;
    let mut unattended = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--headless" => headless = true,
            UNATTENDED_FLAG => unattended = true,
            "--help" | "-h" => {
                // Before any logging setup: `--help` must not create a session
                // log file just to print a paragraph.
                print!("{}", help_text(lang));
                return;
            }
            other => {
                eprintln!("{}", unknown_argument_line(other, lang));
                std::process::exit(2);
            }
        }
    }

    init_logging(headless);
    // Single shared helper, never a locally inlined banner (repo logging
    // contract): this is what makes bridge bug reports comparable to CLI/GUI.
    tyutool_core::diagnostics::log_session_banner("tyutool-bridge", "BRIDGE", VERSION, None);

    let choice = prompt_choice(headless, unattended);
    if headless {
        run_headless(choice, lang);
    } else {
        run_tray(choice, lang);
    }
}

/// The one line an unknown flag gets, before anything else has been set up.
fn unknown_argument_line(argument: &str, lang: Lang) -> String {
    match lang {
        Lang::Zh => format!("tyutool-bridge: 无法识别的参数 {argument:?}；用 --help 看可用选项。"),
        Lang::En => format!(
            "tyutool-bridge: unrecognized argument {argument:?}; run --help for the available options."
        ),
    }
}

/// `--help` / `-h`. Names both flags and says outright what the opt-in removes:
/// a user who reads only this text must still learn that
/// [`UNATTENDED_FLAG`] deletes the confirmation step, not just automates it.
///
/// The flag spellings are literals here (`concat!` cannot take a `const`); the
/// `the_help_text_documents_both_flags_and_the_risk` test asserts this text
/// contains [`UNATTENDED_FLAG`], so the two cannot drift apart unnoticed.
fn help_text(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => concat!(
            "Cobuilder Bridge —— 常驻本机的烧录 helper，监听 ws://127.0.0.1:18730\n",
            "\n",
            "用法: tyutool-bridge [选项]\n",
            "\n",
            "选项:\n",
            "  --headless                  不建托盘图标，在前台跑服务（CI / 服务器 / ssh 会话）。\n",
            "                              这种环境里没有人能看到确认框，所以默认拒绝所有危险\n",
            "                              操作（烧录、写授权码），只读能力照常可用。\n",
            "                              「默认拒绝」是无条件的：即使托盘模式下有人点过允许、\n",
            "                              授权记录还存在本机，这里也一律不认，照样拒绝。\n",
            "  --allow-unattended-writes   仅在 --headless 下有意义：关闭人工确认，危险操作一律\n",
            "                              自动放行，每次放行都会写一条 warn 日志。等于交出 B7 的\n",
            "                              「必须用户点一次」保证，只在这台机器的控制台本身可信、\n",
            "                              且确实要无人值守烧录时才用。\n",
            "  -h, --help                  打印本帮助并退出。\n",
            "\n",
            "不带 --headless 时：托盘常驻，每个危险操作弹一次系统确认框，默认按钮是「拒绝」。\n",
        ),
        Lang::En => concat!(
            "Cobuilder Bridge — resident local flash helper, listening on ws://127.0.0.1:18730\n",
            "\n",
            "Usage: tyutool-bridge [options]\n",
            "\n",
            "Options:\n",
            "  --headless                  No tray icon; serve in the foreground (CI, a server,\n",
            "                              an ssh session). Nobody can see a confirmation dialog\n",
            "                              in such an environment, so every dangerous operation\n",
            "                              (flashing, writing an authorization code) is refused\n",
            "                              by default; read-only features keep working.\n",
            "                              That refusal is unconditional: even if somebody once\n",
            "                              clicked Allow in tray mode and the grant is still on\n",
            "                              this machine, stored grants are never honoured here.\n",
            "  --allow-unattended-writes   Only meaningful together with --headless.\n",
            "                              It turns the human confirmation off, so every\n",
            "                              dangerous operation is approved automatically,\n",
            "                              each with a warn log line. That gives up the\n",
            "                              \"the user must click once\" guarantee — use it\n",
            "                              only if this machine's own console is trusted\n",
            "                              and you really do need unattended flashing.\n",
            "  -h, --help                  Print this help and exit.\n",
            "\n",
            "Without --headless: the tray stays resident and every dangerous operation raises\n",
            "one system confirmation dialog, whose default button is the refusing one.\n",
        ),
    }
}

/// Who answers a confirmation request in a given run mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptChoice {
    /// Ask the person sitting in front of the machine.
    SystemDialog,
    /// Refuse every dangerous operation, and say how to opt in.
    DenyAll,
    /// Approve every dangerous operation, loudly.
    UnattendedAutoApprove,
}

/// The headless confirmation policy, as a pure decision.
///
/// A GUI session has a user, so it always asks — [`UNATTENDED_FLAG`] only means
/// anything where nobody can answer. Headless without the flag refuses up front
/// rather than popping a dialog into a display nobody is watching: that dialog
/// would only burn the full 60s confirmation window and then refuse anyway.
fn prompt_choice(headless: bool, unattended: bool) -> PromptChoice {
    match (headless, unattended) {
        (false, _) => PromptChoice::SystemDialog,
        (true, false) => PromptChoice::DenyAll,
        (true, true) => PromptChoice::UnattendedAutoApprove,
    }
}

/// Whether this run may lean on a grant an earlier session persisted.
///
/// Derived from [`PromptChoice`] rather than re-read from the raw flags on
/// purpose: the two decisions must not be able to drift apart, and "there is
/// nobody to ask" is exactly the condition under which a stored grant must stop
/// counting. One `grants.json` is shared with the tray shell, so without this a
/// single earlier confirmation at a keyboard would let `--headless` write devices
/// unattended and never call the refusing prompt at all — silently defeating the
/// documented default. `--allow-unattended-writes` exists precisely so that
/// unattended operation is *declared*; a leftover grant may not stand in for
/// that declaration.
fn grant_policy_for(choice: PromptChoice) -> GrantPolicy {
    match choice {
        PromptChoice::DenyAll => GrantPolicy::Ignore,
        // Attended, or unattended-by-declaration: in the latter the prompt
        // approves everything anyway, so grants are moot but harmless.
        PromptChoice::SystemDialog | PromptChoice::UnattendedAutoApprove => GrantPolicy::Honour,
    }
}

// ── Logging ──────────────────────────────────────────────────────────────────

/// stderr (developer diagnostics, kept from B1) plus a per-session log file, so
/// a tray-mode user who never sees stderr can still attach logs to an issue.
///
/// Never fatal: a missing/unwritable data dir degrades to stderr only.
fn init_logging(headless: bool) {
    let (log_path, file_chain) = match open_session_log() {
        Ok((path, file)) => {
            if headless {
                eprintln!("[log] Writing to: {}", path.display());
            }
            (Some(path), Some(file))
        }
        Err(e) => {
            eprintln!("tyutool-bridge: file logging disabled: {e:#}");
            (None, None)
        }
    };

    let mut dispatch = fern::Dispatch::new()
        .level(log::LevelFilter::Info)
        // The 1s discovery poller makes core's per-enumeration INFO line an
        // unbounded 1 Hz stream; keep only its warnings/errors — on both sinks.
        .level_for("tyutool_core::serial", log::LevelFilter::Warn)
        .chain(
            fern::Dispatch::new()
                .format(|out, message, record| {
                    out.finish(format_args!(
                        "[{}][{}] {}",
                        record.level(),
                        record.target(),
                        message
                    ))
                })
                .chain(std::io::stderr()),
        );

    if let Some(file) = file_chain {
        dispatch = dispatch.chain(
            fern::Dispatch::new()
                .format(|out, message, record| {
                    out.finish(format_args!(
                        "[{} {} {}] {}",
                        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"),
                        record.level(),
                        record.target(),
                        message
                    ))
                })
                .chain(file),
        );
    }

    if let Err(e) = dispatch.apply() {
        eprintln!("tyutool-bridge: logger init failed: {e}");
        return;
    }
    // Recorded in the log itself so a tray-mode user asked for "the log" can
    // find it without knowing the platform's data directory.
    if let Some(path) = log_path {
        log::info!("bridge session log: {}", path.display());
    }
}

/// Create `{data_dir}/tyutool-bridge/tyutool-bridge-<UTC timestamp>.log` and
/// prune older sessions. The name follows the CLI's `tyutool-<timestamp>.log`
/// scheme so "newest `*.log` by mtime" stays a valid way to find the live file.
///
/// TODO: no in-session size rollover yet (the CLI's `SessionLogWriter` caps a
/// single file at 10 MB); add it when the bridge grows chatty enough to matter.
fn open_session_log() -> anyhow::Result<(std::path::PathBuf, std::fs::File)> {
    let dir = dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("no platform data directory"))?
        .join("tyutool-bridge");
    std::fs::create_dir_all(&dir).map_err(|e| anyhow::anyhow!("create {}: {e}", dir.display()))?;
    prune_log_files(&dir);

    let path = dir.join(format!(
        "{LOG_FILE_PREFIX}{}.log",
        chrono::Utc::now().format("%Y%m%d-%H%M%SZ")
    ));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| anyhow::anyhow!("open {}: {e}", path.display()))?;
    Ok((path, file))
}

/// Delete the oldest session logs until the directory is within both limits.
/// Always keeps at least one file; only touches `LOG_FILE_PREFIX` files.
fn prune_log_files(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::path::PathBuf, u64)> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "log")
                && path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .is_some_and(|stem| stem.starts_with(LOG_FILE_PREFIX))
        })
        .map(|path| {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            (path, size)
        })
        .collect();

    // The timestamped names sort chronologically, so name order is age order.
    files.sort_by(|a, b| a.0.file_name().cmp(&b.0.file_name()));

    let mut count = files.len();
    let mut total: u64 = files.iter().map(|(_, size)| size).sum();
    for (path, size) in &files {
        if count <= 1 || (count <= MAX_LOG_FILES && total <= MAX_LOG_BYTES_TOTAL) {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            count -= 1;
            total = total.saturating_sub(*size);
        } else {
            // Locked by another instance: leave it and keep going.
            count -= 1;
        }
    }
}

// ── Security wiring ──────────────────────────────────────────────────────────

/// The persistent grant store, or an in-memory one when it cannot be opened.
///
/// Degrading is deliberate: a config directory that is missing or read-only must
/// not stop the helper from working at all — the user simply gets asked again
/// after a restart, which is the same answer B7 gives to any lost grant.
fn open_token_store() -> Arc<dyn TokenStore> {
    match FileTokenStore::open() {
        Ok(store) => Arc::new(store),
        Err(e) => {
            log::warn!(
                "bridge cannot open the persistent grant store ({e:#}); \
                 grants will only last for this session"
            );
            Arc::new(MemoryTokenStore::default())
        }
    }
}

// ── Confirmation dialog ──────────────────────────────────────────────────────

/// Title of the confirmation dialog.
fn confirm_title(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => "Cobuilder Bridge 需要你的确认",
        Lang::En => "Cobuilder Bridge needs your confirmation",
    }
}

/// The one label that authorizes a write, and the one that does not.
///
/// A pair, resolved once per dialog, rather than two lookups: on macOS the same
/// labels are both interpolated into the AppleScript *and* compared against what
/// osascript prints back, so if those two ever read from different [`Lang`]
/// values an English user's press of "Allow" would parse as a refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DialogLabels {
    approve: &'static str,
    reject: &'static str,
}

fn dialog_labels(lang: Lang) -> DialogLabels {
    match lang {
        Lang::Zh => DialogLabels {
            approve: "允许",
            reject: "拒绝",
        },
        Lang::En => DialogLabels {
            approve: "Allow",
            reject: "Deny",
        },
    }
}

/// Seconds the dialog waits before giving up by itself, matching the bridge's own
/// confirmation timeout. Both paths end in the same `user_rejected`, so the race
/// between them is harmless.
const DIALOG_TIMEOUT_SECS: u32 = 60;

/// The real human-in-the-loop gate: a modal the user has to answer before
/// anything is written to a device.
///
/// Without it injected, the library refuses every dangerous operation
/// (`DenyPrompt`), so the shipped helper could not flash at all.
struct SystemPrompt {
    /// The language the dialog is written in, snapshotted at startup.
    lang: Lang,
}

impl AuthPrompt for SystemPrompt {
    /// Returns immediately, as the trait requires: the dialog blocks a throwaway
    /// thread, never the async worker that is holding the execution right (and
    /// certainly not the tray's UI thread).
    fn request(&self, request: ConfirmRequest, respond: ConfirmResponder) {
        // Developer diagnostics, so the op is its stable `as_str()` label rather
        // than the dialog's wording (logging contract: `log::*` is never
        // localized) — same spelling as the audit line and `DenyPrompt` /
        // `UnattendedPrompt` below, so a grep for `flash`/`authorize` matches all.
        let what = format!(
            "{} on {} from {}",
            request.op.as_str(),
            request.port,
            request.origin
        );
        let lang = self.lang;
        let spawned = std::thread::Builder::new()
            .name("bridge-confirm".to_string())
            .spawn(move || respond(ask_user(&request, lang)));
        if let Err(e) = spawned {
            // `respond` went down with the closure, and the library reads a
            // dropped responder as "no consent given" — exactly the refusal this
            // situation calls for, so there is nothing to answer here.
            log::error!("bridge could not raise a confirmation dialog for {what}: {e}; refusing");
        }
    }
}

/// Headless default: refuse, and name the one switch that changes that.
///
/// A local copy of the library's inert `DenyPrompt` (which is private) rather
/// than "just leave the prompt uninjected": the library's own refusal cannot
/// mention [`UNATTENDED_FLAG`], and an operator whose flash silently fails needs
/// to be told *why* and what the alternative is. Making the library's default
/// chattier instead would be the wrong direction — its job is to be safe, not to
/// know about this binary's flags.
struct DenyPrompt;

impl AuthPrompt for DenyPrompt {
    fn request(&self, request: ConfirmRequest, respond: ConfirmResponder) {
        // Once per refusal: the operator sees one actionable line per attempt,
        // which is also what makes "it refused again" greppable.
        log::warn!(
            "headless mode refuses dangerous operations; pass {UNATTENDED_FLAG} if this machine \
             is meant to flash unattended — refused {:?} on {} (chip {}) from {}",
            request.op,
            request.port,
            request.chip_id,
            request.origin
        );
        respond(ConfirmDecision::Reject);
    }
}

/// The explicit escape hatch: approve immediately, and shout about it.
///
/// This is the one configuration in which a local process can write the user's
/// board with no human in the loop, so the log line is the only remaining trace
/// of who did what — it is warn level on *every* approval, never sampled or
/// summarised. The library's own audit trail records the same event
/// (`confirm … decision=approved`) from inside the confirm path, so this needs no
/// `AuditSink` of its own — one is not reachable from an `AuthPrompt` anyway
/// without changing the library.
struct UnattendedPrompt;

impl AuthPrompt for UnattendedPrompt {
    fn request(&self, request: ConfirmRequest, respond: ConfirmResponder) {
        log::warn!(
            "UNATTENDED APPROVAL ({UNATTENDED_FLAG}): allowing {:?} with no human confirmation — \
             origin={} chip={} port={}",
            request.op,
            request.origin,
            request.chip_id,
            request.port
        );
        respond(ConfirmDecision::Approve);
    }
}

/// The prompt that goes into `with_auth_prompt` for this run.
fn build_prompt(choice: PromptChoice, lang: Lang) -> Arc<dyn AuthPrompt> {
    match choice {
        PromptChoice::SystemDialog => Arc::new(SystemPrompt { lang }),
        PromptChoice::DenyAll => Arc::new(DenyPrompt),
        PromptChoice::UnattendedAutoApprove => Arc::new(UnattendedPrompt),
    }
}

/// What the user is being asked to authorize, in the wording the UI uses.
fn op_label(op: DangerousOp, lang: Lang) -> &'static str {
    match (op, lang) {
        (DangerousOp::Flash, Lang::Zh) => "烧录固件",
        (DangerousOp::Flash, Lang::En) => "Flash firmware",
        (DangerousOp::Authorize, Lang::Zh) => "写入授权码",
        (DangerousOp::Authorize, Lang::En) => "Write authorization code",
    }
}

/// The dialog body: who is asking, what for, and on which device.
///
/// Carries no credential — [`ConfirmRequest`] has no `uuid` / `auth_key` field by
/// construction, and it must stay that way.
fn confirm_message(request: &ConfirmRequest, lang: Lang) -> String {
    let labels = dialog_labels(lang);
    let (origin, op, chip, port) = (
        or_dash(&request.origin),
        op_label(request.op, lang),
        or_dash(&request.chip_id),
        or_dash(&request.port),
    );
    let mut text = match lang {
        Lang::Zh => format!("来源：{origin}\n操作：{op}\n芯片：{chip}\n串口：{port}\n"),
        Lang::En => format!("Origin: {origin}\nOperation: {op}\nChip: {chip}\nPort: {port}\n"),
    };
    if request.op == DangerousOp::Flash {
        let size = firmware_size_text(request.firmware_bytes, lang);
        text.push_str(&match lang {
            Lang::Zh => format!("固件大小：{size}\n"),
            Lang::En => format!("Firmware size: {size}\n"),
        });
    }
    // The button labels are interpolated rather than spelled out again: the text
    // must name the button the user is actually looking at.
    text.push_str(&match lang {
        Lang::Zh => format!("\n点「{}」即向该设备写入数据。", labels.approve),
        Lang::En => format!(
            "\nChoosing '{}' writes data to this device.",
            labels.approve
        ),
    });
    if request.op == DangerousOp::Authorize {
        text.push_str(match lang {
            Lang::Zh => "授权码写入会覆盖原有的值，且无法撤销。",
            Lang::En => {
                " Writing an authorization code overwrites the existing value, and \
                 cannot be undone."
            }
        });
    }
    text.push_str(&match lang {
        Lang::Zh => format!(
            "\n如果这不是你本人刚刚在页面上发起的操作，请点「{}」。",
            labels.reject
        ),
        Lang::En => format!(
            "\nIf you did not just start this yourself from the page, choose '{}'.",
            labels.reject
        ),
    });
    text
}

/// Client-supplied strings can be empty; an empty line reads as a broken dialog.
fn or_dash(value: &str) -> &str {
    if value.trim().is_empty() {
        "-"
    } else {
        value
    }
}

/// Firmware size for humans. `None` = the payload's base64 was malformed, so the
/// job will fail later anyway; the dialog says so rather than inventing a number.
fn firmware_size_text(bytes: Option<u64>, lang: Lang) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    let Some(n) = bytes else {
        return match lang {
            Lang::Zh => "未知（固件数据异常）".to_string(),
            Lang::En => "unknown (malformed firmware data)".to_string(),
        };
    };
    if n < KIB {
        return match lang {
            Lang::Zh => format!("{n} 字节"),
            Lang::En => format!("{n} bytes"),
        };
    }
    let (value, unit) = if n < MIB {
        (n as f64 / KIB as f64, "KiB")
    } else {
        (n as f64 / MIB as f64, "MiB")
    };
    match lang {
        Lang::Zh => format!("{value:.1} {unit}（{n} 字节）"),
        Lang::En => format!("{value:.1} {unit} ({n} bytes)"),
    }
}

/// Ask the user, blocking until they answer (or the dialog gives up).
///
/// Everything that is not an explicit press of the approve button
/// ([`DialogLabels::approve`]) — a refusal, a
/// cancel, the dialog giving up, a missing dialog tool, unparsable output — maps
/// to [`ConfirmDecision::Reject`]: silence never opens the door.
/// The AppleScript for one confirmation dialog, in the labels it was handed.
///
/// Split out of [`ask_user`] together with [`macos_reply_approves`] so the two
/// halves of the label coupling can be tested against each other: they take the
/// *same* [`DialogLabels`] value, and the test round-trips the button label out
/// of the generated script and back through the parser.
///
/// `default button` is the *refusing* one on purpose: a stray Return keypress
/// must never authorize a flash.
#[cfg(target_os = "macos")]
fn macos_dialog_script(request: &ConfirmRequest, labels: &DialogLabels, lang: Lang) -> String {
    format!(
        "display dialog \"{message}\" with title \"{title}\" \
         buttons {{\"{reject}\", \"{approve}\"}} \
         default button \"{reject}\" with icon caution \
         giving up after {DIALOG_TIMEOUT_SECS}",
        message = applescript_escape(&confirm_message(request, lang)),
        title = applescript_escape(confirm_title(lang)),
        reject = labels.reject,
        approve = labels.approve,
    )
}

/// Did osascript report a press of the approving button?
///
/// `display dialog` prints one record line, e.g.
/// `button returned:允许, gave up:false`. A refusal, an Escape (osascript exits
/// non-zero) and the giving-up path all fail this check.
#[cfg(target_os = "macos")]
fn macos_reply_approves(stdout: &str, labels: &DialogLabels) -> bool {
    let pressed_approve = stdout.lines().next().is_some_and(|line| {
        line.split(", ")
            .any(|field| field.trim() == format!("button returned:{}", labels.approve))
    });
    pressed_approve && !stdout.contains("gave up:true")
}

#[cfg(target_os = "macos")]
fn ask_user(request: &ConfirmRequest, lang: Lang) -> ConfirmDecision {
    // Interim path until the helper ships as a signed `.app` bundle: an unbundled
    // binary has no bundle identity, so a native `NSAlert` / `UNUserNotification`
    // is not available to it, while `osascript` (which is itself a bundled app)
    // works today. The packaging slice replaces this with a real NSAlert.
    //
    // One snapshot for both the script and the reply parsing: see [`DialogLabels`].
    let labels = dialog_labels(lang);
    let script = macos_dialog_script(request, &labels, lang);

    let output = match hidden_command("osascript").arg("-e").arg(&script).output() {
        Ok(output) => output,
        Err(e) => {
            log::error!(
                "bridge could not run osascript for the confirmation dialog: {e}; refusing"
            );
            return ConfirmDecision::Reject;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let approved = output.status.success() && macos_reply_approves(&stdout, &labels);
    decision(approved, &stdout)
}

#[cfg(target_os = "windows")]
fn ask_user(request: &ConfirmRequest, lang: Lang) -> ConfirmDecision {
    // Untested on the machine this was written on (compile-only), so it is kept
    // to one obviously correct PowerShell line. WPF's MessageBox ships with every
    // supported Windows and needs no extra runtime.
    //
    // `'No'` is the default result, so Return / Escape does not authorize a write.
    // Two knowingly accepted differences from macOS: the buttons read
    // 是/否 (the system's own labels, not 允许/拒绝), and a MessageBox has no
    // timeout — after DIALOG_TIMEOUT_SECS the bridge has already refused the
    // operation, and this dialog's late answer is simply ignored.
    let script = format!(
        "Add-Type -AssemblyName PresentationFramework; \
         [System.Windows.MessageBox]::Show({message},{title},'YesNo','Exclamation','No')",
        message = powershell_string(&confirm_message(request, lang)),
        title = powershell_string(confirm_title(lang)),
    );

    let output = match hidden_command("powershell")
        .args(["-NoProfile", "-Command"])
        .arg(&script)
        .output()
    {
        Ok(output) => output,
        Err(e) => {
            log::error!(
                "bridge could not run powershell for the confirmation dialog: {e}; refusing"
            );
            return ConfirmDecision::Reject;
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let approved = output.status.success() && stdout.trim() == "Yes";
    decision(approved, &stdout)
}

/// Which Linux dialog program the text is being built for.
///
/// The two differ in what they can promise the user, not just in argv: only
/// zenity can put the focus on the refusing button (see [`linux_dialog_text`]).
///
/// Compiled in test builds on every platform (like [`powershell_string`]) so the
/// escaping is covered on the machines this repo is actually developed on.
#[cfg(any(all(unix, not(target_os = "macos")), test))]
#[derive(Debug, Copy, Clone, PartialEq)]
enum LinuxDialogTool {
    Zenity,
    KDialog,
}

/// The exact string handed to `zenity --text` / `kdialog --yesno`.
///
/// Both programs render **markup**: zenity interprets Pango markup, and kdialog
/// hands the text to Qt, which auto-detects rich text. [`confirm_message`]
/// interpolates client-controlled values (`chip_id`, `port`), so without escaping
/// a local process could restyle the very dialog that is asking about it — hide
/// the 来源 line, fake an official-looking banner, shrink the warning to nothing.
/// That would break the one invariant the whole confirmation design rests on: the
/// dialog must accurately describe the operation being authorized.
///
/// The hostile characters are escaped, never dropped: a user who is being
/// attacked should see the literal `<b>` in their dialog rather than text that was
/// quietly rewritten behind their back.
#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn linux_dialog_text(request: &ConfirmRequest, tool: LinuxDialogTool, lang: Lang) -> String {
    let mut text = escape_markup(&confirm_message(request, lang));
    if tool == LinuxDialogTool::KDialog {
        // Only on this branch: zenity gets `--default-cancel`, and a warning that
        // is shown always is a warning users learn to skip.
        text.push_str(kdialog_default_button_warning(lang));
    }
    text
}

/// kdialog's `--yesno` cannot say which button is focused, so its text has to.
#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn kdialog_default_button_warning(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => {
            "\n\n注意：这个对话框无法把「否」设为默认按钮，直接按回车有可能就等于同意。\
             不确定的话请用鼠标点「否」。"
        }
        // 'No' rather than the localized reject label: kdialog's --yesno takes
        // the system's own button wording, which this binary does not set.
        Lang::En => {
            "\n\nNote: this dialog cannot make 'No' its default button, so pressing Return \
             may count as agreeing. If in doubt, click 'No' with the mouse."
        }
    }
}

/// Escape `text` so a markup-rendering dialog shows it verbatim.
///
/// Double escaping is impossible by construction, not by arm order: this is a
/// single pass over the *input* chars, each consumed exactly once, and the
/// replacement text is appended to the output rather than re-scanned. So `<`
/// becomes `&lt;` and the `&` that produced never comes back round — unlike a
/// chain of `str::replace` passes, where the order really would matter.
#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn escape_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(all(unix, not(target_os = "macos")))]
fn ask_user(request: &ConfirmRequest, lang: Lang) -> ConfirmDecision {
    // Compile-only on the machine this was written on, like the Windows arm.
    // Arguments go to the program as argv (no shell in the loop), so there is no
    // quoting to do — but both programs render markup, hence the escaping in
    // `linux_dialog_text`.
    //
    // Deliberately *not* passing zenity's `--no-markup`: it does not exist on
    // every version still in the field, and an unknown flag makes zenity exit
    // non-zero, which would turn every dangerous operation into a hard refusal.
    // The escaping is the guarantee; the flag would only have been belt-and-braces.
    let labels = dialog_labels(lang);
    let zenity_text = linux_dialog_text(request, LinuxDialogTool::Zenity, lang);

    // zenity first: it is the only one of the two that can make the refusing
    // button the default, so a stray Return cannot authorize a write.
    let zenity = hidden_command("zenity")
        .arg("--question")
        .args(["--title", confirm_title(lang)])
        .arg("--text")
        .arg(&zenity_text)
        .args(["--ok-label", labels.approve])
        .args(["--cancel-label", labels.reject])
        .arg("--default-cancel")
        .arg(format!("--timeout={DIALOG_TIMEOUT_SECS}"))
        .status();
    match zenity {
        // Exit 0 is the OK label; refusal (1), timeout (5) and errors are not.
        Ok(status) => return decision(status.success(), &format!("zenity exit {status}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            log::error!("bridge could not run zenity for the confirmation dialog: {e}; refusing");
            return ConfirmDecision::Reject;
        }
    }

    // KDE fallback. `--yesno` has no way to say which button is focused, hence
    // second place — and hence the extra warning line in its text.
    let kdialog = hidden_command("kdialog")
        .args(["--title", confirm_title(lang)])
        .arg("--yesno")
        .arg(linux_dialog_text(request, LinuxDialogTool::KDialog, lang))
        .status();
    match kdialog {
        Ok(status) => return decision(status.success(), &format!("kdialog exit {status}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            log::error!("bridge could not run kdialog for the confirmation dialog: {e}; refusing");
            return ConfirmDecision::Reject;
        }
    }

    log::error!(
        "bridge found no confirmation dialog tool (install zenity or kdialog); \
         refusing the operation — without a dialog there is no way to ask the user"
    );
    ConfirmDecision::Reject
}

/// Map an "approved?" answer onto the decision, logging what was observed.
fn decision(approved: bool, observed: &str) -> ConfirmDecision {
    if approved {
        log::info!("bridge confirmation dialog: the user allowed the operation");
        ConfirmDecision::Approve
    } else {
        log::info!(
            "bridge confirmation dialog: not approved, refusing ({})",
            observed.trim()
        );
        ConfirmDecision::Reject
    }
}

/// Escape `text` for an AppleScript **string literal**.
///
/// The values interpolated into the script are client-supplied (port, chip id),
/// so a quote or a backslash must not be able to end the literal and turn the
/// rest into script. Newlines become the `\n` escape (an AppleScript literal
/// cannot span source lines), and any other control character becomes a space.
///
/// No shell quoting is needed on top: the script is passed to `osascript` as a
/// single argv element, so no shell ever parses it.
#[cfg(target_os = "macos")]
fn applescript_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Render `text` as a PowerShell expression producing that exact string.
///
/// Single-quoted literal (no escape processing at all inside one, so nothing can
/// be interpolated), with `''` for a quote and an explicit `[char]10` for a
/// newline — that way the generated script text has no line breaks of its own,
/// whatever the message contains.
///
/// Compiled in test builds on every platform (not just Windows) so its quoting
/// rules are covered by CI on the machines this repo is actually developed on.
#[cfg(any(target_os = "windows", test))]
fn powershell_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for ch in text.chars() {
        match ch {
            '\'' => out.push_str("''"),
            '\n' => out.push_str("' + [char]10 + '"),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

// ── System notifications ─────────────────────────────────────────────────────

/// Best-effort system notification: what the user sees when the bridge could not
/// start, or when a revocation went through, without opening the tray menu.
///
/// Fire and forget on its own thread — the UI thread must never wait on a
/// subprocess, and a missing notification tool is a warning, never a failure.
fn notify(title: &str, body: &str) {
    let owned_title = title.to_string();
    let owned_body = body.to_string();
    let spawned = std::thread::Builder::new()
        .name("bridge-notify".to_string())
        .spawn(move || show_notification(&owned_title, &owned_body));
    if let Err(e) = spawned {
        log::warn!("bridge could not spawn the notifier for {title:?}: {e}");
    }
}

/// `display notification` works from a bare binary (osascript supplies the bundle
/// identity), which is why the startup-failure notification does not have to wait
/// for the .app packaging slice.
#[cfg(target_os = "macos")]
fn show_notification(title: &str, body: &str) {
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        applescript_escape(body),
        applescript_escape(title)
    );
    match hidden_command("osascript").arg("-e").arg(&script).status() {
        Ok(status) if status.success() => {}
        Ok(status) => log::warn!("bridge notification not shown: osascript exit {status}"),
        Err(e) => log::warn!("bridge notification not shown: {e}"),
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn show_notification(title: &str, body: &str) {
    // argv, no shell: nothing to escape.
    match hidden_command("notify-send").arg(title).arg(body).status() {
        Ok(status) if status.success() => {}
        Ok(status) => log::warn!("bridge notification not shown: notify-send exit {status}"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            log::warn!("bridge notification not shown: notify-send is not installed")
        }
        Err(e) => log::warn!("bridge notification not shown: {e}"),
    }
}

/// Windows toasts need an installed app identity (AppUserModelID), which a bare
/// `.exe` does not have — the packaging slice owns this one.
#[cfg(target_os = "windows")]
fn show_notification(title: &str, body: &str) {
    log::warn!(
        "bridge has no system notification on Windows yet (a toast needs an installed \
         app identity — the packaging slice owns it); would have shown {title:?}: {body:?}"
    );
}

// ── Headless mode ────────────────────────────────────────────────────────────

/// What `--headless` prints to the console about its confirmation policy.
///
/// The operator has to be able to tell "it refused" from "it was never going to
/// ask", so both branches name [`UNATTENDED_FLAG`] — one as the switch that is
/// already on, the other as the switch that would turn writes on.
fn headless_startup_line(choice: PromptChoice, lang: Lang) -> String {
    match (choice, lang) {
        (PromptChoice::UnattendedAutoApprove, Lang::Zh) => format!(
            "tyutool-bridge: {UNATTENDED_FLAG} 已开启——烧录/写授权码将自动放行，不再询问用户。"
        ),
        (PromptChoice::UnattendedAutoApprove, Lang::En) => format!(
            "tyutool-bridge: {UNATTENDED_FLAG} is on — flashing and authorization writes are \
             approved automatically, with no user confirmation."
        ),
        (_, Lang::Zh) => format!(
            "tyutool-bridge: headless 模式默认拒绝烧录/写授权码（无人可确认）；\
             需要无人值守烧录请加 {UNATTENDED_FLAG}。"
        ),
        (_, Lang::En) => format!(
            "tyutool-bridge: headless mode refuses flashing and authorization writes by default \
             (there is nobody to confirm with); pass {UNATTENDED_FLAG} to allow unattended writes."
        ),
    }
}

/// Serve until killed. Exits non-zero when the port is taken: a supervisor or
/// smoke script needs that signal, whereas the tray shell deliberately stays
/// resident and shows the error in its status line instead.
fn run_headless(choice: PromptChoice, lang: Lang) {
    // Stated once at startup, on both channels: the operator has to be able to
    // tell "it refused" from "it was never going to ask" without reading code.
    // The log half is developer diagnostics and stays English (logging contract);
    // only the console half, which the operator reads, follows the system
    // language.
    match choice {
        PromptChoice::UnattendedAutoApprove => log::warn!(
            "bridge headless mode started with {UNATTENDED_FLAG}: every dangerous operation \
             will be approved automatically, with no human confirmation"
        ),
        _ => log::info!(
            "bridge headless mode refuses dangerous operations (no user to confirm with); \
             pass {UNATTENDED_FLAG} to allow unattended writes"
        ),
    }
    eprintln!("{}", headless_startup_line(choice, lang));

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("tyutool-bridge: failed to start the async runtime: {e}");
            std::process::exit(1);
        }
    };
    runtime.block_on(async {
        let server = match bind(DEFAULT_PORT).await {
            Ok(server) => server,
            Err(e) => {
                eprintln!("tyutool-bridge: failed to start on 127.0.0.1:{DEFAULT_PORT}: {e:#}");
                std::process::exit(1);
            }
        };
        // Headless is a serving mode, and by itself never a "skip the
        // confirmation" mode: `prompt_choice` gives it a refusing prompt unless
        // the operator opted in with UNATTENDED_FLAG, and `grant_policy_for`
        // makes the grant file stop counting in the same breath — otherwise the
        // refusing prompt would never even be consulted (see its doc comment).
        //
        // No tray means no "撤销所有授权" item here; deleting the grant file (or
        // starting the tray shell once) is the revocation path in this mode.
        let server = server
            .with_token_store(open_token_store())
            .with_auth_prompt(build_prompt(choice, lang))
            .with_grant_policy(grant_policy_for(choice));
        println!("tyutool-bridge listening on ws://127.0.0.1:{DEFAULT_PORT}");
        if let Err(e) = server.run().await {
            eprintln!("tyutool-bridge: server error: {e:#}");
            std::process::exit(1);
        }
    });
}

// ── Tray mode ────────────────────────────────────────────────────────────────

/// Everything the background runtime and the menu report back to the UI thread.
#[derive(Debug)]
enum UserEvent {
    /// New counters from the server's watch channel.
    Stats(StatsSnapshot),
    /// The server could not start; the status line becomes the error state.
    StartupFailed(String),
    /// The server is up and handed over its revocation control, which the tray's
    /// "撤销所有授权" item drives. Travels the same way [`UserEvent::Stats`] does
    /// because the two threads are separate: the server runs on the background
    /// runtime, the menu on the UI thread.
    AuthorityReady(Authority),
    /// A tray menu item was activated.
    Menu(muda::MenuId),
}

fn run_tray(choice: PromptChoice, lang: Lang) {
    // Before anything shared is touched. The authoritative bind still happens on
    // the server thread below (and still handles the case where someone grabs the
    // port in between), but by then this process has already reconciled the login
    // item and put an icon in the menu bar — both of which a doomed instance must
    // not do. See the test in `tests`.
    exit_if_already_running();

    // macOS pins the whole menu-bar/NSApplication stack to the main thread, so
    // the event loop must be built here and the server pushed to a side thread
    // (not the other way round).
    // `mut` is consumed only by the macOS activation policy below.
    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        // Menu-bar-only process: no Dock icon, no app switcher entry.
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
    }
    let proxy = event_loop.create_proxy();

    // muda delivers menu activations on its own global channel; funnel them into
    // the event loop so all UI mutation happens in one place.
    let menu_proxy = proxy.clone();
    muda::MenuEvent::set_event_handler(Some(move |event: muda::MenuEvent| {
        let _ = menu_proxy.send_event(UserEvent::Menu(event.id));
    }));

    // Reconcile the login-item registration with what the user last chose, and
    // remember what was actually reached — that, not a hardcoded default, is what
    // the menu's tick shows. See `autostart`'s module docs for why the recorded
    // choice and the OS bit are two different things.
    let autostart = open_autostart();
    let autostart_state = match &autostart {
        Some((preference, registration)) => {
            autostart::apply_at_startup(preference, registration.as_ref())
        }
        // Nothing to reconcile against and nothing the user can toggle; the tick
        // says "off" because that is the truth about this session.
        None => false,
    };

    // Detached on purpose: the tray owns the process lifetime, and quitting
    // tears the runtime down with it.
    let server_proxy = proxy.clone();
    let spawned = std::thread::Builder::new()
        .name("bridge-server".to_string())
        .spawn(move || serve_in_background(server_proxy, choice, lang));
    if let Err(e) = spawned {
        log::error!("bridge server thread could not be started: {e}");
        let _ = proxy.send_event(UserEvent::StartupFailed(status::startup_failed_line(
            e, lang,
        )));
    }

    let mut tray: Option<TrayShell> = None;
    let mut status_text = status::status_line(VERSION, &StatsSnapshot::default(), lang);
    // `None` until the server thread reports in; clicking "撤销所有授权" before
    // then is a no-op, not a panic.
    let mut authority: Option<Authority> = None;
    // Whatever the reconciliation above actually reached — the value the tick is
    // built from and the value the next toggle flips, so the menu can never claim
    // a setting the OS did not accept.
    let mut autostart_on = autostart_state;

    event_loop.run(move |event, _target, control_flow| {
        // Purely event-driven: nothing to poll between stats pushes and clicks.
        *control_flow = ControlFlow::Wait;
        match event {
            // tao guarantees this is the first event, and on macOS the status
            // item may only be created once the app is initialized.
            Event::NewEvents(StartCause::Init) => {
                match TrayShell::build(&status_text, autostart_on, lang) {
                    Ok(shell) => tray = Some(shell),
                    // No icon means no menu, and no menu means no way to quit: the
                    // process would keep serving from a UI loop that can never
                    // receive an event, invisible and killable only from Activity
                    // Monitor / taskkill. So fail loudly and name the mode that
                    // works in a tray-less environment (a real case on Linux
                    // desktops without a StatusNotifier host).
                    //
                    // Not "degrade to headless in place": `tao::EventLoop::run`
                    // never returns (on macOS it exits the process), so there is no
                    // after-the-loop to fall through to. Exiting non-zero also
                    // makes the failure visible to whatever autostarts us, which a
                    // silent resident process would not be.
                    Err(e) => {
                        log::error!(
                            "bridge tray icon could not be created: {e:#}; no usable system tray \
                         in this environment — run `tyutool-bridge --headless` instead"
                        );
                        eprintln!(
                            "tyutool-bridge: no usable system tray ({e:#}); \
                         run `tyutool-bridge --headless` instead"
                        );
                        // The error line is the whole point of this exit; make sure
                        // it reached the log file before the process goes away.
                        log::logger().flush();
                        std::process::exit(1);
                    }
                }
            }
            Event::UserEvent(UserEvent::Stats(snapshot)) => {
                status_text = status::status_line(VERSION, &snapshot, lang);
                if let Some(shell) = &tray {
                    shell.set_status(&status_text);
                }
            }
            Event::UserEvent(UserEvent::StartupFailed(text)) => {
                status_text = text;
                if let Some(shell) = &tray {
                    shell.set_status(&status_text);
                }
                // The status line alone only reaches a user who opens the menu,
                // and a helper that never came up is precisely the case where
                // nobody thinks to look there.
                notify(startup_failed_notification_title(lang), &status_text);
            }
            Event::UserEvent(UserEvent::AuthorityReady(handle)) => {
                authority = Some(handle);
            }
            Event::UserEvent(UserEvent::Menu(id)) => {
                if let Some(shell) = &tray {
                    match shell.action_for(&id) {
                        Some(MenuAction::OpenCobuilder) => open_url(COBUILDER_URL),
                        Some(MenuAction::LatestVersion) => open_url(LATEST_VERSION_URL),
                        Some(MenuAction::ToggleAutostart) => {
                            autostart_on = match &autostart {
                                Some((preference, registration)) => autostart::toggle(
                                    preference,
                                    registration.as_ref(),
                                    autostart_on,
                                ),
                                None => false,
                            };
                            // The platform already ticked the item on click, so
                            // this only matters when the flip did *not* take —
                            // and that is exactly when the menu must not lie.
                            shell.set_autostart_checked(autostart_on);
                        }
                        Some(MenuAction::RevokeGrants) => revoke_all(authority.as_ref(), lang),
                        Some(MenuAction::Quit) => *control_flow = ControlFlow::Exit,
                        None => {}
                    }
                }
            }
            // Drop the status item explicitly: on macOS `run` ends the process
            // without unwinding, so relying on Drop would leave the icon behind.
            Event::LoopDestroyed => {
                tray = None;
                log::info!("bridge tray shell exiting");
            }
            _ => {}
        }
    });
}

/// Background runtime: bind, publish stats to the UI thread, serve forever.
fn serve_in_background(proxy: EventLoopProxy<UserEvent>, choice: PromptChoice, lang: Lang) {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            log::error!("bridge async runtime could not be created: {e}");
            let _ = proxy.send_event(UserEvent::StartupFailed(status::startup_failed_line(
                e, lang,
            )));
            return;
        }
    };

    runtime.block_on(async move {
        let server = match bind(DEFAULT_PORT).await {
            Ok(server) => server,
            Err(e) => {
                let diagnosis = status::diagnose_bind_error(&e);
                if status::tray_startup_failure_action(diagnosis)
                    == status::StartupFailureAction::ExitSilently
                {
                    // The bridge the user wanted is already running; double-click
                    // is a routine gesture and gets no feedback. Logged (never
                    // shown) so a bug report still explains the "nothing
                    // happened" — see `tray_startup_failure_action`.
                    log::info!(
                        "bridge is already running on 127.0.0.1:{DEFAULT_PORT}; \
                         this instance exits without showing anything: {e:#}"
                    );
                    log::logger().flush();
                    // From the server thread: this ends the whole process, which
                    // is the point — returning would leave the event loop running
                    // a second, permanently idle status item.
                    std::process::exit(0);
                }
                // Resident on every other failure (unlike --headless): the whole
                // point of the tray is that the user finds out *why* nothing works.
                let line = status::startup_error_line(diagnosis, &e, lang);
                // The status-line copy lands in the log too, so a bug report
                // shows exactly what the user was reading in the tray.
                log::error!(
                    "bridge failed to start on 127.0.0.1:{DEFAULT_PORT} ({diagnosis:?}): {e:#}; \
                     tray status line: {line}"
                );
                let _ = proxy.send_event(UserEvent::StartupFailed(line));
                return;
            }
        };
        let server = server
            .with_token_store(open_token_store())
            .with_auth_prompt(build_prompt(choice, lang))
            // Always `Honour` in tray mode (a user is present), stated explicitly
            // so the prompt and the grant policy stay one decision.
            .with_grant_policy(grant_policy_for(choice));
        // Taken *after* `with_token_store` (it snapshots the store) and *before*
        // `run_with_stats` (which consumes the server), then handed to the UI
        // thread — the tray's "撤销所有授权" item lives over there.
        if proxy
            .send_event(UserEvent::AuthorityReady(server.authority()))
            .is_err()
        {
            log::warn!("bridge tray is gone before the server started; not serving");
            return;
        }
        log::info!("bridge listening on ws://127.0.0.1:{DEFAULT_PORT}");

        let (stats_tx, mut stats_rx) = tokio::sync::watch::channel(StatsSnapshot::default());
        let stats_proxy = proxy.clone();
        tokio::spawn(async move {
            while stats_rx.changed().await.is_ok() {
                let snapshot = *stats_rx.borrow_and_update();
                if stats_proxy.send_event(UserEvent::Stats(snapshot)).is_err() {
                    // The event loop is gone: the process is on its way out.
                    return;
                }
            }
        });

        if let Err(e) = server.run_with_stats(stats_tx).await {
            log::error!("bridge server stopped: {e:#}");
            let _ = proxy.send_event(UserEvent::StartupFailed(status::server_stopped_line(
                e, lang,
            )));
        }
    });
}

/// Withdraw every authorization the user ever granted.
///
/// Runs on the UI thread: `revoke_all` neither awaits nor blocks on the network
/// (it clears a small local file and queues one frame per live connection), so
/// the menu does not need a worker thread for it.
fn revoke_all(authority: Option<&Authority>, lang: Lang) {
    let Some(authority) = authority else {
        // Clicked in the window between the tray appearing and the server
        // reporting in — or after a startup failure, when there is nothing to
        // revoke. Either way: say so, never panic in a resident process.
        log::warn!("bridge cannot revoke: the server has not started yet, nothing was granted");
        return;
    };
    authority.revoke_all();
    log::info!("bridge revoked all authorizations from the tray menu");
    // Confirmation the user can see: the menu item gives no feedback of its own,
    // and a security control that looks like it did nothing invites a second click.
    notify("Cobuilder Bridge", revoked_notification_body(lang));
}

/// The tray menu's command items, in menu order.
///
/// A struct rather than an array: the items are built and matched by name, and a
/// positional list is exactly how a translation ends up wiring "Quit" to the
/// revocation item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MenuLabels {
    open_cobuilder: &'static str,
    latest_version: &'static str,
    autostart: &'static str,
    revoke_grants: &'static str,
    quit: &'static str,
}

fn menu_labels(lang: Lang) -> MenuLabels {
    match lang {
        Lang::Zh => MenuLabels {
            open_cobuilder: "打开 Cobuilder",
            latest_version: "获取最新版本",
            // A checkable item, so the label states the setting rather than an
            // action ("开机自启" + a tick), the convention every platform's own
            // menus use for a toggle.
            autostart: "开机自启",
            revoke_grants: "撤销所有授权",
            quit: "退出",
        },
        Lang::En => MenuLabels {
            open_cobuilder: "Open Cobuilder",
            latest_version: "Get the latest version",
            autostart: "Start at login",
            revoke_grants: "Revoke all authorizations",
            quit: "Quit",
        },
    }
}

/// Title of the notification fired when the bridge could not start.
///
/// The status line alone only reaches a user who opens the menu, and a helper
/// that never came up is precisely the case where nobody thinks to look there.
fn startup_failed_notification_title(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => "Cobuilder Bridge 启动失败",
        Lang::En => "Cobuilder Bridge failed to start",
    }
}

/// The revocation notification's body — the only feedback the menu item gives,
/// so it says both what happened and what changes next time.
fn revoked_notification_body(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => "已撤销所有授权，下次烧录会重新询问你。",
        Lang::En => "All authorizations revoked; the next flash will ask you again.",
    }
}

/// What a tray menu item does. Kept separate from the muda ids so the event
/// handling above reads as behaviour rather than id comparisons.
enum MenuAction {
    OpenCobuilder,
    LatestVersion,
    ToggleAutostart,
    RevokeGrants,
    Quit,
}

/// The live status item: icon, menu, and the ids needed to route clicks.
///
/// Held by the UI thread only — muda/tray-icon handles are not `Send`.
struct TrayShell {
    // Both handles must stay alive for the item to remain in the menu bar.
    _icon: tray_icon::TrayIcon,
    _menu: muda::Menu,
    status_item: muda::MenuItem,
    /// Held (not just its id) because the tick has to be updated after a toggle.
    autostart_item: muda::CheckMenuItem,
    open_cobuilder: muda::MenuId,
    latest_version: muda::MenuId,
    autostart: muda::MenuId,
    revoke_grants: muda::MenuId,
    quit: muda::MenuId,
}

impl TrayShell {
    fn build(status_text: &str, autostart_on: bool, lang: Lang) -> anyhow::Result<Self> {
        let labels = menu_labels(lang);
        // Disabled: a status readout, not a command.
        let status_item = muda::MenuItem::new(status_text, false, None);
        let open_cobuilder = muda::MenuItem::new(labels.open_cobuilder, true, None);
        let latest_version = muda::MenuItem::new(labels.latest_version, true, None);
        // Built from the *reconciled* state, not from a hardcoded `true`: the tick
        // is the only place the user can read what the setting currently is, so it
        // must reflect what `autostart::apply_at_startup` actually achieved —
        // including the case where the OS refused.
        let autostart_item = muda::CheckMenuItem::new(labels.autostart, true, autostart_on, None);
        let revoke_grants = muda::MenuItem::new(labels.revoke_grants, true, None);
        let quit = muda::MenuItem::new(labels.quit, true, None);

        let menu = muda::Menu::new();
        menu.append_items(&[
            &status_item,
            &muda::PredefinedMenuItem::separator(),
            &open_cobuilder,
            &latest_version,
            // Grouped with the settings above rather than next to "退出": it is a
            // preference, and its neighbour on the other side is a security
            // control that must not be a mis-click away from a routine toggle.
            &autostart_item,
            &revoke_grants,
            &muda::PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|e| anyhow::anyhow!("build tray menu: {e}"))?;

        let (glyph, is_template) = tray_glyph_icon()?;
        let icon = tray_icon::TrayIconBuilder::new()
            .with_menu(Box::new(menu.clone()))
            .with_icon(glyph)
            // macOS-only, and the only reason light/dark works there for free: the
            // system tints a template image to match the menu bar. Passed as the
            // artwork's own flag rather than a literal `true`, because on Windows
            // and Linux nothing recolours the icon — they get the colour logo and
            // this must be `false` to describe it honestly.
            .with_icon_as_template(is_template)
            .with_tooltip("Cobuilder Bridge")
            .build()
            .map_err(|e| anyhow::anyhow!("create tray icon: {e}"))?;

        Ok(Self {
            _icon: icon,
            open_cobuilder: open_cobuilder.id().clone(),
            latest_version: latest_version.id().clone(),
            autostart: autostart_item.id().clone(),
            revoke_grants: revoke_grants.id().clone(),
            quit: quit.id().clone(),
            status_item,
            autostart_item,
            _menu: menu,
        })
    }

    fn set_status(&self, text: &str) {
        self.status_item.set_text(text);
    }

    /// Force the tick to a known state.
    ///
    /// Not a no-op even right after a click: the platform menu ticks the item
    /// itself on activation, so when the OS refuses the change the tick is already
    /// wrong and has to be put back — otherwise the menu claims a setting that is
    /// not in effect.
    fn set_autostart_checked(&self, checked: bool) {
        self.autostart_item.set_checked(checked);
    }

    fn action_for(&self, id: &muda::MenuId) -> Option<MenuAction> {
        if *id == self.open_cobuilder {
            Some(MenuAction::OpenCobuilder)
        } else if *id == self.latest_version {
            Some(MenuAction::LatestVersion)
        } else if *id == self.autostart {
            Some(MenuAction::ToggleAutostart)
        } else if *id == self.revoke_grants {
            Some(MenuAction::RevokeGrants)
        } else if *id == self.quit {
            Some(MenuAction::Quit)
        } else {
            None
        }
    }
}

/// The product logo, from the artwork embedded by [`tray_glyph`].
///
/// Which of the two assets and whether to ask for template rendering is that
/// module's decision, not this one's — the flag travels with the pixels so a
/// black silhouette can never be handed to a platform that will not tint it (it
/// would be invisible on a dark taskbar). See its module docs.
fn tray_glyph_icon() -> anyhow::Result<(tray_icon::Icon, bool)> {
    let glyph = tray_glyph::for_this_platform()?;
    let icon = tray_icon::Icon::from_rgba(glyph.rgba, glyph.width, glyph.height)
        .map_err(|e| anyhow::anyhow!("build tray icon bitmap: {e}"))?;
    Ok((icon, glyph.is_template))
}

/// Hand the URL to the platform's default handler. `std::process::Command`
/// rather than a helper crate: one command per platform is the whole feature.
///
/// Waited on in a throwaway thread so the launcher process is reaped without
/// the UI thread ever blocking on it.
fn open_url(url: &'static str) {
    let spawned = std::thread::Builder::new()
        .name("bridge-open-url".to_string())
        .spawn(move || {
            #[cfg(target_os = "macos")]
            let mut command = {
                let mut c = hidden_command("open");
                c.arg(url);
                c
            };
            #[cfg(target_os = "windows")]
            let mut command = {
                let mut c = hidden_command("cmd");
                // Empty title argument: `start` treats a lone quoted argument
                // as the window title otherwise.
                c.args(["/C", "start", "", url]);
                c
            };
            #[cfg(all(unix, not(target_os = "macos")))]
            let mut command = {
                let mut c = hidden_command("xdg-open");
                c.arg(url);
                c
            };

            match command.status() {
                Ok(status) if status.success() => {}
                Ok(status) => log::warn!("bridge could not open {url}: exit {status}"),
                Err(e) => log::warn!("bridge could not open {url}: {e}"),
            }
        });
    if let Err(e) = spawned {
        log::warn!("bridge could not spawn the URL opener for {url}: {e}");
    }
}

// ── Autostart ────────────────────────────────────────────────────────────────

/// Quit immediately, and quietly, if another instance already holds the port.
///
/// A cheap synchronous probe: bind the port and drop it again. It duplicates no
/// *decision* — the verdict still comes from [`status::diagnose_bind_error`] and
/// [`status::tray_startup_failure_action`], the same pair the server thread
/// consults — it only asks the question earlier, while giving up is still free.
///
/// Reliable on all three platforms despite `SO_REUSEADDR` looking like it should
/// spoil it: on unix `std` sets `SO_REUSEADDR`, but that only affects `TIME_WAIT`
/// sockets, so binding over a *live* listener still fails with `EADDRINUSE`; and
/// on Windows `std` deliberately does **not** set it, precisely because there it
/// would allow hijacking an active listener.
///
/// The window between this probe and the real bind is harmless: losing that race
/// just lands in the server thread's error path, which reaches the same verdict.
fn exit_if_already_running() {
    let probe = std::net::TcpListener::bind(("127.0.0.1", DEFAULT_PORT));
    let Err(e) = probe else {
        return;
    };
    let error = anyhow::Error::new(e).context(format!("probe 127.0.0.1:{DEFAULT_PORT}"));
    let diagnosis = status::diagnose_bind_error(&error);
    if status::tray_startup_failure_action(diagnosis) != status::StartupFailureAction::ExitSilently
    {
        // Something other than "already running" — let the normal startup path
        // reach it, so the user gets the tray shell that explains the failure.
        return;
    }
    log::info!(
        "bridge is already running on 127.0.0.1:{DEFAULT_PORT}; this instance exits \
         without showing anything and without touching autostart: {error:#}"
    );
    log::logger().flush();
    std::process::exit(0);
}

/// The pieces the autostart toggle needs: where the user's choice is recorded,
/// and the platform registration to apply it to.
///
/// `None` when either could not be opened — no config directory, or no resolvable
/// executable path. Advisory throughout: the bridge's job is flashing devices, and
/// a login item it cannot manage is never a reason to refuse to run. The menu
/// item stays visible but unticked in that case, which is the honest report.
type Autostart = (
    autostart::AutostartPreference,
    Box<dyn autostart::AutostartRegistration>,
);

fn open_autostart() -> Option<Autostart> {
    let preference = match autostart::AutostartPreference::open() {
        Ok(preference) => preference,
        Err(e) => {
            log::warn!("bridge autostart preference unavailable: {e}");
            return None;
        }
    };
    match autostart::SystemAutostart::for_current_exe(AUTOSTART_APP_NAME) {
        Ok(registration) => {
            log::info!(
                "bridge autostart target: {}",
                registration.target().display()
            );
            Some((preference, Box::new(registration)))
        }
        Err(e) => {
            log::warn!("bridge autostart unavailable: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A second instance must give up **before** it touches anything shared.
    ///
    /// Found on a real machine, not reasoned about: launching a stray copy of the
    /// app while the installed one was running produced this log order —
    ///
    /// ```text
    /// bridge autostart target: …/stage-a/Cobuilder Bridge.app/…
    /// bridge autostart enabled
    /// bridge is already running on 127.0.0.1:18730; this instance exits …
    /// ```
    ///
    /// — i.e. the doomed instance re-pointed the login item at *itself* on the way
    /// out. The user-visible consequence is a login item aimed at whatever stray
    /// copy was double-clicked last (a leftover in ~/Downloads, say), which breaks
    /// autostart for good the day that copy is deleted. It is the same defect
    /// family as the stale-path bug in `autostart`: the registration has to point
    /// at the app that is actually being used.
    ///
    /// Ordering, like the console attach in `proc`, has no local symptom and no
    /// unit-testable seam — `run_tray` builds a real event loop. So the invariant
    /// is asserted on source order: the give-up check comes first.
    #[test]
    fn a_second_instance_gives_up_before_touching_the_autostart_registration() {
        let src = include_str!("main.rs");
        let body = src
            .split_once("fn run_tray(")
            .expect("run_tray must exist")
            .1;
        let line_of = |needle: &str| {
            body.lines()
                .position(|line| line.contains(needle))
                .unwrap_or_else(|| panic!("expected {needle} inside run_tray"))
        };

        assert!(
            line_of("exit_if_already_running(") < line_of("open_autostart()"),
            "run_tray must bail out on an occupied port before it reconciles \
             autostart, or a second instance rewrites the login item on its way out"
        );
    }

    /// Does `text` contain Chinese?
    ///
    /// The English half of every bilingual pair asserts this is false, which is
    /// what catches the realistic mistake: a string that was translated except
    /// for the one clause somebody forgot, or a Chinese full-width colon left
    /// behind in an otherwise English line. Punctuation ranges are in, because
    /// 「」／：／、 leak just as visibly as an ideograph does.
    fn has_chinese(text: &str) -> bool {
        text.chars().any(|c| {
            matches!(c as u32,
                0x3000..=0x303F   // CJK punctuation: 、。「」
                | 0x4E00..=0x9FFF // CJK unified ideographs
                | 0xFF00..=0xFFEF // fullwidth forms: ：（）
            )
        })
    }

    #[test]
    fn the_chinese_detector_the_english_assertions_rely_on_actually_detects_chinese() {
        // Without this, every `!has_chinese(...)` assertion below would pass for
        // free if the ranges were wrong, and a half-translated string would ship.
        assert!(has_chinese("烧录固件"), "ideographs");
        assert!(has_chinese("Origin：localhost"), "a fullwidth colon");
        assert!(has_chinese("press 「Allow」"), "CJK brackets");
        assert!(!has_chinese(
            "Flash firmware on /dev/tty.usbserial-1 (1.0 MiB)"
        ));
    }

    fn flash_request() -> ConfirmRequest {
        ConfirmRequest {
            op: DangerousOp::Flash,
            origin: "http://localhost:3000".to_string(),
            chip_id: "t5ai".to_string(),
            port: "/dev/tty.usbserial-1".to_string(),
            firmware_bytes: Some(1_048_576),
        }
    }

    fn authorize_request() -> ConfirmRequest {
        ConfirmRequest {
            op: DangerousOp::Authorize,
            firmware_bytes: None,
            ..flash_request()
        }
    }

    #[test]
    fn the_flash_dialog_names_the_source_the_operation_and_the_device() {
        let text = confirm_message(&flash_request(), Lang::Zh);

        assert!(text.contains("http://localhost:3000"), "{text}");
        assert!(text.contains("烧录固件"), "{text}");
        assert!(text.contains("t5ai"), "{text}");
        assert!(text.contains("/dev/tty.usbserial-1"), "{text}");
        // 1 MiB, so the user can tell a real image from an empty one.
        assert!(text.contains("1.0 MiB"), "{text}");
        assert!(text.contains("写入"), "{text}");
    }

    #[test]
    fn the_english_flash_dialog_names_the_source_the_operation_and_the_device() {
        let text = confirm_message(&flash_request(), Lang::En);

        assert!(text.contains("http://localhost:3000"), "{text}");
        assert!(text.contains("Flash firmware"), "{text}");
        assert!(text.contains("t5ai"), "{text}");
        assert!(text.contains("/dev/tty.usbserial-1"), "{text}");
        // The size line is the one part that is assembled rather than picked, so
        // it is the one most likely to keep a Chinese unit suffix behind.
        assert!(text.contains("1.0 MiB"), "{text}");
        assert!(text.contains("1048576 bytes"), "{text}");
        assert!(!has_chinese(&text), "{text}");
    }

    #[test]
    fn the_authorization_dialog_says_the_write_cannot_be_undone() {
        let text = confirm_message(&authorize_request(), Lang::Zh);

        assert!(text.contains("写入授权码"), "{text}");
        assert!(
            text.contains("不可撤销") || text.contains("无法撤销"),
            "{text}"
        );
        // An authorization write carries no image, so no size line is invented.
        assert!(!text.contains("固件大小"), "{text}");
    }

    /// The irreversibility warning is a safety statement, not decoration: an
    /// authorization write overwrites a value that cannot be restored, so it has
    /// to survive into every language the dialog is shown in.
    #[test]
    fn the_english_authorization_dialog_says_the_write_cannot_be_undone() {
        let text = confirm_message(&authorize_request(), Lang::En);

        assert!(text.contains("Write authorization code"), "{text}");
        assert!(text.contains("cannot be undone"), "{text}");
        assert!(!text.contains("Firmware size"), "{text}");
        assert!(!has_chinese(&text), "{text}");
    }

    #[test]
    fn a_malformed_firmware_size_is_reported_rather_than_invented() {
        // `None` means the payload's base64 was malformed; the dialog must say
        // so in either language rather than print a plausible-looking number.
        assert!(firmware_size_text(None, Lang::Zh).contains("未知"));
        let en = firmware_size_text(None, Lang::En);
        assert!(en.contains("unknown"), "{en}");
        assert!(!has_chinese(&en), "{en}");
        // Below 1 KiB the raw byte count is the clearest form, and its unit is
        // the easiest one to leave untranslated.
        assert_eq!(firmware_size_text(Some(512), Lang::En), "512 bytes");
    }

    #[test]
    fn the_dialog_buttons_and_title_follow_the_system_language() {
        let zh = dialog_labels(Lang::Zh);
        assert_eq!(zh.approve, "允许");
        assert_eq!(zh.reject, "拒绝");
        assert!(confirm_title(Lang::Zh).contains("确认"));

        let en = dialog_labels(Lang::En);
        assert_eq!(en.approve, "Allow");
        assert_eq!(en.reject, "Deny");
        assert!(
            !has_chinese(confirm_title(Lang::En)),
            "{}",
            confirm_title(Lang::En)
        );
        // The body has to quote the buttons the user is actually looking at,
        // otherwise it tells an English user to press a button that is not there.
        let body = confirm_message(&authorize_request(), Lang::En);
        assert!(body.contains(en.approve), "{body}");
        assert!(body.contains(en.reject), "{body}");
    }

    /// The approving button label the dialog script actually declares.
    ///
    /// Read back out of the generated AppleScript rather than from
    /// `dialog_labels` a second time — that is what makes the test below a
    /// coupling test: it compares the *script* against the *parser*, so a parser
    /// reading a different language's label cannot pass.
    #[cfg(target_os = "macos")]
    fn approve_button_in(script: &str) -> String {
        let buttons = script
            .split_once("buttons {")
            .expect("the script declares a buttons list")
            .1
            .split_once('}')
            .expect("the buttons list is closed")
            .0;
        // `{"<reject>", "<approve>"}` — the approving one is second, which is
        // also why the refusing one can be the default button.
        buttons
            .rsplit_once('"')
            .and_then(|(head, _)| head.rsplit_once('"'))
            .map(|(_, label)| label.to_string())
            .expect("the approving label is quoted")
    }

    /// The dialog is only a gate if pressing "Allow" is *read* as allowing.
    ///
    /// The macOS path builds an AppleScript with one set of button labels and
    /// then string-matches osascript's reply against them; if those two ever came
    /// from different `Lang` values, an English user's press of the approving
    /// button would parse as a refusal — a silent, total failure of the flash
    /// path that no type would catch.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_button_the_dialog_offers_is_the_button_the_reply_parser_accepts() {
        for lang in [Lang::Zh, Lang::En] {
            let labels = dialog_labels(lang);
            let script = macos_dialog_script(&flash_request(), &labels, lang);
            // What osascript prints when the user presses the approving button
            // this very script declared.
            let reply = format!(
                "button returned:{}, gave up:false\n",
                approve_button_in(&script)
            );
            assert!(
                macos_reply_approves(&reply, &labels),
                "{lang:?}: pressing the offered button must read as approval: {reply}"
            );
        }

        // The failure mode stated as its own assertion: a reply carrying the
        // other language's label is not an approval, so a mismatched snapshot
        // could never quietly approve either.
        let en = dialog_labels(Lang::En);
        let zh_reply = format!(
            "button returned:{}, gave up:false\n",
            dialog_labels(Lang::Zh).approve
        );
        assert!(
            !macos_reply_approves(&zh_reply, &en),
            "a label from another language must never count as approval"
        );

        // The dialog giving up by itself is not a press of anything.
        let en_timeout = format!("button returned:{}, gave up:true\n", en.approve);
        assert!(!macos_reply_approves(&en_timeout, &en), "{en_timeout}");
    }

    /// A hostile string like a port name could arrive from the WS client; it must
    /// stay *data* in both script languages.
    const HOSTILE: &str = "/dev/tty\" & (do shell script \"echo pwned\") & \"x\\";

    #[cfg(target_os = "macos")]
    #[test]
    fn a_hostile_value_stays_inside_the_applescript_string_literal() {
        // Round-tripped through the real interpreter rather than compared to an
        // expected escaping: what matters is what AppleScript makes of it.
        let script = format!("return \"{}\"", applescript_escape(HOSTILE));
        let out = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .expect("run osascript");

        assert!(
            out.status.success(),
            "the script must stay valid: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        // Exact equality *is* the "nothing executed" assertion: had the literal
        // been broken out of, AppleScript would have concatenated the result of
        // `do shell script` and printed `/dev/ttypwnedx\` instead of this text.
        let printed = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            printed.trim_end_matches('\n'),
            HOSTILE,
            "the value must survive as data, not run as script"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn applescript_escaping_covers_quotes_backslashes_and_control_characters() {
        assert_eq!(applescript_escape("plain"), "plain");
        assert_eq!(applescript_escape("say \"hi\""), "say \\\"hi\\\"");
        assert_eq!(applescript_escape("a\\b"), "a\\\\b");
        // A literal cannot span source lines, so a newline becomes its escape.
        assert_eq!(applescript_escape("a\nb"), "a\\nb");
        // Anything else non-printable is flattened rather than passed through.
        assert_eq!(applescript_escape("a\u{7}b\tc"), "a b c");
    }

    #[test]
    fn a_powershell_literal_cannot_be_broken_out_of() {
        // Single quotes are the only thing that can end the literal, and they
        // double. Everything else — double quotes, backslashes, `$(...)`, a
        // subexpression — is inert inside a single-quoted PowerShell string.
        assert_eq!(powershell_string("plain"), "'plain'");
        assert_eq!(powershell_string("it's"), "'it''s'");
        assert_eq!(powershell_string(HOSTILE), format!("'{HOSTILE}'"));
        assert_eq!(
            powershell_string("$(Remove-Item C:\\)"),
            "'$(Remove-Item C:\\)'"
        );
        // Newlines never reach the generated script text as line breaks.
        assert_eq!(powershell_string("a\nb"), "'a' + [char]10 + 'b'");
        assert_eq!(powershell_string("a\u{7}b"), "'a b'");
    }

    #[test]
    fn markup_never_survives_into_a_linux_dialog() {
        // zenity renders Pango markup and kdialog auto-detects Qt rich text, so a
        // client-supplied chip_id / port could otherwise restyle the dialog: hide
        // the real Origin line, fake a bold "已验证" banner, or push the warning
        // off-screen. The dialog must always describe the operation it is really
        // authorizing, so the text handed to either renderer carries no markup.
        // A *bare* ampersand on purpose: it is both invalid Pango (zenity would
        // fail or render garbage) and the one character an escaper is most likely
        // to forget. A fixture carrying a pre-escaped `&amp;` would let an
        // implementation that never touches `&` pass this test.
        const HOSTILE: &str = "<b>Cobuilder 官方</b><br/>A & B <span size='1'>";
        let request = ConfirmRequest {
            op: DangerousOp::Flash,
            origin: "http://localhost:3000".to_string(),
            chip_id: HOSTILE.to_string(),
            port: "/dev/tty.<i>fake</i>".to_string(),
            firmware_bytes: Some(5),
        };

        // Both languages: the escaping happens once, but a translation is exactly
        // the kind of edit that reintroduces raw markup into one arm only.
        for lang in [Lang::Zh, Lang::En] {
            for tool in [LinuxDialogTool::Zenity, LinuxDialogTool::KDialog] {
                let text = linux_dialog_text(&request, tool, lang);
                assert!(
                    !text.contains('<') && !text.contains('>'),
                    "{lang:?}/{tool:?}: angle brackets must be escaped: {text}"
                );
                assert!(
                    text.contains("&lt;b&gt;"),
                    "{lang:?}/{tool:?}: the hostile tag must appear escaped, not dropped: {text}"
                );
                assert!(
                    text.contains("A &amp; B"),
                    "{lang:?}/{tool:?}: the bare ampersand must be escaped: {text}"
                );
                // Exactly once: escaping the `&` of an escape already emitted would
                // show the user `&amp;lt;b&amp;gt;` instead of `<b>`.
                assert!(
                    !text.contains("&amp;amp;"),
                    "{lang:?}/{tool:?}: double-escaped ampersand: {text}"
                );
                // Escaping must not eat the information the user decides on.
                assert!(
                    text.contains("http://localhost:3000") && text.contains("fake"),
                    "{lang:?}/{tool:?}: the dialog lost its content: {text}"
                );
            }
        }
    }

    #[test]
    fn the_kdialog_fallback_warns_that_its_default_button_is_the_permissive_one() {
        let request = ConfirmRequest {
            op: DangerousOp::Authorize,
            origin: "http://localhost:3000".to_string(),
            chip_id: "t5ai".to_string(),
            port: "/dev/tty.fakeA".to_string(),
            firmware_bytes: None,
        };

        // kdialog cannot be told which button is focused, so Return may authorize.
        // zenity (tried first) can, and must not carry the warning — otherwise the
        // warning becomes noise users learn to skip.
        let kdialog = linux_dialog_text(&request, LinuxDialogTool::KDialog, Lang::Zh);
        let zenity = linux_dialog_text(&request, LinuxDialogTool::Zenity, Lang::Zh);
        assert!(
            kdialog.contains("回车"),
            "the kdialog fallback must warn about its default button: {kdialog}"
        );
        assert!(
            !zenity.contains("回车"),
            "zenity defaults to refusing and needs no such warning: {zenity}"
        );
    }

    /// Same warning, same asymmetry, in English: it is the only thing standing
    /// between a stray Return and an authorized write on the kdialog fallback.
    #[test]
    fn the_english_kdialog_fallback_warns_that_its_default_button_is_the_permissive_one() {
        let request = authorize_request();

        let kdialog = linux_dialog_text(&request, LinuxDialogTool::KDialog, Lang::En);
        let zenity = linux_dialog_text(&request, LinuxDialogTool::Zenity, Lang::En);
        assert!(
            kdialog.contains("pressing Return"),
            "the kdialog fallback must warn about its default button: {kdialog}"
        );
        assert!(!has_chinese(&kdialog), "{kdialog}");
        assert!(
            !zenity.contains("pressing Return"),
            "zenity defaults to refusing and needs no such warning: {zenity}"
        );
    }

    #[test]
    fn the_tray_menu_items_follow_the_system_language() {
        let zh = menu_labels(Lang::Zh);
        assert_eq!(zh.open_cobuilder, "打开 Cobuilder");
        assert_eq!(zh.latest_version, "获取最新版本");
        assert_eq!(zh.autostart, "开机自启");
        assert_eq!(zh.revoke_grants, "撤销所有授权");
        assert_eq!(zh.quit, "退出");

        let en = menu_labels(Lang::En);
        assert_eq!(en.open_cobuilder, "Open Cobuilder");
        assert_eq!(en.latest_version, "Get the latest version");
        assert_eq!(en.autostart, "Start at login");
        // The security control in the menu: it has to name what it withdraws,
        // not just say "reset".
        assert_eq!(en.revoke_grants, "Revoke all authorizations");
        assert_eq!(en.quit, "Quit");

        // Same guard the notification strings carry: an English build that kept
        // one Chinese label is the realistic translation mistake, and the tray
        // menu is where it is most visible.
        for label in [
            en.open_cobuilder,
            en.latest_version,
            en.autostart,
            en.revoke_grants,
            en.quit,
        ] {
            assert!(!has_chinese(label), "{label}");
        }
    }

    #[test]
    fn the_tray_notifications_follow_the_system_language() {
        assert!(startup_failed_notification_title(Lang::Zh).contains("启动失败"));
        let title = startup_failed_notification_title(Lang::En);
        assert!(title.contains("Cobuilder Bridge"), "{title}");
        assert!(title.contains("failed to start"), "{title}");
        assert!(!has_chinese(title), "{title}");

        assert!(revoked_notification_body(Lang::Zh).contains("已撤销"));
        let body = revoked_notification_body(Lang::En);
        // Both halves: what happened, and what the user will see next time — a
        // security control that looks like it did nothing invites a second click.
        assert!(body.contains("revoked"), "{body}");
        assert!(body.contains("ask you again"), "{body}");
        assert!(!has_chinese(body), "{body}");
    }

    #[test]
    fn headless_mode_refuses_dangerous_operations_unless_explicitly_opted_in() {
        // The tray shell has a user in front of it, so it asks.
        assert_eq!(
            prompt_choice(false, false),
            PromptChoice::SystemDialog,
            "tray mode must ask the user"
        );
        // --headless means "no GUI" (CI, a server, an ssh session). Popping a
        // dialog nobody can see just burns the 60s window and then refuses, so
        // refuse up front and say why.
        assert_eq!(
            prompt_choice(true, false),
            PromptChoice::DenyAll,
            "headless must not pop a GUI dialog"
        );
        // The escape hatch is explicit and only means anything unattended.
        assert_eq!(
            prompt_choice(true, true),
            PromptChoice::UnattendedAutoApprove,
            "the opt-in flag is what makes unattended writes possible"
        );
        assert_eq!(
            prompt_choice(false, true),
            PromptChoice::SystemDialog,
            "with a GUI session present there is a user to ask, so still ask"
        );
    }

    #[test]
    fn the_help_text_documents_both_flags_and_the_risk() {
        let help = help_text(Lang::Zh);
        assert!(help.contains("--headless"), "{help}");
        assert!(help.contains(UNATTENDED_FLAG), "{help}");
        // The escape hatch removes the only thing standing between a local process
        // and the user's board, so --help has to say so. Matched on wording unique
        // to that flag's paragraph rather than a bare 「确认」, which also appears
        // in the --headless text above and would keep this green after a deletion.
        assert!(
            help.contains("关闭人工确认"),
            "the help text must explain what the flag disables: {help}"
        );
        // The one sentence an operator reads to learn that a grant left behind by
        // the tray shell does not quietly re-enable headless writes. It is the
        // documented half of a security decision, so it gets its own assertion
        // instead of relying on somebody remembering not to delete it.
        assert!(
            help.contains("这里也一律不认"),
            "--help must say the headless refusal ignores stored grants: {help}"
        );
    }

    /// The English `--help` has to carry the same two security statements, not
    /// just the flag names: an operator reading it in English must still learn
    /// what the opt-in removes and that a stored grant does not re-enable
    /// headless writes.
    #[test]
    fn the_english_help_text_documents_both_flags_and_the_risk() {
        let help = help_text(Lang::En);
        assert!(help.contains("--headless"), "{help}");
        assert!(help.contains(UNATTENDED_FLAG), "{help}");
        assert!(
            help.contains("turns the human confirmation off"),
            "the help text must explain what the flag disables: {help}"
        );
        assert!(
            help.contains("stored grants are never honoured here"),
            "--help must say the headless refusal ignores stored grants: {help}"
        );
        assert!(!has_chinese(help), "{help}");
    }

    #[test]
    fn an_unknown_flag_is_reported_in_the_system_language() {
        // The typo the doc comment on the argument loop calls out: it must be
        // echoed back, in either language, so the user can see what they typed.
        const TYPO: &str = "--allow-unattended-write";

        let zh = unknown_argument_line(TYPO, Lang::Zh);
        assert!(zh.contains("无法识别的参数"), "{zh}");
        assert!(zh.contains(TYPO), "{zh}");

        let en = unknown_argument_line(TYPO, Lang::En);
        assert!(en.contains("unrecognized argument"), "{en}");
        assert!(en.contains(TYPO), "{en}");
        assert!(!has_chinese(&en), "{en}");
    }

    #[test]
    fn the_headless_console_banner_names_the_opt_in_flag_in_the_system_language() {
        for lang in [Lang::Zh, Lang::En] {
            // Both branches, because the flag name is the actionable half of
            // each: "it is on" and "this is how you would turn it on".
            let refusing = headless_startup_line(PromptChoice::DenyAll, lang);
            let approving = headless_startup_line(PromptChoice::UnattendedAutoApprove, lang);
            assert!(refusing.contains(UNATTENDED_FLAG), "{lang:?}: {refusing}");
            assert!(approving.contains(UNATTENDED_FLAG), "{lang:?}: {approving}");
            assert_ne!(
                refusing, approving,
                "{lang:?}: the two policies must not read the same"
            );
        }

        let refusing = headless_startup_line(PromptChoice::DenyAll, Lang::En);
        let approving = headless_startup_line(PromptChoice::UnattendedAutoApprove, Lang::En);
        assert!(refusing.contains("refuses"), "{refusing}");
        assert!(approving.contains("no user confirmation"), "{approving}");
        assert!(!has_chinese(&refusing), "{refusing}");
        assert!(!has_chinese(&approving), "{approving}");
    }

    #[test]
    fn only_an_attended_session_may_lean_on_a_persisted_grant() {
        // Composed through `prompt_choice` on purpose: the grant policy and the
        // prompt choice must never be able to drift apart, so this pins the pair.
        assert_eq!(
            grant_policy_for(prompt_choice(false, false)),
            GrantPolicy::Honour,
            "tray mode must keep honouring the token it issued"
        );
        // The whole point of requiring --allow-unattended-writes is that running
        // unattended has to be declared. A grant left behind by an earlier
        // attended session must not quietly satisfy that declaration.
        assert_eq!(
            grant_policy_for(prompt_choice(true, false)),
            GrantPolicy::Ignore,
            "headless without the opt-in must not accept a stored grant"
        );
        assert_eq!(
            grant_policy_for(prompt_choice(true, true)),
            GrantPolicy::Honour,
            "with the opt-in declared, grants are moot but harmless"
        );
        assert_eq!(
            grant_policy_for(prompt_choice(false, true)),
            GrantPolicy::Honour
        );
    }
}
