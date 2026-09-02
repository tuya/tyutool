//! Start-with-session registration and the user's recorded choice about it.
//!
//! Product decision: **on by default, and the user may turn it off.** Both
//! halves matter, and the second one is the part that is easy to ship broken.
//!
//! The OS registration alone cannot express it. A LaunchAgent plist / `Run`
//! registry value is a single bit, so "absent" has two meanings — *nobody has
//! asked yet* (→ turn it on, that is the default) and *the user turned it off*
//! (→ leave it alone, forever). A bridge that reads only that bit re-enables
//! autostart on the next launch and the toggle looks like it does nothing but
//! actually reverts itself, which is worse than not having the toggle.
//!
//! So the user's choice is recorded separately, and startup *reconciles* the two:
//!
//! | recorded choice          | OS says     | what happens                           |
//! |--------------------------|-------------|----------------------------------------|
//! | nothing yet              | either      | enable — this is the "default on" rule |
//! | on, at **this** path     | not enabled | re-enable (a plist something deleted)  |
//! | on, at **this** path     | enabled     | nothing to do                          |
//! | on, at a different path  | either      | re-enable — **the app moved**          |
//! | off                      | enabled     | **disable** — an installer or an older |
//! |                          |             | build left an entry the user refused   |
//! | off                      | not enabled | nothing to do, and above all: no repair|
//!
//! The "at a different path" rows are the second thing that is easy to ship
//! broken, and the reason the record stores a path and not just a boolean.
//! `auto-launch` answers `is_enabled()` by asking whether the registration
//! **exists** — and it does that on every platform: macOS checks its LaunchAgent
//! plist for existence, Linux checks the XDG `.desktop` file, Windows reads the
//! `Run` key by name and discards the value. None of them compare the path
//! inside. So the ordinary macOS install gesture — drag the app out of the `.dmg`
//! into /Applications — leaves a registration aimed at where the app used to be,
//! which still "exists", so nothing ever repairs it and autostart quietly
//! launches nothing. Comparing against our own recorded path catches that without
//! having to parse three different registration formats.
//!
//! Everything here is advisory: a failed registration is a warning, never a
//! reason to keep the bridge from running.

use std::path::{Path, PathBuf};

/// The platform's autostart registration, behind a trait so the reconciliation
/// above can be tested without touching the developer's real login items.
///
/// Errors are surfaced rather than swallowed so the caller can log *which* step
/// failed; the caller is also the one that decides they are non-fatal.
pub trait AutostartRegistration {
    /// The executable `enable` would register — i.e. where this app is *now*.
    ///
    /// Part of the trait because [`apply_at_startup`] has to compare it against
    /// the path it registered last time; see the module docs on why `is_enabled`
    /// cannot answer that question.
    fn target(&self) -> &Path;
    fn is_enabled(&self) -> anyhow::Result<bool>;
    fn enable(&self) -> anyhow::Result<()>;
    fn disable(&self) -> anyhow::Result<()>;
}

/// The user's recorded answer to "start Cobuilder Bridge when I log in?".
///
/// Its own tiny file rather than a field in `grants.json`: that file is a
/// security record with its own 0600 handling and revocation semantics, and a UI
/// preference has no business sharing its lifetime — "撤销所有授权" must not also
/// reset autostart.
pub struct AutostartPreference {
    path: PathBuf,
}

impl AutostartPreference {
    /// Production location: `{config_dir}/com.tyutool.bridge/autostart.json`.
    ///
    /// The config class, alongside `grants.json`, for the same reason given there:
    /// this is user configuration, not a diagnostic artefact.
    pub fn open() -> anyhow::Result<Self> {
        Ok(Self::at(crate::config_dir()?.join("autostart.json")))
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `None` when the user has never expressed a choice.
    ///
    /// An unreadable or malformed file also reads as `None`: the content is one
    /// boolean and a path, and treating a corrupt byte as "the user said no" would
    /// silently take autostart away from someone who never asked for that. The
    /// next write replaces it.
    pub fn read(&self) -> Option<Stored> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        match serde_json::from_str::<Stored>(&text) {
            Ok(stored) => Some(stored),
            Err(e) => {
                log::warn!(
                    "bridge autostart preference at {} is unreadable ({e}); \
                     treating it as 'never chosen'",
                    self.path.display()
                );
                None
            }
        }
    }

    /// Record the choice and the executable it was applied to.
    ///
    /// Failure is logged, not propagated: the user's click has already taken
    /// effect on the OS registration, and refusing to continue would be a worse
    /// outcome than a choice that does not survive a restart.
    pub fn write(&self, enabled: bool, target: &Path) {
        if let Some(parent) = self.path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                log::warn!("bridge could not create {}: {e}", parent.display());
                return;
            }
        }
        let stored = Stored {
            enabled,
            path: Some(target.to_string_lossy().into_owned()),
        };
        match serde_json::to_vec_pretty(&stored) {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&self.path, bytes) {
                    log::warn!(
                        "bridge could not record the autostart choice in {}: {e}",
                        self.path.display()
                    );
                }
            }
            Err(e) => log::warn!("bridge could not serialize the autostart choice: {e}"),
        }
    }
}

/// The recorded choice, as it sits on disk.
///
/// An object rather than a bare `true` from the start, which is what made adding
/// `path` a compatible change instead of a migration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Stored {
    pub enabled: bool,
    /// The executable the registration was pointed at when this was written.
    ///
    /// `Option` so a record from a build that predates it still parses — such a
    /// record reads as "path unknown", which deliberately counts as a mismatch and
    /// re-registers once at the current path.
    #[serde(default)]
    pub path: Option<String>,
}

impl Stored {
    /// Does the recorded registration already point at `target`?
    fn points_at(&self, target: &Path) -> bool {
        self.path.as_deref() == Some(&*target.to_string_lossy())
    }
}

/// The real registration: `auto-launch` over a LaunchAgent plist (macOS), the
/// `HKCU\...\Run` value (Windows) or an XDG autostart `.desktop` (Linux).
pub struct SystemAutostart {
    launcher: auto_launch::AutoLaunch,
    exe: PathBuf,
}

impl SystemAutostart {
    /// Point the registration at the running executable.
    ///
    /// Re-resolved on every start rather than remembered. That alone is not enough
    /// to survive the app being moved — `is_enabled()` would still report the old
    /// registration as fine — which is why [`apply_at_startup`] compares this
    /// against the path in the recorded preference. See the module docs.
    pub fn for_current_exe(app_name: &str) -> anyhow::Result<Self> {
        let exe = std::env::current_exe()
            .map_err(|e| anyhow::anyhow!("bridge cannot resolve its own path: {e}"))?;
        let launcher = auto_launch::AutoLaunchBuilder::new()
            .set_app_name(app_name)
            .set_app_path(&exe.to_string_lossy())
            // ⚠ Explicit, and not the crate's default. `WindowsEnableMode::Dynamic`
            // tries `HKEY_LOCAL_MACHINE` first and only falls back to the current
            // user on access-denied — so when the bridge happens to run elevated
            // (started from an installer that has just asked for admin, or from an
            // elevated shell) one user's tick box would silently register
            // autostart for **every account on the machine**, and no other user
            // could untick it. Autostart is a per-user preference; say so.
            .set_windows_enable_mode(auto_launch::WindowsEnableMode::CurrentUser)
            // A LaunchAgent plist works for both a bare binary and a bundled
            // `.app`; the AppleScript login-item and `SMAppService` modes need a
            // bundle, and `SMAppService` additionally needs a *signed* one plus
            // macOS 13 (`auto-launch` returns `UnsupportedOS` below that, which
            // would take autostart away from macOS 12 users entirely).
            //
            // TODO(signing): revisit `MacOSLaunchMode::SMAppService` once the app
            // ships with a Developer ID signature — it is the only mode that
            // appears in System Settings ▸ General ▸ Login Items as a togglable
            // entry, which is where a macOS user looks for this.
            .set_macos_launch_mode(auto_launch::MacOSLaunchMode::LaunchAgent)
            .build()
            .map_err(|e| anyhow::anyhow!("bridge autostart not configured: {e}"))?;
        Ok(Self { launcher, exe })
    }

    /// For log lines: which executable the registration points at.
    pub fn target(&self) -> &Path {
        &self.exe
    }
}

impl AutostartRegistration for SystemAutostart {
    fn target(&self) -> &Path {
        &self.exe
    }

    fn is_enabled(&self) -> anyhow::Result<bool> {
        self.launcher
            .is_enabled()
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    fn enable(&self) -> anyhow::Result<()> {
        self.launcher.enable().map_err(|e| anyhow::anyhow!("{e}"))
    }

    fn disable(&self) -> anyhow::Result<()> {
        self.launcher.disable().map_err(|e| anyhow::anyhow!("{e}"))
    }
}

/// Bring the OS registration in line with the user's recorded choice, and return
/// the state the menu should now show.
///
/// Called once per tray start. See the table in the module docs; the row that
/// this function exists for is the last one — *recorded off, OS off* must do
/// nothing at all, because the obvious implementation ("enable if not enabled")
/// is exactly the bug that makes the toggle revert itself.
pub fn apply_at_startup(
    preference: &AutostartPreference,
    registration: &dyn AutostartRegistration,
) -> bool {
    let recorded = preference.read();
    let target = registration.target();
    let enabled = match registration.is_enabled() {
        Ok(enabled) => enabled,
        Err(e) => {
            // Nothing safe to reconcile against. Report the recorded intent so
            // the menu at least matches what the user last asked for.
            log::warn!("bridge autostart state unknown: {e}; leaving it untouched");
            return recorded.map(|s| s.enabled).unwrap_or(false);
        }
    };

    let Some(recorded) = recorded else {
        // First run of this install. "Default on" is a choice like any other, so
        // it is registered *and* recorded — recording it is what later makes an
        // explicit "off" distinguishable from "never asked".
        //
        // Registered unconditionally, without consulting `enabled`: an entry that
        // already exists here belongs to some earlier install at some other path,
        // and adopting it would inherit exactly the stale-path bug this guards.
        //
        // Recorded only on success. `apply` reports failure as `None`, and falling
        // back to `enabled` would substitute the OS bit *from before the write* —
        // which a stale entry makes `true`. Writing that would claim a
        // registration at `target` that does not exist, and since the record would
        // then agree with both `points_at` and `is_enabled`, no later launch would
        // ever retry. Leaving the record unset is what keeps the retry alive.
        return match apply(registration, true) {
            Some(reached) => {
                preference.write(reached, target);
                reached
            }
            None => enabled,
        };
    };

    if !recorded.enabled {
        // The user said no. Only act if something contradicts that.
        if !enabled {
            return false;
        }
        return apply(registration, false).map(|_| false).unwrap_or(enabled);
    }

    // The user said yes. Re-register when the OS has no entry, and also when it
    // has one that points somewhere other than this executable — see the module
    // docs: `is_enabled()` cannot tell those two apart from "correctly enabled".
    if enabled && recorded.points_at(target) {
        return true;
    }
    //
    // Same rule as the first-run branch: record the new path only once the OS has
    // accepted it. `unwrap_or(enabled)` here would be worse than useless — in this
    // branch `enabled` is `true` precisely *because* an entry for the **old** path
    // exists, so a failed re-registration would record the new path as live and
    // permanently mask the stale-path bug this branch exists to fix.
    match apply(registration, true) {
        Some(_) => {
            preference.write(true, target);
            true
        }
        None => enabled,
    }
}

/// Flip the setting from the tray menu and remember it. Returns the new state,
/// or the unchanged one if the OS refused.
pub fn toggle(
    preference: &AutostartPreference,
    registration: &dyn AutostartRegistration,
    currently_enabled: bool,
) -> bool {
    let wanted = !currently_enabled;
    let Some(reached) = apply(registration, wanted) else {
        return currently_enabled;
    };
    // Written only after the OS actually accepted it, so a failed enable does not
    // leave a recorded "on" that `apply_at_startup` would keep retrying forever.
    preference.write(reached, registration.target());
    reached
}

/// `Some(wanted)` once the OS has accepted the change, `None` if it refused.
fn apply(registration: &dyn AutostartRegistration, wanted: bool) -> Option<bool> {
    let outcome = if wanted {
        registration.enable()
    } else {
        registration.disable()
    };
    match outcome {
        Ok(()) => {
            log::info!(
                "bridge autostart {}",
                if wanted { "enabled" } else { "disabled" }
            );
            Some(wanted)
        }
        Err(e) => {
            log::warn!(
                "bridge could not {} autostart: {e}",
                if wanted { "enable" } else { "disable" }
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// A preference file of its own per test, in the same style as the temp
    /// firmware staging in `lib.rs`.
    fn temp_preference() -> (AutostartPreference, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "tyutool_bridge_autostart_{}_{}.json",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&path);
        (AutostartPreference::at(path.clone()), path)
    }

    /// Stands in for the login item / `Run` key: holds one bit and counts writes,
    /// so a test can tell "left alone" from "set to the same value".
    struct FakeRegistration {
        target: PathBuf,
        enabled: Cell<bool>,
        writes: Cell<usize>,
        fails: bool,
    }

    impl FakeRegistration {
        fn new(enabled: bool) -> Self {
            Self::at("/Applications/Cobuilder Bridge.app", enabled)
        }

        fn at(target: impl Into<PathBuf>, enabled: bool) -> Self {
            Self {
                target: target.into(),
                enabled: Cell::new(enabled),
                writes: Cell::new(0),
                fails: false,
            }
        }

        fn failing(enabled: bool) -> Self {
            Self {
                fails: true,
                ..Self::new(enabled)
            }
        }

        fn failing_at(target: impl Into<PathBuf>, enabled: bool) -> Self {
            Self {
                fails: true,
                ..Self::at(target, enabled)
            }
        }
    }

    impl AutostartRegistration for FakeRegistration {
        fn target(&self) -> &Path {
            &self.target
        }

        fn is_enabled(&self) -> anyhow::Result<bool> {
            Ok(self.enabled.get())
        }

        fn enable(&self) -> anyhow::Result<()> {
            if self.fails {
                return Err(anyhow::anyhow!("no permission"));
            }
            self.writes.set(self.writes.get() + 1);
            self.enabled.set(true);
            Ok(())
        }

        fn disable(&self) -> anyhow::Result<()> {
            if self.fails {
                return Err(anyhow::anyhow!("no permission"));
            }
            self.writes.set(self.writes.get() + 1);
            self.enabled.set(false);
            Ok(())
        }
    }

    /// The headline behaviour, and the one a naive implementation gets wrong:
    /// turning autostart off has to survive the next launch.
    ///
    /// Written as the whole round trip — first start, user toggles off, restart —
    /// rather than as an assertion about an internal decision, because the defect
    /// this guards against only appears when those three steps run in order.
    #[test]
    fn turning_autostart_off_survives_a_restart() {
        let (preference, path) = temp_preference();
        let registration = FakeRegistration::new(false);

        // First ever start: default on.
        assert!(
            apply_at_startup(&preference, &registration),
            "the default must be on"
        );
        assert!(registration.enabled.get());

        // The user unticks it.
        assert!(!toggle(&preference, &registration, true));
        assert!(!registration.enabled.get());

        // Next launch of the same install.
        assert!(
            !apply_at_startup(&preference, &registration),
            "a recorded 'off' must not be re-enabled on the next start"
        );
        assert!(
            !registration.enabled.get(),
            "the OS registration must still be gone"
        );

        let _ = std::fs::remove_file(path);
    }

    /// The other direction: a recorded "on" is repaired if the registration went
    /// missing — a plist some cleanup tool removed, a `Run` value that was wiped.
    #[test]
    fn a_recorded_yes_is_re_registered_when_the_os_entry_disappeared() {
        let (preference, path) = temp_preference();
        let registration = FakeRegistration::new(false);
        preference.write(true, registration.target());

        assert!(apply_at_startup(&preference, &registration));
        assert!(registration.enabled.get());

        let _ = std::fs::remove_file(path);
    }

    /// **Moving the app must move the autostart entry with it.**
    ///
    /// This is the one that a reasonable implementation gets wrong, and it was
    /// found on a real machine rather than reasoned about: `auto-launch` reports
    /// `is_enabled()` by asking whether the registration *exists* — on all three
    /// platforms. macOS checks `~/Library/LaunchAgents/<label>.plist` for
    /// existence, Linux checks the XDG `.desktop` file, and Windows reads the
    /// `Run` key by name and throws the value away. None of them look at the path
    /// inside.
    ///
    /// So the normal macOS install gesture — drag the app out of the `.dmg` into
    /// /Applications, where it now lives at a new path — leaves a registration
    /// pointing at wherever the app used to be. It still "exists", so nothing
    /// repairs it, and autostart silently launches nothing for good. Observed
    /// exactly that: a plist from an earlier bare-binary run kept pointing at
    /// `target/debug/tyutool-bridge` while the freshly installed `.app` recorded
    /// `enabled: true` and touched nothing.
    ///
    /// The fix cannot lean on `is_enabled`; the recorded choice has to carry the
    /// path we registered so a mismatch is detectable without reading the OS's
    /// registration format on three platforms.
    #[test]
    fn moving_the_app_re_registers_autostart_at_the_new_path() {
        let (preference, path) = temp_preference();

        // First run from wherever the user opened it (e.g. the mounted .dmg).
        let downloads =
            FakeRegistration::at("/Volumes/Cobuilder Bridge/Cobuilder Bridge.app", false);
        assert!(apply_at_startup(&preference, &downloads));
        assert!(downloads.enabled.get());

        // They drag it into /Applications and launch it from there. The OS still
        // says "enabled" — the old registration is sitting right there.
        let installed = FakeRegistration::at("/Applications/Cobuilder Bridge.app", true);
        assert!(apply_at_startup(&preference, &installed));

        assert_eq!(
            installed.writes.get(),
            1,
            "the registration still points at the old path; it must be rewritten"
        );

        let _ = std::fs::remove_file(path);
    }

    /// The guard on the rule above: once the recorded path agrees with where the
    /// app is, launching again must not keep rewriting the registration.
    #[test]
    fn launching_repeatedly_from_the_same_path_rewrites_nothing() {
        let (preference, path) = temp_preference();
        let here = FakeRegistration::at("/Applications/Cobuilder Bridge.app", false);

        apply_at_startup(&preference, &here);
        let after_first_run = here.writes.get();
        apply_at_startup(&preference, &here);
        apply_at_startup(&preference, &here);

        assert_eq!(
            here.writes.get(),
            after_first_run,
            "nothing changed, so nothing should have been written"
        );

        let _ = std::fs::remove_file(path);
    }

    /// A recorded "off" plus an entry that exists anyway is not a contradiction
    /// to shrug at: an installer with admin rights, or an older build of this
    /// bridge, can create one. The user's answer wins.
    #[test]
    fn a_recorded_no_removes_an_entry_something_else_created() {
        let (preference, path) = temp_preference();
        let registration = FakeRegistration::new(true);
        preference.write(false, registration.target());

        assert!(!apply_at_startup(&preference, &registration));
        assert!(!registration.enabled.get());

        let _ = std::fs::remove_file(path);
    }

    /// Startup must not rewrite a registration that already agrees with the
    /// recorded choice. It runs on every launch, and a plist rewritten on every
    /// login is both pointless churn and a way to lose a user's manual edit.
    #[test]
    fn a_state_that_already_agrees_is_left_untouched() {
        let (preference, path) = temp_preference();
        let registration = FakeRegistration::new(true);
        preference.write(true, registration.target());

        assert!(apply_at_startup(&preference, &registration));
        assert_eq!(
            registration.writes.get(),
            0,
            "nothing should have been written to the OS registration"
        );

        let _ = std::fs::remove_file(path);
    }

    /// A registration that **failed** must never be recorded as done.
    ///
    /// The trap is the fallback: `apply()` reports failure as `None`, and
    /// `unwrap_or(enabled)` substitutes the OS bit *as measured before the write*.
    /// When a stale entry from an earlier install is already sitting there, that
    /// bit is `true`, so a failed write looks like a success and gets recorded
    /// against the current path. After that, `points_at(target)` and
    /// `is_enabled()` both agree, the reconciliation takes its early return, and
    /// the failure is masked **permanently** — autostart is aimed at wherever the
    /// old install was, and nothing will ever try to fix it again.
    ///
    /// This is the stale-path bug returning through the error path, which is why
    /// it gets its own case rather than being folded into the tests above.
    #[test]
    fn a_registration_that_failed_is_not_recorded_as_successful() {
        let (preference, path) = temp_preference();
        // A leftover entry from an earlier install at some other location, so the
        // OS reports "enabled" while pointing at the wrong executable.
        let registration = FakeRegistration::failing_at("/Applications/Cobuilder Bridge.app", true);
        preference.write(true, Path::new("/Volumes/old install/Cobuilder Bridge.app"));

        apply_at_startup(&preference, &registration);

        assert_ne!(
            preference.read().and_then(|s| s.path).as_deref(),
            Some("/Applications/Cobuilder Bridge.app"),
            "the write failed, so recording it as registered here would mask the \
             failure forever — the next launch must try again"
        );

        let _ = std::fs::remove_file(path);
    }

    /// Same trap on the very first run: nothing recorded yet, a stale entry
    /// present, and the write fails. Recording "on at this path" would make every
    /// later launch take the early return and never retry.
    #[test]
    fn a_failed_first_registration_leaves_nothing_recorded_to_retry_against() {
        let (preference, path) = temp_preference();
        let registration = FakeRegistration::failing_at("/Applications/Cobuilder Bridge.app", true);

        apply_at_startup(&preference, &registration);

        assert_ne!(
            preference.read().and_then(|s| s.path).as_deref(),
            Some("/Applications/Cobuilder Bridge.app"),
            "nothing was registered, so nothing should claim it was"
        );

        let _ = std::fs::remove_file(path);
    }

    /// An OS that refuses the change must not leave a recorded choice the menu
    /// would then show as done — the checkmark has to keep telling the truth.
    #[test]
    fn a_refused_toggle_changes_neither_the_menu_nor_the_record() {
        let (preference, path) = temp_preference();
        let registration = FakeRegistration::failing(true);
        preference.write(true, registration.target());

        assert!(
            toggle(&preference, &registration, true),
            "a refused disable must report the setting as still on"
        );
        assert_eq!(
            preference.read().map(|s| s.enabled),
            Some(true),
            "and must not have recorded the change it failed to make"
        );

        let _ = std::fs::remove_file(path);
    }
}
