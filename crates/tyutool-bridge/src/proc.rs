//! Subprocess construction for a GUI-subsystem binary.
//!
//! Once the tray binary is linked with `windows_subsystem = "windows"` it owns no
//! console — but **its children still get one**: `CreateProcess` allocates a fresh
//! console for a console-subsystem child whose parent has none, and that console
//! is a real window that appears on screen. So the black window the packaging
//! slice is meant to remove does not go away by changing the subsystem; it moves
//! from "one window for the bridge" to "one window per child process".
//!
//! The worst of those children is not the rare one. `detect_os_version` runs
//! `cmd /c ver` (`lib.rs`) from `Hello::current()`, which is built for **every**
//! accepted WebSocket connection — so without this module a user would get one
//! black flash every time the web page connects, which is far more visible than
//! the single console window we started with.
//!
//! [`hidden_command`] is therefore the **only** sanctioned way to build a
//! `std::process::Command` in this crate, and
//! [`tests::every_production_subprocess_goes_through_hidden_command`] enforces
//! that lexically — a `creation_flags` call cannot be asserted from macOS, so the
//! invariant is locked on the source text instead of on behaviour. That is the
//! same trade the frontend makes for its scss red lines: when the mechanism is
//! invisible to the test runner, scan for the mechanism.

/// `CREATE_NO_WINDOW` from `processthreadsapi.h`.
///
/// Spelled out rather than pulled from `windows-sys`: it is one documented
/// constant that has never changed, and `std::os::windows::process::CommandExt`
/// (std, no dependency) is what consumes it.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A `Command` that will not flash a console window on Windows.
///
/// A plain `Command::new` everywhere else: the flag has no counterpart on
/// macOS/Linux, so this is a pass-through there and callers need no `cfg`.
pub fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    // Only the Windows arm below mutates it; kept as one construction site rather
    // than a per-platform `return` so there is exactly one place a `Command` is
    // born in this crate — which is what the lexical guard in `tests` relies on.
    #[allow(unused_mut)]
    let mut command = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Reconnect stdout/stderr to the console of whoever launched us, if there is one.
///
/// A GUI-subsystem binary is given no console, so its standard handles are NULL
/// when it is started from `cmd.exe` without redirection. That does **not**
/// crash: `std` maps the resulting `ERROR_INVALID_HANDLE` to `Ok(len)` via
/// `handle_ebadf`, so `print!` and `eprintln!` succeed while writing nowhere.
/// Silent success is the whole problem — `--help` would print nothing and
/// `--headless` would start with no visible startup line, both of which have
/// human audiences.
///
/// Failure is expected and ignored: there is simply no parent console when the
/// user double-clicks the app or a LaunchAgent-equivalent starts it, and the
/// silent-discard behaviour above is exactly the right outcome then. Redirected
/// output (`> out.txt`, a CI shell's pipe) never reaches the NULL case at all,
/// because inherited handles are independent of the subsystem.
///
/// Must run before any output. Called as the first statement of `main`, and
/// [`tests::the_console_attach_precedes_every_output_site_in_main`] keeps it there.
pub fn attach_parent_console() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        // Returns 0 on failure; nothing to do about it and nowhere to report it.
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(test)]
mod tests {
    /// Production source of both compilation units, test modules stripped.
    ///
    /// The `#[cfg(test)]` blocks are deliberately excluded: `main.rs` round-trips
    /// a hostile string through the *real* `osascript` to prove the AppleScript
    /// escaping holds, and a test process on macOS has a console either way.
    /// Only what a user's Windows machine executes is in scope here.
    fn production_sources() -> [(&'static str, &'static str); 2] {
        [
            ("main.rs", include_str!("main.rs")),
            ("lib.rs", include_str!("lib.rs")),
        ]
    }

    fn strip_tests(src: &str) -> &str {
        // Both files end with a single `#[cfg(test)] mod tests { ... }`.
        src.split("\nmod tests {").next().unwrap_or(src)
    }

    /// Is this line a way to construct a `Command` without going through
    /// [`super::hidden_command`]?
    ///
    /// Two patterns, because catching only the first leaves an easy way around it:
    ///
    ///  * **any** `Command::new` — not just the fully-qualified
    ///    `std::process::Command::new`. A contributor who adds
    ///    `use std::process::Command;` and then writes `Command::new(…)` would
    ///    reintroduce the console flash while a check for the qualified spelling
    ///    stayed green;
    ///  * importing `Command` at all, which is what shuts the remaining door:
    ///    `use std::process::Command as Cmd;` makes the call site read `Cmd::new`
    ///    and no call-site pattern can see it. `CommandExt` is explicitly not an
    ///    offender — that trait is how `hidden_command` sets the flag in the first
    ///    place.
    fn builds_a_command_outside_the_helper(line: &str) -> bool {
        let imports_command = line.contains("use ")
            && line.contains("process::Command")
            && !line.contains("CommandExt");
        line.contains("Command::new") || imports_command
    }

    /// Every child process a user can trigger must be built by [`super::hidden_command`].
    ///
    /// This is not a style rule. A bare `Command::new` in a GUI-subsystem binary
    /// is a visible black window on the user's screen, and the offending call
    /// site is usually nowhere near the code that "opened a dialog" — the one
    /// that matters most (`cmd /c ver`) hides inside the WS hello frame.
    #[test]
    fn every_production_subprocess_goes_through_hidden_command() {
        let offenders: Vec<String> = production_sources()
            .iter()
            .flat_map(|(name, src)| {
                strip_tests(src)
                    .lines()
                    .enumerate()
                    .filter(|(_, line)| builds_a_command_outside_the_helper(line))
                    .map(move |(index, line)| format!("{name}:{} {}", index + 1, line.trim()))
            })
            .collect();

        assert!(
            offenders.is_empty(),
            "these production lines can build a Command outside proc::hidden_command \
             and will flash a console window on Windows:\n  {}",
            offenders.join("\n  ")
        );
    }

    /// The guard's own guard.
    ///
    /// The check above is a substring scan, so it is only worth what its patterns
    /// cover — and a scan that quietly stopped matching would leave the console-flash
    /// bug free to come back with the test still green. These are the three shapes
    /// that must stay caught, and the one that must not be.
    #[test]
    fn the_scan_catches_every_way_around_the_helper() {
        for offender in [
            "    let output = std::process::Command::new(\"osascript\")",
            "    let output = Command::new(\"osascript\")",
            "use std::process::Command as Cmd;",
            "use std::process::Command;",
        ] {
            assert!(
                builds_a_command_outside_the_helper(offender),
                "must be treated as an offender: {offender}"
            );
        }

        for allowed in [
            "    let mut command = hidden_command(program);",
            "        use std::os::windows::process::CommandExt;",
        ] {
            assert!(
                !builds_a_command_outside_the_helper(allowed),
                "must not be flagged: {allowed}"
            );
        }
    }

    /// The console attach is only useful if nothing has written yet.
    ///
    /// This ordering has no local symptom — on macOS every arrangement passes,
    /// and on Windows the failure mode is *silence*, not an error: `std` turns
    /// writes to a NULL handle into `Ok`, so a misplaced attach loses `--help`
    /// output with no crash, no log line and a zero exit code. Nothing else in
    /// the suite can see that, hence a source-order assertion.
    #[test]
    fn the_console_attach_precedes_every_output_site_in_main() {
        let main_src = strip_tests(include_str!("main.rs"));
        let line_of = |needle: &str| {
            main_src
                .lines()
                .position(|line| line.contains(needle))
                .map(|index| index + 1)
        };

        let attach = line_of("attach_parent_console()")
            .expect("main.rs must call proc::attach_parent_console()");

        // `print!(help)` is the earliest output and the one with a human waiting
        // on it; the logger's stderr chain is the earliest *implicit* writer.
        for first_writer in ["print!(", "eprintln!(", "init_logging("] {
            let writer = line_of(first_writer)
                .unwrap_or_else(|| panic!("expected {first_writer} somewhere in main.rs"));
            assert!(
                attach < writer,
                "attach_parent_console() is at line {attach} but the first {first_writer} \
                 is at line {writer} — on Windows that output would be discarded silently"
            );
        }
    }
}
