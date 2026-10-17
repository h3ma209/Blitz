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
}

impl RequestProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "HTTP",
            Self::WebSocket => "WebSocket",
            Self::Grpc => "gRPC",
            Self::Mqtt => "MQTT",
            Self::Mcp => "MCP",
        }
    }

    pub fn all() -> &'static [RequestProtocol] {
        &[
            Self::Http,
            Self::WebSocket,
            Self::Grpc,
            Self::Mqtt,
            Self::Mcp,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestDraft {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub protocol: RequestProtocol,
    pub method: HttpMethod,
    pub url: String,
    pub params: Vec<KeyValue>,
    pub headers: Vec<KeyValue>,
    pub body_mode: BodyMode,
    pub body: String,
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
}

impl Default for RequestDraft {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: "Untitled Request".into(),
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
            mqtt_client_id: "notpostman".into(),
            mcp_command: String::new(),
            mcp_args: String::new(),
            mcp_tool_name: String::new(),
            mcp_tool_args_json: "{}".into(),
        }
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
        }
    }
}
