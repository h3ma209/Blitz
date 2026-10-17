//! App + per-request HTTP settings (proxy, TLS, timeout, client certs).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Per-request send options (timeout / redirects / TLS verify).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestSettings {
    /// Request timeout in milliseconds (0 = no timeout).
    pub timeout_ms: u64,
    pub follow_redirects: bool,
    /// When false, skip TLS certificate verification (insecure).
    pub ssl_verify: bool,
}

impl Default for RequestSettings {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000,
            follow_redirects: true,
            ssl_verify: true,
        }
    }
}

/// Proxy mode for the HTTP client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    /// Use `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` env vars when set.
    #[default]
    Env,
    /// No proxy.
    None,
    /// Custom `host:port` (optional scheme; defaults to http).
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProxySettings {
    pub mode: ProxyMode,
    /// e.g. `127.0.0.1:8080` or `http://127.0.0.1:8080`
    pub host_port: String,
}

/// Client certificate stub — path persisted; wired when PEM identity available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ClientCertSettings {
    /// Path to PEM (cert + key) or PKCS#12 — TODO wire identity into reqwest.
    pub cert_path: String,
    /// Optional passphrase (prefer secret:// / vault later).
    pub passphrase: String,
    pub enabled: bool,
}

fn default_ui_font_size_px() -> u32 {
    13
}

fn default_editor_font_size_px() -> u32 {
    14
}

fn default_sidebar_width_px() -> u32 {
    268
}

fn default_request_pane_height_px() -> u32 {
    220
}

fn default_console_height_px() -> u32 {
    148
}

/// Font sizes + shell layout prefs (persisted in snapshot).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiSettings {
    /// Base UI font size in px (drives `--fs-base` / type scale).
    #[serde(default = "default_ui_font_size_px")]
    pub ui_font_size_px: u32,
    /// Mono editor / body textarea font size (`--fs-editor`).
    #[serde(default = "default_editor_font_size_px")]
    pub editor_font_size_px: u32,
    /// Left sidebar width in px (`--sidebar-w`).
    #[serde(default = "default_sidebar_width_px")]
    pub sidebar_width_px: u32,
    /// Request builder pane height in px (`--request-h`).
    #[serde(default = "default_request_pane_height_px")]
    pub request_pane_height_px: u32,
    /// Console body height in px when open (`--console-h`).
    #[serde(default = "default_console_height_px")]
    pub console_height_px: u32,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            ui_font_size_px: default_ui_font_size_px(),
            editor_font_size_px: default_editor_font_size_px(),
            sidebar_width_px: default_sidebar_width_px(),
            request_pane_height_px: default_request_pane_height_px(),
            console_height_px: default_console_height_px(),
        }
    }
}

impl UiSettings {
    pub const FONT_MIN_PX: u32 = 10;
    pub const FONT_MAX_PX: u32 = 22;
    pub const SIDEBAR_MIN_PX: u32 = 180;
    pub const SIDEBAR_MAX_PX: u32 = 480;
    pub const REQUEST_PANE_MIN_PX: u32 = 120;
    pub const REQUEST_PANE_MAX_PX: u32 = 560;
    pub const CONSOLE_MIN_PX: u32 = 72;
    pub const CONSOLE_MAX_PX: u32 = 420;

    pub fn clamp_font_size(px: u32) -> u32 {
        px.clamp(Self::FONT_MIN_PX, Self::FONT_MAX_PX)
    }

    pub fn clamp_sidebar_width(px: u32) -> u32 {
        px.clamp(Self::SIDEBAR_MIN_PX, Self::SIDEBAR_MAX_PX)
    }

    pub fn clamp_request_pane_height(px: u32) -> u32 {
        px.clamp(Self::REQUEST_PANE_MIN_PX, Self::REQUEST_PANE_MAX_PX)
    }

    pub fn clamp_console_height(px: u32) -> u32 {
        px.clamp(Self::CONSOLE_MIN_PX, Self::CONSOLE_MAX_PX)
    }

    /// Normalize all fields into allowed ranges.
    pub fn clamped(mut self) -> Self {
        self.ui_font_size_px = Self::clamp_font_size(self.ui_font_size_px);
        self.editor_font_size_px = Self::clamp_font_size(self.editor_font_size_px);
        self.sidebar_width_px = Self::clamp_sidebar_width(self.sidebar_width_px);
        self.request_pane_height_px = Self::clamp_request_pane_height(self.request_pane_height_px);
        self.console_height_px = Self::clamp_console_height(self.console_height_px);
        self
    }
}

/// Default GitHub remote for EvilPostman team sync.
pub const DEFAULT_CLOUD_REMOTE: &str = "https://github.com/h3ma209/evilpostman-cloud.git";

/// Git-backed cloud sync prefs (collections/envs → remote; secrets stay local).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudSyncSettings {
    /// When true, each local save also exports tree → commit → push.
    #[serde(default)]
    pub enabled: bool,
    /// Remote URL (HTTPS or SSH).
    #[serde(default = "default_cloud_remote")]
    pub remote_url: String,
    /// Override clone dir; empty = `{data_root}/cloud`.
    #[serde(default)]
    pub clone_path: String,
    /// Last sync / git status message for UI.
    #[serde(default)]
    pub last_status: String,
    /// Branch checked out after last env switch / sync (`env/<slug>`).
    #[serde(default)]
    pub last_branch: String,
}

fn default_cloud_remote() -> String {
    DEFAULT_CLOUD_REMOTE.to_string()
}

impl Default for CloudSyncSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            remote_url: default_cloud_remote(),
            clone_path: String::new(),
            last_status: String::new(),
            last_branch: String::new(),
        }
    }
}

impl CloudSyncSettings {
    /// Resolve working clone path under app data (or override).
    pub fn resolved_clone_path(&self, data_root: &std::path::Path) -> PathBuf {
        let custom = self.clone_path.trim();
        if custom.is_empty() {
            data_root.join("cloud")
        } else {
            PathBuf::from(custom)
        }
    }
}

/// App-wide settings persisted in the snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AppSettings {
    pub proxy: ProxySettings,
    pub client_cert: ClientCertSettings,
    #[serde(default)]
    pub ui: UiSettings,
    #[serde(default)]
    pub cloud_sync: CloudSyncSettings,
}

impl AppSettings {
    /// Resolve custom proxy URL for reqwest, if Custom mode with a host.
    pub fn custom_proxy_url(&self) -> Option<String> {
        if self.proxy.mode != ProxyMode::Custom {
            return None;
        }
        let hp = self.proxy.host_port.trim();
        if hp.is_empty() {
            return None;
        }
        if hp.contains("://") {
            Some(hp.to_string())
        } else {
            Some(format!("http://{hp}"))
        }
    }

    /// Default directory hint for client certs under app data.
    pub fn certs_dir_hint(data_root: &std::path::Path) -> PathBuf {
        data_root.join("certs")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_defaults_and_clamp() {
        let ui = UiSettings::default();
        assert_eq!(ui.ui_font_size_px, 13);
        assert_eq!(ui.editor_font_size_px, 14);
        assert_eq!(ui.sidebar_width_px, 268);
        assert_eq!(ui.request_pane_height_px, 220);
        assert_eq!(ui.console_height_px, 148);
        assert_eq!(UiSettings::clamp_sidebar_width(100), 180);
        assert_eq!(UiSettings::clamp_sidebar_width(999), 480);
        assert_eq!(UiSettings::clamp_font_size(8), 10);
        assert_eq!(UiSettings::clamp_font_size(40), 22);
        assert_eq!(UiSettings::clamp_request_pane_height(40), 120);
        assert_eq!(UiSettings::clamp_request_pane_height(900), 560);
        assert_eq!(UiSettings::clamp_console_height(10), 72);
        assert_eq!(UiSettings::clamp_console_height(900), 420);
    }

    #[test]
    fn app_settings_ui_serde_default() {
        let s: AppSettings = serde_json::from_str(r#"{"proxy":{"mode":"env","host_port":""},"client_cert":{"cert_path":"","passphrase":"","enabled":false}}"#).unwrap();
        assert_eq!(s.ui, UiSettings::default());
        assert!(!s.cloud_sync.enabled);
        assert_eq!(s.cloud_sync.remote_url, DEFAULT_CLOUD_REMOTE);
    }
}
