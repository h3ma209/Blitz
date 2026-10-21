//! App update checking and SHA-256 verified installer download against GitHub Releases API.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GITHUB_REPO: &str = "h3ma209/Blitz";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub release_url: String,
    pub release_name: String,
    pub has_update: bool,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    name: Option<String>,
    draft: Option<bool>,
    prerelease: Option<bool>,
}

/// Check if a newer version is available on GitHub Releases.
/// Runs in background thread; returns `Some(UpdateInfo)` if check succeeds.
pub fn check_for_updates() -> Option<UpdateInfo> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("Blitz/{CURRENT_VERSION}"))
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;

    let resp = client.get(&url).send().ok()?;
    if !resp.status().is_success() {
        return None;
    }

    let release: GitHubRelease = resp.json().ok()?;
    if release.draft.unwrap_or(false) || release.prerelease.unwrap_or(false) {
        return None;
    }

    let latest_tag = release.tag_name.trim();
    let latest_version = latest_tag.strip_prefix('v').unwrap_or(latest_tag).to_string();
    let current_version = CURRENT_VERSION.to_string();

    let has_update = is_newer_version(&current_version, &latest_version);

    Some(UpdateInfo {
        current_version,
        latest_version,
        release_url: release.html_url,
        release_name: release.name.unwrap_or_else(|| latest_tag.to_string()),
    has_update,
    })
}

/// Compare semver components (e.g. "0.1.0" vs "0.2.0" or "0.1.1").
pub fn is_newer_version(current: &str, latest: &str) -> bool {
    let parse = |v: &str| -> Vec<u32> {
        v.split('.')
            .map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
            .filter_map(|s| s.parse::<u32>().ok())
            .collect()
    };

    let c = parse(current);
    let l = parse(latest);

    for (cur, lat) in c.iter().zip(l.iter()) {
        if lat > cur {
            return true;
        }
        if lat < cur {
            return false;
        }
    }
    l.len() > c.len()
}

/// Return the canonical release asset filename for the current OS/architecture.
pub fn platform_asset_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Blitz-macos-arm64.dmg"
    }
    #[cfg(target_os = "windows")]
    {
        "Blitz-windows-x86_64-setup.exe"
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        "Blitz-linux-x86_64.tar.gz"
    }
}

/// Verify `bytes` against the expected SHA-256 entry for `asset_name` inside a `SHA256SUMS` manifest.
pub fn verify_sha256_manifest(
    sums_text: &str,
    asset_name: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let expected_hex = sums_text
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            let mut parts = trimmed.split_whitespace();
            let hash = parts.next()?;
            let file = parts.next()?.trim_start_matches('*').trim_start_matches("./");
            let file_base = Path::new(file)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(file);
            if file_base == asset_name {
                Some(hash.to_ascii_lowercase())
            } else {
                None
            }
        })
        .next()
        .ok_or_else(|| format!("asset '{asset_name}' not found in SHA256SUMS"))?;

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let actual_hex = hex::encode(hasher.finalize());

    if actual_hex.eq_ignore_ascii_case(&expected_hex) {
        Ok(())
    } else {
        Err(format!(
            "SHA-256 mismatch for {asset_name}: expected {expected_hex}, got {actual_hex}"
        ))
    }
}

/// Download the platform installer for `info.latest_version` into the system temp directory
/// and verify its SHA-256 checksum against the release's `SHA256SUMS` asset when present.
pub fn download_and_verify_update(info: &UpdateInfo) -> Result<PathBuf, String> {
    let asset = platform_asset_name();
    let tag = if info.latest_version.starts_with('v') {
        info.latest_version.clone()
    } else {
        format!("v{}", info.latest_version)
    };
    let base_url = format!("https://github.com/{GITHUB_REPO}/releases/download/{tag}");
    let asset_url = format!("{base_url}/{asset}");
    let sums_url = format!("{base_url}/SHA256SUMS");

    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("Blitz/{CURRENT_VERSION}"))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    let resp = client
        .get(&asset_url)
        .send()
        .map_err(|e| format!("download failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("asset HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .map_err(|e| format!("read asset bytes: {e}"))?;

    // Check SHA256SUMS if published on this release
    if let Ok(sums_resp) = client.get(&sums_url).send() {
        if sums_resp.status().is_success() {
            if let Ok(sums_text) = sums_resp.text() {
                verify_sha256_manifest(&sums_text, asset, &bytes)?;
            }
        }
    }

    let out_dir = std::env::temp_dir().join(format!("blitz-update-{tag}"));
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("temp dir: {e}"))?;
    let out_path = out_dir.join(asset);
    std::fs::write(&out_path, &bytes).map_err(|e| format!("write installer: {e}"))?;
    Ok(out_path)
}

/// Open or reveal the downloaded installer package using the OS default handler.
pub fn reveal_or_open_installer(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(parent) = path.parent() {
            let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
        }
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn();
    }
}

/// Open arbitrary URL in default browser.
pub fn open_browser_url(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd").args(["/C", "start", url]).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison() {
        assert!(is_newer_version("0.1.0", "0.1.1"));
        assert!(is_newer_version("0.1.0", "0.2.0"));
        assert!(is_newer_version("0.1.0", "1.0.0"));
        assert!(!is_newer_version("0.1.0", "0.1.0"));
        assert!(!is_newer_version("0.2.0", "0.1.9"));
        assert!(!is_newer_version("1.0.0", "0.9.9"));
        assert!(is_newer_version("0.1.0", "0.1.0.1"));
    }

    #[test]
    fn sha256_manifest_verification() {
        let payload = b"blitz-binary-payload-v0.2.0";
        let mut hasher = Sha256::new();
        hasher.update(payload);
        let digest = hex::encode(hasher.finalize());

        let manifest = format!(
            "0000000000000000000000000000000000000000000000000000000000000000  other.tar.gz\n\
             {digest}  dist/Blitz-macos-arm64.dmg\n"
        );

        assert!(verify_sha256_manifest(&manifest, "Blitz-macos-arm64.dmg", payload).is_ok());
        assert!(verify_sha256_manifest(&manifest, "Blitz-macos-arm64.dmg", b"tampered").is_err());
        assert!(verify_sha256_manifest(&manifest, "missing.dmg", payload).is_err());
    }
}
