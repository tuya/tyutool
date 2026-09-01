//! On-demand RAM loader assets.
//!
//! Two chips cannot be flashed by their own mask ROM: it can do little more than write
//! SRAM and jump to it, so tyutool has to upload a vendor downloader *first* and let
//! that program the flash. Those images (`ram-loader-ln882h-*.bin`,
//! `ram-loader-gd32vw553-*.bin`) used to be `include_bytes!`-compiled into this crate.
//! They are now published assets — sources in `assets/ram-loader/`, released to the
//! `ram-loader` tag on GitHub/Gitee and mirrored on the Tuya CDN — downloaded once and
//! cached, so no shipped binary carries vendor firmware. See
//! `assets/ram-loader/README.md` for the layout, naming and publishing rules.
//!
//! **The digest is pinned by the calling plugin, never taken from the manifest.** Each
//! plugin declares the one [`RamLoaderRef`] it was written against, and [`resolve`]
//! verifies the bytes against *that* — the manifest's own `sha256` is only a cross-check
//! logged on mismatch. So a rolled or tampered manifest cannot feed a plugin a different
//! loader, and publishing a new loader version never changes what an already-shipped
//! tool uploads.
//!
//! Resolution order, first hit wins:
//!
//! 1. `TYUTOOL_RAM_LOADER_DIR`, if set — a local directory, never the network. This is
//!    the escape hatch for an air-gapped production line: drop the bins there and the
//!    tool never reaches out.
//! 2. The cache under [`cache_dir`], written by a previous run.
//! 3. A download, when the `download` feature is on (it is for every shipped binary).
//!
//! A file that exists but fails verification is an error, not a cache miss: silently
//! re-downloading over an operator's deliberately placed file would hide the mistake.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::FlashError;
use crate::flash_event::FlashEvent;
#[cfg(feature = "download")]
use crate::flash_event::FlashPhase;

/// Points a run at a directory of loader bins instead of the network. Read-only: nothing
/// is ever written here, and a miss falls through to the cache, not to a download.
pub const OFFLINE_DIR_ENV: &str = "TYUTOOL_RAM_LOADER_DIR";

/// Manifest mirrors, tried in order. GitHub first, Gitee second (reachable from mainland
/// China), the Tuya CDN last. Mirrors `AUTH_FIRMWARE_SOURCES` in
/// `src/features/batch-flash-auth/auth-firmware.ts` and the allowlist in
/// `src-tauri/src/updater.rs` — keep the host set in sync with both.
pub const MANIFEST_MIRRORS: &[&str] = &[
    "https://github.com/tuya/tyutool/releases/download/ram-loader/ram-loader.json",
    "https://gitee.com/tuya-open/tyutool/releases/download/ram-loader/ram-loader.json",
    "https://airtake-public-data-1254153901.cos.ap-shanghai.myqcloud.com/smart/embed/pruduct/tyutool/ram-loader/ram-loader.json",
];

/// The one loader image a plugin was written against.
///
/// `version` is tyutool's own asset version: neither vendor ships a usable version
/// number (the GD32 image leaves its own `SDK release version:` banner empty; the LN882H
/// image carries none at all), so provenance lives in the asset's `.txt` notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RamLoaderRef {
    /// Lowercase chip id, matching the `assets/ram-loader/<chip>/` directory.
    pub chip: &'static str,
    pub version: &'static str,
    /// Expected length in bytes. Checked before the digest so a truncated download
    /// reports the obvious thing.
    pub size: usize,
    /// Lowercase hex SHA-256 of the image.
    pub sha256: &'static str,
}

impl RamLoaderRef {
    /// Published (and cached) file name for this loader.
    pub fn file_name(&self) -> String {
        format!("ram-loader-{}-{}.bin", self.chip, self.version)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Check `bytes` against what the plugin pinned.
fn verify(bytes: &[u8], loader: &RamLoaderRef) -> Result<(), String> {
    if bytes.len() != loader.size {
        return Err(format!(
            "{} is {} bytes, expected {}",
            loader.file_name(),
            bytes.len(),
            loader.size
        ));
    }
    let actual = hex(&Sha256::digest(bytes));
    if actual != loader.sha256 {
        return Err(format!(
            "{} SHA-256 mismatch: expected {}, got {}",
            loader.file_name(),
            loader.sha256,
            actual
        ));
    }
    Ok(())
}

/// Look for `loader` in `dir`, accepting both the flat layout a user would hand-assemble
/// and the `<chip>/` layout `assets/ram-loader/` uses, so either can be copied verbatim
/// onto a production machine.
///
/// `None` means not present. `Some(Err(..))` means present but wrong — a hard error, so
/// the operator hears about the file they placed rather than having it silently ignored.
fn load_from_dir(dir: &Path, loader: &RamLoaderRef) -> Option<Result<Vec<u8>, String>> {
    let name = loader.file_name();
    let candidates = [dir.join(&name), dir.join(loader.chip).join(&name)];
    let path = candidates.iter().find(|p| p.is_file())?;
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return Some(Err(format!("cannot read {}: {e}", path.display()))),
    };
    Some(match verify(&bytes, loader) {
        Ok(()) => Ok(bytes),
        Err(e) => Err(format!("{} ({})", e, path.display())),
    })
}

/// Where downloaded loaders are kept: `<user cache dir>/tyutool/ram-loader`.
///
/// One directory for every frontend, so a loader the CLI fetched is one the GUI and the
/// bridge already have. Deliberately not the log directory and not the GUI's
/// `app_cache_dir()`, which is per-bundle-id.
pub fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("tyutool").join("ram-loader"))
}

/// Cache path for one loader under `root`.
#[cfg(any(feature = "download", test))]
fn cache_path(root: &Path, loader: &RamLoaderRef) -> PathBuf {
    root.join(loader.chip).join(loader.file_name())
}

/// Write `bytes` to the cache, replacing whatever was there. Best-effort: a failure to
/// cache is logged and swallowed, because the job itself can still run on the bytes we
/// already hold.
#[cfg(any(feature = "download", test))]
fn store_in_cache(root: &Path, loader: &RamLoaderRef, bytes: &[u8]) {
    let dest = cache_path(root, loader);
    let tmp = dest.with_extension(format!("tmp-{}", std::process::id()));
    let write = dest
        .parent()
        .map(std::fs::create_dir_all)
        .unwrap_or(Ok(()))
        .and_then(|()| std::fs::write(&tmp, bytes))
        .and_then(|()| std::fs::rename(&tmp, &dest));
    match write {
        Ok(()) => log::info!("cached RAM loader at {}", dest.display()),
        Err(e) => {
            log::warn!("could not cache RAM loader at {}: {e}", dest.display());
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

/// The published manifest. Mirrors `RamLoaderManifest` in
/// `scripts/generate-ram-loader-manifest.ts`.
#[cfg(any(feature = "download", test))]
#[derive(Debug, serde::Deserialize)]
struct Manifest {
    loaders: Vec<ManifestEntry>,
}

#[cfg(any(feature = "download", test))]
#[derive(Debug, serde::Deserialize)]
struct ManifestEntry {
    chip: String,
    version: String,
    url: String,
    sha256: String,
}

/// Find the entry for `loader` in a manifest body, returning its download URL.
#[cfg(any(feature = "download", test))]
fn entry_url(body: &str, loader: &RamLoaderRef) -> Result<String, String> {
    let manifest: Manifest =
        serde_json::from_str(body).map_err(|e| format!("malformed manifest: {e}"))?;
    let entry = manifest
        .loaders
        .into_iter()
        .find(|e| e.chip == loader.chip && e.version == loader.version)
        .ok_or_else(|| format!("manifest has no {} version {}", loader.chip, loader.version))?;
    // Advisory only — the bytes are checked against the pinned digest either way. A
    // mismatch here means the publishing side and this build disagree, which is worth a
    // line in the log even though it changes nothing.
    if entry.sha256.to_lowercase() != loader.sha256 {
        log::warn!(
            "manifest sha256 for {} {} ({}) differs from the pinned {}",
            loader.chip,
            loader.version,
            entry.sha256,
            loader.sha256
        );
    }
    Ok(entry.url)
}

#[cfg(feature = "download")]
mod net {
    use super::{entry_url, RamLoaderRef, MANIFEST_MIRRORS};
    use std::time::Duration;

    /// Manifests are a few hundred bytes and loaders a few tens of KiB; a mirror that
    /// cannot answer inside this is one we should be trying the next of.
    const TIMEOUT: Duration = Duration::from_secs(20);
    /// Nothing legitimate here is anywhere near this big (the largest loader is 37 KiB).
    /// Bounds memory before the digest check runs, like `download_auth_firmware`'s cap.
    const MAX_BYTES: usize = 4 * 1024 * 1024;

    /// Host part of a mirror URL, for error messages. Falls back to the whole URL rather
    /// than hiding which mirror an error came from.
    fn host_of(url: &str) -> String {
        reqwest::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_else(|| url.to_owned())
    }

    fn get(url: &str) -> Result<Vec<u8>, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(TIMEOUT)
            // Release asset URLs redirect to a CDN; bound the chain rather than
            // following it indefinitely.
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client.get(url).send().map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        let bytes = resp.bytes().map_err(|e| e.to_string())?;
        if bytes.len() > MAX_BYTES {
            return Err(format!("response is larger than {MAX_BYTES} bytes"));
        }
        Ok(bytes.to_vec())
    }

    /// Fetch `loader` from the first mirror that can serve it. The returned bytes are
    /// **unverified** — the caller checks them against the pinned digest.
    pub(super) fn fetch(loader: &RamLoaderRef) -> Result<Vec<u8>, String> {
        let mut errors = Vec::new();
        for manifest_url in MANIFEST_MIRRORS {
            let attempt = get(manifest_url)
                .and_then(|body| {
                    String::from_utf8(body).map_err(|_| "manifest is not UTF-8".to_string())
                })
                .and_then(|body| entry_url(&body, loader))
                .and_then(|url| {
                    log::info!("downloading RAM loader from {url}");
                    get(&url)
                });
            match attempt {
                Ok(bytes) => return Ok(bytes),
                Err(e) => {
                    log::warn!("RAM loader mirror {manifest_url} failed: {e}");
                    // Name the mirror: three bare `HTTP 404`s in a row tell the user
                    // nothing about which source to go look at.
                    errors.push(format!("{}: {e}", host_of(manifest_url)));
                }
            }
        }
        Err(errors.join("; "))
    }
}

/// Resolve the bytes of `loader`, downloading and caching them if needed.
///
/// Call this *before* opening the port: a loader that cannot be resolved should fail the
/// job while the device is still untouched.
pub fn resolve(
    loader: &RamLoaderRef,
    progress: &dyn Fn(FlashEvent),
) -> Result<Vec<u8>, FlashError> {
    if let Some(dir) = std::env::var_os(OFFLINE_DIR_ENV) {
        let dir = PathBuf::from(dir);
        match load_from_dir(&dir, loader) {
            Some(Ok(bytes)) => {
                log::info!(
                    "using RAM loader {} from {OFFLINE_DIR_ENV}",
                    loader.file_name()
                );
                return Ok(bytes);
            }
            Some(Err(e)) => return Err(FlashError::Plugin(e)),
            None => log::info!(
                "{OFFLINE_DIR_ENV}={} has no {}",
                dir.display(),
                loader.file_name()
            ),
        }
    }

    let cache_root = cache_dir();
    if let Some(root) = cache_root.as_deref() {
        match load_from_dir(root, loader) {
            Some(Ok(bytes)) => {
                log::info!("RAM loader cache hit: {}", loader.file_name());
                return Ok(bytes);
            }
            // A corrupt cache entry is ours, not the operator's: say so and re-fetch.
            Some(Err(e)) => log::warn!("discarding cached RAM loader: {e}"),
            None => {}
        }
    }

    #[cfg(feature = "download")]
    {
        progress(FlashEvent::Phase {
            phase: FlashPhase::FetchRamLoader,
        });
        let bytes = net::fetch(loader).map_err(|e| FlashError::Plugin(unavailable(loader, &e)))?;
        verify(&bytes, loader).map_err(|e| {
            FlashError::Plugin(format!(
                "{e} — the download did not match the loader this build expects"
            ))
        })?;
        if let Some(root) = cache_root.as_deref() {
            store_in_cache(root, loader, &bytes);
        }
        Ok(bytes)
    }
    #[cfg(not(feature = "download"))]
    {
        let _ = progress;
        Err(FlashError::Plugin(unavailable(
            loader,
            "this build cannot download it (the `download` feature is off)",
        )))
    }
}

/// The message a user gets when the loader is nowhere to be found. It has to say what is
/// missing, that it is normally fetched once, and how to supply it by hand — the tool no
/// longer carries the image, so this is the whole recovery path.
fn unavailable(loader: &RamLoaderRef, reason: &str) -> String {
    format!(
        "{} needs its RAM loader {}, which is not cached locally and {}. Connect to the \
         network once to let tyutool fetch it, or download it from the `ram-loader` \
         release and point {} at the directory holding it.",
        loader.chip.to_uppercase(),
        loader.file_name(),
        reason,
        OFFLINE_DIR_ENV
    )
}

/// The `assets/ram-loader/` copy of `loader`, verified against the pinned digest.
///
/// Each plugin asserts its own constant against this: `assets/ram-loader/` is the source
/// the release workflow publishes from, so a mismatch means either the pinned constant
/// or the file about to be published is wrong — and nothing else would catch it, since
/// the image is no longer compiled in.
#[cfg(test)]
pub(crate) fn repo_asset_bytes(loader: &RamLoaderRef) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/ram-loader")
        .join(loader.chip)
        .join(loader.file_name());
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("cannot read the published asset {}: {e}", path.display()));
    if let Err(e) = verify(&bytes, loader) {
        panic!("{} does not match the pinned loader: {e}", path.display());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const FIXTURE: RamLoaderRef = RamLoaderRef {
        chip: "testchip",
        version: "9.9.9",
        size: 4,
        // sha256("abcd")
        sha256: "88d4266fd4e6338d13b845fcf289579d209c897823b9217da3e161936f031589",
    };

    #[test]
    fn file_name_follows_the_published_naming_rule() {
        assert_eq!(FIXTURE.file_name(), "ram-loader-testchip-9.9.9.bin");
    }

    #[test]
    fn verify_accepts_the_pinned_image_and_rejects_the_rest() {
        assert!(verify(b"abcd", &FIXTURE).is_ok());

        let short = verify(b"abc", &FIXTURE).expect_err("wrong length must fail");
        assert!(short.contains("3 bytes, expected 4"), "{short}");

        let wrong = verify(b"abce", &FIXTURE).expect_err("wrong digest must fail");
        assert!(wrong.contains("SHA-256 mismatch"), "{wrong}");
    }

    #[test]
    fn load_from_dir_reads_both_the_flat_and_the_per_chip_layout() {
        let dir = tempfile::tempdir().unwrap();

        assert!(
            load_from_dir(dir.path(), &FIXTURE).is_none(),
            "an empty directory is a miss, not an error"
        );

        fs::write(dir.path().join(FIXTURE.file_name()), b"abcd").unwrap();
        assert_eq!(
            load_from_dir(dir.path(), &FIXTURE).unwrap().unwrap(),
            b"abcd"
        );

        let nested = tempfile::tempdir().unwrap();
        let chip_dir = nested.path().join(FIXTURE.chip);
        fs::create_dir_all(&chip_dir).unwrap();
        fs::write(chip_dir.join(FIXTURE.file_name()), b"abcd").unwrap();
        assert_eq!(
            load_from_dir(nested.path(), &FIXTURE).unwrap().unwrap(),
            b"abcd"
        );
    }

    #[test]
    fn a_wrong_file_is_an_error_naming_the_path_not_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FIXTURE.file_name());
        fs::write(&path, b"nope").unwrap();

        let err = load_from_dir(dir.path(), &FIXTURE)
            .expect("a present file must not read as a miss")
            .expect_err("wrong bytes must fail");
        assert!(err.contains("SHA-256 mismatch"), "{err}");
        assert!(err.contains(&path.display().to_string()), "{err}");
    }

    #[test]
    fn cache_layout_is_per_chip() {
        let path = cache_path(Path::new("/cache"), &FIXTURE);
        assert!(
            path.ends_with("testchip/ram-loader-testchip-9.9.9.bin"),
            "{path:?}"
        );
    }

    #[test]
    fn storing_in_the_cache_creates_the_chip_dir_and_leaves_no_temp_file() {
        let root = tempfile::tempdir().unwrap();
        store_in_cache(root.path(), &FIXTURE, b"abcd");

        assert_eq!(
            load_from_dir(root.path(), &FIXTURE).unwrap().unwrap(),
            b"abcd"
        );
        let strays: Vec<_> = fs::read_dir(root.path().join(FIXTURE.chip))
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
            .filter(|n| n.contains("tmp"))
            .collect();
        assert!(strays.is_empty(), "temp file left behind: {strays:?}");
    }

    #[test]
    fn entry_url_picks_the_pinned_chip_and_version() {
        let body = r#"{"loaders":[
            {"chip":"testchip","version":"1.0.0","url":"https://x/old.bin","sha256":"aa","size":1},
            {"chip":"testchip","version":"9.9.9","url":"https://x/new.bin","sha256":"88d4266fd4e6338d13b845fcf289579d209c897823b9217da3e161936f031589","size":4},
            {"chip":"other","version":"9.9.9","url":"https://x/other.bin","sha256":"bb","size":1}
        ]}"#;
        assert_eq!(entry_url(body, &FIXTURE).unwrap(), "https://x/new.bin");
    }

    #[test]
    fn entry_url_reports_a_version_the_manifest_does_not_carry() {
        let body = r#"{"loaders":[{"chip":"testchip","version":"1.0.0","url":"u","sha256":"aa","size":1}]}"#;
        let err = entry_url(body, &FIXTURE).expect_err("missing version must fail");
        assert!(err.contains("no testchip version 9.9.9"), "{err}");
    }

    #[test]
    fn entry_url_rejects_a_malformed_manifest() {
        let err = entry_url("not json", &FIXTURE).expect_err("garbage must fail");
        assert!(err.contains("malformed manifest"), "{err}");
    }

    #[test]
    fn the_unavailable_message_names_the_file_and_the_escape_hatch() {
        let msg = unavailable(&FIXTURE, "every mirror failed");
        assert!(msg.contains("ram-loader-testchip-9.9.9.bin"), "{msg}");
        assert!(msg.contains(OFFLINE_DIR_ENV), "{msg}");
        assert!(msg.contains("every mirror failed"), "{msg}");
    }

    /// Every mirror must serve the same manifest name from the same tag, and must be a
    /// host `src-tauri/src/updater.rs` already allows the app to reach.
    #[test]
    fn manifest_mirrors_are_https_and_named_consistently() {
        assert_eq!(MANIFEST_MIRRORS.len(), 3);
        for url in MANIFEST_MIRRORS {
            assert!(url.starts_with("https://"), "{url}");
            assert!(url.ends_with("/ram-loader.json"), "{url}");
        }
    }
}
