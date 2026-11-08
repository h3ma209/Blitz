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
        "blitz-windows-x86_64.zip"
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        "blitz-linux-x86_64.tar.gz"
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

/// Find the running `.app` bundle directory if running on macOS as a bundle.
#[allow(dead_code)]
pub fn current_app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    for ancestor in exe.ancestors() {
        if ancestor.extension().and_then(|s| s.to_str()) == Some("app") {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

/// Silently install the downloaded update and relaunch the updated application,
/// without requiring the user to manually drag into Applications or open a disk image.
pub fn install_update_and_restart(installer_path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        install_and_restart_macos(installer_path)
    }
    #[cfg(target_os = "linux")]
    {
        install_and_restart_linux(installer_path)
    }
    #[cfg(target_os = "windows")]
    {
        install_and_restart_windows(installer_path)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        reveal_or_open_installer(installer_path);
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn install_and_restart_macos(installer_path: &Path) -> Result<(), String> {
    let pid = std::process::id();
    let is_dmg = installer_path
        .extension()
        .and_then(|s| s.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("dmg"))
        .unwrap_or(false);

    let stage_dir = std::env::temp_dir().join(format!("blitz-stage-{pid}"));
    let _ = std::fs::remove_dir_all(&stage_dir);
    std::fs::create_dir_all(&stage_dir).map_err(|e| format!("create stage dir: {e}"))?;

    let staged_app = stage_dir.join("Blitz.app");

    if is_dmg {
        let mount_dir = std::env::temp_dir().join(format!("blitz-mnt-{pid}"));
        let _ = std::fs::remove_dir_all(&mount_dir);
        std::fs::create_dir_all(&mount_dir).map_err(|e| format!("create mount dir: {e}"))?;

        // Attach DMG silently without opening any Finder window
        let attach_status = std::process::Command::new("hdiutil")
            .args([
                "attach",
                installer_path.to_str().ok_or("invalid installer path")?,
                "-nobrowse",
                "-noautoopen",
                "-readonly",
                "-mountpoint",
                mount_dir.to_str().ok_or("invalid mount dir")?,
            ])
            .status()
            .map_err(|e| format!("hdiutil attach failed: {e}"))?;

        if !attach_status.success() {
            let _ = std::fs::remove_dir_all(&mount_dir);
            return Err(format!("hdiutil attach exited with {attach_status}"));
        }

        let app_source = mount_dir.join("Blitz.app");
        if !app_source.exists() {
            let _ = std::process::Command::new("hdiutil")
                .args(["detach", mount_dir.to_str().unwrap(), "-force"])
                .status();
            let _ = std::fs::remove_dir_all(&mount_dir);
            return Err("Blitz.app not found inside DMG".into());
        }

        // Copy Blitz.app out of DMG into local staging directory
        let cp_res = std::process::Command::new("cp")
            .args(["-R", app_source.to_str().unwrap(), staged_app.to_str().unwrap()])
            .status();

        // Detach DMG immediately
        let _ = std::process::Command::new("hdiutil")
            .args(["detach", mount_dir.to_str().unwrap(), "-force"])
            .status();
        let _ = std::fs::remove_dir_all(&mount_dir);

        let cp_status = cp_res.map_err(|e| format!("cp Blitz.app failed: {e}"))?;
        if !cp_status.success() {
            return Err(format!("cp Blitz.app exited with {cp_status}"));
        }
    } else {
        let status = std::process::Command::new("tar")
            .args(["-xzf", installer_path.to_str().unwrap(), "-C", stage_dir.to_str().unwrap()])
            .status()
            .map_err(|e| format!("tar extract failed: {e}"))?;
        if !status.success() {
            return Err(format!("tar exited with {status}"));
        }
    }

    // Strip quarantine flag on staged app
    let _ = std::process::Command::new("xattr")
        .args(["-cr", staged_app.to_str().unwrap()])
        .status();

    // Determine target location: existing running bundle or /Applications/Blitz.app
    let target_app = current_app_bundle().unwrap_or_else(|| {
        let sys = PathBuf::from("/Applications/Blitz.app");
        let user = dirs::home_dir()
            .map(|h| h.join("Applications/Blitz.app"))
            .unwrap_or_else(|| sys.clone());
        if user.exists() {
            user
        } else {
            sys
        }
    });

    // Write supervisor restart script that waits for old process to exit, swaps app bundle, and launches new one
    let script_path = std::env::temp_dir().join(format!("blitz-updater-{pid}.sh"));
    let script_content = format!(
        r#"#!/bin/bash
PID={pid}
STAGED="{staged}"
TARGET="{target}"
BACKUP="{target}.old.$$"

for i in $(seq 1 100); do
    if ! kill -0 "$PID" 2>/dev/null; then
        break
    fi
    sleep 0.1
done

if [ -d "$TARGET" ]; then
    mv "$TARGET" "$BACKUP" 2>/dev/null || rm -rf "$TARGET" 2>/dev/null
fi

cp -R "$STAGED" "$TARGET" 2>/dev/null || rsync -a "$STAGED/" "$TARGET/" 2>/dev/null
xattr -cr "$TARGET" 2>/dev/null || true

rm -rf "$BACKUP" 2>/dev/null || true
rm -rf "$STAGED" 2>/dev/null || true
rm -f "$0" 2>/dev/null || true

open -a "$TARGET" 2>/dev/null || open "$TARGET" 2>/dev/null
"#,
        pid = pid,
        staged = staged_app.display(),
        target = target_app.display()
    );

    std::fs::write(&script_path, script_content).map_err(|e| format!("write updater script: {e}"))?;

    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755));

    // Spawn script in background detached
    std::process::Command::new("/bin/bash")
        .arg(&script_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn updater script: {e}"))?;

    // Exit old process cleanly so supervisor script proceeds
    std::process::exit(0);
}

#[cfg(target_os = "linux")]
fn install_and_restart_linux(installer_path: &Path) -> Result<(), String> {
    let pid = std::process::id();
    let stage_dir = std::env::temp_dir().join(format!("blitz-stage-{pid}"));
    let _ = std::fs::remove_dir_all(&stage_dir);
    std::fs::create_dir_all(&stage_dir).map_err(|e| format!("create stage dir: {e}"))?;

    let status = std::process::Command::new("tar")
        .args(["-xzf", installer_path.to_str().unwrap(), "-C", stage_dir.to_str().unwrap()])
        .status()
        .map_err(|e| format!("tar extract failed: {e}"))?;
    if !status.success() {
        return Err(format!("tar exited with {status}"));
    }

    let staged_bin = stage_dir.join("blitz");
    let target_bin = std::env::current_exe().unwrap_or_else(|_| {
        dirs::home_dir().map(|h| h.join(".local/bin/blitz")).unwrap_or_else(|| PathBuf::from("/usr/local/bin/blitz"))
    });

    let script_path = std::env::temp_dir().join(format!("blitz-updater-{pid}.sh"));
    let script_content = format!(
        r#"#!/bin/bash
PID={pid}
STAGED="{staged}"
TARGET="{target}"

for i in $(seq 1 100); do
    if ! kill -0 "$PID" 2>/dev/null; then
        break
    fi
    sleep 0.1
done

cp "$STAGED" "$TARGET" 2>/dev/null
chmod +x "$TARGET" 2>/dev/null
rm -rf "{stage_dir}" 2>/dev/null
rm -f "$0" 2>/dev/null

nohup "$TARGET" >/dev/null 2>&1 &
"#,
        pid = pid,
        staged = staged_bin.display(),
        target = target_bin.display(),
        stage_dir = stage_dir.display()
    );

    std::fs::write(&script_path, script_content).map_err(|e| format!("write updater script: {e}"))?;

    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755));

    std::process::Command::new("/bin/bash")
        .arg(&script_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn updater script: {e}"))?;

    std::process::exit(0);
}

#[cfg(target_os = "windows")]
fn install_and_restart_windows(installer_path: &Path) -> Result<(), String> {
    let pid = std::process::id();
    let stage_dir = std::env::temp_dir().join(format!("blitz-stage-{pid}"));
    let _ = std::fs::remove_dir_all(&stage_dir);
    std::fs::create_dir_all(&stage_dir).map_err(|e| format!("create stage dir: {e}"))?;

    let ps_cmd = format!(
        "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
        installer_path.display(),
        stage_dir.display()
    );
    let status = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &ps_cmd])
        .status()
        .map_err(|e| format!("unzip failed: {e}"))?;
    if !status.success() {
        return Err(format!("Expand-Archive exited with {status}"));
    }

    let staged_exe = stage_dir.join("blitz.exe");
    let target_exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;

    let bat_path = std::env::temp_dir().join(format!("blitz-updater-{pid}.bat"));
    let bat_content = format!(
        r#"@echo off
set PID={pid}
set STAGED="{staged}"
set TARGET="{target}"

:waitloop
tasklist /FI "PID eq %PID%" 2>NUL | find /I "%PID%" >NUL
if not errorlevel 1 (
    timeout /T 1 /NOBREAK >NUL
    goto waitloop
)

copy /Y "%STAGED%" "%TARGET%" >NUL
start "" "%TARGET%"
del "%~f0" & exit
"#,
        pid = pid,
        staged = staged_exe.display(),
        target = target_exe.display()
    );

    std::fs::write(&bat_path, bat_content).map_err(|e| format!("write bat: {e}"))?;

    std::process::Command::new("cmd.exe")
        .args(["/C", "start", "", bat_path.to_str().unwrap()])
        .spawn()
        .map_err(|e| format!("spawn updater: {e}"))?;

    std::process::exit(0);
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
    fn test_platform_asset_name_non_empty() {
        let name = platform_asset_name();
        assert!(!name.is_empty());
        #[cfg(target_os = "macos")]
        assert!(name.ends_with(".dmg") || name.ends_with(".tar.gz"));
        #[cfg(target_os = "linux")]
        assert!(name.ends_with(".tar.gz"));
        #[cfg(target_os = "windows")]
        assert!(name.ends_with(".zip") || name.ends_with(".exe"));
    }

    #[test]
    fn test_current_app_bundle_helper() {
        // When running `cargo test`, binary is inside `target/debug/deps`, not a .app bundle
        let bundle = current_app_bundle();
        if let Some(b) = bundle {
            assert!(b.extension().and_then(|s| s.to_str()) == Some("app"));
        }
    }
}
