//! Primary request send path (reports/07 pseudocode, simplified).

use std::error::Error as StdError;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use reqwest::blocking::{multipart, Client, Response as BlockingResponse};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::redirect::Policy;
use thiserror::Error;

use crate::core::auth_extra::{auth_type_label, inject_extra_auth};
use crate::core::pkcs12_util::load_client_identity_pem;
use crate::core::{
    apply_request_header_upserts, body_mode_label, build_digest_authorization, digest_uri,
    run_script_ex, AuthConfig, AuthType, BodyMode, Cookie, CookieJar, DigestChallenge,
    FormDataField, HttpMethod, KeyValue, RequestDraft, RequestSettings, ResponseView,
    SandboxExtras, SandboxResponse, TestResult, TimingBreakdown, VariableContext,
    AppSettings, ClientCertSettings, ProxyMode, ProxySettings,
};
use crate::runtime::decode_body::decode_response_body;
use crate::runtime::postman_defaults::{
    inject_postman_system_headers, prepare_raw_body_for_send,
};
use crate::secrets::{self, SecretsStore};

#[derive(Debug, Error)]
pub enum SendError {
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("HTTP client error: {0}")]
    Http(String),
    #[error("header error: {0}")]
    Header(String),
    #[error("digest auth: {0}")]
    Digest(String),
    #[error("client: {0}")]
    Client(String),
    #[error("cancelled")]
    Cancelled,
}

pub type SendResult = Result<ResponseView, SendError>;

/// reqwest `Display` for Kind::Builder is just `"builder error"` — chain sources.
fn format_reqwest_error(e: &reqwest::Error) -> String {
    let mut parts = Vec::new();
    let mut cur: Option<&dyn StdError> = Some(e);
    while let Some(err) = cur {
        let s = err.to_string();
        if parts.last() != Some(&s) {
            parts.push(s);
        }
        cur = err.source();
    }
    parts.join(" — ")
}

fn map_reqwest(e: reqwest::Error) -> SendError {
    SendError::Http(format_reqwest_error(&e))
}

/// Reject unresolved `{{vars}}` / non-http(s) URLs before reqwest (avoids opaque builder error).
/// `raw_before_resolve` is the draft URL before `{{…}}` substitution (for better diagnostics).
fn ensure_absolute_http_url(url: &str, raw_before_resolve: &str) -> Result<(), SendError> {
    let trimmed = url.trim();
    let raw = raw_before_resolve.trim();
    let template_hint = if raw.contains("{{") {
        format!(" (raw URL had templates: {raw})")
    } else {
        String::new()
    };

    if trimmed.is_empty() {
        return Err(SendError::InvalidUrl(format!(
            "empty URL{template_hint}"
        )));
    }
    if trimmed.contains("{{") {
        return Err(SendError::InvalidUrl(format!(
            "unresolved template in URL (missing env/collection var?): {trimmed}"
        )));
    }
    // Missing `{{base}}` resolves to "" → path-only "/auth/login".
    if trimmed.starts_with('/') || !trimmed.contains("://") {
        return Err(SendError::InvalidUrl(format!(
            "not an absolute http(s) URL: {trimmed}{template_hint}"
        )));
    }
    let parsed = url::Url::parse(trimmed).map_err(|e| {
        SendError::InvalidUrl(format!("{e}: {trimmed}{template_hint}"))
    })?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(SendError::InvalidUrl(format!(
                "unsupported scheme '{other}' (need http/https): {trimmed}"
            )));
        }
    }
    if parsed.host_str().is_none() {
        return Err(SendError::InvalidUrl(format!(
            "missing host: {trimmed}{template_hint}"
        )));
    }
    Ok(())
}

/// Optional inputs for a send (collection auth, cookies, secrets, app settings).
#[derive(Default)]
pub struct SendOptions<'a> {
    pub secrets_store: Option<&'a dyn SecretsStore>,
    pub collection_auth: Option<&'a AuthConfig>,
    /// Hierarchical ancestor auth: (effective AuthConfig, origin name/label e.g. "folder 'Auth'")
    pub ancestor_auth: Option<(&'a AuthConfig, &'a str)>,
    pub cookie_jar: Option<&'a mut CookieJar>,
    pub app_settings: Option<&'a AppSettings>,
    /// Pre-request scripts outside the request (collection → folder), run first.
    pub inherited_pre: Vec<&'a str>,
    /// Post-response scripts outside the request (folder → collection), run after request tests.
    pub inherited_post: Vec<&'a str>,
    /// Extra sandbox bridge: iteration data JSON object string.
    pub iteration_data_json: Option<&'a str>,
    /// Current 0-based iteration index for `pm.info.iteration`.
    pub iteration: usize,
    /// Mutable execution hints from sandbox (`setNextRequest` / `skipRequest`).
    pub execution: Option<&'a mut ExecutionHints>,
    /// Cooperative cancel: set to true to abort (best-effort during blocking HTTP).
    pub cancel: Option<Arc<AtomicBool>>,
    /// Optional workspace root for resolving relative assets paths (e.g. assets/payloads/file.pdf).
    pub workspace_root: Option<&'a std::path::Path>,
}

fn is_cancelled(cancel: &Option<Arc<AtomicBool>>) -> bool {
    cancel
        .as_ref()
        .is_some_and(|c| c.load(Ordering::SeqCst))
}

fn check_cancelled(cancel: &Option<Arc<AtomicBool>>) -> Result<(), SendError> {
    if is_cancelled(cancel) {
        Err(SendError::Cancelled)
    } else {
        Ok(())
    }
}

/// Run a blocking HTTP call on a worker thread; poll `cancel` every 50ms.
/// On cancel, return [`SendError::Cancelled`] immediately and abandon the worker
/// (reqwest blocking has no mid-flight abort; the OS call may finish in the background).
fn blocking_http_cancelable<T, F>(
    cancel: &Option<Arc<AtomicBool>>,
    work: F,
) -> Result<T, SendError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, reqwest::Error> + Send + 'static,
{
    check_cancelled(cancel)?;
    let Some(flag) = cancel.clone() else {
        return work().map_err(map_reqwest);
    };
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("blitz-http".into())
        .spawn(move || {
            let _ = tx.send(work());
        })
        .map_err(|e| SendError::Client(format!("spawn http worker: {e}")))?;
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Ok(v)) => return Ok(v),
            Ok(Err(e)) => return Err(map_reqwest(e)),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if flag.load(Ordering::SeqCst) {
                    return Err(SendError::Cancelled);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(SendError::Client("http worker died".into()));
            }
        }
    }
}

fn send_builder_cancelable(
    builder: reqwest::blocking::RequestBuilder,
    cancel: &Option<Arc<AtomicBool>>,
) -> Result<BlockingResponse, SendError> {
    blocking_http_cancelable(cancel, move || builder.send())
}

/// Filled by `blitz.execution.*` in the sandbox.
#[derive(Debug, Clone, Default)]
pub struct ExecutionHints {
    pub next_request: Option<String>,
    pub skip_request: bool,
}

/// Resolve `{{vars}}` / `{{vault:key}}` across URL, params, headers, body, auth fields.
fn resolve_draft(
    draft: &RequestDraft,
    ctx: &VariableContext,
    secrets: Option<&dyn SecretsStore>,
) -> RequestDraft {
    let resolve = |s: &str| ctx.resolve_string_with_secrets(s, secrets);
    let resolve_kv = |rows: &[KeyValue]| -> Vec<KeyValue> {
        rows.iter()
            .map(|kv| KeyValue {
                key: resolve(&kv.key),
                value: resolve(&kv.value),
                enabled: kv.enabled,
            })
            .collect()
    };
    let resolve_fd = |rows: &[FormDataField]| -> Vec<FormDataField> {
        rows.iter()
            .map(|f| FormDataField {
                key: resolve(&f.key),
                value: resolve(&f.value),
                enabled: f.enabled,
                is_file: f.is_file,
            })
            .collect()
    };

    let mut out = draft.clone();
    out.url = resolve(&draft.url);
    out.params = resolve_kv(&draft.params);
    out.headers = resolve_kv(&draft.headers);
    out.body = resolve(&draft.body);
    out.binary_path = resolve(&draft.binary_path);
    out.body_urlencoded = resolve_kv(&draft.body_urlencoded);
    out.body_formdata = resolve_fd(&draft.body_formdata);
    out.auth.token = resolve(&draft.auth.token);
    out.auth.username = resolve(&draft.auth.username);
    out.auth.password = resolve(&draft.auth.password);
    out.auth.api_key_header = resolve(&draft.auth.api_key_header);
    out.auth.extra.key = resolve(&draft.auth.extra.key);
    out.auth.extra.secret = resolve(&draft.auth.extra.secret);
    out.auth.extra.token = resolve(&draft.auth.extra.token);
    out.auth.extra.token_secret = resolve(&draft.auth.extra.token_secret);
    out
}

/// Expand `secret://…` refs in auth fields via the given store.
fn resolve_auth_secrets(
    draft: &mut RequestDraft,
    store: &dyn SecretsStore,
    log: &mut Vec<String>,
) {
    for (label, field) in [
        ("token", &mut draft.auth.token),
        ("username", &mut draft.auth.username),
        ("password", &mut draft.auth.password),
    ] {
        if !secrets::is_secret_ref(field) {
            continue;
        }
        match secrets::resolve_secret_value(store, field) {
            Ok(resolved) => {
                *field = resolved;
                log.push(format!("secrets: resolved auth.{label} from keychain ref"));
            }
            Err(e) => {
                log.push(format!("secrets: auth.{label} resolve failed: {e}"));
            }
        }
    }
}

fn auth_has_secret_refs(draft: &RequestDraft) -> bool {
    secrets::is_secret_ref(&draft.auth.token)
        || secrets::is_secret_ref(&draft.auth.username)
        || secrets::is_secret_ref(&draft.auth.password)
}

fn resolve_inherit_auth(
    draft: &mut RequestDraft,
    ancestor_auth: Option<(&AuthConfig, &str)>,
    collection_auth: Option<&AuthConfig>,
    log: &mut Vec<String>,
) {
    if draft.auth.auth_type != AuthType::Inherit {
        return;
    }
    if let Some((auth, origin)) = ancestor_auth {
        if auth.auth_type != AuthType::Inherit && auth.auth_type != AuthType::NoAuth {
            draft.auth = auth.clone();
            log.push(format!(
                "auth: inherited from {} ({})",
                origin,
                auth_label(auth.auth_type)
            ));
            return;
        }
    }
    match collection_auth {
        Some(ca) if ca.auth_type != AuthType::Inherit => {
            draft.auth = ca.clone();
            log.push(format!(
                "auth: inherited from collection ({})",
                auth_label(ca.auth_type)
            ));
        }
        _ => {
            draft.auth.auth_type = AuthType::NoAuth;
            log.push("auth: inherit → none (no ancestor auth configured)".into());
        }
    }
}

fn auth_label(t: AuthType) -> &'static str {
    auth_type_label(t)
}

/// Inject auth headers (Basic / Bearer / API Key). Digest handled at HTTP layer.
fn inject_auth(draft: &mut RequestDraft, log: &mut Vec<String>) {
    match draft.auth.auth_type {
        AuthType::NoAuth | AuthType::Inherit => log.push("auth: none".into()),
        AuthType::Bearer => {
            if !draft.auth.token.is_empty() {
                draft.headers.push(KeyValue::new(
                    "Authorization",
                    format!("Bearer {}", draft.auth.token),
                ));
                log.push("auth: bearer injected".into());
            } else {
                log.push("auth: bearer skipped (empty token)".into());
            }
        }
        AuthType::Basic => {
            use std::io::Write;
            let mut buf = Vec::new();
            let _ = write!(
                &mut buf,
                "{}:{}",
                draft.auth.username, draft.auth.password
            );
            let encoded = base64_encode(&buf);
            draft
                .headers
                .push(KeyValue::new("Authorization", format!("Basic {encoded}")));
            log.push("auth: basic injected".into());
        }
        AuthType::ApiKey => {
            let header = if draft.auth.api_key_header.is_empty() {
                "X-Api-Key"
            } else {
                draft.auth.api_key_header.as_str()
            };
            draft
                .headers
                .push(KeyValue::new(header, draft.auth.token.clone()));
            log.push(format!("auth: api-key injected as {header}"));
        }
        AuthType::Digest => {
            log.push("auth: digest — will challenge/respond after 401".into());
        }
        AuthType::Oauth2 => {
            let token = if !draft.auth.oauth2.access_token.is_empty() {
                draft.auth.oauth2.access_token.clone()
            } else {
                draft.auth.token.clone()
            };
            if !token.is_empty() {
                draft.headers.push(KeyValue::new(
                    "Authorization",
                    format!("Bearer {token}"),
                ));
                log.push("auth: oauth2 bearer injected".into());
            } else {
                log.push("auth: oauth2 skipped (empty access_token — use Get Token)".into());
            }
        }
        AuthType::Oauth1
        | AuthType::Hawk
        | AuthType::Ntlm
        | AuthType::AwsSigV4
        | AuthType::EdgeGrid => {
            inject_extra_auth(draft, log);
        }
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut n = (chunk[0] as u32) << 16;
        if chunk.len() > 1 {
            n |= (chunk[1] as u32) << 8;
        }
        if chunk.len() > 2 {
            n |= chunk[2] as u32;
        }
        out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn build_url(draft: &RequestDraft) -> Result<String, SendError> {
    let mut url = draft.url.clone();
    if url.is_empty() {
        return Err(SendError::InvalidUrl("empty URL".into()));
    }

    let enabled: Vec<_> = draft
        .params
        .iter()
        .filter(|p| p.enabled && !p.key.is_empty())
        .collect();

    if !enabled.is_empty() {
        let query: String = enabled
            .iter()
            .map(|p| {
                format!(
                    "{}={}",
                    urlencoding_encode(&p.key),
                    urlencoding_encode(&p.value)
                )
            })
            .collect::<Vec<_>>()
            .join("&");
        if url.contains('?') {
            url.push('&');
        } else {
            url.push('?');
        }
        url.push_str(&query);
    }
    Ok(url)
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn build_urlencoded_body(rows: &[KeyValue]) -> String {
    rows.iter()
        .filter(|p| p.enabled && !p.key.is_empty())
        .map(|p| {
            format!(
                "{}={}",
                urlencoding_encode(&p.key),
                urlencoding_encode(&p.value)
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn has_header(draft: &RequestDraft, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    draft.headers
        .iter()
        .any(|h| h.enabled && h.key.eq_ignore_ascii_case(&lower) && !h.key.is_empty())
}

fn build_headers(draft: &RequestDraft) -> Result<HeaderMap, SendError> {
    let mut map = HeaderMap::new();
    for kv in draft.headers.iter().filter(|h| h.enabled && !h.key.is_empty()) {
        let name = HeaderName::from_bytes(kv.key.as_bytes())
            .map_err(|e| SendError::Header(format!("{}: {e}", kv.key)))?;
        let value = HeaderValue::from_str(&kv.value)
            .map_err(|e| SendError::Header(format!("{}: {e}", kv.key)))?;
        map.append(name, value);
    }
    Ok(map)
}

fn parse_host_path(url: &str) -> (String, String) {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let (host_port, path) = match rest.split_once('/') {
        Some((h, p)) => (h, format!("/{p}")),
        None => (rest, "/".into()),
    };
    let host = host_port.split('?').next().unwrap_or(host_port);
    let path = path.split('?').next().unwrap_or(&path).to_string();
    (host.to_string(), path)
}

fn script_cookie_host(url: &str) -> String {
    let (host, _) = parse_host_path(url);
    host.split(':')
        .next()
        .unwrap_or(&host)
        .to_lowercase()
}

fn absorb_script_side_effects(
    pre: &crate::core::ScriptRunOutcome,
    extras: &mut SandboxExtras,
    header_upserts: &mut Vec<(String, String)>,
    cookie_jar: Option<&mut CookieJar>,
    draft_url: &str,
) {
    for (k, v) in &pre.request_header_upserts {
        if let Some((_, existing)) = extras
            .request_headers
            .iter_mut()
            .find(|(ek, _)| ek.eq_ignore_ascii_case(k))
        {
            *existing = v.clone();
        } else {
            extras.request_headers.push((k.clone(), v.clone()));
        }
        if let Some((_, existing)) = header_upserts
            .iter_mut()
            .find(|(ek, _)| ek.eq_ignore_ascii_case(k))
        {
            *existing = v.clone();
        } else {
            header_upserts.push((k.clone(), v.clone()));
        }
    }
    if let Some(cookies) = &pre.cookies {
        extras.cookies = cookies.clone();
        if let Some(jar) = cookie_jar {
            let host = script_cookie_host(draft_url);
            if cookies.is_empty() {
                jar.cookies.retain(|c| c.domain != host);
            } else {
                for (name, value) in cookies {
                    jar.upsert(Cookie {
                        domain: host.clone(),
                        name: name.clone(),
                        value: value.clone(),
                        path: "/".into(),
                    });
                }
            }
        }
    }
}

fn build_client(
    settings: &RequestSettings,
    app: Option<&AppSettings>,
    log: &mut Vec<String>,
) -> Result<Client, SendError> {
    // UA also injected as a header (PostmanRuntime); keep client default aligned.
    let mut builder = Client::builder().user_agent("PostmanRuntime/7.43.0");

    if settings.timeout_ms == 0 {
        builder = builder.pool_idle_timeout(None);
        log.push("settings: timeout=none".into());
    } else {
        builder = builder.timeout(std::time::Duration::from_millis(settings.timeout_ms));
        log.push(format!("settings: timeout={}ms", settings.timeout_ms));
    }

    if settings.follow_redirects {
        builder = builder.redirect(Policy::limited(10));
        log.push("settings: follow_redirects=true".into());
    } else {
        builder = builder.redirect(Policy::none());
        log.push("settings: follow_redirects=false".into());
    }

    if !settings.ssl_verify {
        builder = builder.danger_accept_invalid_certs(true);
        log.push("settings: ssl_verify=false (INSECURE)".into());
    }

    // Proxy
    if let Some(app) = app {
        match app.proxy.mode {
            ProxyMode::None => {
                builder = builder.no_proxy();
                log.push("proxy: disabled".into());
            }
            ProxyMode::Env => {
                log.push("proxy: env HTTP(S)_PROXY".into());
            }
            ProxyMode::Custom => {
                if let Some(url) = app.custom_proxy_url() {
                    let proxy = reqwest::Proxy::all(&url).map_err(|e| {
                        SendError::Client(format!("proxy: {}", format_reqwest_error(&e)))
                    })?;
                    builder = builder.proxy(proxy);
                    log.push(format!("proxy: custom {url}"));
                } else {
                    // Custom selected but host empty — do not call Proxy::all("").
                    log.push("proxy: custom empty — ignored".into());
                }
            }
        }

        // Client cert — PEM or PKCS#12 (via openssl conversion). Never fail the builder
        // when path/identity is bad; skip and log so default HTTPS Send still works.
        if app.client_cert.enabled {
            let path = app.client_cert.cert_path.trim();
            if path.is_empty() {
                log.push("client-cert: enabled but path empty — skipped".into());
            } else if !std::path::Path::new(path).is_file() {
                log.push(format!("client-cert: path missing ({path}) — skipped"));
            } else {
                match load_client_identity_pem(path, &app.client_cert.passphrase) {
                    Ok(pem) => match reqwest::Identity::from_pem(&pem) {
                        Ok(id) => {
                            builder = builder.identity(id);
                            log.push(format!("client-cert: loaded {path}"));
                        }
                        Err(e) => {
                            log.push(format!(
                                "client-cert: identity failed ({}) — skipped",
                                format_reqwest_error(&e)
                            ));
                        }
                    },
                    Err(e) => {
                        log.push(format!("client-cert: {e} — skipped"));
                    }
                }
            }
        } else if !app.client_cert.cert_path.trim().is_empty() {
            log.push("client-cert: path set but disabled".into());
        }
    }

    builder
        .build()
        .map_err(|e| SendError::Client(format_reqwest_error(&e)))
}

fn build_formdata(
    fields: &[FormDataField],
    workspace_root: Option<&std::path::Path>,
    log: &mut Vec<String>,
) -> Result<Option<multipart::Form>, SendError> {
    let enabled: Vec<_> = fields
        .iter()
        .filter(|f| f.enabled && !f.key.is_empty())
        .collect();
    if enabled.is_empty() {
        return Ok(None);
    }
    let mut form = multipart::Form::new();
    let mut text_n = 0usize;
    let mut file_stub = 0usize;
    let mut file_n = 0usize;
    for f in enabled {
        if f.is_file {
            let path_str = f.value.trim();
            let resolved_path = if let Some(ws) = workspace_root {
                crate::core::resolve_asset_path(ws, path_str)
            } else {
                std::path::PathBuf::from(path_str)
            };
            match fs::read(&resolved_path) {
                Ok(bytes) => {
                    let filename = resolved_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("file")
                        .to_string();
                    let part = multipart::Part::bytes(bytes).file_name(filename);
                    form = form.part(f.key.clone(), part);
                    file_n += 1;
                }
                Err(e) => {
                    form = form.text(f.key.clone(), format!("[file missing: {path_str}: {e}]"));
                    file_stub += 1;
                }
            }
        } else {
            form = form.text(f.key.clone(), f.value.clone());
            text_n += 1;
        }
    }
    log.push(format!(
        "body: form-data text={text_n} files={file_n} missing={file_stub}"
    ));
    Ok(Some(form))
}

fn method_from(m: HttpMethod) -> reqwest::Method {
    match m {
        HttpMethod::Get => reqwest::Method::GET,
        HttpMethod::Post => reqwest::Method::POST,
        HttpMethod::Put => reqwest::Method::PUT,
        HttpMethod::Patch => reqwest::Method::PATCH,
        HttpMethod::Delete => reqwest::Method::DELETE,
        HttpMethod::Head => reqwest::Method::HEAD,
        HttpMethod::Options => reqwest::Method::OPTIONS,
    }
}

/// Full send pipeline (blocking — call from worker thread / spawn_blocking).
///
/// Mutates `ctx` in place so `blitz.environment` / `blitz.collectionVariables` / globals
/// side-effects from scripts are visible to the caller (Postman semantics).
#[cfg(test)]
pub fn send_request(draft: &RequestDraft, ctx: &mut VariableContext) -> SendResult {
    send_request_with_options(draft, ctx, SendOptions::default())
}

pub fn send_request_with_options(
    draft: &RequestDraft,
    ctx: &mut VariableContext,
    mut opts: SendOptions<'_>,
) -> SendResult {
    let mut log = Vec::new();
    let mut console = Vec::new();

    let secrets_owned;
    let secrets_ref: Option<&dyn SecretsStore> = match opts.secrets_store {
        Some(s) => Some(s),
        None => {
            secrets_owned = secrets::open_default_store();
            Some(secrets_owned.as_ref())
        }
    };

    let mut extras = SandboxExtras {
        vault: ctx.vault.entries.clone(),
        cookies: opts
            .cookie_jar
            .as_ref()
            .map(|j| {
                j.cookies
                    .iter()
                    .map(|c| (c.name.clone(), c.value.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        request_url: draft.url.clone(),
        request_method: draft.method.as_str().into(),
        request_headers: draft
            .headers
            .iter()
            .filter(|h| h.enabled && !h.key.is_empty())
            .map(|h| (h.key.clone(), h.value.clone()))
            .collect(),
        request_body_mode: body_mode_label(draft.body_mode).into(),
        request_body_raw: draft.body.clone(),
        iteration_data_json: opts
            .iteration_data_json
            .unwrap_or("{}")
            .to_string(),
        request_name: draft.name.clone(),
        event_name: "prerequest".into(),
        iteration: opts.iteration as u32,
    };

    let mut header_upserts: Vec<(String, String)> = Vec::new();

    // 1. Pre-request: collection → folder → request
    let mut apply_exec = |exec: crate::core::SandboxExecution| {
        if let Some(h) = opts.execution.as_mut() {
            if exec.skip_request {
                h.skip_request = true;
            }
            if exec.next_request.is_some() {
                h.next_request = exec.next_request;
            }
        }
    };
    for (i, script) in opts.inherited_pre.iter().enumerate() {
        let pre = run_script_ex(
            script,
            &format!("pre-request/inherited-{i}"),
            ctx,
            None,
            &extras,
        );
        log.extend(pre.messages.iter().cloned());
        console.extend(pre.messages.iter().cloned());
        if let Some(err) = pre.error.clone() {
            log.push(err);
        }
        apply_exec(pre.execution.clone());
        absorb_script_side_effects(
            &pre,
            &mut extras,
            &mut header_upserts,
            opts.cookie_jar.as_deref_mut(),
            &draft.url,
        );
    }
    let pre = run_script_ex(
        &draft.pre_request_script,
        "pre-request",
        ctx,
        None,
        &extras,
    );
    log.extend(pre.messages.iter().cloned());
    console.extend(pre.messages.iter().cloned());
    if let Some(err) = pre.error.clone() {
        log.push(err);
    }
    apply_exec(pre.execution.clone());
    absorb_script_side_effects(
        &pre,
        &mut extras,
        &mut header_upserts,
        opts.cookie_jar.as_deref_mut(),
        &draft.url,
    );
    if opts
        .execution
        .as_ref()
        .map(|e| e.skip_request)
        .unwrap_or(false)
    {
        log.push("execution: skipRequest — aborting send".into());
        return Ok(ResponseView {
            status: 0,
            status_text: "SKIPPED".into(),
            headers: vec![],
            body: String::new(),
            elapsed_ms: 0,
            error: None,
            pipeline_log: log.clone(),
            console_messages: console,
            test_results: vec![],
            resolved_url: String::new(),
            timing: TimingBreakdown::default(),
            visualizer_html: String::new(),
        });
    }

    // 2. Variable + vault resolution
    let mut resolved = resolve_draft(draft, ctx, secrets_ref);
    log.push("vars: resolved {{…}} / {{vault:…}}".into());

    // 2a. Headers from blitz.request.headers.upsert (pre-request scripts)
    if !header_upserts.is_empty() {
        apply_request_header_upserts(&mut resolved, &header_upserts);
        log.push(format!(
            "scripts: applied {} request header upsert(s)",
            header_upserts.len()
        ));
    }

    // 2b. Inherit auth
    resolve_inherit_auth(
        &mut resolved,
        opts.ancestor_auth,
        opts.collection_auth,
        &mut log,
    );

    // 2c. secret:// in auth
    if auth_has_secret_refs(&resolved) {
        if let Some(store) = secrets_ref {
            resolve_auth_secrets(&mut resolved, store, &mut log);
        }
    }

    // 3. Build URL with params & prepare body before signing auth (SigV4/OAuth1/Hawk/EdgeGrid)
    let url = build_url(&resolved)?;
    ensure_absolute_http_url(&url, &draft.url)?;
    resolved.url = url.clone();
    resolved.params.clear();

    // 3a. Raw JSON: strip // comments (Hakki scratchpads) + Content-Type
    if let Err(e) = prepare_raw_body_for_send(&mut resolved, &mut log) {
        log.push(format!("body: {e}"));
    }

    // 4. Body prep
    let use_formdata = resolved.body_mode == BodyMode::FormData;
    let mut body_bytes: Option<Vec<u8>> = None;
    match resolved.body_mode {
        BodyMode::None | BodyMode::FormData => {}
        BodyMode::Raw => {
            if !resolved.body.is_empty() {
                body_bytes = Some(resolved.body.as_bytes().to_vec());
            }
        }
        BodyMode::Graphql => {
            let vars: serde_json::Value =
                serde_json::from_str(if resolved.graphql_variables.trim().is_empty() {
                    "{}"
                } else {
                    &resolved.graphql_variables
                })
                .unwrap_or(serde_json::json!({}));
            let payload = serde_json::json!({
                "query": resolved.graphql_query,
                "variables": vars,
            });
            if !has_header(&resolved, "Content-Type") {
                resolved
                    .headers
                    .push(KeyValue::new("Content-Type", "application/json"));
            }
            let payload_str = payload.to_string();
            resolved.body = payload_str.clone();
            body_bytes = Some(payload_str.into_bytes());
            if matches!(resolved.method, HttpMethod::Get) {
                resolved.method = HttpMethod::Post;
            }
            log.push("body: graphql JSON".into());
        }
        BodyMode::Urlencoded => {
            let encoded = build_urlencoded_body(&resolved.body_urlencoded);
            if !encoded.is_empty() {
                if !has_header(&resolved, "Content-Type") {
                    resolved.headers.push(KeyValue::new(
                        "Content-Type",
                        "application/x-www-form-urlencoded",
                    ));
                }
                resolved.body = encoded.clone();
                body_bytes = Some(encoded.into_bytes());
                log.push("body: urlencoded".into());
            }
        }
        BodyMode::Binary => {
            let path_str = resolved.binary_path.trim();
            if path_str.is_empty() {
                log.push("body: binary skipped (empty path)".into());
            } else {
                let resolved_path = if let Some(ws) = opts.workspace_root {
                    crate::core::resolve_asset_path(ws, path_str)
                } else {
                    std::path::PathBuf::from(path_str)
                };
                match fs::read(&resolved_path) {
                    Ok(bytes) => {
                        log.push(format!("body: binary {} bytes from {}", bytes.len(), resolved_path.display()));
                        body_bytes = Some(bytes);
                        if !has_header(&resolved, "Content-Type") {
                            resolved.headers.push(KeyValue::new(
                                "Content-Type",
                                "application/octet-stream",
                            ));
                        }
                    }
                    Err(e) => {
                        log.push(format!("body: binary read failed {}: {e}", resolved_path.display()));
                    }
                }
            }
        }
    }

    // 4a. Auth inject (except Digest) — signs resolved.url (with query) and prepared resolved.body
    inject_auth(&mut resolved, &mut log);

    // 4b. Postman Runtime system headers (Accept, UA, Cache-Control, …)
    inject_postman_system_headers(&mut resolved, &mut log);

    // 4c. Cookies
    let (host, path) = parse_host_path(&url);
    if let Some(jar) = opts.cookie_jar.as_deref() {
        if let Some(cookie_hdr) = jar.header_for(&host, &path) {
            if !has_header(&resolved, "Cookie") {
                resolved
                    .headers
                    .push(KeyValue::new("Cookie", cookie_hdr));
                log.push("cookies: jar header attached".into());
            }
        }
    }

    // 5. HTTP client
    check_cancelled(&opts.cancel)?;
    let client = build_client(&resolved.settings, opts.app_settings, &mut log)?;
    let method = method_from(resolved.method);
    let headers = build_headers(&resolved)?;

    console.push(format!("→ {} {}", resolved.method.as_str(), url));

    let attach_body = |builder: reqwest::blocking::RequestBuilder,
                       form_fields: &[FormDataField],
                       bytes: &Option<Vec<u8>>,
                       log: &mut Vec<String>|
     -> Result<reqwest::blocking::RequestBuilder, SendError> {
        if use_formdata {
            if let Some(f) = build_formdata(form_fields, opts.workspace_root, log)? {
                return Ok(builder.multipart(f));
            }
        } else if let Some(b) = bytes {
            if matches!(
                resolved.method,
                HttpMethod::Post | HttpMethod::Put | HttpMethod::Patch | HttpMethod::Delete
            ) {
                return Ok(builder.body(b.clone()));
            }
        }
        Ok(builder)
    };

    let started = Instant::now();
    let ttfb_mark = Instant::now();
    let mut response = {
        let builder = client.request(method.clone(), &url).headers(headers.clone());
        let builder =
            attach_body(builder, &resolved.body_formdata, &body_bytes, &mut log)?;
        send_builder_cancelable(builder, &opts.cancel)?
    };
    let ttfb_ms = ttfb_mark.elapsed().as_millis();
    check_cancelled(&opts.cancel)?;

    // Digest challenge/response
    if resolved.auth.auth_type == AuthType::Digest && response.status().as_u16() == 401 {
        let www = response
            .headers()
            .get_all(reqwest::header::WWW_AUTHENTICATE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find(|s| s.to_ascii_lowercase().contains("digest"))
            .map(|s| s.to_string());

        if let Some(www) = www {
            let challenge = DigestChallenge::parse(&www)
                .ok_or_else(|| SendError::Digest("failed to parse WWW-Authenticate".into()))?;
            let uri = digest_uri(&url);
            let cnonce = format!("{:08x}", uuid::Uuid::new_v4().as_u128() as u32);
            let auth_val = build_digest_authorization(
                &challenge,
                &resolved.auth.username,
                &resolved.auth.password,
                resolved.method.as_str(),
                &uri,
                1,
                &cnonce,
            )
            .map_err(SendError::Digest)?;

            log.push("auth: digest Authorization built".into());
            let mut headers2 = headers;
            headers2.insert(
                reqwest::header::AUTHORIZATION,
                HeaderValue::from_str(&auth_val)
                    .map_err(|e| SendError::Header(format!("Authorization: {e}")))?,
            );

            let builder = client.request(method, &url).headers(headers2);
            let builder =
                attach_body(builder, &resolved.body_formdata, &body_bytes, &mut log)?;
            response = send_builder_cancelable(builder, &opts.cancel)?;
            check_cancelled(&opts.cancel)?;
        } else {
            log.push("auth: digest 401 but no Digest WWW-Authenticate".into());
        }
    }

    let elapsed_ms = started.elapsed().as_millis();

    let status = response.status().as_u16();
    let status_text = response
        .status()
        .canonical_reason()
        .unwrap_or("")
        .to_string();
    let resp_headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    if let Some(jar) = opts.cookie_jar.as_mut() {
        let set_cookies: Vec<String> = response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
            .collect();
        if !set_cookies.is_empty() {
            jar.store_from_set_cookie(&host, &set_cookies);
            log.push(format!("cookies: stored {} Set-Cookie", set_cookies.len()));
        }
    }

    let raw_bytes = blocking_http_cancelable(&opts.cancel, move || {
        Ok(response.bytes().unwrap_or_default().to_vec())
    })?;
    check_cancelled(&opts.cancel)?;
    let body = decode_response_body(&raw_bytes, &resp_headers, &mut log);
    let download_ms = started.elapsed().as_millis().saturating_sub(ttfb_ms);
    log.push(format!("http: {status} in {elapsed_ms}ms"));
    console.push(format!(
        "← {status} {status_text} ({elapsed_ms} ms, {} bytes)",
        body.len()
    ));

    // Approximate timing waterfall (reqwest blocking lacks fine DNS/connect hooks).
    let timing = TimingBreakdown {
        dns_ms: (ttfb_ms / 10).min(30),
        connect_ms: (ttfb_ms / 5).min(80),
        tls_ms: if url.starts_with("https") {
            (ttfb_ms / 4).min(100)
        } else {
            0
        },
        ttfb_ms,
        download_ms,
        total_ms: elapsed_ms,
    };

    // 6. Post-response / tests: request → folder → collection
    check_cancelled(&opts.cancel)?;
    let sandbox_resp = SandboxResponse {
        status,
        status_text: status_text.clone(),
        body: body.clone(),
        headers: resp_headers.clone(),
        response_time: elapsed_ms as u64,
    };
    extras.event_name = "test".into();
    extras.vault = ctx.vault.entries.clone();
    let mut test_results: Vec<TestResult> = Vec::new();
    let mut visualizer_html = String::new();
    let mut run_post = |script: &str, phase: &str| {
        let tests = run_script_ex(script, phase, ctx, Some(&sandbox_resp), &extras);
        log.extend(tests.messages.iter().cloned());
        console.extend(tests.messages.iter().cloned());
        if let Some(err) = tests.error {
            log.push(err);
        }
        test_results.extend(tests.tests);
        if !tests.visualizer_html.is_empty() {
            visualizer_html = tests.visualizer_html;
        }
        if let Some(h) = opts.execution.as_mut() {
            if tests.execution.skip_request {
                h.skip_request = true;
            }
            if tests.execution.next_request.is_some() {
                h.next_request = tests.execution.next_request;
            }
        }
    };
    let assertion_script = crate::core::assertions_to_script(&draft.assertions);
    if !assertion_script.is_empty() {
        run_post(&assertion_script, "assertions");
    }
    run_post(&draft.test_script, "tests");
    for (i, script) in opts.inherited_post.iter().enumerate() {
        run_post(script, &format!("tests/inherited-{i}"));
    }

    // Fallback visualizer: raw HTML response
    if visualizer_html.is_empty() {
        let t = body.trim_start().to_ascii_lowercase();
        if t.starts_with("<!doctype html") || t.starts_with("<html") {
            visualizer_html = body.clone();
        }
    }

    Ok(ResponseView {
        status,
        status_text,
        headers: resp_headers,
        body,
        elapsed_ms,
        error: None,
        pipeline_log: log,
        console_messages: console,
        test_results,
        resolved_url: url,
        timing,
        visualizer_html,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::{service_name, InMemorySecrets, SecretsStore, DEFAULT_ACCOUNT};

    #[test]
    fn resolve_auth_secret_refs_from_memory_store() {
        let store = InMemorySecrets::default();
        store
            .set(&service_name("local"), DEFAULT_ACCOUNT, "vault-token")
            .unwrap();

        let mut draft = RequestDraft::default();
        draft.auth.auth_type = AuthType::Bearer;
        draft.auth.token = "secret://local".into();

        let mut log = Vec::new();
        resolve_auth_secrets(&mut draft, &store, &mut log);

        assert_eq!(draft.auth.token, "vault-token");
        assert!(log.iter().any(|l| l.contains("resolved auth.token")));
    }

    #[test]
    fn inherit_auth_from_collection() {
        let mut draft = RequestDraft::default();
        draft.auth.auth_type = AuthType::Inherit;
        let col = AuthConfig {
            auth_type: AuthType::Bearer,
            token: "col-tok".into(),
            ..Default::default()
        };
        let mut log = Vec::new();
        resolve_inherit_auth(&mut draft, None, Some(&col), &mut log);
        assert_eq!(draft.auth.auth_type, AuthType::Bearer);
        assert_eq!(draft.auth.token, "col-tok");

        // Test folder ancestor auth overrides collection
        let mut draft2 = RequestDraft::default();
        draft2.auth.auth_type = AuthType::Inherit;
        let folder_auth = AuthConfig {
            auth_type: AuthType::ApiKey,
            api_key_header: "X-Folder-Key".into(),
            token: "f-123".into(),
            ..Default::default()
        };
        let mut log2 = Vec::new();
        resolve_inherit_auth(&mut draft2, Some((&folder_auth, "folder 'V1'")), Some(&col), &mut log2);
        assert_eq!(draft2.auth.auth_type, AuthType::ApiKey);
        assert_eq!(draft2.auth.api_key_header, "X-Folder-Key");
        assert_eq!(draft2.auth.token, "f-123");
        assert!(log2.iter().any(|l| l.contains("folder 'V1'")));
    }

    #[test]
    fn urlencoded_body_builder() {
        let rows = vec![KeyValue::new("a", "1"), KeyValue::new("b", "x y")];
        let s = build_urlencoded_body(&rows);
        assert_eq!(s, "a=1&b=x%20y");
    }

    #[test]
    fn formdata_builds_text_fields() {
        let mut log = Vec::new();
        let fields = vec![
            FormDataField::text("name", "alice"),
            FormDataField {
                key: "f".into(),
                value: "/tmp/x".into(),
                enabled: true,
                is_file: true,
            },
        ];
        let form = build_formdata(&fields, None, &mut log).unwrap();
        assert!(form.is_some());
        assert!(log.iter().any(|l| l.contains("form-data")));
    }

    #[test]
    fn cancel_flag_aborts_before_http() {
        let flag = Arc::new(AtomicBool::new(true));
        let draft = RequestDraft {
            url: "https://example.invalid/slow".into(),
            ..Default::default()
        };
        let mut ctx = VariableContext::default();
        let err = send_request_with_options(
            &draft,
            &mut ctx,
            SendOptions {
                cancel: Some(flag),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(matches!(err, SendError::Cancelled));
    }

    #[test]
    fn blocking_http_cancelable_returns_cancelled() {
        let flag = Arc::new(AtomicBool::new(false));
        let flag2 = flag.clone();
        let handle = thread::spawn(move || {
            blocking_http_cancelable(&Some(flag2), || {
                thread::sleep(Duration::from_secs(30));
                Ok("done".to_string())
            })
        });
        thread::sleep(Duration::from_millis(80));
        flag.store(true, Ordering::SeqCst);
        let err = handle.join().unwrap().unwrap_err();
        assert!(matches!(err, SendError::Cancelled));
    }

    #[test]
    fn ensure_url_rejects_unresolved_templates() {
        let err =
            ensure_absolute_http_url("{{endpoint_rcell_auth}}/auth/login", "{{endpoint_rcell_auth}}/auth/login")
                .unwrap_err();
        match err {
            SendError::InvalidUrl(msg) => {
                assert!(msg.contains("unresolved template"), "{msg}");
                assert!(msg.contains("endpoint_rcell_auth"), "{msg}");
            }
            other => panic!("expected InvalidUrl, got {other}"),
        }
    }

    #[test]
    fn ensure_url_rejects_path_only_after_empty_var() {
        let err = ensure_absolute_http_url(
            "/auth/login",
            "{{endpoint_rcell_auth}}/auth/login",
        )
        .unwrap_err();
        match err {
            SendError::InvalidUrl(msg) => {
                assert!(msg.contains("not an absolute"), "{msg}");
                assert!(msg.contains("endpoint_rcell_auth"), "{msg}");
            }
            other => panic!("expected InvalidUrl, got {other}"),
        }
    }

    #[test]
    fn ensure_url_accepts_https() {
        ensure_absolute_http_url("https://httpbin.org/get", "https://httpbin.org/get").unwrap();
    }

    #[test]
    fn format_reqwest_error_includes_source_chain() {
        let err = reqwest::blocking::Client::new()
            .get("{{endpoint}}/x")
            .build()
            .unwrap_err();
        let msg = format_reqwest_error(&err);
        assert!(msg.contains("builder error"), "{msg}");
        assert!(
            msg.contains("relative URL") || msg.contains("without a base"),
            "{msg}"
        );
    }

    #[test]
    fn build_client_custom_proxy_empty_does_not_fail() {
        let settings = RequestSettings::default();
        let app = AppSettings {
            proxy: ProxySettings {
                mode: ProxyMode::Custom,
                host_port: String::new(),
            },
            ..Default::default()
        };
        let mut log = Vec::new();
        build_client(&settings, Some(&app), &mut log).expect("empty custom proxy must build");
        assert!(log.iter().any(|l| l.contains("custom empty")));
    }

    #[test]
    fn build_client_missing_cert_path_skips_not_fails() {
        let settings = RequestSettings::default();
        let app = AppSettings {
            client_cert: ClientCertSettings {
                enabled: true,
                cert_path: "/tmp/blitz-missing-cert.pem".into(),
                passphrase: String::new(),
            },
            ..Default::default()
        };
        let mut log = Vec::new();
        build_client(&settings, Some(&app), &mut log).expect("missing cert must not fail builder");
        assert!(log.iter().any(|l| l.contains("path missing") || l.contains("skipped")));
    }

    #[test]
    fn send_unresolved_url_surfaces_invalid_url_not_builder() {
        let draft = RequestDraft {
            url: "{{endpoint_rcell_auth}}/auth/login".into(),
            ..Default::default()
        };
        let mut ctx = VariableContext::default();
        let err = send_request(&draft, &mut ctx).unwrap_err();
        let msg = err.to_string();
        assert!(
            matches!(err, SendError::InvalidUrl(_)),
            "got {err:?} / {msg}"
        );
        // Missing var → "" → "/auth/login"; message must keep raw template hint.
        assert!(msg.contains("endpoint_rcell_auth") || msg.contains("not an absolute"), "{msg}");
        assert!(!msg.contains("HTTP client error: builder error"), "{msg}");
    }
}
