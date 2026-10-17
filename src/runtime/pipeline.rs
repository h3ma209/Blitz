//! Primary request send path (reports/07 pseudocode, simplified).

use std::fs;
use std::time::Instant;

use reqwest::blocking::{multipart, Client};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::redirect::Policy;
use thiserror::Error;

use crate::core::auth_extra::{auth_type_label, inject_extra_auth};
use crate::core::pkcs12_util::load_client_identity_pem;
use crate::core::{
    build_digest_authorization, digest_uri, run_script_ex, AuthConfig, AuthType, BodyMode,
    CookieJar, DigestChallenge, FormDataField, HttpMethod, KeyValue, RequestDraft, RequestSettings,
    ResponseView, SandboxExtras, SandboxResponse, TestResult, TimingBreakdown, VariableContext,
    AppSettings, ProxyMode,
};
use crate::runtime::postman_defaults::{
    inject_postman_system_headers, prepare_raw_body_for_send,
};
use crate::secrets::{self, SecretsStore};

#[derive(Debug, Error)]
pub enum SendError {
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("HTTP client error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("header error: {0}")]
    Header(String),
    #[error("digest auth: {0}")]
    Digest(String),
    #[error("client: {0}")]
    Client(String),
}

pub type SendResult = Result<ResponseView, SendError>;

/// Optional inputs for a send (collection auth, cookies, secrets, app settings).
#[derive(Default)]
pub struct SendOptions<'a> {
    pub secrets_store: Option<&'a dyn SecretsStore>,
    pub collection_auth: Option<&'a AuthConfig>,
    pub cookie_jar: Option<&'a mut CookieJar>,
    pub app_settings: Option<&'a AppSettings>,
    /// Pre-request scripts outside the request (collection → folder), run first.
    pub inherited_pre: Vec<&'a str>,
    /// Post-response scripts outside the request (folder → collection), run after request tests.
    pub inherited_post: Vec<&'a str>,
    /// Extra sandbox bridge: iteration data JSON object string.
    pub iteration_data_json: Option<&'a str>,
    /// Mutable execution hints from sandbox (`setNextRequest` / `skipRequest`).
    pub execution: Option<&'a mut ExecutionHints>,
}

/// Filled by `pm.execution.*` in the sandbox.
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
    collection_auth: Option<&AuthConfig>,
    log: &mut Vec<String>,
) {
    if draft.auth.auth_type != AuthType::Inherit {
        return;
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
            log.push("auth: inherit → none (no collection auth)".into());
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
                    let proxy = reqwest::Proxy::all(&url)
                        .map_err(|e| SendError::Client(format!("proxy: {e}")))?;
                    builder = builder.proxy(proxy);
                    log.push(format!("proxy: custom {url}"));
                } else {
                    log.push("proxy: custom empty — ignored".into());
                }
            }
        }

        // Client cert — PEM or PKCS#12 (via openssl conversion)
        if app.client_cert.enabled && !app.client_cert.cert_path.trim().is_empty() {
            let path = app.client_cert.cert_path.trim();
            match load_client_identity_pem(path, &app.client_cert.passphrase) {
                Ok(pem) => match reqwest::Identity::from_pem(&pem) {
                    Ok(id) => {
                        builder = builder.identity(id);
                        log.push(format!("client-cert: loaded {path}"));
                    }
                    Err(e) => {
                        log.push(format!(
                            "client-cert: identity failed ({e}) — need PEM cert+key"
                        ));
                    }
                },
                Err(e) => {
                    log.push(format!("client-cert: {e}"));
                }
            }
        } else if !app.client_cert.cert_path.trim().is_empty() {
            log.push("client-cert: path set but disabled".into());
        }
    }

    builder
        .build()
        .map_err(|e| SendError::Client(e.to_string()))
}

fn build_formdata(
    fields: &[FormDataField],
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
            let path = f.value.trim();
            match fs::read(path) {
                Ok(bytes) => {
                    let filename = std::path::Path::new(path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("file")
                        .to_string();
                    let part = multipart::Part::bytes(bytes).file_name(filename);
                    form = form.part(f.key.clone(), part);
                    file_n += 1;
                }
                Err(e) => {
                    form = form.text(f.key.clone(), format!("[file missing: {path}: {e}]"));
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
pub fn send_request(draft: &RequestDraft, ctx: &VariableContext) -> SendResult {
    send_request_with_options(draft, ctx, SendOptions::default())
}

/// Like [`send_request`], but uses an explicit secrets backend when provided.
pub fn send_request_with_secrets(
    draft: &RequestDraft,
    ctx: &VariableContext,
    secrets_store: Option<&dyn SecretsStore>,
) -> SendResult {
    send_request_with_options(
        draft,
        ctx,
        SendOptions {
            secrets_store,
            ..Default::default()
        },
    )
}

pub fn send_request_with_options(
    draft: &RequestDraft,
    ctx: &VariableContext,
    mut opts: SendOptions<'_>,
) -> SendResult {
    let mut log = Vec::new();
    let mut console = Vec::new();
    let mut ctx = ctx.clone();

    let secrets_owned;
    let secrets_ref: Option<&dyn SecretsStore> = match opts.secrets_store {
        Some(s) => Some(s),
        None => {
            secrets_owned = secrets::open_default_store();
            Some(secrets_owned.as_ref())
        }
    };

    let extras = SandboxExtras {
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
        iteration_data_json: opts
            .iteration_data_json
            .unwrap_or("{}")
            .to_string(),
    };

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
            &mut ctx,
            None,
            &extras,
        );
        log.extend(pre.messages.iter().cloned());
        console.extend(pre.messages);
        if let Some(err) = pre.error {
            log.push(err);
        }
        apply_exec(pre.execution);
    }
    let pre = run_script_ex(
        &draft.pre_request_script,
        "pre-request",
        &mut ctx,
        None,
        &extras,
    );
    log.extend(pre.messages.iter().cloned());
    console.extend(pre.messages);
    if let Some(err) = pre.error {
        log.push(err);
    }
    apply_exec(pre.execution);
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
    let mut resolved = resolve_draft(draft, &ctx, secrets_ref);
    log.push("vars: resolved {{…}} / {{vault:…}}".into());

    // 2b. Inherit auth
    resolve_inherit_auth(&mut resolved, opts.collection_auth, &mut log);

    // 2c. secret:// in auth
    if auth_has_secret_refs(&resolved) {
        if let Some(store) = secrets_ref {
            resolve_auth_secrets(&mut resolved, store, &mut log);
        }
    }

    // 3. Auth inject (except Digest)
    inject_auth(&mut resolved, &mut log);

    // 3a. Postman Runtime system headers (Accept, UA, Cache-Control, …)
    inject_postman_system_headers(&mut resolved, &mut log);

    // 3b. Cookies
    let url = build_url(&resolved)?;
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

    // 3c. Raw JSON: strip // comments (Hakki scratchpads) + Content-Type
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
            body_bytes = Some(payload.to_string().into_bytes());
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
                body_bytes = Some(encoded.into_bytes());
                log.push("body: urlencoded".into());
            }
        }
        BodyMode::Binary => {
            let path = resolved.binary_path.trim();
            if path.is_empty() {
                log.push("body: binary skipped (empty path)".into());
            } else {
                match fs::read(path) {
                    Ok(bytes) => {
                        log.push(format!("body: binary {} bytes from {path}", bytes.len()));
                        body_bytes = Some(bytes);
                        if !has_header(&resolved, "Content-Type") {
                            resolved.headers.push(KeyValue::new(
                                "Content-Type",
                                "application/octet-stream",
                            ));
                        }
                    }
                    Err(e) => {
                        log.push(format!("body: binary read failed {path}: {e}"));
                    }
                }
            }
        }
    }

    // 5. HTTP client
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
            if let Some(f) = build_formdata(form_fields, log)? {
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
        attach_body(builder, &resolved.body_formdata, &body_bytes, &mut log)?.send()?
    };
    let ttfb_ms = ttfb_mark.elapsed().as_millis();

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
            response =
                attach_body(builder, &resolved.body_formdata, &body_bytes, &mut log)?.send()?;
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

    let body = response.text().unwrap_or_default();
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
    let sandbox_resp = SandboxResponse {
        status,
        status_text: status_text.clone(),
        body: body.clone(),
        headers: resp_headers.clone(),
    };
    let mut test_results: Vec<TestResult> = Vec::new();
    let mut visualizer_html = String::new();
    let mut run_post = |script: &str, phase: &str| {
        let tests = run_script_ex(script, phase, &mut ctx, Some(&sandbox_resp), &extras);
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
        resolve_inherit_auth(&mut draft, Some(&col), &mut log);
        assert_eq!(draft.auth.auth_type, AuthType::Bearer);
        assert_eq!(draft.auth.token, "col-tok");
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
        let form = build_formdata(&fields, &mut log).unwrap();
        assert!(form.is_some());
        assert!(log.iter().any(|l| l.contains("form-data")));
    }
}
