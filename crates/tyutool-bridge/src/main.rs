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

use std::sync::Arc;

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tyutool_bridge::status::{self, StatsSnapshot};
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
                print!("{}", help_text());
                return;
            }
            other => {
                eprintln!("tyutool-bridge: 无法识别的参数 {other:?}；用 --help 看可用选项。");
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
        run_headless(choice);
    } else {
        run_tray(choice);
    }
}

/// `--help` / `-h`. Names both flags and says outright what the opt-in removes:
/// a user who reads only this text must still learn that
/// [`UNATTENDED_FLAG`] deletes the confirmation step, not just automates it.
///
/// The flag spellings are literals here (`concat!` cannot take a `const`); the
/// `the_help_text_documents_both_flags_and_the_risk` test asserts this text
/// contains [`UNATTENDED_FLAG`], so the two cannot drift apart unnoticed.
fn help_text() -> &'static str {
    concat!(
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
    )
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

/// Title of the confirmation dialog and of the notifications.
const CONFIRM_TITLE: &str = "Cobuilder Bridge 需要你的确认";
/// The one label that authorizes a write, and the one that does not.
const APPROVE_LABEL: &str = "允许";
const REJECT_LABEL: &str = "拒绝";
/// Seconds the dialog waits before giving up by itself, matching the bridge's own
/// confirmation timeout. Both paths end in the same `user_rejected`, so the race
/// between them is harmless.
const DIALOG_TIMEOUT_SECS: u32 = 60;

/// The real human-in-the-loop gate: a modal the user has to answer before
/// anything is written to a device.
///
/// Without it injected, the library refuses every dangerous operation
/// (`DenyPrompt`), so the shipped helper could not flash at all.
struct SystemPrompt;

impl AuthPrompt for SystemPrompt {
    /// Returns immediately, as the trait requires: the dialog blocks a throwaway
    /// thread, never the async worker that is holding the execution right (and
    /// certainly not the tray's UI thread).
    fn request(&self, request: ConfirmRequest, respond: ConfirmResponder) {
        let what = format!(
            "{} on {} from {}",
            op_label(request.op),
            request.port,
            request.origin
        );
        let spawned = std::thread::Builder::new()
            .name("bridge-confirm".to_string())
            .spawn(move || respond(ask_user(&request)));
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
fn build_prompt(choice: PromptChoice) -> Arc<dyn AuthPrompt> {
    match choice {
        PromptChoice::SystemDialog => Arc::new(SystemPrompt),
        PromptChoice::DenyAll => Arc::new(DenyPrompt),
        PromptChoice::UnattendedAutoApprove => Arc::new(UnattendedPrompt),
    }
}

/// What the user is being asked to authorize, in the wording the UI uses.
fn op_label(op: DangerousOp) -> &'static str {
    match op {
        DangerousOp::Flash => "烧录固件",
        DangerousOp::Authorize => "写入授权码",
    }
}

/// The dialog body: who is asking, what for, and on which device.
///
/// Carries no credential — [`ConfirmRequest`] has no `uuid` / `auth_key` field by
/// construction, and it must stay that way.
fn confirm_message(request: &ConfirmRequest) -> String {
    let mut text = format!(
        "来源：{}\n操作：{}\n芯片：{}\n串口：{}\n",
        or_dash(&request.origin),
        op_label(request.op),
        or_dash(&request.chip_id),
        or_dash(&request.port),
    );
    if request.op == DangerousOp::Flash {
        text.push_str(&format!(
            "固件大小：{}\n",
            firmware_size_text(request.firmware_bytes)
        ));
    }
    text.push_str("\n点「允许」即向该设备写入数据。");
    if request.op == DangerousOp::Authorize {
        text.push_str("授权码写入会覆盖原有的值，且无法撤销。");
    }
    text.push_str("\n如果这不是你本人刚刚在页面上发起的操作，请点「拒绝」。");
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
fn firmware_size_text(bytes: Option<u64>) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    match bytes {
        None => "未知（固件数据异常）".to_string(),
        Some(n) if n < KIB => format!("{n} 字节"),
        Some(n) if n < MIB => format!("{:.1} KiB（{n} 字节）", n as f64 / KIB as f64),
        Some(n) => format!("{:.1} MiB（{n} 字节）", n as f64 / MIB as f64),
    }
}

/// Ask the user, blocking until they answer (or the dialog gives up).
///
/// Everything that is not an explicit press of [`APPROVE_LABEL`] — a refusal, a
/// cancel, the dialog giving up, a missing dialog tool, unparsable output — maps
/// to [`ConfirmDecision::Reject`]: silence never opens the door.
#[cfg(target_os = "macos")]
fn ask_user(request: &ConfirmRequest) -> ConfirmDecision {
    // Interim path until the helper ships as a signed `.app` bundle: an unbundled
    // binary has no bundle identity, so a native `NSAlert` / `UNUserNotification`
    // is not available to it, while `osascript` (which is itself a bundled app)
    // works today. The packaging slice replaces this with a real NSAlert.
    //
    // `default button` is the *refusing* one on purpose: a stray Return keypress
    // must never authorize a flash.
    let script = format!(
        "display dialog \"{message}\" with title \"{title}\" \
         buttons {{\"{REJECT_LABEL}\", \"{APPROVE_LABEL}\"}} \
         default button \"{REJECT_LABEL}\" with icon caution \
         giving up after {DIALOG_TIMEOUT_SECS}",
        message = applescript_escape(&confirm_message(request)),
        title = applescript_escape(CONFIRM_TITLE),
    );

    let output = match std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
    {
        Ok(output) => output,
        Err(e) => {
            log::error!(
                "bridge could not run osascript for the confirmation dialog: {e}; refusing"
            );
            return ConfirmDecision::Reject;
        }
    };

    // `display dialog` prints one record line, e.g.
    // `button returned:允许, gave up:false`. A refusal, an Escape (osascript exits
    // non-zero) and the giving-up path all fail this check.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let pressed_approve = stdout.lines().next().is_some_and(|line| {
        line.split(", ")
            .any(|field| field.trim() == format!("button returned:{APPROVE_LABEL}"))
    });
    let approved = output.status.success() && pressed_approve && !stdout.contains("gave up:true");
    decision(approved, &stdout)
}

#[cfg(target_os = "windows")]
fn ask_user(request: &ConfirmRequest) -> ConfirmDecision {
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
        message = powershell_string(&confirm_message(request)),
        title = powershell_string(CONFIRM_TITLE),
    );

    let output = match std::process::Command::new("powershell")
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
fn linux_dialog_text(request: &ConfirmRequest, tool: LinuxDialogTool) -> String {
    let mut text = escape_markup(&confirm_message(request));
    if tool == LinuxDialogTool::KDialog {
        // Only on this branch: zenity gets `--default-cancel`, and a warning that
        // is shown always is a warning users learn to skip.
        text.push_str(
            "\n\n注意：这个对话框无法把「否」设为默认按钮，直接按回车有可能就等于同意。\
             不确定的话请用鼠标点「否」。",
        );
    }
    text
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
fn ask_user(request: &ConfirmRequest) -> ConfirmDecision {
    // Compile-only on the machine this was written on, like the Windows arm.
    // Arguments go to the program as argv (no shell in the loop), so there is no
    // quoting to do — but both programs render markup, hence the escaping in
    // `linux_dialog_text`.
    //
    // Deliberately *not* passing zenity's `--no-markup`: it does not exist on
    // every version still in the field, and an unknown flag makes zenity exit
    // non-zero, which would turn every dangerous operation into a hard refusal.
    // The escaping is the guarantee; the flag would only have been belt-and-braces.
    let zenity_text = linux_dialog_text(request, LinuxDialogTool::Zenity);

    // zenity first: it is the only one of the two that can make the refusing
    // button the default, so a stray Return cannot authorize a write.
    let zenity = std::process::Command::new("zenity")
        .arg("--question")
        .args(["--title", CONFIRM_TITLE])
        .arg("--text")
        .arg(&zenity_text)
        .args(["--ok-label", APPROVE_LABEL])
        .args(["--cancel-label", REJECT_LABEL])
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
    let kdialog = std::process::Command::new("kdialog")
        .args(["--title", CONFIRM_TITLE])
        .arg("--yesno")
        .arg(linux_dialog_text(request, LinuxDialogTool::KDialog))
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
    match std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => log::warn!("bridge notification not shown: osascript exit {status}"),
        Err(e) => log::warn!("bridge notification not shown: {e}"),
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn show_notification(title: &str, body: &str) {
    // argv, no shell: nothing to escape.
    match std::process::Command::new("notify-send")
        .arg(title)
        .arg(body)
        .status()
    {
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

/// Serve until killed. Exits non-zero when the port is taken: a supervisor or
/// smoke script needs that signal, whereas the tray shell deliberately stays
/// resident and shows the error in its status line instead.
fn run_headless(choice: PromptChoice) {
    // Stated once at startup, on both channels: the operator has to be able to
    // tell "it refused" from "it was never going to ask" without reading code.
    match choice {
        PromptChoice::UnattendedAutoApprove => {
            log::warn!(
                "bridge headless mode started with {UNATTENDED_FLAG}: every dangerous operation \
                 will be approved automatically, with no human confirmation"
            );
            eprintln!(
                "tyutool-bridge: {UNATTENDED_FLAG} 已开启——烧录/写授权码将自动放行，不再询问用户。"
            );
        }
        _ => {
            log::info!(
                "bridge headless mode refuses dangerous operations (no user to confirm with); \
                 pass {UNATTENDED_FLAG} to allow unattended writes"
            );
            eprintln!(
                "tyutool-bridge: headless 模式默认拒绝烧录/写授权码（无人可确认）；\
                 需要无人值守烧录请加 {UNATTENDED_FLAG}。"
            );
        }
    }

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
            .with_auth_prompt(build_prompt(choice))
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

fn run_tray(choice: PromptChoice) {
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

    register_autostart();

    // Detached on purpose: the tray owns the process lifetime, and quitting
    // tears the runtime down with it.
    let server_proxy = proxy.clone();
    let spawned = std::thread::Builder::new()
        .name("bridge-server".to_string())
        .spawn(move || serve_in_background(server_proxy, choice));
    if let Err(e) = spawned {
        log::error!("bridge server thread could not be started: {e}");
        let _ = proxy.send_event(UserEvent::StartupFailed(format!("启动失败：{e}")));
    }

    let mut tray: Option<TrayShell> = None;
    let mut status_text = status::status_line(VERSION, &StatsSnapshot::default());
    // `None` until the server thread reports in; clicking "撤销所有授权" before
    // then is a no-op, not a panic.
    let mut authority: Option<Authority> = None;

    event_loop.run(move |event, _target, control_flow| {
        // Purely event-driven: nothing to poll between stats pushes and clicks.
        *control_flow = ControlFlow::Wait;
        match event {
            // tao guarantees this is the first event, and on macOS the status
            // item may only be created once the app is initialized.
            Event::NewEvents(StartCause::Init) => match TrayShell::build(&status_text) {
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
            },
            Event::UserEvent(UserEvent::Stats(snapshot)) => {
                status_text = status::status_line(VERSION, &snapshot);
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
                notify("Cobuilder Bridge 启动失败", &status_text);
            }
            Event::UserEvent(UserEvent::AuthorityReady(handle)) => {
                authority = Some(handle);
            }
            Event::UserEvent(UserEvent::Menu(id)) => {
                if let Some(shell) = &tray {
                    match shell.action_for(&id) {
                        Some(MenuAction::OpenCobuilder) => open_url(COBUILDER_URL),
                        Some(MenuAction::LatestVersion) => open_url(LATEST_VERSION_URL),
                        Some(MenuAction::RevokeGrants) => revoke_all(authority.as_ref()),
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
fn serve_in_background(proxy: EventLoopProxy<UserEvent>, choice: PromptChoice) {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            log::error!("bridge async runtime could not be created: {e}");
            let _ = proxy.send_event(UserEvent::StartupFailed(format!("启动失败：{e}")));
            return;
        }
    };

    runtime.block_on(async move {
        let server = match bind(DEFAULT_PORT).await {
            Ok(server) => server,
            Err(e) => {
                // Resident on failure (unlike --headless): the whole point of
                // the tray is that the user finds out *why* nothing works.
                let diagnosis = status::diagnose_bind_error(&e);
                let line = status::startup_error_line(diagnosis, &e);
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
            .with_auth_prompt(build_prompt(choice))
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
            let _ = proxy.send_event(UserEvent::StartupFailed(format!("服务已停止：{e}")));
        }
    });
}

/// Withdraw every authorization the user ever granted.
///
/// Runs on the UI thread: `revoke_all` neither awaits nor blocks on the network
/// (it clears a small local file and queues one frame per live connection), so
/// the menu does not need a worker thread for it.
fn revoke_all(authority: Option<&Authority>) {
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
    notify("Cobuilder Bridge", "已撤销所有授权，下次烧录会重新询问你。");
}

/// What a tray menu item does. Kept separate from the muda ids so the event
/// handling above reads as behaviour rather than id comparisons.
enum MenuAction {
    OpenCobuilder,
    LatestVersion,
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
    open_cobuilder: muda::MenuId,
    latest_version: muda::MenuId,
    revoke_grants: muda::MenuId,
    quit: muda::MenuId,
}

impl TrayShell {
    fn build(status_text: &str) -> anyhow::Result<Self> {
        // Disabled: a status readout, not a command.
        let status_item = muda::MenuItem::new(status_text, false, None);
        let open_cobuilder = muda::MenuItem::new("打开 Cobuilder", true, None);
        let latest_version = muda::MenuItem::new("获取最新版本", true, None);
        let revoke_grants = muda::MenuItem::new("撤销所有授权", true, None);
        let quit = muda::MenuItem::new("退出", true, None);

        let menu = muda::Menu::new();
        menu.append_items(&[
            &status_item,
            &muda::PredefinedMenuItem::separator(),
            &open_cobuilder,
            &latest_version,
            &revoke_grants,
            &muda::PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|e| anyhow::anyhow!("build tray menu: {e}"))?;

        let icon = tray_icon::TrayIconBuilder::new()
            .with_menu(Box::new(menu.clone()))
            .with_icon(placeholder_icon()?)
            // macOS recolors a template image for the current menu bar
            // appearance, which is what keeps a black glyph visible in dark mode.
            .with_icon_as_template(true)
            .with_tooltip("Cobuilder Bridge")
            .build()
            .map_err(|e| anyhow::anyhow!("create tray icon: {e}"))?;

        Ok(Self {
            _icon: icon,
            open_cobuilder: open_cobuilder.id().clone(),
            latest_version: latest_version.id().clone(),
            revoke_grants: revoke_grants.id().clone(),
            quit: quit.id().clone(),
            status_item,
            _menu: menu,
        })
    }

    fn set_status(&self, text: &str) {
        self.status_item.set_text(text);
    }

    fn action_for(&self, id: &muda::MenuId) -> Option<MenuAction> {
        if *id == self.open_cobuilder {
            Some(MenuAction::OpenCobuilder)
        } else if *id == self.latest_version {
            Some(MenuAction::LatestVersion)
        } else if *id == self.revoke_grants {
            Some(MenuAction::RevokeGrants)
        } else if *id == self.quit {
            Some(MenuAction::Quit)
        } else {
            None
        }
    }
}

/// Generated ring glyph (opaque black on transparent), drawn in code so the
/// binary needs no asset pipeline yet. Black + alpha is exactly what a macOS
/// template image wants; other platforms show it as-is.
///
/// TODO: replace with the real Cobuilder Bridge artwork (proper per-platform
/// icon set, `.ico` on Windows) when the design lands.
fn placeholder_icon() -> anyhow::Result<tray_icon::Icon> {
    const SIZE: u32 = 32;
    const OUTER: f32 = 14.0;
    const INNER: f32 = 8.0;
    let center = (SIZE as f32 - 1.0) / 2.0;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let distance_sq = dx * dx + dy * dy;
            let on_ring = (INNER * INNER..=OUTER * OUTER).contains(&distance_sq);
            rgba.extend_from_slice(if on_ring {
                &[0x00, 0x00, 0x00, 0xFF]
            } else {
                &[0x00, 0x00, 0x00, 0x00]
            });
        }
    }
    tray_icon::Icon::from_rgba(rgba, SIZE, SIZE)
        .map_err(|e| anyhow::anyhow!("build tray icon bitmap: {e}"))
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
                let mut c = std::process::Command::new("open");
                c.arg(url);
                c
            };
            #[cfg(target_os = "windows")]
            let mut command = {
                let mut c = std::process::Command::new("cmd");
                // Empty title argument: `start` treats a lone quoted argument
                // as the window title otherwise.
                c.args(["/C", "start", "", url]);
                c
            };
            #[cfg(all(unix, not(target_os = "macos")))]
            let mut command = {
                let mut c = std::process::Command::new("xdg-open");
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

/// Register the bridge to start with the user's session, once. Advisory only:
/// every failure is a warning, never a reason not to run.
///
/// TODO: once the bridge ships as a macOS .app bundle, switch to
/// `SMAppService` (`MacOSLaunchMode::SMAppService`, or the objc2 API directly)
/// — that is the packaging slice's job, together with cleaning up the
/// LaunchAgent plist this leaves behind when a user disables autostart.
fn register_autostart() {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            log::warn!("bridge autostart skipped, own path unknown: {e}");
            return;
        }
    };

    let builder = auto_launch::AutoLaunchBuilder::new()
        .set_app_name(AUTOSTART_APP_NAME)
        .set_app_path(&exe.to_string_lossy())
        // A LaunchAgent plist works for a bare binary; both the AppleScript
        // login item and `SMAppService` modes want a real .app bundle.
        .set_macos_launch_mode(auto_launch::MacOSLaunchMode::LaunchAgent)
        .build();

    let launcher = match builder {
        Ok(launcher) => launcher,
        Err(e) => {
            log::warn!("bridge autostart not configured: {e}");
            return;
        }
    };

    match launcher.is_enabled() {
        Ok(true) => log::info!("bridge autostart already registered"),
        Ok(false) => match launcher.enable() {
            Ok(()) => log::info!("bridge autostart registered for {}", exe.display()),
            Err(e) => log::warn!("bridge autostart registration failed: {e}"),
        },
        Err(e) => log::warn!("bridge autostart state unknown: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flash_request() -> ConfirmRequest {
        ConfirmRequest {
            op: DangerousOp::Flash,
            origin: "http://localhost:3000".to_string(),
            chip_id: "t5ai".to_string(),
            port: "/dev/tty.usbserial-1".to_string(),
            firmware_bytes: Some(1_048_576),
        }
    }

    #[test]
    fn the_flash_dialog_names_the_source_the_operation_and_the_device() {
        let text = confirm_message(&flash_request());

        assert!(text.contains("http://localhost:3000"), "{text}");
        assert!(text.contains("烧录固件"), "{text}");
        assert!(text.contains("t5ai"), "{text}");
        assert!(text.contains("/dev/tty.usbserial-1"), "{text}");
        // 1 MiB, so the user can tell a real image from an empty one.
        assert!(text.contains("1.0 MiB"), "{text}");
        assert!(text.contains("写入"), "{text}");
    }

    #[test]
    fn the_authorization_dialog_says_the_write_cannot_be_undone() {
        let text = confirm_message(&ConfirmRequest {
            op: DangerousOp::Authorize,
            firmware_bytes: None,
            ..flash_request()
        });

        assert!(text.contains("写入授权码"), "{text}");
        assert!(
            text.contains("不可撤销") || text.contains("无法撤销"),
            "{text}"
        );
        // An authorization write carries no image, so no size line is invented.
        assert!(!text.contains("固件大小"), "{text}");
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

        for tool in [LinuxDialogTool::Zenity, LinuxDialogTool::KDialog] {
            let text = linux_dialog_text(&request, tool);
            assert!(
                !text.contains('<') && !text.contains('>'),
                "{tool:?}: angle brackets must be escaped: {text}"
            );
            assert!(
                text.contains("&lt;b&gt;"),
                "{tool:?}: the hostile tag must appear escaped, not dropped: {text}"
            );
            assert!(
                text.contains("A &amp; B"),
                "{tool:?}: the bare ampersand must be escaped: {text}"
            );
            // Exactly once: escaping the `&` of an escape already emitted would
            // show the user `&amp;lt;b&amp;gt;` instead of `<b>`.
            assert!(
                !text.contains("&amp;amp;"),
                "{tool:?}: double-escaped ampersand: {text}"
            );
            // Escaping must not eat the information the user decides on.
            assert!(
                text.contains("http://localhost:3000") && text.contains("fake"),
                "{tool:?}: the dialog lost its content: {text}"
            );
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
        let kdialog = linux_dialog_text(&request, LinuxDialogTool::KDialog);
        let zenity = linux_dialog_text(&request, LinuxDialogTool::Zenity);
        assert!(
            kdialog.contains("回车"),
            "the kdialog fallback must warn about its default button: {kdialog}"
        );
        assert!(
            !zenity.contains("回车"),
            "zenity defaults to refusing and needs no such warning: {zenity}"
        );
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
        let help = help_text();
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
