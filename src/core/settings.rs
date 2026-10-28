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
    15
}

fn default_editor_font_size_px() -> u32 {
    16
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

fn default_tab_size() -> u32 {
    2
}

fn default_wrap_editor_text() -> bool {
    true
}

fn default_show_status_bar() -> bool {
    true
}

fn default_splash_min_ms() -> u32 {
    80
}

fn default_history_collapsed() -> bool {
    true
}

pub fn default_zoom_percent() -> u32 {
    100
}

/// Padding density for workbench chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum UiDensity {
    #[default]
    Comfortable,
    Compact,
}

/// Workbench color theme (class token on `.app-shell`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum UiTheme {
    /// Dark glass + Spotify green accent (default).
    #[default]
    Spotify,
    /// Postman dark stage + signature orange accent ("notPostman").
    NotPostman,
    /// Obsidian & Electric Amber / Cyberpunk minimal terminal dark stage.
    Obsidian,
    /// Swiss Blueprint / Industrial Hardware Studio (floating islands, 0px mechanical geometry, International Orange).
    Blueprint,
    /// Swiss Blueprint Light — warm drafting paper + International Orange on `.chassis-blueprint`.
    BlueprintLight,
    /// Cool dark slate + blue accent.
    Slate,
    /// Light surfaces + green accent.
    Light,
    /// High-contrast dark (accessibility).
    HighContrast,
}

impl UiTheme {
    /// CSS class on `.app-shell` (e.g. `theme-spotify`).
    pub fn class_token(self) -> &'static str {
        match self {
            Self::Spotify => "theme-spotify",
            Self::NotPostman => "theme-not-postman",
            Self::Obsidian => "theme-obsidian",
            Self::Blueprint => "theme-blueprint",
            Self::BlueprintLight => "theme-blueprint-light",
            Self::Slate => "theme-slate",
            Self::Light => "theme-light",
            Self::HighContrast => "theme-high-contrast",
        }
    }

    /// Structural layout family (shared geometry, separate palettes).
    pub fn chassis_token(self) -> Option<&'static str> {
        matches!(self, Self::Blueprint | Self::BlueprintLight).then_some("chassis-blueprint")
    }

    /// Human label for Settings dropdown.
    pub fn label(self) -> &'static str {
        match self {
            Self::Spotify => "Spotify (default)",
            Self::NotPostman => "notPostman",
            Self::Obsidian => "Obsidian Ember",
            Self::Blueprint => "Hardware Blueprint",
            Self::BlueprintLight => "Hardware Blueprint Light",
            Self::Slate => "Dark slate",
            Self::Light => "Light",
            Self::HighContrast => "High contrast",
        }
    }

    /// All themes in Settings display order.
    pub fn all() -> &'static [UiTheme] {
        &[
            Self::Spotify,
            Self::NotPostman,
            Self::Obsidian,
            Self::Blueprint,
            Self::BlueprintLight,
            Self::Slate,
            Self::Light,
            Self::HighContrast,
        ]
    }
}

/// Accent soft-fill strength (Spotify green stays; only intensity scales).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AccentIntensity {
    Soft,
    #[default]
    Normal,
    Strong,
}

/// Workbench layout orientation for request and response panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkbenchLayout {
    /// Top/bottom split (default).
    #[default]
    Column,
    /// Side-by-side split.
    Row,
}

impl WorkbenchLayout {
    pub fn toggle(self) -> Self {
        match self {
            Self::Column => Self::Row,
            Self::Row => Self::Column,
        }
    }

    pub fn class_token(self) -> &'static str {
        match self {
            Self::Column => "layout-column",
            Self::Row => "layout-row",
        }
    }
}

/// Default response pane tab after Send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DefaultResponseTab {
    #[default]
    Pretty,
    Raw,
    Headers,
}

/// Font sizes + shell layout prefs (persisted in snapshot).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiSettings {
    /// Request / response pane orientation (row = side-by-side, column = top/bottom).
    #[serde(default)]
    pub layout: WorkbenchLayout,
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
    /// Compact vs comfortable padding (`--space-*` scale).
    #[serde(default)]
    pub density: UiDensity,
    /// Soft-wrap body / script editors and response pre.
    #[serde(default = "default_wrap_editor_text")]
    pub wrap_editor_text: bool,
    /// CSS `tab-size` on editors (2 or 4).
    #[serde(default = "default_tab_size")]
    pub tab_size: u32,
    /// Color theme class on `.app-shell`.
    #[serde(default)]
    pub theme: UiTheme,
    /// Accent soft-fill intensity class on `.app-shell`.
    #[serde(default)]
    pub accent_intensity: AccentIntensity,
    /// Disable UI animations / transitions.
    #[serde(default)]
    pub reduce_motion: bool,
    /// Show workbench status line when there is status / mock info.
    #[serde(default = "default_show_status_bar")]
    pub show_status_bar: bool,
    /// Initial response pane tab after a successful send view mount.
    #[serde(default)]
    pub default_response_tab: DefaultResponseTab,
    /// Minimum splash hold before fade starts (ms).
    #[serde(default = "default_splash_min_ms")]
    pub splash_min_ms: u32,
    /// UI zoom percentage (default 100%, 50%..=200%, steps of 10% like Postman Cmd+/Cmd-).
    #[serde(default = "default_zoom_percent")]
    pub zoom_percent: u32,
    /// Sidebar History section collapsed (default on — Collections keep vertical space).
    #[serde(default = "default_history_collapsed")]
    pub history_collapsed: bool,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            layout: WorkbenchLayout::default(),
            ui_font_size_px: default_ui_font_size_px(),
            editor_font_size_px: default_editor_font_size_px(),
            sidebar_width_px: default_sidebar_width_px(),
            request_pane_height_px: default_request_pane_height_px(),
            console_height_px: default_console_height_px(),
            density: UiDensity::default(),
            wrap_editor_text: default_wrap_editor_text(),
            tab_size: default_tab_size(),
            theme: UiTheme::default(),
            accent_intensity: AccentIntensity::default(),
            reduce_motion: false,
            show_status_bar: default_show_status_bar(),
            default_response_tab: DefaultResponseTab::default(),
            splash_min_ms: default_splash_min_ms(),
            zoom_percent: default_zoom_percent(),
            history_collapsed: default_history_collapsed(),
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
    pub const SPLASH_MIN_MS: u32 = 0;
    pub const SPLASH_MAX_MS: u32 = 2_000;
    pub const ZOOM_MIN_PERCENT: u32 = 50;
    pub const ZOOM_MAX_PERCENT: u32 = 200;
    pub const ZOOM_STEP_PERCENT: u32 = 10;

    pub fn clamp_zoom_percent(pct: u32) -> u32 {
        pct.clamp(Self::ZOOM_MIN_PERCENT, Self::ZOOM_MAX_PERCENT)
    }

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

    pub fn clamp_tab_size(n: u32) -> u32 {
        if n >= 4 {
            4
        } else {
            2
        }
    }

    pub fn clamp_splash_min_ms(ms: u32) -> u32 {
        ms.clamp(Self::SPLASH_MIN_MS, Self::SPLASH_MAX_MS)
    }

    /// Normalize all fields into allowed ranges.
    pub fn clamped(mut self) -> Self {
        self.ui_font_size_px = Self::clamp_font_size(self.ui_font_size_px);
        self.editor_font_size_px = Self::clamp_font_size(self.editor_font_size_px);
        self.sidebar_width_px = Self::clamp_sidebar_width(self.sidebar_width_px);
        self.request_pane_height_px = Self::clamp_request_pane_height(self.request_pane_height_px);
        self.console_height_px = Self::clamp_console_height(self.console_height_px);
        self.tab_size = Self::clamp_tab_size(self.tab_size);
        self.splash_min_ms = Self::clamp_splash_min_ms(self.splash_min_ms);
        self.zoom_percent = Self::clamp_zoom_percent(self.zoom_percent);
        self
    }

    /// Extra class tokens for `.app-shell` (density / wrap / accent / motion).
    pub fn shell_class_tokens(&self) -> String {
        let density = match self.density {
            UiDensity::Comfortable => "density-comfortable",
            UiDensity::Compact => "density-compact",
        };
        let wrap = if self.wrap_editor_text {
            "editor-wrap"
        } else {
            "editor-nowrap"
        };
        let theme = self.theme.class_token();
        let chassis = self.theme.chassis_token().unwrap_or("");
        let accent = match self.accent_intensity {
            AccentIntensity::Soft => "accent-soft",
            AccentIntensity::Normal => "accent-normal",
            AccentIntensity::Strong => "accent-strong",
        };
        let motion = if self.reduce_motion {
            "reduce-motion"
        } else {
            ""
        };
        format!("{theme} {chassis} {density} {wrap} {accent} {motion}")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Default GitHub remote for Blitz team sync.
pub const DEFAULT_CLOUD_REMOTE: &str = "https://github.com/h3ma209/blitz-cloud.git";

/// Git-backed cloud sync prefs (collections/envs → remote; secrets stay local).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudSyncSettings {
    /// When true, Save also exports tree → local git commit (never push). Sync pulls+pushes.
    #[serde(default)]
    pub enabled: bool,
    /// Remote URL (HTTPS or SSH).
    #[serde(default = "default_cloud_remote")]
    pub remote_url: String,
    /// Override clone dir; empty = `{data_root}/cloud`.
    #[serde(default)]
    pub clone_path: String,
    /// Optional commit message override; empty = auto from git name-status + UTC timestamp.
    #[serde(default)]
    pub commit_message: String,
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
            // On by default so Sync is usable without hunting Settings first.
            enabled: true,
            remote_url: default_cloud_remote(),
            clone_path: String::new(),
            commit_message: String::new(),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_defaults_and_clamp() {
        let ui = UiSettings::default();
        assert_eq!(ui.ui_font_size_px, 15);
        assert_eq!(ui.editor_font_size_px, 16);
        assert_eq!(ui.theme, UiTheme::Spotify);
        assert_eq!(ui.sidebar_width_px, 268);
        assert_eq!(ui.request_pane_height_px, 220);
        assert_eq!(ui.console_height_px, 148);
        assert_eq!(ui.density, UiDensity::Comfortable);
        assert!(ui.wrap_editor_text);
        assert_eq!(ui.tab_size, 2);
        assert_eq!(ui.accent_intensity, AccentIntensity::Normal);
        assert!(!ui.reduce_motion);
        assert!(ui.show_status_bar);
        assert_eq!(ui.default_response_tab, DefaultResponseTab::Pretty);
        assert_eq!(ui.splash_min_ms, 80);
        assert_eq!(ui.zoom_percent, 100);
        assert!(ui.history_collapsed);
        assert_eq!(UiSettings::clamp_zoom_percent(20), 50);
        assert_eq!(UiSettings::clamp_zoom_percent(250), 200);
        assert_eq!(UiSettings::clamp_zoom_percent(120), 120);
        assert_eq!(UiSettings::clamp_sidebar_width(100), 180);
        assert_eq!(UiSettings::clamp_sidebar_width(999), 480);
        assert_eq!(UiSettings::clamp_font_size(8), 10);
        assert_eq!(UiSettings::clamp_font_size(40), 22);
        assert_eq!(UiSettings::clamp_request_pane_height(40), 120);
        assert_eq!(UiSettings::clamp_request_pane_height(900), 560);
        assert_eq!(UiSettings::clamp_console_height(10), 72);
        assert_eq!(UiSettings::clamp_console_height(900), 420);
        assert_eq!(UiSettings::clamp_tab_size(1), 2);
        assert_eq!(UiSettings::clamp_tab_size(3), 2);
        assert_eq!(UiSettings::clamp_tab_size(4), 4);
        assert_eq!(UiSettings::clamp_tab_size(8), 4);
        assert_eq!(UiSettings::clamp_splash_min_ms(9_999), 2_000);
        let tokens = ui.shell_class_tokens();
        assert!(tokens.contains("theme-spotify"));
        assert!(tokens.contains("density-comfortable"));
        assert!(tokens.contains("editor-wrap"));
        assert!(tokens.contains("accent-normal"));
        assert!(!tokens.contains("reduce-motion"));
    }

    #[test]
    fn ui_shell_class_tokens_compact_motion() {
        let ui = UiSettings {
            density: UiDensity::Compact,
            wrap_editor_text: false,
            accent_intensity: AccentIntensity::Strong,
            reduce_motion: true,
            ..Default::default()
        };
        let tokens = ui.shell_class_tokens();
        assert!(tokens.contains("density-compact"));
        assert!(tokens.contains("editor-nowrap"));
        assert!(tokens.contains("accent-strong"));
        assert!(tokens.contains("reduce-motion"));
    }

    #[test]
    fn app_settings_ui_serde_default() {
        let s: AppSettings = serde_json::from_str(r#"{"proxy":{"mode":"env","host_port":""},"client_cert":{"cert_path":"","passphrase":"","enabled":false}}"#).unwrap();
        assert_eq!(s.ui, UiSettings::default());
        // Missing cloud_sync in JSON → Default (enabled true + canonical remote).
        assert!(s.cloud_sync.enabled);
        assert_eq!(s.cloud_sync.remote_url, DEFAULT_CLOUD_REMOTE);
    }

    #[test]
    fn custom_proxy_url_empty_is_none() {
        let s = AppSettings {
            proxy: ProxySettings {
                mode: ProxyMode::Custom,
                host_port: "  ".into(),
            },
            ..Default::default()
        };
        assert_eq!(s.custom_proxy_url(), None);
    }

    #[test]
    fn custom_proxy_url_adds_http_scheme() {
        let s = AppSettings {
            proxy: ProxySettings {
                mode: ProxyMode::Custom,
                host_port: "127.0.0.1:8080".into(),
            },
            ..Default::default()
        };
        assert_eq!(s.custom_proxy_url().as_deref(), Some("http://127.0.0.1:8080"));
    }

    #[test]
    fn workbench_layout_toggle_and_serde() {
        assert_eq!(WorkbenchLayout::default(), WorkbenchLayout::Column);
        assert_eq!(WorkbenchLayout::Column.toggle(), WorkbenchLayout::Row);
        assert_eq!(WorkbenchLayout::Row.toggle(), WorkbenchLayout::Column);
        assert_eq!(WorkbenchLayout::Column.class_token(), "layout-column");
        assert_eq!(WorkbenchLayout::Row.class_token(), "layout-row");

        let mut ui = UiSettings::default();
        assert_eq!(ui.layout, WorkbenchLayout::Column);
        ui.layout = WorkbenchLayout::Row;

        let json = serde_json::to_string(&ui).unwrap();
        let decoded: UiSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.layout, WorkbenchLayout::Row);

        // Omitting layout from JSON falls back to Column via serde default
        let json_without = r#"{"ui_font_size_px":13}"#;
        let decoded_default: UiSettings = serde_json::from_str(json_without).unwrap();
        assert_eq!(decoded_default.layout, WorkbenchLayout::Column);
    }

    #[test]
    fn blueprint_chassis_tokens() {
        assert_eq!(UiTheme::Blueprint.chassis_token(), Some("chassis-blueprint"));
        assert_eq!(
            UiTheme::BlueprintLight.chassis_token(),
            Some("chassis-blueprint")
        );
        assert_eq!(UiTheme::Spotify.chassis_token(), None);
        assert_eq!(UiTheme::NotPostman.chassis_token(), None);
        assert_eq!(UiTheme::NotPostman.class_token(), "theme-not-postman");
        assert_eq!(UiTheme::NotPostman.label(), "notPostman");

        let ui_light = UiSettings {
            theme: UiTheme::BlueprintLight,
            ..Default::default()
        };
        let tokens = ui_light.shell_class_tokens();
        assert!(tokens.contains("theme-blueprint-light"));
        assert!(tokens.contains("chassis-blueprint"));
    }
}
