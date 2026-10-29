use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::settings::RequestSettings;

/// Workbench request protocol (local clients — not Postman Cloud).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RequestProtocol {
    #[default]
    Http,
    WebSocket,
    Grpc,
    Mqtt,
    Mcp,
    SocketIo,
    Sse,
    GraphqlWs,
}

impl RequestProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "HTTP",
            Self::WebSocket => "WebSocket",
            Self::Grpc => "gRPC",
            Self::Mqtt => "MQTT",
            Self::Mcp => "MCP",
            Self::SocketIo => "Socket.IO",
            Self::Sse => "SSE",
            Self::GraphqlWs => "GraphQL-WS",
        }
    }

    pub fn all() -> &'static [RequestProtocol] {
        &[
            Self::Http,
            Self::WebSocket,
            Self::Grpc,
            Self::Mqtt,
            Self::Mcp,
            Self::SocketIo,
            Self::Sse,
            Self::GraphqlWs,
        ]
    }
}

impl std::fmt::Display for RequestProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl HttpMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
        }
    }

    pub fn all() -> &'static [HttpMethod] {
        &[
            Self::Get,
            Self::Post,
            Self::Put,
            Self::Patch,
            Self::Delete,
            Self::Head,
            Self::Options,
        ]
    }
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
    pub enabled: bool,
}

impl KeyValue {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BodyMode {
    #[default]
    None,
    Raw,
    Urlencoded,
    /// Multipart form-data (text + file-by-path).
    FormData,
    /// GraphQL over HTTP (query + variables JSON).
    Graphql,
    /// Raw file bytes from `binary_path`.
    Binary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    #[default]
    Inherit,
    NoAuth,
    Bearer,
    Basic,
    ApiKey,
    /// HTTP Digest (RFC 2617 MD5) via challenge/response.
    Digest,
    /// OAuth 2.0 (client_credentials or authorization_code).
    Oauth2,
    Oauth1,
    Hawk,
    Ntlm,
    AwsSigV4,
    EdgeGrid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OAuth2Grant {
    #[default]
    ClientCredentials,
    AuthorizationCode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OAuth2Config {
    #[serde(default)]
    pub grant: OAuth2Grant,
    #[serde(default)]
    pub auth_url: String,
    #[serde(default)]
    pub token_url: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub scope: String,
    /// Loopback redirect, e.g. http://127.0.0.1:8765/callback
    #[serde(default)]
    pub redirect_uri: String,
    /// Cached access token after Get Token.
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
}

/// Saved response example (mocks / docs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedExample {
    pub id: Uuid,
    pub name: String,
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl SavedExample {
    pub fn from_response(name: impl Into<String>, view: &ResponseView) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            status: view.status,
            status_text: view.status_text.clone(),
            headers: view.headers.clone(),
            body: view.body.clone(),
        }
    }
}

/// Form-data row: text value, or local file path when `is_file`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormDataField {
    pub key: String,
    pub value: String,
    pub enabled: bool,
    /// When true, `value` is a local filesystem path uploaded as a file part.
    #[serde(default)]
    pub is_file: bool,
}

impl FormDataField {
    pub fn text(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            enabled: true,
            is_file: false,
        }
    }
}

impl Default for FormDataField {
    fn default() -> Self {
        Self::text("", "")
    }
}

/// Extra fields for OAuth1 / Hawk / AWS / EdgeGrid / NTLM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AuthExtra {
    /// OAuth1 consumer key / Hawk id / EdgeGrid client token / AWS access key.
    #[serde(default)]
    pub key: String,
    /// OAuth1 consumer secret / Hawk key / EdgeGrid client secret / AWS secret key.
    #[serde(default)]
    pub secret: String,
    /// OAuth1 token / EdgeGrid access token / AWS session token.
    #[serde(default)]
    pub token: String,
    /// OAuth1 token secret / EdgeGrid host (base URL host).
    #[serde(default)]
    pub token_secret: String,
    /// OAuth1 signature method (HMAC-SHA1 / HMAC-SHA256 / PLAINTEXT).
    #[serde(default)]
    pub signature_method: String,
    /// AWS region (default us-east-1).
    #[serde(default)]
    pub region: String,
    /// AWS service name (default execute-api).
    #[serde(default)]
    pub service: String,
    /// Hawk algorithm (sha256 / sha1).
    #[serde(default)]
    pub algorithm: String,
    /// Hawk ext / optional.
    #[serde(default)]
    pub ext: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AuthConfig {
    #[serde(default)]
    pub auth_type: AuthType,
    /// Bearer token or API key. Plaintext, or `secret://{id}` / `secret://{id}/{account}`.
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub api_key_header: String,
    #[serde(default)]
    pub oauth2: OAuth2Config,
    #[serde(default)]
    pub extra: AuthExtra,
}

/// Approximate timing phases (ms). DNS/connect/TTFB approximated when not available from reqwest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TimingBreakdown {
    pub dns_ms: u128,
    pub connect_ms: u128,
    pub tls_ms: u128,
    pub ttfb_ms: u128,
    pub download_ms: u128,
    pub total_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestDraft {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub protocol: RequestProtocol,
    #[serde(default)]
    pub method: HttpMethod,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub params: Vec<KeyValue>,
    /// Path parameters (e.g. `:cabinetID` in `/cabinets/:cabinetID/stats`).
    #[serde(default)]
    pub path_params: Vec<KeyValue>,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub body_mode: BodyMode,
    #[serde(default)]
    pub body: String,
    /// Local file path when `body_mode == Binary`.
    #[serde(default)]
    pub binary_path: String,
    /// Manual sibling order (lower first). Used by DnD reorder.
    #[serde(default)]
    pub sort_key: i64,
    /// Used when `body_mode == Urlencoded`.
    #[serde(default)]
    pub body_urlencoded: Vec<KeyValue>,
    /// Used when `body_mode == FormData` (text fields; file = path when is_file).
    #[serde(default)]
    pub body_formdata: Vec<FormDataField>,
    /// GraphQL query (when body_mode == Graphql).
    #[serde(default)]
    pub graphql_query: String,
    /// GraphQL variables JSON object string.
    #[serde(default)]
    pub graphql_variables: String,
    #[serde(default)]
    pub auth: AuthConfig,
    /// Pre-request script (boa JS sandbox + helpers).
    #[serde(default)]
    pub pre_request_script: String,
    /// Post-response / test script (boa JS sandbox).
    #[serde(default)]
    pub test_script: String,
    #[serde(default)]
    pub settings: RequestSettings,
    /// Saved response examples for local mocks.
    #[serde(default)]
    pub examples: Vec<SavedExample>,
    /// WebSocket outbound message draft.
    #[serde(default)]
    pub ws_message: String,
    /// gRPC method path, e.g. `helloworld.Greeter/SayHello`.
    #[serde(default)]
    pub grpc_method: String,
    /// MQTT topic for subscribe/publish.
    #[serde(default)]
    pub mqtt_topic: String,
    #[serde(default)]
    pub mqtt_client_id: String,
    /// MCP STDIO command (executable).
    #[serde(default)]
    pub mcp_command: String,
    /// MCP args (whitespace-separated).
    #[serde(default)]
    pub mcp_args: String,
    #[serde(default)]
    pub mcp_tool_name: String,
    #[serde(default)]
    pub mcp_tool_args_json: String,
    /// Socket.IO event name to emit / listen.
    #[serde(default)]
    pub socketio_event: String,
    /// GraphQL-WS subscription query (when protocol == GraphqlWs).
    #[serde(default)]
    pub graphql_ws_query: String,
    /// Mock scenario: match example by this request header name (optional).
    #[serde(default)]
    pub mock_match_header: String,
    /// Mock scenario: match example by name substring (optional).
    #[serde(default)]
    pub mock_match_name: String,
    /// No-code response assertions (compiled to a test script at send time).
    #[serde(default)]
    pub assertions: Vec<super::assertions::Assertion>,
}

impl Default for RequestDraft {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: "Untitled Request".into(),
            description: String::new(),
            protocol: RequestProtocol::Http,
            method: HttpMethod::Get,
            url: "https://httpbin.org/get".into(),
            params: vec![KeyValue::new("", "")],
            path_params: Vec::new(),
            headers: vec![
                KeyValue::new("User-Agent", "PostmanRuntime/7.43.0"),
                KeyValue::new("Accept", "*/*"),
                KeyValue::new("Accept-Encoding", "gzip, deflate, br"),
                KeyValue::new("Connection", "keep-alive"),
                KeyValue::new("", ""),
            ],
            body_mode: BodyMode::None,
            body: String::new(),
            binary_path: String::new(),
            sort_key: 0,
            body_urlencoded: vec![KeyValue::new("", "")],
            body_formdata: vec![FormDataField::default()],
            graphql_query: String::new(),
            graphql_variables: "{}".into(),
            auth: AuthConfig::default(),
            pre_request_script: String::new(),
            test_script: String::new(),
            settings: RequestSettings::default(),
            examples: vec![],
            ws_message: String::new(),
            grpc_method: "helloworld.Greeter/SayHello".into(),
            mqtt_topic: "test/topic".into(),
            mqtt_client_id: "blitz".into(),
            mcp_command: String::new(),
            mcp_args: String::new(),
            mcp_tool_name: String::new(),
            mcp_tool_args_json: "{}".into(),
            socketio_event: "message".into(),
            graphql_ws_query: String::new(),
            mock_match_header: String::new(),
            mock_match_name: String::new(),
            assertions: Vec::new(),
        }
    }
}

impl RequestDraft {
    /// Apply environment-specific overrides to this request draft.
    /// Returns a new RequestDraft with overrides applied.
    pub fn apply_env_override(&self, override_: &crate::core::collection::RequestEnvOverride) -> Self {
        let mut resolved = self.clone();
        if let Some(url) = &override_.url {
            resolved.url = url.clone();
        }
        if let Some(method) = override_.method {
            resolved.method = method;
        }
        if !override_.headers.is_empty() {
            resolved.headers = override_.headers.clone();
        }
        if let Some(body_mode) = override_.body_mode {
            resolved.body_mode = body_mode;
        }
        if !override_.body_raw.is_empty() {
            resolved.body = override_.body_raw.clone();
        }
        if !override_.body_content_type.is_empty() {
            // Content-Type is stored in headers, update or add it
            let ct_key = "Content-Type";
            if let Some(h) = resolved.headers.iter_mut().find(|h| h.key.eq_ignore_ascii_case(ct_key)) {
                h.value = override_.body_content_type.clone();
            } else {
                resolved.headers.push(KeyValue::new(ct_key, override_.body_content_type.clone()));
            }
        }
        if !override_.body_urlencoded.is_empty() {
            resolved.body_urlencoded = override_.body_urlencoded.clone();
        }
        if !override_.body_formdata.is_empty() {
            resolved.body_formdata = override_.body_formdata.clone();
        }
        if !override_.graphql_query.is_empty() {
            resolved.graphql_query = override_.graphql_query.clone();
        }
        if !override_.graphql_variables.is_empty() {
            resolved.graphql_variables = override_.graphql_variables.clone();
        }
        if !override_.binary_path.is_empty() {
            resolved.binary_path = override_.binary_path.clone();
        }
        if let Some(auth) = &override_.auth {
            resolved.auth = auth.clone();
        }
        if !override_.pre_request_script.is_empty() {
            resolved.pre_request_script = override_.pre_request_script.clone();
        }
        if !override_.test_script.is_empty() {
            resolved.test_script = override_.test_script.clone();
        }
        resolved
    }

    /// Lower-cased blob of every string that makes this request identifiable
    /// (name + URL + method + params + headers + body + scripts + …).
    /// Used by the sidebar search box to match request *content*, not just the
    /// request's display name.
    pub fn search_text(&self) -> String {
        let cap = self.name.len() + self.url.len() + 256;
        let mut s = String::with_capacity(cap);
        s.push_str(&self.name.to_ascii_lowercase());
        s.push(' ');
        s.push_str(&self.url.to_ascii_lowercase());
        s.push(' ');
        s.push_str(self.method.as_str().to_ascii_lowercase().as_str());
        s.push(' ');
        for kv in &self.params {
            s.push_str(&kv.key.to_ascii_lowercase());
            s.push(' ');
            s.push_str(&kv.value.to_ascii_lowercase());
            s.push(' ');
        }
        for kv in &self.headers {
            s.push_str(&kv.key.to_ascii_lowercase());
            s.push(' ');
            s.push_str(&kv.value.to_ascii_lowercase());
            s.push(' ');
        }
        // Cap payload/scripts to 2048 bytes each to prevent UI freezes on massive payloads
        const MAX_PAYLOAD_SEARCH: usize = 2048;
        for text in [
            &self.body,
            &self.description,
            &self.pre_request_script,
            &self.test_script,
            &self.graphql_query,
            &self.graphql_variables,
            &self.ws_message,
            &self.grpc_method,
            &self.mqtt_topic,
            &self.mcp_command,
            &self.socketio_event,
            &self.graphql_ws_query,
        ] {
            if !text.is_empty() {
                let slice = if text.len() > MAX_PAYLOAD_SEARCH {
                    &text[..text.floor_char_boundary(MAX_PAYLOAD_SEARCH)]
                } else {
                    text.as_str()
                };
                s.push_str(&slice.to_ascii_lowercase());
                s.push(' ');
            }
        }
        s
    }
}

/// Test assertion result from `pm.test`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseView {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub elapsed_ms: u128,
    pub error: Option<String>,
    /// Pipeline stage notes (stubs that ran).
    pub pipeline_log: Vec<String>,
    /// Console-oriented script / pipeline messages.
    #[serde(default)]
    pub console_messages: Vec<String>,
    /// Test Results panel (stub entries when scripts present but not executed).
    #[serde(default)]
    pub test_results: Vec<TestResult>,
    /// Resolved URL actually requested.
    #[serde(default)]
    pub resolved_url: String,
    /// Timing waterfall (approximated when fine-grained phases unavailable).
    #[serde(default)]
    pub timing: TimingBreakdown,
    /// Visualizer HTML from `blitz.visualizer.set` or HTML response body.
    #[serde(default)]
    pub visualizer_html: String,
}

impl ResponseView {
    pub fn size_bytes(&self) -> usize {
        self.body.len()
    }

    pub fn error(msg: impl Into<String>, log: Vec<String>) -> Self {
        Self {
            status: 0,
            status_text: "ERROR".into(),
            headers: vec![],
            body: String::new(),
            elapsed_ms: 0,
            error: Some(msg.into()),
            pipeline_log: log.clone(),
            console_messages: log,
            test_results: vec![],
            resolved_url: String::new(),
            timing: TimingBreakdown::default(),
            visualizer_html: String::new(),
        }
    }
}

/// Split URL at '?' into base URL and parsed KeyValue query parameters.
pub fn extract_url_query_params(url: &str) -> (String, Vec<KeyValue>) {
    if let Some((base, query)) = url.split_once('?') {
        let mut params = Vec::new();
        for part in query.split('&') {
            if part.is_empty() {
                continue;
            }
            if let Some((k, v)) = part.split_once('=') {
                params.push(KeyValue {
                    key: k.to_string(),
                    value: v.to_string(),
                    enabled: true,
                });
            } else {
                params.push(KeyValue {
                    key: part.to_string(),
                    value: String::new(),
                    enabled: true,
                });
            }
        }
        (base.to_string(), params)
    } else {
        (url.to_string(), Vec::new())
    }
}

/// Rebuild URL with enabled KeyValue query parameters.
pub fn build_url_with_params(base_url: &str, params: &[KeyValue]) -> String {
    let clean_base = if let Some((base, _)) = base_url.split_once('?') {
        base
    } else {
        base_url
    };
    let enabled_parts: Vec<String> = params
        .iter()
        .filter(|p| p.enabled && (!p.key.is_empty() || !p.value.is_empty()))
        .map(|p| {
            if p.value.is_empty() {
                p.key.clone()
            } else {
                format!("{}={}", p.key, p.value)
            }
        })
        .collect();

    if enabled_parts.is_empty() {
        clean_base.to_string()
    } else {
        format!("{}?{}", clean_base, enabled_parts.join("&"))
    }
}

/// Extract the path portion of a URL (excluding scheme, host:port, query string, and fragment).
pub fn extract_url_path(url: &str) -> &str {
    let without_query = match url.split_once('?') {
        Some((b, _)) => b,
        None => match url.split_once('#') {
            Some((b, _)) => b,
            None => url,
        },
    };
    if let Some(pos) = without_query.find("://") {
        let after_scheme = &without_query[pos + 3..];
        if let Some(slash_pos) = after_scheme.find('/') {
            &after_scheme[slash_pos..]
        } else {
            ""
        }
    } else if let Some(slash_pos) = without_query.find('/') {
        &without_query[slash_pos..]
    } else {
        without_query
    }
}

/// Extract path variable keys from a URL (e.g. `/cabinets/:cabinetID/stats` -> `["cabinetID"]`).
pub fn extract_path_param_keys(url: &str) -> Vec<String> {
    let path = extract_url_path(url);
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"/(?P<token>:(?P<name>[a-zA-Z_][a-zA-Z0-9_]*))").unwrap()
    });
    let mut keys = Vec::new();
    for cap in re.captures_iter(path) {
        if let Some(m) = cap.name("name") {
            let key = m.as_str().to_string();
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    keys
}

/// Synchronize a list of path parameter key-values with the path variables found in the URL.
/// Preserves existing values and enabled flags for matching keys.
pub fn sync_path_params(url: &str, existing: &[KeyValue]) -> Vec<KeyValue> {
    let keys = extract_path_param_keys(url);
    keys.into_iter()
        .map(|k| {
            if let Some(found) = existing.iter().find(|e| e.key == k) {
                found.clone()
            } else {
                KeyValue::new(k, "")
            }
        })
        .collect()
}

/// Substitute path parameter values into the URL (e.g. `/cabinets/:cabinetID/stats` -> `/cabinets/123/stats`).
pub fn substitute_path_params(url: &str, path_params: &[KeyValue]) -> String {
    if path_params.is_empty() {
        return url.to_string();
    }
    let mut result = url.to_string();
    for p in path_params {
        if !p.enabled || p.key.trim().is_empty() || p.value.trim().is_empty() {
            continue;
        }
        let target = format!("/:{}", p.key.trim());
        let replacement = format!("/{}", p.value.trim());
        result = result.replace(&target, &replacement);

        let target_start = format!(":{}", p.key.trim());
        if result.starts_with(&target_start) {
            result = format!("{}{}", p.value.trim(), &result[target_start.len()..]);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_and_build_query_params() {
        let url = "https://api.example.com/v1/users?role=admin&active=true&empty";
        let (base, params) = extract_url_query_params(url);
        assert_eq!(base, "https://api.example.com/v1/users");
        assert_eq!(params.len(), 3);
        assert_eq!(params[0].key, "role");
        assert_eq!(params[0].value, "admin");
        assert_eq!(params[1].key, "active");
        assert_eq!(params[1].value, "true");
        assert_eq!(params[2].key, "empty");
        assert_eq!(params[2].value, "");

        let rebuilt = build_url_with_params(&base, &params);
        assert_eq!(rebuilt, url);

        // Test with disabled param
        let mut modified = params.clone();
        modified[1].enabled = false;
        let rebuilt2 = build_url_with_params(&base, &modified);
        assert_eq!(rebuilt2, "https://api.example.com/v1/users?role=admin&empty");
    }

    #[test]
    fn test_extract_url_without_query() {
        let url = "https://api.example.com/v1/users";
        let (base, params) = extract_url_query_params(url);
        assert_eq!(base, url);
        assert!(params.is_empty());
        assert_eq!(build_url_with_params(&base, &params), url);
    }

    #[test]
    fn test_path_params_lifecycle() {
        let url = "https://api.example.com:8080/cabinets/:cabinetID/stats";
        let keys = extract_path_param_keys(url);
        assert_eq!(keys, vec!["cabinetID"]);

        let synced = sync_path_params(url, &[]);
        assert_eq!(synced.len(), 1);
        assert_eq!(synced[0].key, "cabinetID");
        assert_eq!(synced[0].value, "");

        let populated = vec![KeyValue::new("cabinetID", "123")];
        let substituted = substitute_path_params(url, &populated);
        assert_eq!(
            substituted,
            "https://api.example.com:8080/cabinets/123/stats"
        );

        // Multiple path params
        let url2 = "/api/:version/users/:userId/posts/:postId?sort=desc";
        let keys2 = extract_path_param_keys(url2);
        assert_eq!(keys2, vec!["version", "userId", "postId"]);

        let populated2 = vec![
            KeyValue::new("version", "v1"),
            KeyValue::new("userId", "42"),
            KeyValue::new("postId", "999"),
        ];
        let sub2 = substitute_path_params(url2, &populated2);
        assert_eq!(sub2, "/api/v1/users/42/posts/999?sort=desc");
    }
}
