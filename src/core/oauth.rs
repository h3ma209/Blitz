//! OAuth 2.0 helpers (client_credentials + authorization_code loopback).
//! Guided by reports/11 re-notes (OAuth2WindowManager / grant_type strings).

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use serde::Deserialize;
use thiserror::Error;
use url::Url;

use super::request::{OAuth2Config, OAuth2Grant};

#[derive(Debug, Error)]
pub enum OAuthError {
    #[error("{0}")]
    Msg(String),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Fetch / refresh token into `cfg.access_token`.
pub fn fetch_oauth2_token(cfg: &mut OAuth2Config) -> Result<(), OAuthError> {
    match cfg.grant {
        OAuth2Grant::ClientCredentials => client_credentials(cfg),
        OAuth2Grant::AuthorizationCode => authorization_code(cfg),
    }
}

fn client_credentials(cfg: &mut OAuth2Config) -> Result<(), OAuthError> {
    if cfg.token_url.trim().is_empty() {
        return Err(OAuthError::Msg("token_url required".into()));
    }
    let client = reqwest::blocking::Client::new();
    let mut form = vec![
        ("grant_type", "client_credentials".to_string()),
        ("client_id", cfg.client_id.clone()),
        ("client_secret", cfg.client_secret.clone()),
    ];
    if !cfg.scope.is_empty() {
        form.push(("scope", cfg.scope.clone()));
    }
    let resp = client.post(&cfg.token_url).form(&form).send()?;
    apply_token_response(cfg, resp)
}

fn authorization_code(cfg: &mut OAuth2Config) -> Result<(), OAuthError> {
    if cfg.auth_url.trim().is_empty() || cfg.token_url.trim().is_empty() {
        return Err(OAuthError::Msg("auth_url and token_url required".into()));
    }
    let redirect = if cfg.redirect_uri.trim().is_empty() {
        "http://127.0.0.1:8765/callback".to_string()
    } else {
        cfg.redirect_uri.clone()
    };
    cfg.redirect_uri = redirect.clone();

    let mut auth = Url::parse(&cfg.auth_url)
        .map_err(|e| OAuthError::Msg(format!("auth_url: {e}")))?;
    {
        let mut q = auth.query_pairs_mut();
        q.append_pair("response_type", "code");
        q.append_pair("client_id", &cfg.client_id);
        q.append_pair("redirect_uri", &redirect);
        if !cfg.scope.is_empty() {
            q.append_pair("scope", &cfg.scope);
        }
    }

    // Open system browser (best-effort).
    let _ = open_browser(auth.as_str());

    let code = wait_for_loopback_code(&redirect, Duration::from_secs(120))?;
    let client = reqwest::blocking::Client::new();
    let form = [
        ("grant_type", "authorization_code".to_string()),
        ("code", code),
        ("redirect_uri", redirect),
        ("client_id", cfg.client_id.clone()),
        ("client_secret", cfg.client_secret.clone()),
    ];
    let resp = client.post(&cfg.token_url).form(&form).send()?;
    apply_token_response(cfg, resp)
}

fn apply_token_response(
    cfg: &mut OAuth2Config,
    resp: reqwest::blocking::Response,
) -> Result<(), OAuthError> {
    let status = resp.status();
    let body: TokenResponse = resp.json().map_err(|e| OAuthError::Msg(e.to_string()))?;
    if let Some(err) = body.error {
        return Err(OAuthError::Msg(format!(
            "{err}: {}",
            body.error_description.unwrap_or_default()
        )));
    }
    if !status.is_success() {
        return Err(OAuthError::Msg(format!("token endpoint HTTP {status}")));
    }
    cfg.access_token = body
        .access_token
        .ok_or_else(|| OAuthError::Msg("no access_token in response".into()))?;
    if let Some(rt) = body.refresh_token {
        cfg.refresh_token = rt;
    }
    Ok(())
}

fn wait_for_loopback_code(redirect_uri: &str, timeout: Duration) -> Result<String, OAuthError> {
    let url = Url::parse(redirect_uri).map_err(|e| OAuthError::Msg(e.to_string()))?;
    let host = url.host_str().unwrap_or("127.0.0.1");
    let port = url.port().unwrap_or(8765);
    let listener = TcpListener::bind((host, port))
        .map_err(|e| OAuthError::Msg(format!("bind {host}:{port}: {e}")))?;
    listener
        .set_nonblocking(false)
        .map_err(|e| OAuthError::Msg(e.to_string()))?;
    let _ = listener.set_nonblocking(true);

    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > timeout {
            return Err(OAuthError::Msg("OAuth callback timed out".into()));
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let first = req.lines().next().unwrap_or("");
                // GET /callback?code=... HTTP/1.1
                let path = first.split_whitespace().nth(1).unwrap_or("");
                let code = path
                    .split('?')
                    .nth(1)
                    .unwrap_or("")
                    .split('&')
                    .find_map(|p| {
                        let mut kv = p.splitn(2, '=');
                        let k = kv.next()?;
                        let v = kv.next()?;
                        (k == "code").then(|| v.to_string())
                    });
                let html = if code.is_some() {
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n<html><body>⚡ Blitz: auth OK — close this tab.</body></html>"
                } else {
                    "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nmissing code"
                };
                let _ = stream.write_all(html.as_bytes());
                return code.ok_or_else(|| OAuthError::Msg("callback missing code".into()));
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(OAuthError::Msg(e.to_string())),
        }
    }
}

fn open_browser(url: &str) -> Result<(), OAuthError> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| OAuthError::Msg(e.to_string()))?;
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| OAuthError::Msg(e.to_string()))?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", url])
            .spawn()
            .map_err(|e| OAuthError::Msg(e.to_string()))?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = url;
        Err(OAuthError::Msg("cannot open browser on this OS".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_credentials_needs_token_url() {
        let mut cfg = OAuth2Config::default();
        let err = fetch_oauth2_token(&mut cfg).unwrap_err();
        assert!(err.to_string().contains("token_url"));
    }
}
