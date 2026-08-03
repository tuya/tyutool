//! Embeds the Windows icon and version info into the `.exe`.
//!
//! `cargo-packager` builds the installer but does **not** touch the executable's
//! own resources, and on Windows almost everything the user sees points at the
//! exe rather than at the installer: the Start-menu and desktop `.lnk`
//! shortcuts, the "Programs and Features" entry (whose `DisplayIcon` the NSIS
//! template sets to the installed exe), the taskbar, and Task Manager. Without a
//! `RT_GROUP_ICON` resource all of those show the generic white-page icon, which
//! is exactly the "像个脚本一样的东西" complaint this work exists to fix.
//!
//! The `VERSIONINFO` fields are the other half of looking installed rather than
//! dropped-in-place: right-click ▸ Properties ▸ Details reads them, and so do
//! most software-inventory tools.

fn main() {
    // Guard on the *target*, not the host, so the icon lands in a Windows binary
    // no matter what machine cross-built it — and so a macOS/Linux build simply
    // does nothing instead of failing on a missing resource compiler.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    // Generated from the product logo by `tauri icon` (see icons/README.md).
    println!("cargo:rerun-if-changed=icons/icon.ico");

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("icons/icon.ico");
    // The user-facing product name, deliberately different from the crate /
    // executable name — see the naming note in Cargo.toml's packager section.
    resource.set("FileDescription", "Cobuilder Bridge");
    resource.set("ProductName", "Cobuilder Bridge");
    resource.set("OriginalFilename", "tyutool-bridge.exe");
    resource.set("CompanyName", "Tuya");

    if let Err(e) = resource.compile() {
        // A warning, never a hard error. Cross-compiling from a non-Windows host
        // needs a MinGW `windres` on PATH that a developer machine has no reason
        // to have; the release build runs on a Windows runner where MSVC's
        // `rc.exe` is present, so failing here would only break local
        // `cargo check --target x86_64-pc-windows-msvc` for no benefit.
        //
        // It is loud on purpose: if this ever warns on the Windows runner, the
        // shipped installer has a blank icon and nothing else would say so.
        println!("cargo:warning=tyutool-bridge: could not embed the Windows icon/version resource ({e}); the .exe will show a generic icon");
    }
}
