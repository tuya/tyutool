//! Self-update and auth-firmware download.
//!
//! Everything here reaches the network on behalf of the renderer, so the host
//! allowlist (`assert_allowed_fetch_url`) and the SHA-256 verification in
//! `download_auth_firmware` are the security-relevant parts — they have their
//! own tests at the bottom of this file.

use std::sync::Mutex as StdMutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::sha256_hex;

/// In-app update staged by `update_check`, downloaded by `update_download`,
/// consumed by `update_install`.
pub(crate) struct UpdateState {
    pub pending: StdMutex<Option<PendingUpdate>>,
}

pub(crate) struct PendingUpdate {
    update: tauri_plugin_updater::Update,
    bytes: Option<Vec<u8>>,
}

/// Hosts the Tauri backend is allowed to fetch from on behalf of the renderer
/// (fetch_url) and to download auth firmware from (download_auth_firmware).
/// These mirror the legitimate update/auth-firmware sources enumerated in
/// `update_endpoint`, `tauri.conf.json` `plugins.updater.endpoints`, and
/// `src/features/batch-flash-auth/auth-firmware.ts` AUTH_FIRMWARE_SOURCES.
/// Keep in sync when adding a source.
const ALLOWED_FETCH_HOSTS: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com", // GitHub release asset CDN redirect target
    "gitee.com",
    "airtake-public-data-1254153901.cos.ap-shanghai.myqcloud.com",
];

/// Validate that `url` is https and points at an allowlisted host. Prevents the
/// Tauri bridge from being used as an open SSRF proxy (e.g. fetching cloud
/// metadata endpoints) by a compromised renderer / XSS.
fn assert_allowed_fetch_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| {
        log::warn!("[Update] rejected malformed URL: {}", e);
        format!("invalid URL: {e}")
    })?;
    if parsed.scheme() != "https" {
        log::warn!("[Update] rejected non-https scheme: {}", parsed.scheme());
        return Err(format!(
            "only https URLs are allowed, got {}",
            parsed.scheme()
        ));
    }
    let host = parsed.host_str().unwrap_or("");
    if !ALLOWED_FETCH_HOSTS.contains(&host) {
        log::warn!("[Update] rejected host not in allowlist: {}", host);
        return Err(format!("host '{host}' is not allowed"));
    }
    Ok(())
}

/// Fetch a URL and return body as string. Used by the frontend update checker
/// to bypass WebView CSP restrictions on cross-origin fetch.
#[tauri::command]
pub(crate) async fn fetch_url(url: String, timeout_ms: u64) -> Result<String, String> {
    log::info!("[Update] fetch_url: url={}, timeout_ms={}", url, timeout_ms);
    assert_allowed_fetch_url(&url)?;
    // Bound the renderer-supplied timeout so a compromised page can't pin a
    // connection open indefinitely. 30 s is ample for a small JSON manifest.
    let capped_timeout = timeout_ms.min(30_000);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(capped_timeout))
        // Limit redirects so a malicious redirect chain can't be used to reach
        // a non-allowlisted host via the follow; assert_allowed_fetch_url only
        // inspects the initial URL.
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .map_err(|e| {
            log::error!("[Update] fetch_url: failed to build client: {}", e);
            e.to_string()
        })?;
    let resp = client.get(&url).send().await.map_err(|e| {
        log::warn!("[Update] fetch_url: request failed for {}: {}", url, e);
        e.to_string()
    })?;
    let status = resp.status();
    log::info!("[Update] fetch_url: response status={}", status);
    if !status.is_success() {
        log::warn!("[Update] fetch_url: HTTP error {}", status);
        return Err(format!("HTTP {}", status));
    }
    // Cap the body so a malicious/buggy source can't exhaust memory. The update
    // manifest is a small JSON document; 8 MiB is a generous ceiling.
    const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;
    let body = resp.text().await.map_err(|e| {
        log::warn!("[Update] fetch_url: failed to read body: {}", e);
        e.to_string()
    })?;
    if body.len() > MAX_MANIFEST_BYTES {
        log::warn!(
            "[Update] fetch_url: body too large: {} > {}",
            body.len(),
            MAX_MANIFEST_BYTES
        );
        return Err(format!(
            "response body too large ({} > {} bytes)",
            body.len(),
            MAX_MANIFEST_BYTES
        ));
    }
    log::info!("[Update] fetch_url: body length={}", body.len());
    Ok(body)
}

/// Update-manifest endpoint per source id.
/// Mirrors UPDATE_SOURCES in src/features/settings/update-sources.ts and the
/// fallback `plugins.updater.endpoints` list in tauri.conf.json — keep in sync.
fn update_endpoint(source: &str) -> Option<&'static str> {
    match source {
        "github" => Some("https://github.com/tuya/tyutool/releases/latest/download/latest.json"),
        // "pruduct" is the actual key spelling on the Tuya OSS bucket — do not fix.
        "tuya" => Some(
            "https://airtake-public-data-1254153901.cos.ap-shanghai.myqcloud.com/smart/embed/pruduct/tyutool/latest/release.json",
        ),
        _ => None,
    }
}

// Mirrored by UpdateCheckReply in src/features/settings/in-app-updater.ts
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateCheckReply {
    available: bool,
    version: String,
    current_version: String,
    date: Option<String>,
    body: Option<String>,
}

// Mirrored by UpdateDownloadEvent in src/features/settings/in-app-updater.ts
#[derive(Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all_fields = "camelCase")]
enum UpdateDownloadEvent {
    Started { content_length: Option<u64> },
    Progress { chunk_length: usize },
    Finished,
}

/// Check for an update against the manifest endpoint of the given source
/// ("github" or "tuya"), staging the result for `update_download`. Unlike the
/// plugin's JS `check()`, which always walks the static endpoint list in
/// tauri.conf.json (GitHub first), this honors the source the user picked.
#[tauri::command]
pub(crate) async fn update_check(
    app: AppHandle,
    state: State<'_, UpdateState>,
    source: String,
) -> Result<UpdateCheckReply, String> {
    use tauri_plugin_updater::UpdaterExt;

    let endpoint =
        update_endpoint(&source).ok_or_else(|| format!("unknown update source: {}", source))?;
    log::info!(
        "[Update] update_check: source={}, endpoint={}",
        source,
        endpoint
    );
    let url = endpoint.parse::<tauri::Url>().map_err(|e| e.to_string())?;
    let updater = app
        .updater_builder()
        .endpoints(vec![url])
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| {
        log::warn!("[Update] update_check failed for source={}: {}", source, e);
        e.to_string()
    })?;
    match update {
        Some(update) => {
            log::info!(
                "[Update] update_check: available={} -> {} (source={}, download_url={})",
                update.current_version,
                update.version,
                source,
                update.download_url
            );
            let reply = UpdateCheckReply {
                available: true,
                version: update.version.clone(),
                current_version: update.current_version.clone(),
                date: update.date.map(|d| d.to_string()),
                body: update.body.clone(),
            };
            *state.pending.lock().unwrap() = Some(PendingUpdate {
                update,
                bytes: None,
            });
            Ok(reply)
        }
        None => {
            log::info!(
                "[Update] update_check: already up to date (source={})",
                source
            );
            *state.pending.lock().unwrap() = None;
            Ok(UpdateCheckReply {
                available: false,
                version: String::new(),
                current_version: app.package_info().version.to_string(),
                date: None,
                body: None,
            })
        }
    }
}

/// Download the update staged by `update_check`, emitting
/// `update-download-progress` events, and hold the bytes for `update_install`.
#[tauri::command]
pub(crate) async fn update_download(
    app: AppHandle,
    state: State<'_, UpdateState>,
) -> Result<(), String> {
    let update = state
        .pending
        .lock()
        .unwrap()
        .as_ref()
        .map(|p| p.update.clone())
        .ok_or("no pending update; call update_check first")?;

    log::info!(
        "[Update] update_download: starting, url={}",
        update.download_url
    );
    let mut started = false;
    let bytes = update
        .download(
            |chunk_length, content_length| {
                if !started {
                    started = true;
                    let _ = app.emit(
                        "update-download-progress",
                        UpdateDownloadEvent::Started { content_length },
                    );
                }
                let _ = app.emit(
                    "update-download-progress",
                    UpdateDownloadEvent::Progress { chunk_length },
                );
            },
            || {
                let _ = app.emit("update-download-progress", UpdateDownloadEvent::Finished);
            },
        )
        .await
        .map_err(|e| {
            log::error!("[Update] update_download failed: {}", e);
            e.to_string()
        })?;
    log::info!("[Update] update_download: downloaded {} bytes", bytes.len());
    match state.pending.lock().unwrap().as_mut() {
        Some(pending) => {
            pending.bytes = Some(bytes);
            Ok(())
        }
        None => Err("pending update was cleared during download".to_string()),
    }
}

/// Install the update downloaded by `update_download`. On Windows this launches
/// the installer and exits; the frontend relaunches via plugin-process after.
/// The staged update is kept on failure (e.g. UAC denied) so install can be retried.
#[tauri::command]
pub(crate) async fn update_install(state: State<'_, UpdateState>) -> Result<(), String> {
    let pending = state
        .pending
        .lock()
        .unwrap()
        .take()
        .ok_or("no pending update; call update_check first")?;
    let Some(bytes) = pending.bytes.as_ref() else {
        *state.pending.lock().unwrap() = Some(pending);
        return Err("update not downloaded; call update_download first".to_string());
    };
    log::info!("[Update] update_install: installing {} bytes", bytes.len());
    match pending.update.install(bytes) {
        Ok(()) => Ok(()),
        Err(e) => {
            log::error!("[Update] update_install failed: {}", e);
            let msg = e.to_string();
            *state.pending.lock().unwrap() = Some(pending);
            Err(msg)
        }
    }
}

/// Hex-encoded SHA-256 of the given bytes (lowercase).
/// Derive a path-safe cache filename for an auth-firmware version.
/// Any character outside [A-Za-z0-9._-] is replaced with '_' to prevent traversal.
fn auth_firmware_filename(version: &str) -> String {
    let safe: String = version
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("auth-fw-{}.bin", safe)
}

/// Download an authorization firmware binary to the app cache dir, verifying its
/// SHA-256. Idempotent: if a cached file with the matching hash already exists,
/// it is reused without re-downloading. Returns the absolute local path.
#[tauri::command]
pub(crate) async fn download_auth_firmware(
    app: AppHandle,
    url: String,
    sha256: String,
    version: String,
) -> Result<String, String> {
    log::info!(
        "[AuthFw] download_auth_firmware: version={}, url={}",
        version,
        url
    );
    assert_allowed_fetch_url(&url)?;
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("auth-firmware");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(auth_firmware_filename(&version));
    let expected = sha256.to_lowercase();

    // Idempotent cache hit: reuse existing file when its hash matches.
    if dest.exists() {
        if let Ok(existing) = std::fs::read(&dest) {
            if sha256_hex(&existing) == expected {
                log::info!("[AuthFw] cache hit: {}", dest.display());
                return Ok(dest.to_string_lossy().into_owned());
            }
        }
    }

    let download_start = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().await.map_err(|e| {
        log::warn!(
            "[AuthFw] download request failed: version={} err={}",
            version,
            e
        );
        e.to_string()
    })?;
    if !resp.status().is_success() {
        let status = resp.status();
        log::warn!(
            "[AuthFw] download HTTP error: version={} status={}",
            version,
            status
        );
        return Err(format!("HTTP {}", status));
    }
    let bytes_total = resp.content_length();
    let mut bytes_vec: Vec<u8> = Vec::new();
    // Cap the download so a malicious/buggy source can't exhaust memory before
    // the SHA-256 check runs. Auth firmware binaries are small; 16 MiB is a
    // generous ceiling.
    const MAX_AUTH_FW_BYTES: usize = 16 * 1024 * 1024;
    let mut resp = resp;
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        bytes_vec.extend_from_slice(&chunk);
        if bytes_vec.len() > MAX_AUTH_FW_BYTES {
            log::warn!(
                "[AuthFw] download exceeded size cap: version={} bytes={} > {}",
                version,
                bytes_vec.len(),
                MAX_AUTH_FW_BYTES
            );
            return Err(format!(
                "downloaded firmware exceeds size cap ({} > {} bytes)",
                bytes_vec.len(),
                MAX_AUTH_FW_BYTES
            ));
        }
        if let Some(total) = bytes_total {
            let _ = app.emit(
                "auth-firmware-download-progress",
                serde_json::json!({
                    "bytesDone": bytes_vec.len(),
                    "bytesTotal": total
                }),
            );
        }
    }
    let actual = sha256_hex(&bytes_vec);
    if actual != expected {
        log::warn!(
            "[AuthFw] download SHA256 mismatch: version={} expected={} got={}",
            version,
            expected,
            actual
        );
        return Err(format!(
            "SHA-256 mismatch: expected {}, got {}",
            expected, actual
        ));
    }
    std::fs::write(&dest, &bytes_vec).map_err(|e| e.to_string())?;
    log::info!(
        "[AuthFw] downloaded {} bytes -> {} ({:.1}s)",
        bytes_vec.len(),
        dest.display(),
        download_start.elapsed().as_secs_f64(),
    );
    Ok(dest.to_string_lossy().into_owned())
}

#[cfg(test)]
mod update_endpoint_tests {
    use super::update_endpoint;

    #[test]
    fn update_endpoint_maps_known_sources_and_rejects_unknown() {
        assert_eq!(
            update_endpoint("github"),
            Some("https://github.com/tuya/tyutool/releases/latest/download/latest.json")
        );
        // "pruduct" is the actual key spelling on the Tuya OSS bucket — do not fix.
        assert_eq!(
            update_endpoint("tuya"),
            Some(
                "https://airtake-public-data-1254153901.cos.ap-shanghai.myqcloud.com/smart/embed/pruduct/tyutool/latest/release.json"
            )
        );
        assert_eq!(update_endpoint("gitee"), None);
        assert_eq!(update_endpoint(""), None);
    }
}

#[cfg(test)]
mod auth_firmware_tests {
    use super::{auth_firmware_filename, sha256_hex};

    #[test]
    fn sha256_hex_matches_known_vector() {
        // SHA-256 of the empty input.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // SHA-256 of "abc".
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn auth_firmware_filename_sanitizes_path_separators() {
        assert_eq!(auth_firmware_filename("1.2.3"), "auth-fw-1.2.3.bin");
        assert_eq!(
            auth_firmware_filename("../etc/passwd"),
            "auth-fw-.._etc_passwd.bin"
        );
        assert_eq!(auth_firmware_filename("a/b\\c"), "auth-fw-a_b_c.bin");
    }
}

#[cfg(test)]
mod fetch_allowlist_tests {
    use super::assert_allowed_fetch_url;

    #[test]
    fn allows_github_gitee_and_tuya_oss() {
        assert!(assert_allowed_fetch_url(
            "https://github.com/tuya/tyutool/releases/latest/download/latest.json"
        )
        .is_ok());
        assert!(assert_allowed_fetch_url(
            "https://gitee.com/tuya-open/tyutool/releases/download/auth-firmware/auth-firmware.json"
        )
        .is_ok());
        assert!(assert_allowed_fetch_url(
            "https://airtake-public-data-1254153901.cos.ap-shanghai.myqcloud.com/smart/embed/pruduct/tyutool/latest/release.json"
        )
        .is_ok());
        assert!(assert_allowed_fetch_url(
            "https://objects.githubusercontent.com/tyutool/auth-fw-1.0.0.bin"
        )
        .is_ok());
    }

    #[test]
    fn rejects_non_https_scheme() {
        let err =
            assert_allowed_fetch_url("http://github.com/tuya/tyutool/latest.json").unwrap_err();
        assert!(err.contains("https"), "got: {err}");
        // SSRF: file:// and cloud-metadata http must be refused.
        assert!(assert_allowed_fetch_url("file:///etc/passwd").is_err());
        assert!(assert_allowed_fetch_url("http://169.254.169.254/latest/meta-data/").is_err());
    }

    #[test]
    fn rejects_unlisted_host() {
        let err = assert_allowed_fetch_url("https://evil.example.com/x.json").unwrap_err();
        assert!(err.contains("evil.example.com"), "got: {err}");
    }

    #[test]
    fn rejects_malformed_url() {
        assert!(assert_allowed_fetch_url("not a url at all").is_err());
        assert!(assert_allowed_fetch_url("ht!tp://broken").is_err());
    }
}
