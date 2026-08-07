//! Build-config guard: Windows MSVC artifacts must statically link the VC++ CRT.
//!
//! Field failure this pins down: on a clean Windows 11 machine,
//! `tyutool-bridge.exe` aborts at startup with "cannot find VCRUNTIME140.dll"
//! (Lean e57e59798eed4e5bb5390384649fb2d7). The default `*-pc-windows-msvc`
//! build links the VC++ CRT *dynamically*, so any machine without the VC++
//! Redistributable cannot even start the process — and reinstalling the app
//! does not help, because the missing piece is a system runtime, not the app.
//!
//! The fix is `-C target-feature=+crt-static` scoped to MSVC targets in the
//! workspace `.cargo/config.toml`, which bakes the CRT into the executable.
//! CI builds (`bridge.yml`) run plain `cargo build` from the workspace root
//! with no `RUSTFLAGS` env, so that config file is the single source of truth
//! for the flag — this test fails the (Linux) `check` job if it ever goes
//! missing or loses the flag, instead of shipping a broken Windows exe again.

use std::path::Path;

/// The cargo config `[target]` key the flag must live under. `cfg(...)` keys
/// apply to every matching target triple, so this covers both `x86_64` and any
/// future `aarch64-pc-windows-msvc` build without listing triples one by one.
const MSVC_TARGET_KEY: &str = r#"cfg(all(windows, target_env = "msvc"))"#;

#[test]
fn workspace_cargo_config_forces_static_crt_for_windows_msvc() {
    let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cargo/config.toml");
    let raw = std::fs::read_to_string(&config_path).unwrap_or_else(|err| {
        panic!(
            "workspace cargo config missing at {}: {err}\n\
             it must set `-C target-feature=+crt-static` for MSVC targets, \
             otherwise Windows builds require VCRUNTIME140.dll (VC++ \
             Redistributable) and fail to start on machines without it",
            config_path.display()
        )
    });
    let parsed: toml::Value = raw
        .parse()
        .unwrap_or_else(|err| panic!("{} is not valid TOML: {err}", config_path.display()));

    let rustflags = parsed
        .get("target")
        .and_then(|t| t.get(MSVC_TARGET_KEY))
        .and_then(|t| t.get("rustflags"))
        .and_then(|f| f.as_array())
        .unwrap_or_else(|| {
            panic!(
                "{} must define [target.'{}'] with a `rustflags` array",
                config_path.display(),
                MSVC_TARGET_KEY
            )
        });

    let flags: Vec<&str> = rustflags.iter().filter_map(|v| v.as_str()).collect();
    // Accept both spellings cargo does: ["-C", "target-feature=+crt-static"]
    // and the single-token ["-Ctarget-feature=+crt-static"].
    let has_static_crt = flags
        .windows(2)
        .any(|w| w == ["-C", "target-feature=+crt-static"])
        || flags.contains(&"-Ctarget-feature=+crt-static");
    assert!(
        has_static_crt,
        "rustflags for MSVC targets in {} must contain `-C target-feature=+crt-static` \
         (found {:?}); without it the exe depends on VCRUNTIME140.dll",
        config_path.display(),
        flags
    );
}

/// Second line of defence that runs only where it is meaningful: when the test
/// suite itself is compiled for an MSVC target, the flag from the workspace
/// config must actually have taken effect on this very build.
#[cfg(all(windows, target_env = "msvc"))]
#[test]
fn msvc_test_build_actually_links_crt_statically() {
    assert!(
        cfg!(target_feature = "crt-static"),
        "this MSVC build did not apply `+crt-static` — the produced exe would \
         require VCRUNTIME140.dll at runtime"
    );
}
