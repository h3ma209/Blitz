//! App update checking against GitHub Releases API.

use serde::Deserialize;

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
        .timeout(std::time::Duration::from_secs(5))
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
}
