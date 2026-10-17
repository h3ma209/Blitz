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

/// App-wide settings persisted in the snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AppSettings {
    pub proxy: ProxySettings,
    pub client_cert: ClientCertSettings,
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
