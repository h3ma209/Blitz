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
    NoAuth,
    Inherit,
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
    pub grant: OAuth2Grant,
    pub auth_url: String,
    pub token_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub scope: String,
    /// Loopback redirect, e.g. http://127.0.0.1:8765/callback
    pub redirect_uri: String,
    /// Cached access token after Get Token.
    pub access_token: String,
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
    pub key: String,
    /// OAuth1 consumer secret / Hawk key / EdgeGrid client secret / AWS secret key.
    pub secret: String,
    /// OAuth1 token / EdgeGrid access token / AWS session token.
    pub token: String,
    /// OAuth1 token secret / EdgeGrid host (base URL host).
    pub token_secret: String,
    /// OAuth1 signature method (HMAC-SHA1 / HMAC-SHA256 / PLAINTEXT).
    pub signature_method: String,
    /// AWS region (default us-east-1).
    pub region: String,
    /// AWS service name (default execute-api).
    pub service: String,
    /// Hawk algorithm (sha256 / sha1).
    pub algorithm: String,
    /// Hawk ext / optional.
    pub ext: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AuthConfig {
    pub auth_type: AuthType,
    /// Bearer token or API key. Plaintext, or `secret://{id}` / `secret://{id}/{account}`.
    pub token: String,
    pub username: String,
    pub password: String,
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
    pub method: HttpMethod,
    pub url: String,
    pub params: Vec<KeyValue>,
    pub headers: Vec<KeyValue>,
    pub body_mode: BodyMode,
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
    pub auth: AuthConfig,
    /// Pre-request script (boa JS sandbox + helpers).
    pub pre_request_script: String,
    /// Post-response / test script (boa JS sandbox).
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
            headers: vec![
                KeyValue::new("Accept", "application/json"),
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
    /// Visualizer HTML from `pm.visualizer.set` or HTML response body.
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
