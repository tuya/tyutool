use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

use serde::Deserialize;
use sha2::{Digest, Sha256};

// ─── Manifest types ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CliPlatformEntry {
    url: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct LatestJson {
    version: String,
    #[serde(default)]
    cli: std::collections::HashMap<String, CliPlatformEntry>,
}

// ─── Source endpoints ─────────────────────────────────────────────────────────

const GITHUB_LATEST_JSON: &str =
    "https://github.com/tuya/tyutool/releases/latest/download/latest.json";
const GITEE_LATEST_JSON: &str =
    "https://gitee.com/tuya-open/tyutool/releases/download/latest/latest.json";

// ─── Current version (injected at compile time via Cargo) ─────────────────────

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

// ─── Platform key detection ───────────────────────────────────────────────────

fn platform_key() -> Option<&'static str> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return Some("linux-x86_64");
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return Some("linux-aarch64");
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return Some("darwin-x86_64");
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return Some("darwin-aarch64");
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return Some("windows-x86_64");
    #[allow(unreachable_code)]
    None
}

fn is_windows() -> bool {
    cfg!(target_os = "windows")
}

// ─── Semver comparison ────────────────────────────────────────────────────────

fn is_newer(remote: &str, local: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect()
    };
    let r = parse(remote);
    let l = parse(local);
    for i in 0..3 {
        let rv = r.get(i).copied().unwrap_or(0);
        let lv = l.get(i).copied().unwrap_or(0);
        if rv != lv {
            return rv > lv;
        }
    }
    false
}

// ─── HTTP fetch ───────────────────────────────────────────────────────────────

fn fetch_latest_json(source_url: &str) -> Result<LatestJson, Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()?;
    let resp = client.get(source_url).send()?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()).into());
    }
    let json: LatestJson = resp.json()?;
    Ok(json)
}

fn download_bytes(url: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let mut resp = client.get(url).send()?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()).into());
    }
    let mut buf = Vec::new();
    resp.read_to_end(&mut buf)?;
    Ok(buf)
}

// ─── SHA256 verification ──────────────────────────────────────────────────────

fn verify_sha256(data: &[u8], expected_hex: &str) -> bool {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let hex_str = hex::encode(result);
    hex_str.eq_ignore_ascii_case(expected_hex)
}

// ─── Archive extraction ───────────────────────────────────────────────────────

fn extract_binary_from_tar_gz(data: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    let gz = GzDecoder::new(data);
    let mut archive = Archive::new(gz);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // Look for the CLI binary (tyutool_cli or tyutool_cli.exe)
        if name == "tyutool_cli" || name == "tyutool_cli.exe" || name == "tyutool" {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }
    Err("Binary not found in archive".into())
}

fn extract_binary_from_zip(data: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use std::io::Cursor;
    let cursor = Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        if name.ends_with("tyutool_cli.exe")
            || name.ends_with("tyutool_cli")
            || name.ends_with("tyutool.exe")
        {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }
    Err("Binary not found in zip archive".into())
}

// ─── Self-replace ─────────────────────────────────────────────────────────────

fn replace_self(new_binary: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
    // Write to temp file
    let tmp_path = if is_windows() {
        PathBuf::from(std::env::temp_dir()).join("tyutool_cli_new.exe")
    } else {
        PathBuf::from(std::env::temp_dir()).join("tyutool_cli_new")
    };

    let mut tmp_file = fs::File::create(&tmp_path)?;
    tmp_file.write_all(&new_binary)?;
    drop(tmp_file);

    // Set executable bit on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o755))?;
    }

    // Self-replace
    self_replace::self_replace(&tmp_path)?;

    // Clean up temp file
    let _ = fs::remove_file(&tmp_path);

    Ok(())
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Run the update command.
/// - `check_only`: only check version, don't download
/// - `source`: optional source override ("github" or "gitee")
pub fn run_update(
    check_only: bool,
    source: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Determine source URL(s) to try
    let urls: Vec<&str> = match source.as_deref() {
        Some("gitee") => vec![GITEE_LATEST_JSON],
        Some("github") | None => vec![GITHUB_LATEST_JSON, GITEE_LATEST_JSON],
        Some(s) => {
            return Err(format!("Unknown source '{}'. Use 'github' or 'gitee'.", s).into());
        }
    };

    eprintln!("Checking for updates...");

    let mut manifest: Option<LatestJson> = None;
    for url in &urls {
        match fetch_latest_json(url) {
            Ok(m) => {
                manifest = Some(m);
                break;
            }
            Err(e) => {
                eprintln!("  Source {} failed: {}", url, e);
            }
        }
    }

    let manifest = match manifest {
        Some(m) => m,
        None => return Err("All update sources failed. Check your network connection.".into()),
    };

    eprintln!("Latest version: v{}", manifest.version);
    eprintln!("Current version: v{}", CURRENT_VERSION);

    if !is_newer(&manifest.version, CURRENT_VERSION) {
        eprintln!("✓ Already on the latest version.");
        return Ok(());
    }

    eprintln!("New version available: v{}", manifest.version);

    if check_only {
        eprintln!("  Run 'tyutool update' to download and install.");
        return Ok(());
    }

    // Determine platform key
    let key = platform_key().ok_or("Unsupported platform for auto-update.")?;

    let entry = manifest
        .cli
        .get(key)
        .ok_or_else(|| format!("No CLI binary for platform '{}' in manifest.", key))?;

    eprintln!("Downloading {} ...", entry.url);
    let data = download_bytes(&entry.url)?;

    eprintln!("Verifying SHA256...");
    if !verify_sha256(&data, &entry.sha256) {
        return Err("SHA256 checksum mismatch! Download may be corrupted.".into());
    }

    eprintln!("Extracting binary...");
    let binary = if entry.url.ends_with(".zip") {
        extract_binary_from_zip(&data)?
    } else {
        extract_binary_from_tar_gz(&data)?
    };

    eprintln!("Replacing current binary...");
    replace_self(binary)?;

    eprintln!("✓ Updated to v{}", manifest.version);
    Ok(())
}
