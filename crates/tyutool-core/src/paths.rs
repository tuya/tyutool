//! Where each shipped binary keeps its per-user files.
//!
//! The operating systems split a user's files by **what the data is**, not by which
//! program wrote it, and each class carries different system behaviour:
//!
//! | Class | macOS | What the OS does with it |
//! |---|---|---|
//! | logs | `~/Library/Logs/<id>` | Console.app lists it; not backed up |
//! | config / data | `~/Library/Application Support/<id>` | Time Machine backs it up; never auto-removed |
//! | cache | `~/Library/Caches/<id>` | excluded from backup; treated as reclaimable |
//!
//! Windows draws the same line as roaming (`%APPDATA%`, follows the user between
//! machines) versus local (`%LOCALAPPDATA%`, stays put), and the XDG spec as
//! `XDG_DATA_HOME` versus `XDG_CACHE_HOME` ("non-essential cached data"). So a
//! well-behaved program *is* spread across several directories, and merging them would
//! mean either backing up a re-downloadable cache on every Time Machine run or letting a
//! disk-space sweep delete the user's credentials. We follow the split.
//!
//! What is ours to get right is the **name**: one reverse-DNS id per shipped product,
//! used unchanged in every class it needs. `com.tyutool.desktop` is the GUI's real bundle
//! identifier (`src-tauri/tauri.conf.json`), `com.tyutool.bridge` the bridge's own
//! (`crates/tyutool-bridge/Cargo.toml` `[package.metadata.packager]`); `com.tyutool.cli`
//! is minted here for the CLI, which ships as a bare binary with no bundle of its own,
//! so that the family reads the same way in a file manager. [`SHARED_ID`] covers what no
//! single product owns.
//!
//! [`log_dir`] reproduces Tauri's `app_log_dir()` formula exactly (`tauri`'s
//! `path/desktop.rs`), so the GUI keeps using Tauri's resolver and the CLI and the bridge
//! land beside it under their own ids rather than inventing a second layout.

use std::path::PathBuf;

/// The desktop GUI. Also its macOS bundle identifier and the Windows/Linux WebView
/// profile directory — Tauri derives both from it, so it cannot be renamed without
/// resigning the app and resetting every WebView-stored setting.
pub const DESKTOP_ID: &str = "com.tyutool.desktop";
/// The CLI. No bundle of its own; this id exists so its files sit beside the other two.
pub const CLI_ID: &str = "com.tyutool.cli";
/// The resident bridge. Matches its packager identifier.
///
/// Note this is *not* `AUTOSTART_APP_NAME`: the autostart registration is keyed by that
/// separate constant (a LaunchAgent label, an XDG `.desktop` file name, an `HKCU\Run`
/// value), and renaming it would orphan every existing registration. Paths and
/// registration keys are different namespaces; keep them that way.
pub const BRIDGE_ID: &str = "com.tyutool.bridge";
/// Data no single product owns, so it cannot live under one product's id: caches any of
/// the three may fill and all three read (the RAM loader assets), and the serial-debug
/// session archive, which the GUI and `tyutool-cli serve` both write.
pub const SHARED_ID: &str = "com.tyutool.shared";

/// Log directory for `id`.
///
/// macOS puts logs in their own top-level location; everywhere else they are local
/// (non-roaming) application data. Identical to Tauri's `app_log_dir()`, deliberately:
/// the GUI resolves this path through Tauri and must land in the same place.
pub fn log_dir(id: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|d| d.join("Library").join("Logs").join(id))
    }
    #[cfg(not(target_os = "macos"))]
    {
        dirs::data_local_dir().map(|d| d.join(id).join("logs"))
    }
}

/// Configuration directory for `id` — user choices that must survive and be backed up.
///
/// On macOS and Windows this is the same directory the platform uses for application
/// data; only Linux separates `~/.config` from `~/.local/share`.
pub fn config_dir(id: &str) -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(id))
}

/// Cache directory for `id` — anything that can be fetched or computed again. The OS is
/// free to reclaim it, so nothing whose loss costs the user work belongs here.
pub fn cache_dir(id: &str) -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join(id))
}

/// Temporary directory for `id`, for files that need not survive a reboot.
pub fn temp_dir(id: &str) -> PathBuf {
    std::env::temp_dir().join(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ids are a namespace, and a file manager should sort them together. A stray
    /// name here is how the layout drifted apart in the first place.
    #[test]
    fn every_id_is_reverse_dns_under_one_prefix() {
        for id in [DESKTOP_ID, CLI_ID, BRIDGE_ID, SHARED_ID] {
            assert!(id.starts_with("com.tyutool."), "{id}");
            assert_eq!(id.matches('.').count(), 2, "{id}");
            assert!(
                id.chars().all(|c| c.is_ascii_lowercase() || c == '.'),
                "{id}"
            );
        }
    }

    #[test]
    fn ids_are_distinct() {
        let ids = [DESKTOP_ID, CLI_ID, BRIDGE_ID, SHARED_ID];
        for (i, a) in ids.iter().enumerate() {
            for b in &ids[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    /// The GUI reaches its log directory through Tauri, the other two through here, and
    /// both must produce the same shape or the family stops being one layout.
    #[test]
    fn log_dir_matches_tauris_app_log_dir_shape() {
        let dir = log_dir(CLI_ID).expect("a platform log directory");
        if cfg!(target_os = "macos") {
            assert!(dir.ends_with(CLI_ID), "{dir:?}");
            assert!(dir.parent().unwrap().ends_with("Logs"), "{dir:?}");
        } else {
            assert!(dir.ends_with("logs"), "{dir:?}");
            assert!(dir.parent().unwrap().ends_with(CLI_ID), "{dir:?}");
        }
    }

    #[test]
    fn each_class_puts_the_id_in_the_path() {
        assert!(config_dir(BRIDGE_ID).unwrap().ends_with(BRIDGE_ID));
        assert!(cache_dir(SHARED_ID).unwrap().ends_with(SHARED_ID));
        assert!(temp_dir(SHARED_ID).ends_with(SHARED_ID));
    }

    /// Logs are diagnostics: on Windows they must not follow the user between machines
    /// over a roaming profile, which is exactly what the CLI used to do by resolving
    /// them under `data_dir()`.
    #[cfg(windows)]
    #[test]
    fn windows_logs_are_local_not_roaming() {
        let logs = log_dir(CLI_ID).unwrap();
        let roaming = dirs::data_dir().unwrap();
        assert!(!logs.starts_with(&roaming), "{logs:?} is under {roaming:?}");
        assert!(
            logs.starts_with(dirs::data_local_dir().unwrap()),
            "{logs:?}"
        );
    }
}
