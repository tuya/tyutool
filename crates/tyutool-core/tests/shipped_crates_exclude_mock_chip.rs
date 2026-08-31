//! Guards the second half of the `mock-chip` containment rule: no crate that
//! produces a shipped binary may enable the feature in its manifest.
//!
//! The feature registers a fake device in the *default* plugin registry, so a
//! shipped build carrying it would offer users a chip that pretends to flash.
//! The first guard is the `compile_error!` at the top of `src/lib.rs`, which
//! catches a release build that picked the feature up from a `--features` flag.
//! This one catches the other route in: someone adding it to a manifest, where
//! it would then be inherited by every build of that crate, release or not.
//!
//! Deliberately **not** gated behind `#[cfg(feature = "mock-chip")]` — it has to
//! run on the ordinary `cargo test -p tyutool-core`, which is the run that
//! happens on every push.
//!
//! Same shape as `crates/tyutool-bridge/tests/build_config.rs`, which guards the
//! `+crt-static` rustflag: a build-configuration invariant that no amount of
//! code review reliably catches, pinned by a test instead.

use std::path::{Path, PathBuf};

/// Every crate in the workspace whose build produces something a user receives.
/// `tyutool-core` itself is absent on purpose: it is a library, it *defines* the
/// feature, and enabling it there would be its own `[features]` table rather
/// than a dependency edge.
const SHIPPED_MANIFESTS: &[&str] = &[
    "crates/tyutool-cli/Cargo.toml",
    "crates/tyutool-serve/Cargo.toml",
    "crates/tyutool-bridge/Cargo.toml",
    "src-tauri/Cargo.toml",
];

const FEATURE: &str = "mock-chip";
/// The forwarding spelling, as it would appear in a `[features]` entry.
const FORWARDED: &str = "tyutool-core/mock-chip";

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/tyutool-core.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/tyutool-core has a workspace root two levels up")
        .to_path_buf()
}

/// Walks the whole parsed manifest rather than a fixed list of tables, so a
/// `[target.'cfg(...)'.dependencies]` entry or a future table shape cannot slip
/// past. Two ways in are checked:
///
///   * a `tyutool-core` dependency whose `features` array holds `mock-chip`;
///   * any array holding the forwarding string `tyutool-core/mock-chip`, which
///     is how a `[features]` entry would pull it in.
///
/// Both are structural — a *comment* mentioning the feature is fine, which a
/// plain text scan could not manage given this file's own prose.
fn enables_mock_chip(value: &toml::Value) -> bool {
    match value {
        toml::Value::Table(table) => table.iter().any(|(key, child)| {
            if key == "tyutool-core" && dependency_features(child).contains(&FEATURE) {
                return true;
            }
            enables_mock_chip(child)
        }),
        toml::Value::Array(items) => items
            .iter()
            .any(|item| item.as_str() == Some(FORWARDED) || enables_mock_chip(item)),
        _ => false,
    }
}

fn dependency_features(dependency: &toml::Value) -> Vec<&str> {
    dependency
        .get("features")
        .and_then(toml::Value::as_array)
        .map(|items| items.iter().filter_map(toml::Value::as_str).collect())
        .unwrap_or_default()
}

#[test]
fn no_shipped_crate_enables_mock_chip() {
    let root = workspace_root();

    for relative in SHIPPED_MANIFESTS {
        let path = root.join(relative);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {relative}: {e}"));
        let manifest: toml::Value = text
            .parse()
            .unwrap_or_else(|e| panic!("cannot parse {relative}: {e}"));

        assert!(
            !enables_mock_chip(&manifest),
            "{relative} enables tyutool-core's `{FEATURE}` feature.\n\
             That feature registers a fake chip plugin in the default registry and must never \
             reach a shipped artifact. Enable it on the `cargo test` command line instead \
             (`cargo test -p tyutool-core --features {FEATURE}`), never in a manifest.",
        );
    }
}

/// The list above is only worth anything while it still names real files.
#[test]
fn every_guarded_manifest_exists() {
    let root = workspace_root();
    for relative in SHIPPED_MANIFESTS {
        let path = root.join(relative);
        assert!(
            path.is_file(),
            "{relative} is listed as a shipped crate but does not exist — a crate was renamed \
             or moved and this guard silently stopped covering it",
        );
    }
}
