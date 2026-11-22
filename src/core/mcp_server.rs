//! Built-in Model Context Protocol (MCP) Server for Blitz.
//!
//! Enables AI agents (Antigravity, Gemini, Claude Desktop, Cursor, Windsurf, etc.)
//! to interact directly with Blitz collections, environments, and execution runtime
//! over standard I/O using JSON-RPC 2.0.

use std::collections::HashMap;
use std::io::{self, BufRead, Read, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use serde_json::{json, Value as JsonValue};
use uuid::Uuid;

use crate::core::assertions::{assertions_to_script, suggest_from_response};
use crate::core::codegen::{generate_snippet_resolved, snippet_languages};
use crate::core::collection::{
    find_request_across, flatten_collections, remove_across, Collection, CollectionItem, Folder,
};
use crate::core::collection_lint::{lint_collections, lint_error_count, LintSeverity};
use crate::core::cookies::Cookie;
use crate::core::doc_gen::{generate_static_doc_html, generate_swagger_ui_html};
use crate::core::env_diff::{compare_environments, export_diff_markdown};
use crate::core::environment::{EnvVar, Environment};
use crate::core::history::{push_history, HistoryEntry, HISTORY_MAX};
use crate::core::impex::{
    export_collection_v21, export_openapi3, import_collection_v21, import_curl, import_har,
    import_openapi3, reconcile_openapi3_collection,
};
use crate::core::mock_server::{routes_from_collections, start_mock, MockHandle, MockRoute};
use crate::core::perf_runner::run_performance;
use crate::core::protocols::{
    graphql_ws_subscribe_with_headers, grpc_unary_with_headers, mcp_call_tool, mcp_list_tools,
    mqtt_publish_opts, mqtt_subscribe_once, socketio_session, split_mcp_args, sse_listen,
    ws_connect_session_with_headers, WsSession, WsSessionEvent,
};
use crate::core::request::{
    compact_response_for_disk, AuthConfig, AuthType, BodyMode, FormDataField, HttpMethod,
    KeyValue, RequestDraft, RequestProtocol, SavedExample,
};
use crate::core::response_diff::{diff_bodies, DiffLine};
use crate::core::VariableContext;
use crate::runtime::pipeline::{send_request_with_options, SendOptions};
use crate::runtime::runner::{run_collection_with_config, RunnerConfig};
use crate::storage::{AppSnapshot, JsonStore};

static ACTIVE_MOCK: Mutex<Option<MockHandle>> = Mutex::new(None);

const MAX_MCP_WS_SESSIONS: usize = 32;

struct McpWsSession {
    url: String,
    session: WsSession,
    cursor: usize,
}

static WS_SESSIONS: Mutex<Option<HashMap<String, McpWsSession>>> = Mutex::new(None);

/// Entry point to start the Blitz MCP server on stdio.
/// Reads JSON-RPC requests line-by-line from stdin and writes responses to stdout.
/// Logs and debug messages are routed to stderr to preserve standard stdio protocol framing.
pub fn run_stdio_server() -> i32 {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    eprintln!("[blitz-mcp] Server started (JSON-RPC 2.0, protocol 2024-11-05)");

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                // Support both standard newline-delimited JSON-RPC and Content-Length LSP framing
                let owned_body: Option<String> = if let Some(len_str) =
                    trimmed.to_ascii_lowercase().strip_prefix("content-length:")
                {
                    let mut content_len: usize = len_str.trim().parse().unwrap_or(0);
                    loop {
                        let mut hdr = String::new();
                        match reader.read_line(&mut hdr) {
                            Ok(0) => {
                                content_len = 0;
                                break;
                            }
                            Ok(_) => {
                                if hdr == "\r\n" || hdr == "\n" {
                                    break;
                                }
                                if let Some(v) = hdr
                                    .trim()
                                    .to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                {
                                    content_len = v.trim().parse().unwrap_or(content_len);
                                }
                            }
                            Err(_) => {
                                content_len = 0;
                                break;
                            }
                        }
                    }
                    if content_len == 0 || content_len > 32 * 1024 * 1024 {
                        continue;
                    }
                    let mut buf = vec![0u8; content_len];
                    if reader.read_exact(&mut buf).is_err() {
                        break;
                    }
                    Some(String::from_utf8_lossy(&buf).into_owned())
                } else {
                    None
                };

                let json_str = if let Some(ref b) = owned_body {
                    b.trim()
                } else if trimmed.starts_with('{') {
                    trimmed
                } else if let Some(idx) = trimmed.find('{') {
                    &trimmed[idx..]
                } else {
                    continue;
                };

                let parsed: Result<JsonValue, _> = serde_json::from_str(json_str);
                match parsed {
                    Ok(req) => {
                        if let Some(resp) = handle_mcp_message(&req) {
                            if let Ok(serialized) = serde_json::to_string(&resp) {
                                let _ = writeln!(writer, "{}", serialized);
                                let _ = writer.flush();
                            }
                        }
                    }
                    Err(err) => {
                        eprintln!("[blitz-mcp] Failed to parse JSON-RPC: {err}");
                        let err_resp = json!({
                            "jsonrpc": "2.0",
                            "id": null,
                            "error": {
                                "code": -32700,
                                "message": "Parse error",
                                "data": err.to_string()
                            }
                        });
                        let _ = writeln!(writer, "{}", err_resp);
                        let _ = writer.flush();
                    }
                }
            }
            Err(e) => {
                eprintln!("[blitz-mcp] Stdin read error: {e}");
                break;
            }
        }
    }

    eprintln!("[blitz-mcp] Server stopped");
    0
}

/// Dispatch incoming JSON-RPC message.
/// Returns Some(JsonValue) if a response is expected; None for notifications.
pub fn handle_mcp_message(req: &JsonValue) -> Option<JsonValue> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");

    // Notifications (no "id") do not receive responses
    if id.is_none() && method.starts_with("notifications/") {
        return None;
    }

    let req_id = id.unwrap_or(JsonValue::Null);

    match method {
        "initialize" => {
            let result = json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    },
                    "resources": {
                        "subscribe": false,
                        "listChanged": false
                    }
                },
                "serverInfo": {
                    "name": "blitz",
                    "version": env!("CARGO_PKG_VERSION")
                }
            });
            Some(json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": result
            }))
        }
        "ping" => Some(json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {}
        })),
        "tools/list" => {
            let tools = list_mcp_tools();
            Some(json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "tools": tools
                }
            }))
        }
        "tools/call" => {
            let params = req.get("params").cloned().unwrap_or(json!({}));
            let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

            let tool_res = execute_mcp_tool(tool_name, &arguments);
            match tool_res {
                Ok(content) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "content": [
                            {
                                "type": "text",
                                "text": content
                            }
                        ]
                    }
                })),
                Err(err_msg) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "isError": true,
                        "content": [
                            {
                                "type": "text",
                                "text": format!("Error: {err_msg}")
                            }
                        ]
                    }
                })),
            }
        }
        "resources/list" => {
            let resources = list_mcp_resources();
            Some(json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "resources": resources
                }
            }))
        }
        "resources/read" => {
            let params = req.get("params").cloned().unwrap_or(json!({}));
            let uri = params.get("uri").and_then(|u| u.as_str()).unwrap_or("");
            match read_mcp_resource(uri) {
                Ok((mime, content)) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "contents": [
                            {
                                "uri": uri,
                                "mimeType": mime,
                                "text": content
                            }
                        ]
                    }
                })),
                Err(err) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "error": {
                        "code": -32602,
                        "message": format!("Resource not found: {err}")
                    }
                })),
            }
        }
        _ => Some(json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "error": {
                "code": -32601,
                "message": format!("Method not found: {method}")
            }
        })),
    }
}

/// Tools exposed by Blitz to agents.
fn list_mcp_tools() -> Vec<JsonValue> {
    vec![
        json!({
            "name": "blitz_list_collections",
            "description": "Lists all Blitz API collections, folders, and request endpoints with their methods and IDs.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "filter": {
                        "type": "string",
                        "description": "Optional search term to filter collections or requests by name or URL."
                    }
                }
            }
        }),
        json!({
            "name": "blitz_get_request",
            "description": "Get detailed configuration of a specific saved request (URL, headers, body, auth, and query params) by request ID or name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "request_id": {
                        "type": "string",
                        "description": "The UUID of the request or the exact/case-insensitive name of the request."
                    },
                    "collection": {
                        "type": "string",
                        "description": "Optional collection name to disambiguate requests with identical names."
                    }
                },
                "required": ["request_id"]
            }
        }),
        json!({
            "name": "blitz_send_request",
            "description": "Executes a saved Blitz request by ID or name using the Blitz pipeline (resolving variables, running pre-request and test scripts, managing cookies), and returns status, headers, body, parsed JSON, and test results.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "request_id": {
                        "type": "string",
                        "description": "UUID or name of the request to send."
                    },
                    "collection": {
                        "type": "string",
                        "description": "Optional collection name or UUID to disambiguate identical request names."
                    },
                    "environment": {
                        "type": "string",
                        "description": "Optional name or UUID of the environment to apply (e.g. 'Development', 'Production')."
                    },
                    "override_vars": {
                        "type": "object",
                        "description": "Optional key-value pairs to override variables during this execution."
                    },
                    "save_to_env": {
                        "type": "object",
                        "description": "Optional mapping of environment variable names to response JSON paths or headers (e.g. {\"token\": \"data.token\", \"user_id\": \"data.id\"}) to automatically extract and persist to active environment."
                    }
                },
                "required": ["request_id"]
            }
        }),
        json!({
            "name": "blitz_execute_http",
            "description": "Executes an ad-hoc HTTP request directly through the Blitz networking engine (with variable resolution, cookie jar, and SSL support).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "method": {
                        "type": "string",
                        "description": "HTTP method (GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS).",
                        "default": "GET"
                    },
                    "url": {
                        "type": "string",
                        "description": "Full target URL (supports {{variable}} interpolations)."
                    },
                    "headers": {
                        "type": "object",
                        "description": "Optional key-value map of HTTP headers."
                    },
                    "body": {
                        "type": "string",
                        "description": "Optional raw request body (e.g. JSON string)."
                    },
                    "body_mode": {
                        "type": "string",
                        "description": "Optional body mode: none, raw, urlencoded, form-data, graphql, binary."
                    },
                    "form_data": {
                        "type": "array",
                        "description": "Optional multipart/form-data fields (auto-selects 'form-data' mode). Each item supports {key, value?, file_path?, type?: 'text'|'file', content_type?, enabled?}.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "key": { "type": "string" },
                                "value": { "type": "string" },
                                "file_path": { "type": "string" },
                                "type": { "type": "string", "enum": ["text", "file"] },
                                "content_type": { "type": "string" },
                                "enabled": { "type": "boolean" }
                            },
                            "required": ["key"]
                        }
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Optional local file path for 'binary' body_mode (auto-selects 'binary' mode if body_mode omitted)."
                    },
                    "graphql_query": {
                        "type": "string",
                        "description": "Optional GraphQL query string (automatically uses GraphQL body mode if supplied)."
                    },
                    "graphql_variables": {
                        "type": "string",
                        "description": "Optional JSON variables string for GraphQL requests."
                    },
                    "environment": {
                        "type": "string",
                        "description": "Optional environment name to resolve {{variables}} against."
                    },
                    "override_vars": {
                        "type": "object",
                        "description": "Optional key-value pairs to override {{variables}} for this request."
                    },
                    "save_to_env": {
                        "type": "object",
                        "description": "Optional mapping of environment variable names to response JSON paths or headers (e.g. {\"token\": \"data.token\"}) to automatically extract and persist to active environment."
                    }
                },
                "required": ["url"]
            }
        }),
        json!({
            "name": "blitz_list_environments",
            "description": "Lists all configured environments in Blitz (e.g. Local, Development, Production) and their variable keys.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "include_values": {
                        "type": "boolean",
                        "description": "Whether to include variable effective values in the output (default false)."
                    }
                }
            }
        }),
        json!({
            "name": "blitz_set_env_var",
            "description": "Sets or updates an environment variable in a Blitz environment. If variable exists, its value is updated; otherwise created.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Variable name (e.g. 'token', 'baseUrl')."
                    },
                    "value": {
                        "type": "string",
                        "description": "Variable value."
                    },
                    "environment": {
                        "type": "string",
                        "description": "Optional target environment name or UUID (defaults to active environment)."
                    },
                    "enabled": {
                        "type": "boolean",
                        "description": "Whether the variable is enabled (default true)."
                    }
                },
                "required": ["key", "value"]
            }
        }),
        json!({
            "name": "blitz_set_active_environment",
            "description": "Switches the active environment in Blitz, or clears it ('none').",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "environment": {
                        "type": "string",
                        "description": "Name or UUID of environment to activate, or 'none' / empty string to clear."
                    }
                },
                "required": ["environment"]
            }
        }),
        json!({
            "name": "blitz_upsert_request",
            "description": "Creates or updates an API request draft in a Blitz collection or folder.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "collection": {
                        "type": "string",
                        "description": "Name or UUID of collection. Created if it does not exist."
                    },
                    "name": {
                        "type": "string",
                        "description": "Display name of the request."
                    },
                    "method": {
                        "type": "string",
                        "description": "HTTP method (GET, POST, PUT, DELETE, PATCH, etc.). Default GET.",
                        "default": "GET"
                    },
                    "url": {
                        "type": "string",
                        "description": "Request URL endpoint (supports {{variable}} interpolations)."
                    },
                    "folder": {
                        "type": "string",
                        "description": "Optional folder name inside the collection. Created if it does not exist."
                    },
                    "headers": {
                        "type": "object",
                        "description": "Optional key-value map of HTTP headers."
                    },
                    "params": {
                        "type": "object",
                        "description": "Optional key-value map of URL query parameters."
                    },
                    "body": {
                        "type": "string",
                        "description": "Optional raw request body. Automatically sets body_mode to 'raw' when non-empty unless body_mode is specified."
                    },
                    "body_mode": {
                        "type": "string",
                        "description": "Optional body mode: none, raw, urlencoded, form-data, graphql, binary. Defaults to 'raw' when non-empty body is supplied."
                    },
                    "protocol": {
                        "type": "string",
                        "description": "Optional protocol: http (default), websocket, grpc, sse, mqtt, socketio, graphql_ws, mcp."
                    },
                    "auth": {
                        "type": "object",
                        "description": "Optional auth configuration (e.g. {\"type\":\"bearer\",\"token\":\"...\"}, {\"type\":\"basic\",\"username\":\"...\",\"password\":\"...\"}, {\"type\":\"api_key\",\"header\":\"X-API-Key\",\"token\":\"...\" or \"type\":\"no_auth\"})."
                    },
                    "pre_request_script": {
                        "type": "string",
                        "description": "Optional JS pre-request script (blitz sandbox)."
                    },
                    "test_script": {
                        "type": "string",
                        "description": "Optional JS test/assertion script (blitz.test / blitz.expect)."
                    },
                    "graphql_query": {
                        "type": "string",
                        "description": "Optional GraphQL HTTP query string."
                    },
                    "graphql_ws_query": {
                        "type": "string",
                        "description": "Optional GraphQL-WS subscription query string."
                    },
                    "graphql_variables": {
                        "type": "string",
                        "description": "Optional GraphQL variables JSON string."
                    },
                    "grpc_method": {
                        "type": "string",
                        "description": "Optional gRPC method (package.Service/Method)."
                    },
                    "mqtt_topic": {
                        "type": "string",
                        "description": "Optional MQTT topic."
                    },
                    "socketio_event": {
                        "type": "string",
                        "description": "Optional Socket.IO event name."
                    },
                    "description": {
                        "type": "string",
                        "description": "Optional documentation / description for the request."
                    },
                    "request_id": {
                        "type": "string",
                        "description": "Optional UUID to update an existing request explicitly."
                    }
                },
                "required": ["collection", "name", "url"]
            }
        }),
        json!({
            "name": "blitz_import_curl",
            "description": "Imports a raw cURL command into a Blitz request draft, optionally saving it into a collection.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "curl": {
                        "type": "string",
                        "description": "The raw cURL command string."
                    },
                    "collection": {
                        "type": "string",
                        "description": "Optional collection name or UUID to save the imported request into."
                    },
                    "name": {
                        "type": "string",
                        "description": "Optional custom name for the imported request."
                    }
                },
                "required": ["curl"]
            }
        }),
        json!({
            "name": "blitz_generate_code",
            "description": "Generates client code snippet for a saved request or ad-hoc URL in cURL, Python (httpx/requests), TypeScript (axios), JavaScript fetch, Rust (reqwest), Go, etc.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "language": {
                        "type": "string",
                        "description": "Target language: 'curl', 'python', 'requests', 'typescript', 'javascript', 'rust', 'go', 'java', 'csharp'."
                    },
                    "request_id": {
                        "type": "string",
                        "description": "UUID or name of an existing request in Blitz."
                    },
                    "method": {
                        "type": "string",
                        "description": "HTTP method for ad-hoc snippet (default 'GET')."
                    },
                    "url": {
                        "type": "string",
                        "description": "URL for ad-hoc snippet (required if request_id not provided)."
                    },
                    "headers": {
                        "type": "object",
                        "description": "Optional headers map for ad-hoc snippet."
                    },
                    "body": {
                        "type": "string",
                        "description": "Optional body for ad-hoc snippet."
                    },
                    "environment": {
                        "type": "string",
                        "description": "Optional environment name to interpolate variables."
                    }
                },
                "required": ["language"]
            }
        }),
        json!({
            "name": "blitz_list_history",
            "description": "Lists recent HTTP requests executed in Blitz with status codes, latency, and timestamps.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of recent history entries to return (default 20)."
                    }
                }
            }
        }),
        json!({
            "name": "blitz_run_collection",
            "description": "Runs all requests in a collection (or scoped folder) sequentially, executing assertions and returning a complete test pass/fail report.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "collection": {
                        "type": "string",
                        "description": "UUID or name of the collection to run."
                    },
                    "folder": {
                        "type": "string",
                        "description": "Optional folder name or slash-separated path within collection (e.g. 'User/Account'). Scopes execution to only requests in this folder."
                    },
                    "environment": {
                        "type": "string",
                        "description": "Optional environment name or UUID to use for variables."
                    },
                    "iterations": {
                        "type": "integer",
                        "description": "Optional iteration count (1..=50, default 1)."
                    },
                    "delay_ms": {
                        "type": "integer",
                        "description": "Optional delay between requests in milliseconds (0..=5000, default 0)."
                    }
                },
                "required": ["collection"]
            }
        }),
        json!({
            "name": "blitz_suggest_assertions",
            "description": "Analyzes an HTTP response (or request execution) and generates recommended assertions (status code, content-type header, JSON path presence/equality, response time budget) and compiled JavaScript test script. Optionally applies generated tests directly to the request.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "request_id": {
                        "type": "string",
                        "description": "Optional UUID or name of a saved request. If status/body are omitted, Blitz runs this request or uses its latest history to suggest assertions."
                    },
                    "status": {
                        "type": "integer",
                        "description": "Optional HTTP status code (e.g. 200) to base assertions on."
                    },
                    "body": {
                        "type": "string",
                        "description": "Optional response body string (typically JSON) to generate path assertions from."
                    },
                    "headers": {
                        "type": "object",
                        "description": "Optional response headers map (e.g. {\"content-type\": \"application/json\"})."
                    },
                    "elapsed_ms": {
                        "type": "integer",
                        "description": "Optional observed response latency in milliseconds to calculate time budget ceiling."
                    },
                    "apply_to_request": {
                        "type": "boolean",
                        "description": "If true and request_id is provided, automatically appends the generated test script/assertions to the request."
                    }
                }
            }
        }),
        json!({
            "name": "blitz_perf_test",
            "description": "Runs a concurrent load/performance benchmark against a saved request or collection for a specified duration and virtual users (VUs). Returns throughput (RPS), total requests, error rate, and average/min/max latencies.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "request_id": {
                        "type": "string",
                        "description": "UUID or name of the request to benchmark."
                    },
                    "collection": {
                        "type": "string",
                        "description": "UUID or name of the collection to benchmark (if request_id is not specified)."
                    },
                    "vus": {
                        "type": "integer",
                        "description": "Number of concurrent virtual users (workers). Defaults to 5 (max 50)."
                    },
                    "duration_seconds": {
                        "type": "integer",
                        "description": "Duration in seconds to run load test. Defaults to 5 (max 60)."
                    },
                    "environment": {
                        "type": "string",
                        "description": "Optional environment name or UUID to resolve variables."
                    }
                }
            }
        }),
        json!({
            "name": "blitz_sync_openapi",
            "description": "Imports or syncs an OpenAPI 3.0 specification (raw YAML/JSON or local file path) into a Blitz collection. If the collection already exists, non-destructively reconciles new routes and updates schemas while preserving custom tests, scripts, and auth.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "spec": {
                        "type": "string",
                        "description": "Raw OpenAPI 3.0 JSON/YAML specification string or an absolute local file path to the spec."
                    },
                    "collection": {
                        "type": "string",
                        "description": "Optional collection name to sync into. If omitted, uses the OpenAPI document title."
                    }
                },
                "required": ["spec"]
            }
        }),
        json!({
            "name": "blitz_get_collection",
            "description": "Retrieves the complete configuration of an entire collection (all folders, request definitions, authentication, variables, pre-request and test scripts) by collection name or UUID.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "collection": {
                        "type": "string",
                        "description": "UUID or name of the collection to fetch."
                    }
                },
                "required": ["collection"]
            }
        }),
        json!({
            "name": "blitz_create_environment",
            "description": "Creates a new named environment with initial key-value variables and can optionally set it as the active environment.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name for the new environment (e.g. 'Local Docker', 'Staging AWS')."
                    },
                    "variables": {
                        "type": "object",
                        "description": "Key-value pairs to set in this environment (e.g. {\"baseUrl\": \"http://localhost:8080\", \"apiKey\": \"test\"})."
                    },
                    "set_active": {
                        "type": "boolean",
                        "description": "Whether to switch to this environment as the active environment immediately. Defaults to false."
                    }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "blitz_delete_request",
            "description": "Deletes a saved request from a Blitz collection by its request UUID or name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "request_id": {
                        "type": "string",
                        "description": "UUID or exact name of the request to delete."
                    },
                    "collection": {
                        "type": "string",
                        "description": "Optional collection name or UUID to disambiguate identical request names."
                    }
                },
                "required": ["request_id"]
            }
        }),
        json!({
            "name": "blitz_start_mock",
            "description": "Starts a local HTTP mock server on localhost (127.0.0.1) serving responses matched against saved examples or route definitions from Blitz collections. Returns the bound port and mock URL.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "collection": {
                        "type": "string",
                        "description": "Optional collection name or UUID to limit mock routes to. If omitted, routes from all collections with examples are served."
                    },
                    "port": {
                        "type": "integer",
                        "description": "Optional port to bind on (e.g. 8080). Defaults to 0 for automatic ephemeral port assignment."
                    }
                }
            }
        }),
        json!({
            "name": "blitz_stop_mock",
            "description": "Stops the currently running Blitz local mock HTTP server.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "blitz_export_collection",
            "description": "Exports a Blitz collection into Postman Collection v2.1 or OpenAPI 3.0 specification format. Optionally writes directly to a destination file path.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "collection": {
                        "type": "string",
                        "description": "UUID or name of the collection to export."
                    },
                    "format": {
                        "type": "string",
                        "enum": ["postman", "openapi"],
                        "description": "Export format: 'postman' (default) for Postman v2.1 JSON, or 'openapi' for OpenAPI 3.0.3 specification JSON."
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Optional destination file path to write exported content directly."
                    }
                },
                "required": ["collection"]
            }
        }),
        json!({
            "name": "blitz_diff_responses",
            "description": "Performs line-by-line canonical diff comparison between two response bodies, or executes a saved request across two different environments (e.g. staging vs local) and diffs their status, headers, and bodies.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "body_a": {
                        "type": "string",
                        "description": "First response body text to compare."
                    },
                    "body_b": {
                        "type": "string",
                        "description": "Second response body text to compare."
                    },
                    "request_id": {
                        "type": "string",
                        "description": "Optional request UUID or name to execute live across two environments for comparison."
                    },
                    "environment_a": {
                        "type": "string",
                        "description": "First environment name or UUID (when using request_id)."
                    },
                    "environment_b": {
                        "type": "string",
                        "description": "Second environment name or UUID (when using request_id)."
                    }
                }
            }
        }),
        json!({
            "name": "blitz_delete_collection",
            "description": "Permanently deletes an entire collection by its name or UUID.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "collection": {
                        "type": "string",
                        "description": "UUID or name of the collection to delete."
                    }
                },
                "required": ["collection"]
            }
        }),
        json!({
            "name": "blitz_generate_docs",
            "description": "Compiles a Blitz collection into standalone, self-contained HTML API documentation (choice of responsive static interactive docs website or Swagger UI). Can return raw HTML or write directly to a local file path.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "collection": {
                        "type": "string",
                        "description": "UUID or name of the collection to generate docs for."
                    },
                    "format": {
                        "type": "string",
                        "enum": ["static", "swagger_ui"],
                        "description": "Documentation format: 'static' (default, single-file glassmorphic doc site with search & code snippets) or 'swagger_ui'."
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Optional destination file path to write HTML output directly (e.g. 'docs/api.html')."
                    }
                },
                "required": ["collection"]
            }
        }),
        json!({
            "name": "blitz_create_folder",
            "description": "Creates a new organizational folder inside a Blitz collection (optionally nested under a parent folder) with an optional description.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "collection": {
                        "type": "string",
                        "description": "UUID or name of the collection to add the folder to."
                    },
                    "name": {
                        "type": "string",
                        "description": "Name for the new folder (e.g. 'Auth', 'Users', 'Payments')."
                    },
                    "parent_folder": {
                        "type": "string",
                        "description": "Optional parent folder name or path to nest under."
                    },
                    "description": {
                        "type": "string",
                        "description": "Optional description of the folder's purpose."
                    }
                },
                "required": ["collection", "name"]
            }
        }),
        json!({
            "name": "blitz_delete_folder",
            "description": "Deletes a folder and all requests contained inside it from a Blitz collection.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "collection": {
                        "type": "string",
                        "description": "UUID or name of the collection containing the folder."
                    },
                    "folder": {
                        "type": "string",
                        "description": "Name or path of the folder to delete (e.g. 'Users' or 'Users/Auth')."
                    }
                },
                "required": ["collection", "folder"]
            }
        }),
        json!({
            "name": "blitz_delete_environment",
            "description": "Deletes an environment by its name or UUID.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "environment": {
                        "type": "string",
                        "description": "UUID or name of the environment to delete."
                    }
                },
                "required": ["environment"]
            }
        }),
        json!({
            "name": "blitz_clear_history",
            "description": "Clears saved request execution history (optionally filtered to only entries for a specific request ID).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "request_id": {
                        "type": "string",
                        "description": "Optional request UUID to selectively clear only history for that request. If omitted, clears all execution history."
                    }
                }
            }
        }),
        json!({
            "name": "blitz_import_har",
            "description": "Imports HTTP requests and responses from an HTTP Archive (.har) JSON string or file path into a Blitz collection.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "har": {
                        "type": "string",
                        "description": "Raw HAR JSON content or absolute file path to a .har file."
                    },
                    "collection": {
                        "type": "string",
                        "description": "Optional name for the imported collection. Defaults to 'HAR Import'."
                    }
                },
                "required": ["har"]
            }
        }),
        json!({
            "name": "blitz_manage_cookies",
            "description": "Inspects, sets, or clears session cookies stored in Blitz's global cookie jar.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "set", "clear"],
                        "description": "Action to perform: 'list' (default) to inspect cookies, 'set' to add/update a cookie, or 'clear' to wipe cookies (optionally scoped by domain)."
                    },
                    "domain": {
                        "type": "string",
                        "description": "Domain name for 'set' (e.g. '127.0.0.1' or 'api.example.com') or optional domain filter for 'list'/'clear'."
                    },
                    "name": {
                        "type": "string",
                        "description": "Cookie name (required when action is 'set')."
                    },
                    "value": {
                        "type": "string",
                        "description": "Cookie value (required when action is 'set')."
                    },
                    "path": {
                        "type": "string",
                        "description": "Cookie path (defaults to '/' when action is 'set')."
                    }
                }
            }
        }),
        json!({
            "name": "blitz_websocket_send",
            "description": "Connects to a WebSocket server (ws:// or wss://), transmits one or more text/JSON payloads (with {{variable}} interpolation), captures streaming inbound messages, and gracefully disconnects.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "WebSocket endpoint URL (supports {{variable}} interpolation)."
                    },
                    "message": {
                        "type": "string",
                        "description": "Optional single text or JSON message string to transmit after connection handshake."
                    },
                    "messages": {
                        "type": "array",
                        "description": "Optional ordered sequence of messages to send on the same socket. Each item may be a string or {message: string, delay_ms?: integer}.",
                        "items": {
                            "oneOf": [
                                { "type": "string" },
                                {
                                    "type": "object",
                                    "properties": {
                                        "message": { "type": "string" },
                                        "delay_ms": { "type": "integer" }
                                    },
                                    "required": ["message"]
                                }
                            ]
                        }
                    },
                    "headers": {
                        "type": "object",
                        "description": "Optional handshake HTTP headers dictionary (supports {{variable}} interpolation)."
                    },
                    "environment": {
                        "type": "string",
                        "description": "Optional environment name or UUID to resolve {{variables}} against."
                    },
                    "override_vars": {
                        "type": "object",
                        "description": "Optional key-value pairs to override {{variables}} for this call."
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Duration in milliseconds to wait after sending to collect inbound messages. Defaults to 2000 (max 30000)."
                    },
                    "max_messages": {
                        "type": "integer",
                        "description": "Maximum number of inbound messages to collect before closing session. Defaults to 5 (max 200)."
                    }
                },
                "required": ["url"]
            }
        }),
        json!({
            "name": "blitz_websocket_connect",
            "description": "Opens and keeps alive a persistent WebSocket session (supports concurrent multi-client sessions and {{variable}} interpolation). Returns a session_id for blitz_websocket_session_send, blitz_websocket_read, and blitz_websocket_close.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "WebSocket endpoint URL (supports {{variable}} interpolation)."
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Optional custom session identifier (e.g. 'user_a'). Auto-generated UUID if omitted."
                    },
                    "headers": {
                        "type": "object",
                        "description": "Optional handshake HTTP headers dictionary (supports {{variable}} interpolation)."
                    },
                    "environment": {
                        "type": "string",
                        "description": "Optional environment name or UUID to resolve {{variables}} against."
                    },
                    "override_vars": {
                        "type": "object",
                        "description": "Optional key-value pairs to override {{variables}} for this session."
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Handshake timeout in milliseconds. Defaults to 3000 (max 15000)."
                    }
                },
                "required": ["url"]
            }
        }),
        json!({
            "name": "blitz_websocket_session_send",
            "description": "Sends one or more text/JSON messages over an open persistent WebSocket session without closing it, and returns any newly received inbound messages.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Active session ID returned by blitz_websocket_connect."
                    },
                    "message": {
                        "type": "string",
                        "description": "Single text or JSON message to send (supports {{variable}} interpolation)."
                    },
                    "messages": {
                        "type": "array",
                        "description": "Ordered sequence of messages to send. Each item may be a string or {message: string, delay_ms?: integer}.",
                        "items": {
                            "oneOf": [
                                { "type": "string" },
                                {
                                    "type": "object",
                                    "properties": {
                                        "message": { "type": "string" },
                                        "delay_ms": { "type": "integer" }
                                    },
                                    "required": ["message"]
                                }
                            ]
                        }
                    },
                    "environment": {
                        "type": "string",
                        "description": "Optional environment name or UUID to resolve {{variables}} against."
                    },
                    "override_vars": {
                        "type": "object",
                        "description": "Optional key-value pairs to override {{variables}} for this send."
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Milliseconds to wait after sending for immediate inbound replies. Defaults to 250 (max 10000)."
                    },
                    "max_messages": {
                        "type": "integer",
                        "description": "Maximum unread inbound messages to return. Defaults to 50 (max 200)."
                    }
                },
                "required": ["session_id"]
            }
        }),
        json!({
            "name": "blitz_websocket_read",
            "description": "Reads unread inbound messages from an open persistent WebSocket session without closing it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Active session ID returned by blitz_websocket_connect."
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Milliseconds to wait for inbound messages. Defaults to 1000 (max 15000)."
                    },
                    "max_messages": {
                        "type": "integer",
                        "description": "Maximum messages to return. Defaults to 50 (max 200)."
                    },
                    "include_history": {
                        "type": "boolean",
                        "description": "If true, returns all messages from session start instead of only unread messages since last read/send."
                    }
                },
                "required": ["session_id"]
            }
        }),
        json!({
            "name": "blitz_websocket_close",
            "description": "Closes an open persistent WebSocket session (or all open sessions when session_id is '*' or all=true).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Session ID to close, or '*' to close all sessions."
                    },
                    "all": {
                        "type": "boolean",
                        "description": "When true, closes all open persistent WebSocket sessions."
                    }
                }
            }
        }),
        json!({
            "name": "blitz_grpc_unary",
            "description": "Executes a gRPC unary procedure call over HTTP/2 with JSON body encoding (google.protobuf.Struct) and returns parsed response data, gRPC status code, and latency.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "endpoint": {
                        "type": "string",
                        "description": "gRPC server base URL (e.g. 'http://127.0.0.1:50051' or 'https://grpc.example.com')."
                    },
                    "method": {
                        "type": "string",
                        "description": "Full procedure name in 'package.Service/Method' notation (e.g. 'auth.AuthService/Login')."
                    },
                    "body": {
                        "type": "string",
                        "description": "JSON string payload for the gRPC request message."
                    },
                    "headers": {
                        "type": "object",
                        "description": "Optional gRPC metadata headers dictionary."
                    }
                },
                "required": ["endpoint", "method", "body"]
            }
        }),
        json!({
            "name": "blitz_sse_listen",
            "description": "Connects to a Server-Sent Events (SSE / EventSource) HTTP endpoint, captures streaming events ('event', 'data', 'id'), and returns the collected stream trace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "HTTP/HTTPS streaming endpoint URL."
                    },
                    "headers": {
                        "type": "object",
                        "description": "Optional request headers dictionary (e.g. {'Authorization': 'Bearer ...'})."
                    },
                    "max_events": {
                        "type": "integer",
                        "description": "Maximum number of SSE events to collect. Defaults to 5 (max 50)."
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Listening timeout in milliseconds. Defaults to 3000 (max 15000)."
                    }
                },
                "required": ["url"]
            }
        }),
        json!({
            "name": "blitz_mqtt_publish",
            "description": "Publishes a message to an MQTT broker topic with QoS 1 confirmation and disconnects.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "broker_url": {
                        "type": "string",
                        "description": "MQTT broker host and port (e.g. '127.0.0.1:1883' or 'broker.emqx.io:1883')."
                    },
                    "topic": {
                        "type": "string",
                        "description": "MQTT topic name to publish to (e.g. 'sensors/temp')."
                    },
                    "payload": {
                        "type": "string",
                        "description": "Payload string or JSON to publish."
                    },
                    "client_id": {
                        "type": "string",
                        "description": "Optional MQTT client identifier. Defaults to random 'blitz-mcp-<uuid>'."
                    },
                    "retain": {
                        "type": "boolean",
                        "description": "Optional retain flag for published message (default false)."
                    }
                },
                "required": ["broker_url", "topic", "payload"]
            }
        }),
        json!({
            "name": "blitz_mqtt_subscribe",
            "description": "Subscribes to an MQTT topic on a broker and waits for an incoming message with a timeout.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "broker_url": {
                        "type": "string",
                        "description": "MQTT broker host and port (e.g. '127.0.0.1:1883')."
                    },
                    "topic": {
                        "type": "string",
                        "description": "MQTT topic name to subscribe to."
                    },
                    "client_id": {
                        "type": "string",
                        "description": "Optional MQTT client identifier."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": "Wait timeout in seconds. Defaults to 3 (max 10)."
                    }
                },
                "required": ["broker_url", "topic"]
            }
        }),
        json!({
            "name": "blitz_graphql_ws_subscribe",
            "description": "Connects to a GraphQL WebSocket endpoint using the graphql-transport-ws protocol, executes a subscription query, and collects streaming payloads.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "ws_url": {
                        "type": "string",
                        "description": "GraphQL WebSocket endpoint (e.g. 'ws://127.0.0.1:4000/graphql')."
                    },
                    "query": {
                        "type": "string",
                        "description": "GraphQL subscription document string (e.g. 'subscription { postAdded { id title } }')."
                    },
                    "variables": {
                        "type": "string",
                        "description": "Optional JSON string or object for subscription variables."
                    },
                    "headers": {
                        "type": "object",
                        "description": "Optional WebSocket handshake headers dictionary."
                    },
                    "max_events": {
                        "type": "integer",
                        "description": "Maximum subscription frames to capture. Defaults to 3 (max 20)."
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Listening timeout in milliseconds. Defaults to 3000 (max 10000)."
                    }
                },
                "required": ["ws_url", "query"]
            }
        }),
        json!({
            "name": "blitz_socketio_emit",
            "description": "Connects to a Socket.IO server over WebSocket transport, optionally emits an event with payload, and listens for inbound events.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Socket.IO server URL (e.g. 'http://127.0.0.1:3000')."
                    },
                    "emit_event": {
                        "type": "string",
                        "description": "Optional event name to emit upon connection."
                    },
                    "emit_payload": {
                        "type": "string",
                        "description": "Optional JSON payload string to emit."
                    },
                    "listen_event": {
                        "type": "string",
                        "description": "Event name to listen for. Defaults to 'message'."
                    },
                    "listen_ms": {
                        "type": "integer",
                        "description": "Listening duration in milliseconds. Defaults to 2000 (max 10000)."
                    }
                },
                "required": ["url"]
            }
        }),
        json!({
            "name": "blitz_lint_collection",
            "description": "Runs Blitz collection linter checks (unresolved variables, duplicate request names, weak auth) on a specified collection or all collections.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "collection": {
                        "type": "string",
                        "description": "Optional collection name or UUID to lint. If omitted, lints all collections."
                    },
                    "environment": {
                        "type": "string",
                        "description": "Optional environment name or UUID to resolve variables against during lint."
                    }
                }
            }
        }),
        json!({
            "name": "blitz_diff_environments",
            "description": "Compares variable keys and values between two environments (matching, drift, missing, extra) and returns structured diff plus markdown summary.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "environment_a": {
                        "type": "string",
                        "description": "Baseline environment name or UUID."
                    },
                    "environment_b": {
                        "type": "string",
                        "description": "Target environment name or UUID."
                    }
                },
                "required": ["environment_a", "environment_b"]
            }
        }),
        json!({
            "name": "blitz_set_collection_var",
            "description": "Sets, updates, or deletes a variable inside a collection's variables scope.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "collection": {
                        "type": "string",
                        "description": "Collection name or UUID."
                    },
                    "key": {
                        "type": "string",
                        "description": "Variable name."
                    },
                    "value": {
                        "type": "string",
                        "description": "Variable value (omit with delete=true to remove)."
                    },
                    "enabled": {
                        "type": "boolean",
                        "description": "Whether the variable is enabled (default true)."
                    },
                    "delete": {
                        "type": "boolean",
                        "description": "If true, deletes the collection variable key."
                    }
                },
                "required": ["collection", "key"]
            }
        }),
        json!({
            "name": "blitz_import_collection",
            "description": "Imports a Postman v2.1 collection JSON, OpenAPI 3.0 spec (JSON/YAML), or Blitz collection JSON from raw content or a local file path.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "data": {
                        "type": "string",
                        "description": "Raw collection JSON/YAML string or local file path."
                    },
                    "collection": {
                        "type": "string",
                        "description": "Optional custom name override for the imported collection."
                    }
                },
                "required": ["data"]
            }
        }),
        json!({
            "name": "blitz_mcp_call",
            "description": "Connects to an external STDIO Model Context Protocol (MCP) server, lists its available tools (when 'tool' is omitted) or invokes a specific MCP tool with arguments.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Executable command (e.g. 'node', 'npx', 'python3')."
                    },
                    "args": {
                        "type": "string",
                        "description": "Optional space-separated command arguments (or array of strings)."
                    },
                    "tool": {
                        "type": "string",
                        "description": "Optional MCP tool name to invoke. If omitted, returns list of discovered tools."
                    },
                    "arguments": {
                        "type": "object",
                        "description": "Optional JSON arguments object passed to the target MCP tool."
                    }
                },
                "required": ["command"]
            }
        })
    ]
}

/// Execute a specific Blitz tool call against the configured workspace store.
fn execute_mcp_tool(name: &str, args: &JsonValue) -> Result<String, String> {
    let def_store = JsonStore::open_default().map_err(|e| format!("Open store: {e}"))?;
    let def_snap = def_store
        .load()
        .map_err(|e| format!("Load snapshot: {e}"))?;
    let store = JsonStore::open_for_settings(&def_snap.settings).unwrap_or(def_store);
    execute_mcp_tool_with_store(&store, name, args)
}

fn execute_mcp_tool_with_store(
    store: &JsonStore,
    name: &str,
    args: &JsonValue,
) -> Result<String, String> {
    let snapshot = store.load().map_err(|e| format!("Load snapshot: {e}"))?;

    match name {
        "blitz_list_collections" => {
            let filter = args
                .get("filter")
                .and_then(|f| f.as_str())
                .unwrap_or("")
                .to_lowercase();

            let mut out = Vec::new();
            for col in &snapshot.collections {
                let mut requests_list = Vec::new();
                collect_request_summaries(&col.items, "", &mut requests_list);

                if !filter.is_empty() {
                    let col_match = col.name.to_lowercase().contains(&filter);
                    requests_list.retain(|r| {
                        col_match
                            || r["name"].as_str().unwrap_or("").to_lowercase().contains(&filter)
                            || r["url"].as_str().unwrap_or("").to_lowercase().contains(&filter)
                            || r["path"].as_str().unwrap_or("").to_lowercase().contains(&filter)
                    });
                    if !col_match && requests_list.is_empty() {
                        continue;
                    }
                }

                out.push(json!({
                    "id": col.id.to_string(),
                    "name": col.name,
                    "request_count": requests_list.len(),
                    "requests": requests_list
                }));
            }
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_get_request" => {
            let req_id_str = args
                .get("request_id")
                .and_then(|r| r.as_str())
                .ok_or_else(|| "Missing required parameter 'request_id'".to_string())?;
            let col_name = args.get("collection").and_then(|c| c.as_str());

            let (found_col, found_req) = find_request_by_id_or_name(&snapshot.collections, req_id_str, col_name)?;

            let res = json!({
                "collection_id": found_col.id.to_string(),
                "collection_name": found_col.name,
                "request": {
                    "id": found_req.id.to_string(),
                    "name": found_req.name,
                    "protocol": format!("{:?}", found_req.protocol).to_lowercase(),
                    "method": found_req.method.as_str(),
                    "url": found_req.url,
                    "headers": found_req.headers.iter().filter(|h| h.enabled && !h.key.is_empty()).map(|h| (&h.key, &h.value)).collect::<HashMap<_, _>>(),
                    "params": found_req.params.iter().filter(|p| p.enabled && !p.key.is_empty()).map(|p| (&p.key, &p.value)).collect::<HashMap<_, _>>(),
                    "body_mode": format!("{:?}", found_req.body_mode),
                    "body": found_req.body,
                    "graphql_query": found_req.graphql_query,
                    "graphql_variables": found_req.graphql_variables,
                    "graphql_ws_query": found_req.graphql_ws_query,
                    "grpc_method": found_req.grpc_method,
                    "mqtt_topic": found_req.mqtt_topic,
                    "socketio_event": found_req.socketio_event,
                    "auth_type": format!("{:?}", found_req.auth.auth_type).to_lowercase(),
                    "description": found_req.description,
                    "pre_request_script": found_req.pre_request_script,
                    "test_script": found_req.test_script,
                    "has_pre_request_script": !found_req.pre_request_script.is_empty(),
                    "has_test_script": !found_req.test_script.is_empty()
                }
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_send_request" => {
            let req_id_str = args
                .get("request_id")
                .and_then(|r| r.as_str())
                .ok_or_else(|| "Missing required parameter 'request_id'".to_string())?;
            let col_name_opt = args.get("collection").and_then(|c| c.as_str());

            let (col, req) = find_request_by_id_or_name(&snapshot.collections, req_id_str, col_name_opt)?;

            let mut ctx = snapshot.variables.clone();
            ctx.collection = col.variables_map();

            // Apply explicit or active environment
            ctx.environment = resolve_env_map(&snapshot, args.get("environment").and_then(|e| e.as_str()))?;

            // Apply override vars
            if let Some(overrides) = args.get("override_vars").and_then(|o| o.as_object()) {
                for (k, v) in overrides {
                    if let Some(val_str) = json_val_to_string(v) {
                        ctx.collection.insert(k.clone(), val_str);
                    }
                }
            }

            let mut jar = snapshot.cookies.clone();
            let mut inherited_pre = Vec::new();
            if !col.pre_request_script.is_empty() {
                inherited_pre.push(col.pre_request_script.as_str());
            }
            let mut inherited_post = Vec::new();
            if !col.test_script.is_empty() {
                inherited_post.push(col.test_script.as_str());
            }

            let send_opts = SendOptions {
                collection_auth: Some(&col.auth),
                cookie_jar: Some(&mut jar),
                inherited_pre,
                inherited_post,
                ..Default::default()
            };

            let view = send_request_with_options(&req, &mut ctx, send_opts)
                .map_err(|e| format!("Send failed: {e}"))?;

            let parsed_json: Option<JsonValue> = serde_json::from_str(&view.body).ok();

            // Record to history, cookies, & response cache so Blitz desktop UI reflects this run
            let mut snap_for_save = snapshot;
            snap_for_save.cookies = jar;
            push_history(
                &mut snap_for_save.history,
                HistoryEntry::from_send(
                    &req,
                    &view.resolved_url,
                    Some(view.status),
                    Some(view.elapsed_ms),
                    Some(crate::core::SavedExample::from_response("MCP capture", &view)),
                ),
                HISTORY_MAX,
            );
            snap_for_save
                .responses
                .insert(req.id, compact_response_for_disk(&view));

            // Auto-extract response vars into active/target environment
            let save_to_env = args.get("save_to_env").and_then(|s| s.as_object());
            let env_name_arg = args.get("environment").and_then(|e| e.as_str());
            let saved_vars = apply_saved_vars(
                &mut snap_for_save,
                save_to_env,
                parsed_json.as_ref(),
                &view.headers,
                env_name_arg,
            );

            store.save(&snap_for_save).map_err(|e| format!("Save snapshot: {e}"))?;

            let tests_summary: Vec<_> = view
                .test_results
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "passed": t.passed,
                        "message": t.message
                    })
                })
                .collect();

            let mut out = json!({
                "status": view.status,
                "status_text": view.status_text,
                "elapsed_ms": view.elapsed_ms,
                "resolved_url": view.resolved_url,
                "headers": view.headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<HashMap<_, _>>(),
                "body": view.body,
                "error": view.error,
                "tests": tests_summary
            });

            if let Some(ref j) = parsed_json {
                out["json"] = j.clone();
            }
            if !saved_vars.is_empty() {
                out["saved_to_env"] = json!(saved_vars);
            }

            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_execute_http" => {
            let url = args
                .get("url")
                .and_then(|u| u.as_str())
                .ok_or_else(|| "Missing required parameter 'url'".to_string())?;

            let gql_query = args
                .get("graphql_query")
                .and_then(|q| q.as_str())
                .unwrap_or("")
                .to_string();
            let gql_vars = match args.get("graphql_variables") {
                Some(JsonValue::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            };
            let explicit_method = args.get("method").and_then(|m| m.as_str());
            let method_str = explicit_method.unwrap_or(if gql_query.is_empty() { "GET" } else { "POST" });
            let method = parse_http_method(method_str);

            let mut headers_vec = Vec::new();
            if let Some(hdr_obj) = args.get("headers").and_then(|h| h.as_object()) {
                for (k, v) in hdr_obj {
                    if let Some(val) = v.as_str() {
                        headers_vec.push(KeyValue::new(k, val));
                    }
                }
            }

            let body_str = args
                .get("body")
                .and_then(|b| b.as_str())
                .unwrap_or("")
                .to_string();
            let form_data_fields = parse_form_data_arg(args.get("form_data"))?;
            let binary_path = args
                .get("file_path")
                .or_else(|| args.get("binary_path"))
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();

            let explicit_body_mode = args.get("body_mode").and_then(|m| m.as_str());
            let body_mode = if !gql_query.is_empty() {
                BodyMode::Graphql
            } else if explicit_body_mode.is_none() && !form_data_fields.is_empty() {
                BodyMode::FormData
            } else if explicit_body_mode.is_none() && !binary_path.is_empty() {
                BodyMode::Binary
            } else {
                parse_body_mode_arg(explicit_body_mode, &body_str)
            };

            let mut ctx = build_mcp_var_ctx(
                &snapshot,
                args.get("environment").and_then(|e| e.as_str()),
                args.get("override_vars"),
            )?;

            if body_mode == BodyMode::FormData {
                for f in &form_data_fields {
                    if f.enabled && f.is_file {
                        let resolved = ctx.resolve_string(f.value.trim());
                        if !std::path::Path::new(&resolved).is_file() {
                            return Err(format!(
                                "Multipart file not found for field '{}': {}",
                                f.key, resolved
                            ));
                        }
                    }
                }
            } else if body_mode == BodyMode::Binary && !binary_path.trim().is_empty() {
                let resolved = ctx.resolve_string(binary_path.trim());
                if !std::path::Path::new(&resolved).is_file() {
                    return Err(format!("Binary file not found: {resolved}"));
                }
            }

            let draft = RequestDraft {
                name: "Ad-hoc Request".into(),
                method,
                url: url.into(),
                headers: headers_vec,
                body_mode,
                body: body_str,
                binary_path,
                body_formdata: form_data_fields,
                graphql_query: gql_query,
                graphql_variables: gql_vars,
                ..Default::default()
            };

            let mut jar = snapshot.cookies.clone();
            let send_opts = SendOptions {
                cookie_jar: Some(&mut jar),
                ..Default::default()
            };

            let view = send_request_with_options(&draft, &mut ctx, send_opts)
                .map_err(|e| format!("Execution error: {e}"))?;

            let parsed_json: Option<JsonValue> = serde_json::from_str(&view.body).ok();

            // Record to history & persist updated cookie jar
            let mut snap_for_save = snapshot;
            snap_for_save.cookies = jar;
            push_history(
                &mut snap_for_save.history,
                HistoryEntry::from_send(
                    &draft,
                    &view.resolved_url,
                    Some(view.status),
                    Some(view.elapsed_ms),
                    Some(crate::core::SavedExample::from_response("MCP capture", &view)),
                ),
                HISTORY_MAX,
            );

            // Auto-extract response vars into active/target environment
            let save_to_env = args.get("save_to_env").and_then(|s| s.as_object());
            let env_name_arg = args.get("environment").and_then(|e| e.as_str());
            let saved_vars = apply_saved_vars(
                &mut snap_for_save,
                save_to_env,
                parsed_json.as_ref(),
                &view.headers,
                env_name_arg,
            );

            store.save(&snap_for_save).map_err(|e| format!("Save snapshot: {e}"))?;

            let mut out = json!({
                "status": view.status,
                "status_text": view.status_text,
                "elapsed_ms": view.elapsed_ms,
                "resolved_url": view.resolved_url,
                "headers": view.headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<HashMap<_, _>>(),
                "body": view.body,
                "error": view.error
            });

            if let Some(ref j) = parsed_json {
                out["json"] = j.clone();
            }
            if !saved_vars.is_empty() {
                out["saved_to_env"] = json!(saved_vars);
            }

            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_list_environments" => {
            let include_values = args
                .get("include_values")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let envs: Vec<_> = snapshot
                .environments
                .iter()
                .map(|e| {
                    if include_values {
                        json!({
                            "id": e.id.to_string(),
                            "name": e.name,
                            "variables": e.values.iter().filter(|v| v.enabled && !v.key.is_empty()).map(|v| {
                                json!({
                                    "key": v.key,
                                    "value": v.effective(),
                                    "enabled": v.enabled
                                })
                            }).collect::<Vec<_>>()
                        })
                    } else {
                        json!({
                            "id": e.id.to_string(),
                            "name": e.name,
                            "variable_keys": e.values.iter().filter(|v| v.enabled && !v.key.is_empty()).map(|v| &v.key).collect::<Vec<_>>()
                        })
                    }
                })
                .collect();

            let res = json!({
                "active_environment": snapshot.active_environment,
                "active_environment_id": snapshot.active_environment_id.map(|id| id.to_string()),
                "environments": envs
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_set_env_var" => {
            let key = args
                .get("key")
                .and_then(|k| k.as_str())
                .ok_or_else(|| "Missing required parameter 'key'".to_string())?;
            let value = args
                .get("value")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter 'value'".to_string())?;
            let env_name_opt = args.get("environment").and_then(|e| e.as_str());
            let enabled = args.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true);

            let mut snapshot = snapshot;

            let env_idx = if let Some(target) = env_name_opt {
                if let Some(idx) = snapshot.environments.iter().position(|e| {
                    e.name.eq_ignore_ascii_case(target) || e.id.to_string() == target
                }) {
                    idx
                } else {
                    let new_env = Environment::new(target);
                    snapshot.environments.push(new_env);
                    snapshot.environments.len() - 1
                }
            } else if let Some(active_id) = snapshot.active_environment_id {
                if let Some(idx) = snapshot.environments.iter().position(|e| e.id == active_id) {
                    idx
                } else if !snapshot.environments.is_empty() {
                    0
                } else {
                    snapshot.environments.push(Environment::new("Development"));
                    0
                }
            } else if !snapshot.environments.is_empty() {
                0
            } else {
                snapshot.environments.push(Environment::new("Development"));
                0
            };

            let env = &mut snapshot.environments[env_idx];
            let env_name = env.name.clone();
            let env_id = env.id;

            if let Some(v) = env.values.iter_mut().find(|v| v.key == key) {
                v.current_value = value.to_string();
                if v.initial_value.is_empty() {
                    v.initial_value = value.to_string();
                }
                v.enabled = enabled;
            } else {
                env.values.push(EnvVar {
                    key: key.to_string(),
                    initial_value: value.to_string(),
                    current_value: value.to_string(),
                    enabled,
                });
            }

            // Sync into active variable context if this is current environment
            if snapshot.active_environment_id == Some(env_id)
                || snapshot.active_environment.eq_ignore_ascii_case(&env_name)
            {
                snapshot
                    .variables
                    .environment
                    .insert(key.to_string(), value.to_string());
            }

            store
                .save(&snapshot)
                .map_err(|e| format!("Save snapshot: {e}"))?;

            let res = json!({
                "status": "ok",
                "environment": env_name,
                "environment_id": env_id.to_string(),
                "key": key,
                "value": value,
                "enabled": enabled
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_set_active_environment" => {
            let env_target = args
                .get("environment")
                .and_then(|e| e.as_str())
                .unwrap_or("");
            let mut snapshot = snapshot;

            if env_target.is_empty() || env_target.eq_ignore_ascii_case("none") {
                snapshot.active_environment = "No Environment".to_string();
                snapshot.active_environment_id = None;
                snapshot.variables.environment.clear();
                store
                    .save(&snapshot)
                    .map_err(|e| format!("Save snapshot: {e}"))?;
                let res = json!({
                    "status": "ok",
                    "active_environment": null
                });
                return serde_json::to_string_pretty(&res).map_err(|e| e.to_string());
            }

            let env = snapshot
                .environments
                .iter()
                .find(|e| e.name.eq_ignore_ascii_case(env_target) || e.id.to_string() == env_target)
                .ok_or_else(|| format!("Environment '{env_target}' not found"))?;

            let env_name = env.name.clone();
            let env_id = env.id;
            let env_map = env.to_map();

            snapshot.active_environment = env_name.clone();
            snapshot.active_environment_id = Some(env_id);
            snapshot.variables.environment = env_map;

            store
                .save(&snapshot)
                .map_err(|e| format!("Save snapshot: {e}"))?;

            let res = json!({
                "status": "ok",
                "active_environment": env_name,
                "id": env_id.to_string()
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_upsert_request" => {
            let col_name_or_id = args
                .get("collection")
                .and_then(|c| c.as_str())
                .ok_or_else(|| "Missing required parameter 'collection'".to_string())?;
            let req_name = args
                .get("name")
                .and_then(|n| n.as_str())
                .ok_or_else(|| "Missing required parameter 'name'".to_string())?;
            let url = args
                .get("url")
                .and_then(|u| u.as_str())
                .ok_or_else(|| "Missing required parameter 'url'".to_string())?;
            let has_method = args.get("method").and_then(|m| m.as_str()).is_some();
            let method_str = args
                .get("method")
                .and_then(|m| m.as_str())
                .unwrap_or("GET");
            let method = parse_http_method(method_str);

            let headers_opt = args.get("headers").and_then(|h| h.as_object()).map(|h_obj| {
                let mut vec = Vec::new();
                for (k, v) in h_obj {
                    if let Some(val) = v.as_str() {
                        vec.push(KeyValue::new(k, val));
                    }
                }
                vec
            });

            let params_opt = args.get("params").and_then(|p| p.as_object()).map(|p_obj| {
                let mut vec = Vec::new();
                for (k, v) in p_obj {
                    if let Some(val) = v.as_str() {
                        vec.push(KeyValue::new(k, val));
                    }
                }
                vec
            });

            let body_opt = args
                .get("body")
                .and_then(|b| b.as_str())
                .map(str::to_string);
            let body_mode_opt = args.get("body_mode").and_then(|m| m.as_str());
            let proto_opt = args
                .get("protocol")
                .and_then(|p| p.as_str())
                .map(parse_protocol_arg);
            let auth_opt = args.get("auth").and_then(parse_auth_arg);
            let pre_script_opt = args
                .get("pre_request_script")
                .and_then(|s| s.as_str())
                .map(str::to_string);
            let test_script_opt = args
                .get("test_script")
                .and_then(|s| s.as_str())
                .map(str::to_string);
            let gql_query_opt = args
                .get("graphql_query")
                .and_then(|s| s.as_str())
                .map(str::to_string);
            let gql_ws_query_opt = args
                .get("graphql_ws_query")
                .and_then(|s| s.as_str())
                .map(str::to_string);
            let gql_vars_opt = args.get("graphql_variables").and_then(|v| match v {
                JsonValue::String(s) => Some(s.clone()),
                other if !other.is_null() => Some(other.to_string()),
                _ => None,
            });
            let grpc_method_opt = args
                .get("grpc_method")
                .and_then(|s| s.as_str())
                .map(str::to_string);
            let mqtt_topic_opt = args
                .get("mqtt_topic")
                .and_then(|s| s.as_str())
                .map(str::to_string);
            let socketio_event_opt = args
                .get("socketio_event")
                .and_then(|s| s.as_str())
                .map(str::to_string);
            let description_opt = args
                .get("description")
                .and_then(|d| d.as_str())
                .map(str::to_string);
            let folder_name = args
                .get("folder")
                .and_then(|f| f.as_str())
                .map(str::trim)
                .filter(|f| f.split('/').any(|s| !s.trim().is_empty()));
            let req_id_opt = args
                .get("request_id")
                .and_then(|r| r.as_str())
                .and_then(|s| Uuid::parse_str(s).ok());

            let mut snapshot = snapshot;

            let col_idx = if let Some(idx) = snapshot.collections.iter().position(|c| {
                c.name.eq_ignore_ascii_case(col_name_or_id) || c.id.to_string() == col_name_or_id
            }) {
                idx
            } else {
                let new_col = Collection::new(col_name_or_id);
                snapshot.collections.push(new_col);
                snapshot.collections.len() - 1
            };

            let (col_id_str, col_name_str, updated_draft, was_updated) = {
                let col = &mut snapshot.collections[col_idx];
                let existing_opt = if let Some(req_id) = req_id_opt {
                    find_request_in_items_mut(&mut col.items, &req_id.to_string())
                } else if let Some(f_path) = folder_name {
                    find_folder_by_path_or_name_mut(&mut col.items, f_path).and_then(|folder| {
                        folder.items.iter_mut().find_map(|item| match item {
                            CollectionItem::Request(r) if r.name.eq_ignore_ascii_case(req_name) => {
                                Some(r)
                            }
                            _ => None,
                        })
                    })
                } else {
                    col.items.iter_mut().find_map(|item| match item {
                        CollectionItem::Request(r) if r.name.eq_ignore_ascii_case(req_name) => {
                            Some(r)
                        }
                        _ => None,
                    })
                };

                let (draft_snapshot, updated) = if let Some(existing) = existing_opt {
                    existing.name = req_name.to_string();
                    existing.url = url.to_string();
                    if let Some(proto) = proto_opt {
                        existing.protocol = proto;
                    }
                    if has_method {
                        existing.method = method;
                    }
                    if let Some(h) = headers_opt {
                        existing.headers = h;
                    }
                    if let Some(p) = params_opt {
                        existing.params = p;
                    }
                    if let Some(gq) = gql_query_opt {
                        existing.graphql_query = gq;
                        if body_mode_opt.is_none() && !existing.graphql_query.is_empty() {
                            existing.body_mode = BodyMode::Graphql;
                        }
                    }
                    if let Some(b) = body_opt {
                        existing.body = b;
                        if body_mode_opt.is_some() || existing.body_mode != BodyMode::Graphql {
                            existing.body_mode = parse_body_mode_arg(body_mode_opt, &existing.body);
                        }
                    } else if let Some(bm) = body_mode_opt {
                        existing.body_mode = parse_body_mode_arg(Some(bm), &existing.body);
                    }
                    if let Some(gv) = gql_vars_opt {
                        existing.graphql_variables = gv;
                    }
                    if let Some(gwq) = gql_ws_query_opt {
                        existing.graphql_ws_query = gwq;
                    }
                    if let Some(gm) = grpc_method_opt {
                        existing.grpc_method = gm;
                    }
                    if let Some(mt) = mqtt_topic_opt {
                        existing.mqtt_topic = mt;
                    }
                    if let Some(se) = socketio_event_opt {
                        existing.socketio_event = se;
                    }
                    if let Some(auth) = auth_opt {
                        existing.auth = auth;
                    }
                    if let Some(pre) = pre_script_opt {
                        existing.pre_request_script = pre;
                    }
                    if let Some(ts) = test_script_opt {
                        existing.test_script = ts;
                    }
                    if let Some(desc) = description_opt {
                        existing.description = desc;
                    }
                    (existing.clone(), true)
                } else {
                    let body = body_opt.unwrap_or_default();
                    let graphql_query = gql_query_opt.unwrap_or_default();
                    let body_mode = if body_mode_opt.is_none() && !graphql_query.is_empty() {
                        BodyMode::Graphql
                    } else {
                        parse_body_mode_arg(body_mode_opt, &body)
                    };
                    let method_final = if !has_method && !graphql_query.is_empty() {
                        HttpMethod::Post
                    } else {
                        method
                    };
                    let draft = RequestDraft {
                        id: req_id_opt.unwrap_or_else(Uuid::new_v4),
                        name: req_name.to_string(),
                        protocol: proto_opt.unwrap_or_default(),
                        method: method_final,
                        url: url.to_string(),
                        headers: headers_opt.unwrap_or_default(),
                        params: params_opt.unwrap_or_default(),
                        body_mode,
                        body,
                        graphql_query,
                        graphql_variables: gql_vars_opt.unwrap_or_default(),
                        graphql_ws_query: gql_ws_query_opt.unwrap_or_default(),
                        grpc_method: grpc_method_opt.unwrap_or_default(),
                        mqtt_topic: mqtt_topic_opt.unwrap_or_default(),
                        socketio_event: socketio_event_opt.unwrap_or_default(),
                        auth: auth_opt.unwrap_or_default(),
                        pre_request_script: pre_script_opt.unwrap_or_default(),
                        test_script: test_script_opt.unwrap_or_default(),
                        description: description_opt.unwrap_or_default(),
                        ..Default::default()
                    };
                    if let Some(f_path) = folder_name {
                        let folder = upsert_folder_path_mut(&mut col.items, f_path);
                        folder.add_request(draft.clone());
                    } else {
                        col.add_request(draft.clone());
                    }
                    (draft, false)
                };
                (col.id.to_string(), col.name.clone(), draft_snapshot, updated)
            };

            if let Some(tab) = snapshot
                .open_tabs
                .iter_mut()
                .find(|t| t.id == updated_draft.id)
            {
                *tab = updated_draft.clone();
            }

            store
                .save(&snapshot)
                .map_err(|e| format!("Save snapshot: {e}"))?;

            let res = json!({
                "status": "ok",
                "updated": was_updated,
                "collection_id": col_id_str,
                "collection_name": col_name_str,
                "request_id": updated_draft.id.to_string(),
                "name": updated_draft.name,
                "method": updated_draft.method.as_str(),
                "url": updated_draft.url,
                "body_mode": format!("{:?}", updated_draft.body_mode).to_lowercase()
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_import_curl" => {
            let curl_str = args
                .get("curl")
                .and_then(|c| c.as_str())
                .ok_or_else(|| "Missing required parameter 'curl'".to_string())?;
            let col_name_opt = args.get("collection").and_then(|c| c.as_str());
            let custom_name_opt = args.get("name").and_then(|n| n.as_str());

            let mut draft = import_curl(curl_str).map_err(|e| format!("Import cURL: {e}"))?;
            if let Some(custom_name) = custom_name_opt {
                draft.name = custom_name.to_string();
            }

            let mut snapshot = snapshot;
            let saved_col = if let Some(col_name) = col_name_opt {
                let col = if let Some(c) = snapshot.collections.iter_mut().find(|c| {
                    c.name.eq_ignore_ascii_case(col_name) || c.id.to_string() == col_name
                }) {
                    c
                } else {
                    snapshot.collections.push(Collection::new(col_name));
                    snapshot.collections.last_mut().unwrap()
                };
                let c_name = col.name.clone();
                col.add_request(draft.clone());
                store
                    .save(&snapshot)
                    .map_err(|e| format!("Save snapshot: {e}"))?;
                Some(c_name)
            } else {
                None
            };

            let res = json!({
                "status": "ok",
                "saved": saved_col.is_some(),
                "saved_collection": saved_col,
                "request": {
                    "id": draft.id.to_string(),
                    "name": draft.name,
                    "method": draft.method.as_str(),
                    "url": draft.url,
                    "headers": draft.headers.iter().filter(|h| h.enabled && !h.key.is_empty()).map(|h| (&h.key, &h.value)).collect::<HashMap<_, _>>(),
                    "body": draft.body
                }
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_generate_code" => {
            let lang_str = args
                .get("language")
                .and_then(|l| l.as_str())
                .ok_or_else(|| "Missing required parameter 'language'".to_string())?;

            let draft = if let Some(req_id_str) = args.get("request_id").and_then(|r| r.as_str()) {
                let (_, r) = find_request_by_id_or_name(&snapshot.collections, req_id_str, None)?;
                r
            } else {
                let url = args
                    .get("url")
                    .and_then(|u| u.as_str())
                    .ok_or_else(|| "Must provide either 'request_id' or 'url'".to_string())?;
                let method = parse_http_method(
                    args.get("method").and_then(|m| m.as_str()).unwrap_or("GET"),
                );
                let mut headers = Vec::new();
                if let Some(h_obj) = args.get("headers").and_then(|h| h.as_object()) {
                    for (k, v) in h_obj {
                        if let Some(val) = v.as_str() {
                            headers.push(KeyValue::new(k, val));
                        }
                    }
                }
                let body = args
                    .get("body")
                    .and_then(|b| b.as_str())
                    .unwrap_or("")
                    .to_string();
                let body_mode = parse_body_mode_arg(
                    args.get("body_mode").and_then(|m| m.as_str()),
                    &body,
                );
                RequestDraft {
                    name: "Snippet Request".into(),
                    method,
                    url: url.into(),
                    headers,
                    body_mode,
                    body,
                    ..Default::default()
                }
            };

            let mut ctx = snapshot.variables.clone();
            if let Some(env_name) = args.get("environment").and_then(|e| e.as_str()) {
                if let Some(env) = find_env_by_name_or_id(&snapshot.environments, env_name) {
                    ctx.environment = env.to_map();
                }
            }

            let lang_lower = lang_str.trim().to_lowercase();
            let target_lang: &'static str = match lang_lower.as_str() {
                "curl" | "sh" | "bash" => "cURL",
                "rust" | "reqwest" => "Rust (reqwest)",
                "typescript" | "ts" | "axios" => "TypeScript (axios)",
                "javascript" | "js" | "fetch" => "JavaScript fetch",
                "python" | "py" | "httpx" => "Python (httpx)",
                "requests" | "python-requests" => "Python (requests)",
                "go" | "golang" => "Go net/http",
                "java" | "okhttp" => "Java OkHttp",
                "c#" | "csharp" | "dotnet" | "httpclient" => "C# HttpClient",
                other => snippet_languages()
                    .iter()
                    .copied()
                    .find(|l| l.eq_ignore_ascii_case(other))
                    .unwrap_or("cURL"),
            };

            let code = generate_snippet_resolved(target_lang, &draft, Some(&ctx));
            let res = json!({
                "language": target_lang,
                "code": code
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_list_history" => {
            let limit = args
                .get("limit")
                .and_then(|l| l.as_u64())
                .unwrap_or(20) as usize;
            let entries: Vec<_> = snapshot
                .history
                .iter()
                .take(limit)
                .map(|h| {
                    json!({
                        "id": h.id.to_string(),
                        "at": h.at.to_rfc3339(),
                        "method": h.method.as_str(),
                        "url": h.url,
                        "status": h.status,
                        "elapsed_ms": h.elapsed_ms
                    })
                })
                .collect();

            let res = json!({
                "count": entries.len(),
                "history": entries
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_run_collection" => {
            let col_name = args
                .get("collection")
                .and_then(|c| c.as_str())
                .ok_or_else(|| "Missing required parameter 'collection'".to_string())?;
            let folder_name_opt = args.get("folder").and_then(|f| f.as_str());

            let target_col = snapshot
                .collections
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(col_name) || c.id.to_string() == col_name)
                .ok_or_else(|| format!("Collection '{col_name}' not found"))?;

            let scoped_col_storage;
            let run_col = if let Some(folder_query) = folder_name_opt {
                if let Some(folder) = find_folder_by_path_or_name(&target_col.items, folder_query) {
                    let mut sc = target_col.clone();
                    sc.name = format!("{} / {}", target_col.name, folder.name);
                    sc.items = folder.items.clone();
                    if folder.auth.auth_type != crate::core::AuthType::Inherit {
                        sc.auth = folder.auth.clone();
                    }
                    if !folder.pre_request_script.is_empty() {
                        sc.pre_request_script = folder.pre_request_script.clone();
                    }
                    if !folder.test_script.is_empty() {
                        sc.test_script = folder.test_script.clone();
                    }
                    scoped_col_storage = sc;
                    &scoped_col_storage
                } else {
                    let mut available = Vec::new();
                    collect_folder_paths(&target_col.items, "", &mut available);
                    return Err(format!(
                        "Folder '{folder_query}' not found in collection '{}'. Available folders: {:?}",
                        target_col.name, available
                    ));
                }
            } else {
                target_col
            };

            let mut ctx = snapshot.variables.clone();
            ctx.collection = run_col.variables_map();
            ctx.environment = resolve_env_map(&snapshot, args.get("environment").and_then(|e| e.as_str()))?;

            let iterations = args
                .get("iterations")
                .and_then(|i| i.as_u64())
                .unwrap_or(1)
                .clamp(1, 50) as usize;
            let delay_ms = args
                .get("delay_ms")
                .and_then(|d| d.as_u64())
                .unwrap_or(0)
                .clamp(0, 5_000);

            let cfg = RunnerConfig {
                iterations,
                delay_ms,
                data_file: String::new(),
                ..Default::default()
            };
            let mut jar = snapshot.cookies.clone();

            let report = run_collection_with_config(
                run_col,
                &mut ctx,
                &mut jar,
                Some(&snapshot.settings),
                None,
                &cfg,
            );

            let results: Vec<_> = report
                .results
                .iter()
                .map(|r| {
                    json!({
                        "request_name": r.name,
                        "method": r.method,
                        "url": r.url,
                        "status": r.status,
                        "elapsed_ms": r.elapsed_ms,
                        "passed": r.passed,
                        "error": r.error,
                        "tests": r.test_results.iter().map(|t| json!({ "name": t.name, "passed": t.passed, "message": t.message })).collect::<Vec<_>>()
                    })
                })
                .collect();

            let out = json!({
                "collection": target_col.name,
                "folder": folder_name_opt,
                "total_requests": report.results.len(),
                "passed": report.passed_count(),
                "failed": report.failed_count(),
                "results": results
            });

            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_suggest_assertions" => {
            let request_id_opt = args.get("request_id").and_then(|r| r.as_str());
            let status_opt = args.get("status").and_then(|s| s.as_u64()).map(|s| s as u16);
            let body_opt = args.get("body").and_then(|b| b.as_str());
            let elapsed_ms_opt = args.get("elapsed_ms").and_then(|e| e.as_u64()).map(|e| e as u128);
            let apply_to_request = args.get("apply_to_request").and_then(|a| a.as_bool()).unwrap_or(false);

            let mut final_status = status_opt;
            let mut final_body = body_opt.map(|b| b.to_string());
            let mut final_headers: Vec<(String, String)> = Vec::new();
            let mut final_elapsed = elapsed_ms_opt.unwrap_or(150);

            if let Some(h_obj) = args.get("headers").and_then(|h| h.as_object()) {
                for (k, v) in h_obj {
                    if let Some(val_str) = v.as_str() {
                        final_headers.push((k.clone(), val_str.to_string()));
                    }
                }
            }

            let mut target_request = None;
            let mut target_col_idx = None;
            if let Some(req_id) = request_id_opt {
                if let Ok((col, req)) = find_request_by_id_or_name(&snapshot.collections, req_id, None) {
                    target_request = Some(req);
                    target_col_idx = snapshot.collections.iter().position(|c| c.id == col.id);
                }

                if final_status.is_none() || final_body.is_none() {
                    let mut found_history = false;
                    for h in snapshot.history.iter().rev() {
                        if h.draft.id.to_string() == req_id
                            || target_request.as_ref().map(|r| r.id) == Some(h.draft.id)
                        {
                            final_status = h.status;
                            final_body = Some(h.draft.body.clone());
                            final_elapsed = h.elapsed_ms.unwrap_or(150);
                            for hdr in &h.draft.headers {
                                if hdr.enabled && !hdr.key.is_empty() {
                                    final_headers.push((hdr.key.clone(), hdr.value.clone()));
                                }
                            }
                            found_history = true;
                            break;
                        }
                    }

                    if !found_history {
                        if let Some(ref draft) = target_request {
                            let mut ctx = snapshot.variables.clone();
                            let active_env = if let Some(id) = snapshot.active_environment_id {
                                snapshot.environments.iter().find(|e| e.id == id)
                            } else {
                                snapshot.environments.iter().find(|e| e.name.eq_ignore_ascii_case(&snapshot.active_environment))
                            };
                            if let Some(env) = active_env {
                                ctx.environment = env.to_map();
                            }
                            let mut jar = snapshot.cookies.clone();
                            let view = send_request_with_options(
                                draft,
                                &mut ctx,
                                SendOptions {
                                    cookie_jar: Some(&mut jar),
                                    ..Default::default()
                                },
                            ).map_err(|e| format!("Execute request to capture response: {e}"))?;

                            final_status = Some(view.status);
                            final_body = Some(view.body);
                            final_elapsed = view.timing.total_ms;
                            for (hk, hv) in view.headers {
                                final_headers.push((hk, hv));
                            }
                        }
                    }
                }
            }

            let status_code = final_status.unwrap_or(200);
            let body_str = final_body.unwrap_or_else(|| "{}".to_string());
            let assertions = suggest_from_response(status_code, &final_headers, &body_str, final_elapsed);
            let test_script = assertions_to_script(&assertions);

            let mut applied = false;
            if apply_to_request {
                if let (Some(req_id), Some(col_idx)) = (request_id_opt, target_col_idx) {
                    let mut snapshot = snapshot;
                    let col = &mut snapshot.collections[col_idx];
                    if let Some(req) = find_request_in_items_mut(&mut col.items, req_id) {
                        if req.test_script.is_empty() {
                            req.test_script = test_script.clone();
                        } else {
                            req.test_script.push_str("\n\n// Auto-generated assertions\n");
                            req.test_script.push_str(&test_script);
                        }
                        store.save(&snapshot).map_err(|e| format!("Save snapshot: {e}"))?;
                        applied = true;
                    }
                }
            }

            let assertion_list = assertions
                .iter()
                .map(|a| {
                    json!({
                        "id": a.id.to_string(),
                        "type": a.kind.type_key(),
                        "label": a.kind.label(),
                        "enabled": a.enabled
                    })
                })
                .collect::<Vec<_>>();

            let out = json!({
                "status": status_code,
                "assertion_count": assertions.len(),
                "assertions": assertion_list,
                "test_script": test_script,
                "applied_to_request": applied
            });

            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_perf_test" => {
            let request_id_opt = args.get("request_id").and_then(|r| r.as_str());
            let col_name_opt = args.get("collection").and_then(|c| c.as_str());
            let vus = args.get("vus").and_then(|v| v.as_u64()).unwrap_or(5).clamp(1, 50) as u32;
            let duration_secs = args.get("duration_seconds").and_then(|d| d.as_u64()).unwrap_or(5).clamp(1, 60);

            let (target_label, perf_col) = if let Some(req_id) = request_id_opt {
                let (_, req) = find_request_by_id_or_name(&snapshot.collections, req_id, None)?;
                let label = format!("{} {}", req.method.as_str(), req.url);
                let col = Collection {
                    id: Uuid::new_v4(),
                    name: format!("Perf: {}", req.name),
                    items: vec![CollectionItem::Request(req)],
                    ..Collection::new("PerfTarget")
                };
                (label, col)
            } else if let Some(col_name) = col_name_opt {
                let col = snapshot
                    .collections
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(col_name) || c.id.to_string() == col_name)
                    .ok_or_else(|| format!("Collection '{col_name}' not found"))?;
                (format!("Collection '{}'", col.name), col.clone())
            } else {
                return Err("Missing target: specify 'request_id' or 'collection'".to_string());
            };

            let mut ctx = snapshot.variables.clone();
            ctx.collection = perf_col.variables_map();
            if let Some(env_name) = args.get("environment").and_then(|e| e.as_str()) {
                if let Some(env) = find_env_by_name_or_id(&snapshot.environments, env_name) {
                    ctx.environment = env.to_map();
                }
            } else {
                let active_env = if let Some(id) = snapshot.active_environment_id {
                    snapshot.environments.iter().find(|e| e.id == id)
                } else {
                    snapshot.environments.iter().find(|e| e.name.eq_ignore_ascii_case(&snapshot.active_environment))
                };
                if let Some(env) = active_env {
                    ctx.environment = env.to_map();
                }
            }

            let report = run_performance(&perf_col, &ctx, vus, std::time::Duration::from_secs(duration_secs));

            let total = report.total_requests;
            let dur_actual = report.duration_secs.max(1);
            let rps = (total as f64 / dur_actual as f64 * 100.0).round() / 100.0;
            let err_rate = if total > 0 {
                ((report.errors as f64 / total as f64) * 10000.0).round() / 100.0
            } else {
                0.0
            };
            let min_lat = if report.latency_min_ms == u64::MAX { 0 } else { report.latency_min_ms };

            let out = json!({
                "target": target_label,
                "vus": report.vus,
                "duration_seconds": report.duration_secs,
                "total_requests": total,
                "requests_per_second": rps,
                "errors": report.errors,
                "error_rate_percent": err_rate,
                "avg_latency_ms": (report.avg_latency_ms() * 10.0).round() / 10.0,
                "min_latency_ms": min_lat,
                "max_latency_ms": report.latency_max_ms
            });

            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_sync_openapi" => {
            let spec_arg = args
                .get("spec")
                .and_then(|s| s.as_str())
                .ok_or_else(|| "Missing required parameter 'spec'".to_string())?;
            let col_name_opt = args.get("collection").and_then(|c| c.as_str());

            let spec_content = if std::path::Path::new(spec_arg).exists() {
                std::fs::read_to_string(spec_arg)
                    .map_err(|e| format!("Failed to read OpenAPI file '{spec_arg}': {e}"))?
            } else {
                spec_arg.to_string()
            };

            let mut snapshot = snapshot;
            let target_col_pos = if let Some(c_query) = col_name_opt {
                snapshot.collections.iter().position(|c| {
                    c.name.eq_ignore_ascii_case(c_query) || c.id.to_string() == c_query
                })
            } else {
                None
            };

            let (col_name, added_count, preserved_count, created_new) = if let Some(idx) = target_col_pos {
                let col = &mut snapshot.collections[idx];
                let (added, preserved) = reconcile_openapi3_collection(col, &spec_content)
                    .map_err(|e| format!("Reconcile OpenAPI: {e}"))?;
                (col.name.clone(), added, preserved, false)
            } else {
                let mut imported = import_openapi3(&spec_content)
                    .map_err(|e| format!("Import OpenAPI: {e}"))?;
                if let Some(custom_name) = col_name_opt {
                    imported.name = custom_name.to_string();
                }
                let name = imported.name.clone();
                let count = count_requests_in_items(&imported.items);
                snapshot.collections.push(imported);
                (name, count, 0, true)
            };

            store.save(&snapshot).map_err(|e| format!("Save snapshot: {e}"))?;

            let res = json!({
                "status": "ok",
                "collection": col_name,
                "created_new": created_new,
                "added_routes": added_count,
                "preserved_routes": preserved_count
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_get_collection" => {
            let col_query = args
                .get("collection")
                .and_then(|c| c.as_str())
                .ok_or_else(|| "Missing required parameter 'collection'".to_string())?;

            let target_col = snapshot
                .collections
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(col_query) || c.id.to_string() == col_query)
                .ok_or_else(|| format!("Collection '{col_query}' not found"))?;

            serde_json::to_string_pretty(target_col).map_err(|e| e.to_string())
        }
        "blitz_create_environment" => {
            let env_name = args
                .get("name")
                .and_then(|n| n.as_str())
                .ok_or_else(|| "Missing required parameter 'name'".to_string())?;
            let set_active = args.get("set_active").and_then(|a| a.as_bool()).unwrap_or(false);

            let mut snapshot = snapshot;
            if snapshot.environments.iter().any(|e| e.name.eq_ignore_ascii_case(env_name)) {
                return Err(format!("Environment '{env_name}' already exists"));
            }

            let mut new_env = Environment::new(env_name);
            if let Some(vars_obj) = args.get("variables").and_then(|v| v.as_object()) {
                for (k, v) in vars_obj {
                    let val_str = json_val_to_string(v).unwrap_or_default();
                    new_env.values.push(EnvVar::new(k, val_str));
                }
            }

            let env_id = new_env.id;
            let env_name_str = new_env.name.clone();
            let env_map = new_env.to_map();
            let var_count = new_env.values.len();
            snapshot.environments.push(new_env);

            if set_active {
                snapshot.active_environment_id = Some(env_id);
                snapshot.active_environment = env_name_str.clone();
                snapshot.variables.environment = env_map;
            }

            store.save(&snapshot).map_err(|e| format!("Save snapshot: {e}"))?;

            let res = json!({
                "status": "ok",
                "id": env_id.to_string(),
                "name": env_name_str,
                "variable_count": var_count,
                "is_active": set_active
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_delete_request" => {
            let req_query = args
                .get("request_id")
                .and_then(|r| r.as_str())
                .ok_or_else(|| "Missing required parameter 'request_id'".to_string())?;
            let col_name_opt = args.get("collection").and_then(|c| c.as_str());

            let mut snapshot = snapshot;
            let (target_col, req_draft) = find_request_by_id_or_name(&snapshot.collections, req_query, col_name_opt)?;
            let col_name = target_col.name.clone();
            let req_id = req_draft.id;
            let req_name = req_draft.name.clone();

            let removed = remove_across(&mut snapshot.collections, req_id);
            if !removed {
                return Err(format!("Failed to delete request '{req_query}'"));
            }

            store.save(&snapshot).map_err(|e| format!("Save snapshot: {e}"))?;

            let res = json!({
                "status": "deleted",
                "request_id": req_id.to_string(),
                "name": req_name,
                "collection": col_name
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_start_mock" => {
            let col_name_opt = args.get("collection").and_then(|c| c.as_str());
            let port = args.get("port").and_then(|p| p.as_u64()).unwrap_or(0) as u16;

            let cols: Vec<Collection> = if let Some(col_query) = col_name_opt {
                let col = snapshot
                    .collections
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(col_query) || c.id.to_string() == col_query)
                    .ok_or_else(|| format!("Collection '{col_query}' not found"))?;
                vec![col.clone()]
            } else {
                snapshot.collections.clone()
            };

            let mut routes = routes_from_collections(&cols);
            if routes.is_empty() {
                for col in &cols {
                    for item in flatten_collections(std::slice::from_ref(col)) {
                        if let Some((_, req)) = find_request_across(&cols, item.id) {
                            let clean_path = crate::core::mock_server::path_only(&req.url);
                            routes.push(MockRoute {
                                method: req.method,
                                path: clean_path,
                                example: SavedExample {
                                    id: Uuid::new_v4(),
                                    name: "Default Mock".to_string(),
                                    status: 200,
                                    status_text: "OK".to_string(),
                                    headers: vec![("content-type".to_string(), "application/json".to_string())],
                                    body: if !req.body.is_empty() { req.body.clone() } else { format!("{{\"mock\": true, \"name\": \"{}\"}}", req.name) },
                                },
                                match_header: None,
                            });
                        }
                    }
                }
            }

            if routes.is_empty() {
                return Err("No requests found in collections to mock".to_string());
            }

            let route_summaries: Vec<JsonValue> = routes
                .iter()
                .map(|r| {
                    json!({
                        "method": r.method.as_str(),
                        "path": r.path,
                        "example": r.example.name,
                        "status": r.example.status
                    })
                })
                .collect();

            if let Ok(mut lock) = ACTIVE_MOCK.lock() {
                if let Some(old_handle) = lock.take() {
                    old_handle.stop();
                }
            }

            let handle = start_mock(routes, port).map_err(|e| format!("Failed to start mock: {e}"))?;
            let bound_port = handle.port;
            let mock_url = format!("http://127.0.0.1:{bound_port}");

            if let Ok(mut lock) = ACTIVE_MOCK.lock() {
                *lock = Some(handle);
            }

            let res = json!({
                "status": "running",
                "port": bound_port,
                "mock_url": mock_url,
                "routes_count": route_summaries.len(),
                "routes": route_summaries
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_stop_mock" => {
            let was_running = if let Ok(mut lock) = ACTIVE_MOCK.lock() {
                if let Some(handle) = lock.take() {
                    handle.stop();
                    true
                } else {
                    false
                }
            } else {
                false
            };

            let res = json!({
                "status": if was_running { "stopped" } else { "not_running" },
                "message": if was_running { "Mock server stopped successfully" } else { "No mock server was currently running" }
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_export_collection" => {
            let col_query = args
                .get("collection")
                .and_then(|c| c.as_str())
                .ok_or_else(|| "Missing required parameter 'collection'".to_string())?;
            let format_arg = args
                .get("format")
                .and_then(|f| f.as_str())
                .unwrap_or("postman")
                .to_lowercase();
            let file_path_opt = args.get("file_path").and_then(|p| p.as_str());

            let target_col = snapshot
                .collections
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(col_query) || c.id.to_string() == col_query)
                .ok_or_else(|| format!("Collection '{col_query}' not found"))?;

            let exported_content = if format_arg == "openapi" {
                export_openapi3(target_col).map_err(|e| format!("Export OpenAPI: {e}"))?
            } else {
                export_collection_v21(target_col).map_err(|e| format!("Export Postman: {e}"))?
            };

            let written_to = if let Some(path_str) = file_path_opt {
                std::fs::write(path_str, &exported_content)
                    .map_err(|e| format!("Write to file '{path_str}': {e}"))?;
                Some(path_str.to_string())
            } else {
                None
            };

            let res = json!({
                "status": "ok",
                "collection": target_col.name,
                "format": format_arg,
                "written_to_file": written_to,
                "content": if written_to.is_some() { JsonValue::Null } else { serde_json::from_str::<JsonValue>(&exported_content).unwrap_or(JsonValue::String(exported_content)) }
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_diff_responses" => {
            let body_a_opt = args.get("body_a").and_then(|b| b.as_str());
            let body_b_opt = args.get("body_b").and_then(|b| b.as_str());
            let req_id_opt = args.get("request_id").and_then(|r| r.as_str());

            let (label_a, label_b, body_a, body_b, status_comp) = if let (Some(a), Some(b)) = (body_a_opt, body_b_opt) {
                ("Payload A".to_string(), "Payload B".to_string(), a.to_string(), b.to_string(), None)
            } else if let Some(req_id) = req_id_opt {
                let (_, req_draft) = find_request_by_id_or_name(&snapshot.collections, req_id, None)?;
                let env_a_name = args.get("environment_a").and_then(|e| e.as_str()).unwrap_or("Env A");
                let env_b_name = args.get("environment_b").and_then(|e| e.as_str()).unwrap_or("Env B");

                let mut ctx_a = snapshot.variables.clone();
                if let Some(env_a) = find_env_by_name_or_id(&snapshot.environments, env_a_name) {
                    ctx_a.environment = env_a.to_map();
                }
                let mut jar_a = snapshot.cookies.clone();
                let view_a = send_request_with_options(&req_draft, &mut ctx_a, SendOptions { cookie_jar: Some(&mut jar_a), ..Default::default() })
                    .map_err(|e| format!("Execution in {env_a_name}: {e}"))?;

                let mut ctx_b = snapshot.variables.clone();
                if let Some(env_b) = find_env_by_name_or_id(&snapshot.environments, env_b_name) {
                    ctx_b.environment = env_b.to_map();
                }
                let mut jar_b = snapshot.cookies.clone();
                let view_b = send_request_with_options(&req_draft, &mut ctx_b, SendOptions { cookie_jar: Some(&mut jar_b), ..Default::default() })
                    .map_err(|e| format!("Execution in {env_b_name}: {e}"))?;

                let comp = json!({
                    "status_a": view_a.status,
                    "status_b": view_b.status,
                    "status_match": view_a.status == view_b.status,
                    "latency_a_ms": view_a.timing.total_ms,
                    "latency_b_ms": view_b.timing.total_ms
                });
                (env_a_name.to_string(), env_b_name.to_string(), view_a.body, view_b.body, Some(comp))
            } else {
                return Err("Missing comparison inputs: provide 'body_a' and 'body_b', or 'request_id' with 'environment_a' and 'environment_b'".to_string());
            };

            let (diff_lines, stats) = diff_bodies(&body_a, &body_b);
            let formatted_lines: Vec<String> = diff_lines
                .iter()
                .map(|line| match line {
                    DiffLine::Same(s) => format!("  {s}"),
                    DiffLine::Add(s) => format!("+ {s}"),
                    DiffLine::Del(s) => format!("- {s}"),
                })
                .collect();

            let res = json!({
                "target_a": label_a,
                "target_b": label_b,
                "stats": {
                    "added_lines": stats.added,
                    "removed_lines": stats.removed,
                    "unchanged_lines": stats.unchanged,
                    "identical": stats.added == 0 && stats.removed == 0
                },
                "status_comparison": status_comp,
                "diff": formatted_lines
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_delete_collection" => {
            let col_query = args
                .get("collection")
                .and_then(|c| c.as_str())
                .ok_or_else(|| "Missing required parameter 'collection'".to_string())?;

            let mut snapshot = snapshot;
            let pos = snapshot
                .collections
                .iter()
                .position(|c| c.name.eq_ignore_ascii_case(col_query) || c.id.to_string() == col_query)
                .ok_or_else(|| format!("Collection '{col_query}' not found"))?;

            let removed_col = snapshot.collections.remove(pos);
            store.save(&snapshot).map_err(|e| format!("Save snapshot: {e}"))?;

            let res = json!({
                "status": "deleted",
                "collection_id": removed_col.id.to_string(),
                "name": removed_col.name
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_generate_docs" => {
            let col_query = args
                .get("collection")
                .and_then(|c| c.as_str())
                .ok_or_else(|| "Missing required parameter 'collection'".to_string())?;
            let format_arg = args
                .get("format")
                .and_then(|f| f.as_str())
                .unwrap_or("static")
                .to_lowercase();
            let file_path_opt = args.get("file_path").and_then(|p| p.as_str());

            let target_col = snapshot
                .collections
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(col_query) || c.id.to_string() == col_query)
                .ok_or_else(|| format!("Collection '{col_query}' not found"))?;

            let html_content = if format_arg == "swagger_ui" {
                generate_swagger_ui_html(target_col)
                    .map_err(|e| format!("Generate Swagger UI: {e}"))?
            } else {
                generate_static_doc_html(target_col)
            };

            let written_to = if let Some(path_str) = file_path_opt {
                std::fs::write(path_str, &html_content)
                    .map_err(|e| format!("Write to file '{path_str}': {e}"))?;
                Some(path_str.to_string())
            } else {
                None
            };

            let res = json!({
                "status": "ok",
                "collection": target_col.name,
                "format": format_arg,
                "written_to_file": written_to,
                "html": if written_to.is_some() { JsonValue::Null } else { JsonValue::String(html_content) }
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_create_folder" => {
            let col_query = args
                .get("collection")
                .and_then(|c| c.as_str())
                .ok_or_else(|| "Missing required parameter 'collection'".to_string())?;
            let folder_name = args
                .get("name")
                .and_then(|n| n.as_str())
                .ok_or_else(|| "Missing required parameter 'name'".to_string())?;
            let parent_folder_opt = args.get("parent_folder").and_then(|p| p.as_str());
            let desc_opt = args.get("description").and_then(|d| d.as_str()).unwrap_or("");

            let mut snapshot = snapshot;
            let col = snapshot
                .collections
                .iter_mut()
                .find(|c| c.name.eq_ignore_ascii_case(col_query) || c.id.to_string() == col_query)
                .ok_or_else(|| format!("Collection '{col_query}' not found"))?;

            let mut new_folder = Folder::new(folder_name);
            new_folder.description = desc_opt.to_string();
            let new_folder_id = new_folder.id;

            let path_result = if let Some(parent_query) = parent_folder_opt {
                let parent = find_folder_by_path_or_name_mut(&mut col.items, parent_query)
                    .ok_or_else(|| format!("Parent folder '{parent_query}' not found"))?;
                let path = format!("{}/{}", parent.name, folder_name);
                parent.items.push(CollectionItem::Folder(new_folder));
                path
            } else {
                col.items.push(CollectionItem::Folder(new_folder));
                folder_name.to_string()
            };

            let col_name = col.name.clone();
            store.save(&snapshot).map_err(|e| format!("Save snapshot: {e}"))?;

            let res = json!({
                "status": "ok",
                "id": new_folder_id.to_string(),
                "name": folder_name,
                "collection": col_name,
                "path": path_result
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_delete_folder" => {
            let col_query = args
                .get("collection")
                .and_then(|c| c.as_str())
                .ok_or_else(|| "Missing required parameter 'collection'".to_string())?;
            let folder_query = args
                .get("folder")
                .and_then(|f| f.as_str())
                .ok_or_else(|| "Missing required parameter 'folder'".to_string())?;

            let mut snapshot = snapshot;
            let col = snapshot
                .collections
                .iter_mut()
                .find(|c| c.name.eq_ignore_ascii_case(col_query) || c.id.to_string() == col_query)
                .ok_or_else(|| format!("Collection '{col_query}' not found"))?;

            let folder_id = {
                let f = find_folder_by_path_or_name(&col.items, folder_query)
                    .ok_or_else(|| format!("Folder '{folder_query}' not found in collection '{}'", col.name))?;
                f.id
            };

            let col_name = col.name.clone();
            let removed = col.remove_by_id(folder_id);
            if !removed {
                return Err(format!("Failed to delete folder '{folder_query}'"));
            }

            store.save(&snapshot).map_err(|e| format!("Save snapshot: {e}"))?;

            let res = json!({
                "status": "deleted",
                "folder": folder_query,
                "folder_id": folder_id.to_string(),
                "collection": col_name
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_delete_environment" => {
            let env_query = args
                .get("environment")
                .and_then(|e| e.as_str())
                .ok_or_else(|| "Missing required parameter 'environment'".to_string())?;

            let mut snapshot = snapshot;
            let pos = snapshot
                .environments
                .iter()
                .position(|e| e.name.eq_ignore_ascii_case(env_query) || e.id.to_string() == env_query)
                .ok_or_else(|| format!("Environment '{env_query}' not found"))?;

            let removed = snapshot.environments.remove(pos);
            if snapshot.active_environment_id == Some(removed.id) {
                snapshot.active_environment_id = None;
                snapshot.active_environment.clear();
            }

            store.save(&snapshot).map_err(|e| format!("Save snapshot: {e}"))?;

            let res = json!({
                "status": "deleted",
                "environment_id": removed.id.to_string(),
                "name": removed.name
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_clear_history" => {
            let req_id_opt = args.get("request_id").and_then(|r| r.as_str());

            let mut snapshot = snapshot;
            let initial_len = snapshot.history.len();

            if let Some(req_id) = req_id_opt {
                snapshot.history.retain(|h| {
                    h.draft.id.to_string() != req_id && h.url != req_id
                });
            } else {
                snapshot.history.clear();
            }

            let cleared = initial_len - snapshot.history.len();
            store.save(&snapshot).map_err(|e| format!("Save snapshot: {e}"))?;

            let res = json!({
                "status": "cleared",
                "cleared_count": cleared,
                "remaining_count": snapshot.history.len()
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_import_har" => {
            let har_arg = args
                .get("har")
                .and_then(|h| h.as_str())
                .ok_or_else(|| "Missing required parameter 'har'".to_string())?;
            let col_name_opt = args.get("collection").and_then(|c| c.as_str());

            let har_content = if std::path::Path::new(har_arg).exists() {
                std::fs::read_to_string(har_arg)
                    .map_err(|e| format!("Failed to read HAR file '{har_arg}': {e}"))?
            } else {
                har_arg.to_string()
            };

            let mut col = import_har(&har_content).map_err(|e| format!("Import HAR: {e}"))?;
            if let Some(custom_name) = col_name_opt {
                col.name = custom_name.to_string();
            }

            let col_name = col.name.clone();
            let req_count = count_requests_in_items(&col.items);
            let mut snapshot = snapshot;
            let stats = crate::core::upsert_collection(&mut snapshot.collections, col);
            store.save(&snapshot).map_err(|e| format!("Save snapshot: {e}"))?;

            let res = json!({
                "status": "ok",
                "collection": col_name,
                "created_new": stats.created_new,
                "added_requests": stats.added_requests,
                "updated_requests": stats.updated_requests,
                "request_count": req_count
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_manage_cookies" => {
            let action = args
                .get("action")
                .and_then(|a| a.as_str())
                .unwrap_or("list")
                .to_lowercase();
            let domain_filter = args.get("domain").and_then(|d| d.as_str()).map(|d| d.to_lowercase());

            let mut snapshot = snapshot;
            let res = match action.as_str() {
                "clear" => {
                    let before = snapshot.cookies.cookies.len();
                    if let Some(ref d) = domain_filter {
                        snapshot.cookies.cookies.retain(|c| !c.domain.to_lowercase().contains(d));
                    } else {
                        snapshot.cookies.clear();
                    }
                    let cleared_count = before.saturating_sub(snapshot.cookies.cookies.len());
                    store.save(&snapshot).map_err(|e| format!("Save snapshot: {e}"))?;
                    json!({
                        "status": "cleared",
                        "cleared_count": cleared_count,
                        "remaining_count": snapshot.cookies.cookies.len()
                    })
                }
                "set" => {
                    let domain = args
                        .get("domain")
                        .and_then(|d| d.as_str())
                        .ok_or_else(|| "Missing required parameter 'domain' for cookie set".to_string())?
                        .trim()
                        .to_lowercase();
                    let name = args
                        .get("name")
                        .and_then(|n| n.as_str())
                        .ok_or_else(|| "Missing required parameter 'name' for cookie set".to_string())?;
                    let value = args
                        .get("value")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter 'value' for cookie set".to_string())?;
                    let path = args
                        .get("path")
                        .and_then(|p| p.as_str())
                        .unwrap_or("/")
                        .to_string();
                    snapshot.cookies.upsert(Cookie {
                        domain: domain.clone(),
                        name: name.to_string(),
                        value: value.to_string(),
                        path: path.clone(),
                    });
                    store.save(&snapshot).map_err(|e| format!("Save snapshot: {e}"))?;
                    json!({
                        "status": "ok",
                        "cookie": { "domain": domain, "name": name, "value": value, "path": path },
                        "total_count": snapshot.cookies.cookies.len()
                    })
                }
                _ => {
                    let filtered: Vec<JsonValue> = snapshot
                        .cookies
                        .cookies
                        .iter()
                        .filter(|c| {
                            if let Some(ref d) = domain_filter {
                                c.domain.to_lowercase().contains(d)
                            } else {
                                true
                            }
                        })
                        .map(|c| {
                            json!({
                                "domain": c.domain,
                                "name": c.name,
                                "value": c.value,
                                "path": c.path
                            })
                        })
                        .collect();
                    json!({
                        "count": filtered.len(),
                        "cookies": filtered
                    })
                }
            };
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_websocket_send" => {
            let raw_url = args
                .get("url")
                .and_then(|u| u.as_str())
                .ok_or_else(|| "Missing required parameter 'url'".to_string())?;
            let ctx = build_mcp_var_ctx(
                &snapshot,
                args.get("environment").and_then(|e| e.as_str()),
                args.get("override_vars"),
            )?;
            let url = ctx.resolve_string(raw_url);
            let headers_vec = resolve_headers_with_ctx(parse_headers_arg(args.get("headers")), &ctx);
            let outbound = parse_ws_messages_arg(args, &ctx)?;
            let timeout_ms = args
                .get("timeout_ms")
                .and_then(|t| t.as_u64())
                .unwrap_or(2000)
                .min(30_000);
            let max_msgs = args
                .get("max_messages")
                .and_then(|m| m.as_u64())
                .unwrap_or(5)
                .min(200) as usize;

            let total_start = Instant::now();
            let sess = ws_connect_session_with_headers(&url, &headers_vec)
                .map_err(|e| format!("Connect: {e}"))?;
            if let Err(err) = ws_wait_handshake(&sess, Duration::from_millis(timeout_ms.min(5000))) {
                sess.close();
                return Err(err);
            }

            let mut sent_messages = Vec::with_capacity(outbound.len());
            for (msg, delay_ms) in outbound {
                if delay_ms > 0 {
                    std::thread::sleep(Duration::from_millis(delay_ms));
                }
                if let Err(e) = sess.send_text(&msg) {
                    sess.close();
                    return Err(format!("Send text: {e}"));
                }
                sent_messages.push(msg);
            }

            let mut messages = Vec::new();
            let mut seen_events = 0usize;
            let collect_start = Instant::now();
            let collect_limit = Duration::from_millis(timeout_ms);
            while collect_start.elapsed() < collect_limit && messages.len() < max_msgs {
                let evs = sess.events_snapshot();
                let mut closed_or_err = false;
                if evs.len() > seen_events {
                    for ev in &evs[seen_events..] {
                        match ev {
                            WsSessionEvent::Message(m) => {
                                messages.push(m.clone());
                                if messages.len() >= max_msgs {
                                    break;
                                }
                            }
                            WsSessionEvent::Closed(_) | WsSessionEvent::Error(_) => {
                                closed_or_err = true;
                            }
                            _ => {}
                        }
                    }
                    seen_events = evs.len();
                }
                if closed_or_err || messages.len() >= max_msgs {
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }

            sess.close();

            let res = json!({
                "status": "ok",
                "url": url,
                "sent": sent_messages.first().cloned(),
                "sent_messages": sent_messages,
                "messages": messages,
                "count": messages.len(),
                "elapsed_ms": total_start.elapsed().as_millis()
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_websocket_connect" => {
            let raw_url = args
                .get("url")
                .and_then(|u| u.as_str())
                .ok_or_else(|| "Missing required parameter 'url'".to_string())?;
            let ctx = build_mcp_var_ctx(
                &snapshot,
                args.get("environment").and_then(|e| e.as_str()),
                args.get("override_vars"),
            )?;
            let url = ctx.resolve_string(raw_url);
            let headers_vec = resolve_headers_with_ctx(parse_headers_arg(args.get("headers")), &ctx);
            let timeout_ms = args
                .get("timeout_ms")
                .and_then(|t| t.as_u64())
                .unwrap_or(3000)
                .min(15_000);
            let session_id = args
                .get("session_id")
                .and_then(|s| s.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| Uuid::new_v4().to_string());

            {
                let guard = WS_SESSIONS
                    .lock()
                    .map_err(|_| "WebSocket session registry poisoned".to_string())?;
                if let Some(map) = guard.as_ref() {
                    if map.contains_key(&session_id) {
                        return Err(format!(
                            "WebSocket session '{session_id}' already exists; close it first or choose another session_id"
                        ));
                    }
                    if map.len() >= MAX_MCP_WS_SESSIONS {
                        return Err(format!(
                            "Maximum concurrent WebSocket sessions ({MAX_MCP_WS_SESSIONS}) reached; close unused sessions with blitz_websocket_close"
                        ));
                    }
                }
            }

            let sess = ws_connect_session_with_headers(&url, &headers_vec)
                .map_err(|e| format!("Connect: {e}"))?;
            if let Err(err) = ws_wait_handshake(&sess, Duration::from_millis(timeout_ms)) {
                sess.close();
                return Err(err);
            }

            let evs = sess.events_snapshot();
            let initial_messages: Vec<String> = evs
                .iter()
                .filter_map(|e| match e {
                    WsSessionEvent::Message(m) => Some(m.clone()),
                    _ => None,
                })
                .collect();
            let cursor = evs.len();

            {
                let mut guard = WS_SESSIONS
                    .lock()
                    .map_err(|_| "WebSocket session registry poisoned".to_string())?;
                let map = guard.get_or_insert_with(HashMap::new);
                map.insert(
                    session_id.clone(),
                    McpWsSession {
                        url: url.clone(),
                        session: sess,
                        cursor,
                    },
                );
            }

            let out = json!({
                "status": "connected",
                "session_id": session_id,
                "url": url,
                "initial_messages": initial_messages
            });
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_websocket_session_send" => {
            let session_id = args
                .get("session_id")
                .and_then(|s| s.as_str())
                .ok_or_else(|| "Missing required parameter 'session_id'".to_string())?;
            let ctx = build_mcp_var_ctx(
                &snapshot,
                args.get("environment").and_then(|e| e.as_str()),
                args.get("override_vars"),
            )?;
            let outbound = parse_ws_messages_arg(args, &ctx)?;
            if outbound.is_empty() {
                return Err("Provide 'message' or non-empty 'messages'".to_string());
            }
            let wait_ms = args
                .get("wait_ms")
                .and_then(|t| t.as_u64())
                .unwrap_or(250)
                .min(10_000);
            let max_msgs = args
                .get("max_messages")
                .and_then(|m| m.as_u64())
                .unwrap_or(50)
                .min(200) as usize;

            // Send under lock (non-blocking unbounded channel send), then release lock while sleeping/collecting.
            let (url, start_cursor) = {
                let guard = WS_SESSIONS
                    .lock()
                    .map_err(|_| "WebSocket session registry poisoned".to_string())?;
                let map = guard
                    .as_ref()
                    .ok_or_else(|| format!("WebSocket session '{session_id}' not found"))?;
                let entry = map
                    .get(session_id)
                    .ok_or_else(|| format!("WebSocket session '{session_id}' not found"))?;
                (entry.url.clone(), entry.cursor)
            };

            let mut sent_messages = Vec::with_capacity(outbound.len());
            for (msg, delay_ms) in outbound {
                if delay_ms > 0 {
                    std::thread::sleep(Duration::from_millis(delay_ms));
                }
                let guard = WS_SESSIONS
                    .lock()
                    .map_err(|_| "WebSocket session registry poisoned".to_string())?;
                let entry = guard
                    .as_ref()
                    .and_then(|m| m.get(session_id))
                    .ok_or_else(|| format!("WebSocket session '{session_id}' was closed"))?;
                entry
                    .session
                    .send_text(&msg)
                    .map_err(|e| format!("Send text: {e}"))?;
                sent_messages.push(msg);
            }

            let wait_start = Instant::now();
            let wait_limit = Duration::from_millis(wait_ms);
            let mut new_cursor = start_cursor;
            let mut messages = Vec::new();
            let mut closed = false;
            let mut close_reason: Option<String> = None;
            let mut error: Option<String> = None;

            loop {
                let evs = {
                    let guard = WS_SESSIONS
                        .lock()
                        .map_err(|_| "WebSocket session registry poisoned".to_string())?;
                    let entry = guard
                        .as_ref()
                        .and_then(|m| m.get(session_id))
                        .ok_or_else(|| format!("WebSocket session '{session_id}' was closed"))?;
                    entry.session.events_snapshot()
                };
                if evs.len() > new_cursor {
                    for ev in &evs[new_cursor..] {
                        new_cursor += 1;
                        match ev {
                            WsSessionEvent::Message(m) => {
                                messages.push(m.clone());
                                if messages.len() >= max_msgs {
                                    break;
                                }
                            }
                            WsSessionEvent::Closed(r) => {
                                closed = true;
                                close_reason = Some(r.clone());
                            }
                            WsSessionEvent::Error(e) => {
                                closed = true;
                                error = Some(e.clone());
                            }
                            _ => {}
                        }
                    }
                }
                if closed || messages.len() >= max_msgs || wait_start.elapsed() >= wait_limit {
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }

            if let Ok(mut guard) = WS_SESSIONS.lock() {
                if let Some(entry) = guard.as_mut().and_then(|m| m.get_mut(session_id)) {
                    entry.cursor = new_cursor;
                }
            }

            let out = json!({
                "status": if closed { "closed" } else { "ok" },
                "session_id": session_id,
                "url": url,
                "sent_count": sent_messages.len(),
                "sent_messages": sent_messages,
                "messages": messages,
                "count": messages.len(),
                "closed": closed,
                "close_reason": close_reason,
                "error": error
            });
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_websocket_read" => {
            let session_id = args
                .get("session_id")
                .and_then(|s| s.as_str())
                .ok_or_else(|| "Missing required parameter 'session_id'".to_string())?;
            let timeout_ms = args
                .get("timeout_ms")
                .and_then(|t| t.as_u64())
                .unwrap_or(1000)
                .min(15_000);
            let max_msgs = args
                .get("max_messages")
                .and_then(|m| m.as_u64())
                .unwrap_or(50)
                .min(200) as usize;
            let include_history = args
                .get("include_history")
                .and_then(|b| b.as_bool())
                .unwrap_or(false);

            let (url, start_cursor) = {
                let guard = WS_SESSIONS
                    .lock()
                    .map_err(|_| "WebSocket session registry poisoned".to_string())?;
                let entry = guard
                    .as_ref()
                    .and_then(|m| m.get(session_id))
                    .ok_or_else(|| format!("WebSocket session '{session_id}' not found"))?;
                (
                    entry.url.clone(),
                    if include_history { 0 } else { entry.cursor },
                )
            };

            let read_start = Instant::now();
            let read_limit = Duration::from_millis(timeout_ms);
            let mut new_cursor = start_cursor;
            let mut messages = Vec::new();
            let mut closed = false;
            let mut close_reason: Option<String> = None;
            let mut error: Option<String> = None;

            loop {
                let evs = {
                    let guard = WS_SESSIONS
                        .lock()
                        .map_err(|_| "WebSocket session registry poisoned".to_string())?;
                    let entry = guard
                        .as_ref()
                        .and_then(|m| m.get(session_id))
                        .ok_or_else(|| format!("WebSocket session '{session_id}' was closed"))?;
                    entry.session.events_snapshot()
                };
                if evs.len() > new_cursor {
                    for ev in &evs[new_cursor..] {
                        new_cursor += 1;
                        match ev {
                            WsSessionEvent::Message(m) => {
                                messages.push(m.clone());
                                if messages.len() >= max_msgs {
                                    break;
                                }
                            }
                            WsSessionEvent::Closed(r) => {
                                closed = true;
                                close_reason = Some(r.clone());
                            }
                            WsSessionEvent::Error(e) => {
                                closed = true;
                                error = Some(e.clone());
                            }
                            _ => {}
                        }
                    }
                }
                if closed || messages.len() >= max_msgs || read_start.elapsed() >= read_limit {
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }

            if let Ok(mut guard) = WS_SESSIONS.lock() {
                if let Some(entry) = guard.as_mut().and_then(|m| m.get_mut(session_id)) {
                    entry.cursor = entry.cursor.max(new_cursor);
                }
            }

            let out = json!({
                "status": if closed { "closed" } else { "ok" },
                "session_id": session_id,
                "url": url,
                "messages": messages,
                "count": messages.len(),
                "closed": closed,
                "close_reason": close_reason,
                "error": error
            });
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_websocket_close" => {
            let close_all = args.get("all").and_then(|b| b.as_bool()).unwrap_or(false);
            let session_id = args.get("session_id").and_then(|s| s.as_str()).unwrap_or("");
            if close_all || session_id == "*" {
                let drained: Vec<(String, McpWsSession)> = {
                    let mut guard = WS_SESSIONS
                        .lock()
                        .map_err(|_| "WebSocket session registry poisoned".to_string())?;
                    guard
                        .as_mut()
                        .map(|m| m.drain().collect())
                        .unwrap_or_default()
                };
                let closed_ids: Vec<String> = drained
                    .into_iter()
                    .map(|(id, entry)| {
                        entry.session.close();
                        id
                    })
                    .collect();
                let out = json!({
                    "status": "closed",
                    "closed_sessions": closed_ids,
                    "count": closed_ids.len()
                });
                return serde_json::to_string_pretty(&out).map_err(|e| e.to_string());
            }

            if session_id.trim().is_empty() {
                return Err("Missing required parameter 'session_id' (or pass all: true)".to_string());
            }
            let removed = {
                let mut guard = WS_SESSIONS
                    .lock()
                    .map_err(|_| "WebSocket session registry poisoned".to_string())?;
                guard.as_mut().and_then(|m| m.remove(session_id))
            }
            .ok_or_else(|| format!("WebSocket session '{session_id}' not found"))?;
            removed.session.close();
            let out = json!({
                "status": "closed",
                "session_id": session_id
            });
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_grpc_unary" => {
            let endpoint = args
                .get("endpoint")
                .and_then(|e| e.as_str())
                .ok_or_else(|| "Missing required parameter 'endpoint'".to_string())?;
            let method = args
                .get("method")
                .and_then(|m| m.as_str())
                .ok_or_else(|| "Missing required parameter 'method'".to_string())?;
            let body_val = args
                .get("body")
                .ok_or_else(|| "Missing required parameter 'body'".to_string())?;
            let body_str = match body_val {
                JsonValue::String(s) => s.clone(),
                other => other.to_string(),
            };
            let headers_vec = parse_headers_arg(args.get("headers"));

            let res = grpc_unary_with_headers(endpoint, method, &body_str, &headers_vec);
            let parsed_body: JsonValue = serde_json::from_str(&res.body_json)
                .unwrap_or(JsonValue::String(res.body_json));

            let out = json!({
                "status": res.status,
                "grpc_status": res.grpc_status,
                "grpc_message": res.grpc_message,
                "response": parsed_body,
                "elapsed_ms": res.elapsed_ms,
                "error": res.error,
                "log": res.log
            });
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_sse_listen" => {
            let url = args
                .get("url")
                .and_then(|u| u.as_str())
                .ok_or_else(|| "Missing required parameter 'url'".to_string())?;
            let max_events = args
                .get("max_events")
                .and_then(|m| m.as_u64())
                .unwrap_or(5)
                .min(50) as usize;
            let timeout_ms = args
                .get("timeout_ms")
                .and_then(|t| t.as_u64())
                .unwrap_or(3000)
                .min(15_000);

            let mut headers_vec = Vec::new();
            if let Some(headers_map) = args.get("headers").and_then(|h| h.as_object()) {
                for (k, v) in headers_map {
                    if let Some(v_str) = v.as_str() {
                        headers_vec.push((k.clone(), v_str.to_string()));
                    }
                }
            }

            let (log, events) = sse_listen(url, &headers_vec, max_events, timeout_ms)
                .map_err(|e| format!("SSE error: {e}"))?;

            let events_json: Vec<JsonValue> = events
                .into_iter()
                .map(|e| {
                    json!({
                        "event": e.event,
                        "data": e.data,
                        "id": e.id
                    })
                })
                .collect();

            let out = json!({
                "status": "ok",
                "url": url,
                "events": events_json,
                "count": events_json.len(),
                "log": log
            });
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_mqtt_publish" => {
            let broker_url = args
                .get("broker_url")
                .and_then(|b| b.as_str())
                .ok_or_else(|| "Missing required parameter 'broker_url'".to_string())?;
            let topic = args
                .get("topic")
                .and_then(|t| t.as_str())
                .ok_or_else(|| "Missing required parameter 'topic'".to_string())?;
            let payload = args
                .get("payload")
                .and_then(|p| p.as_str())
                .ok_or_else(|| "Missing required parameter 'payload'".to_string())?;
            let default_client_id = format!("blitz-mcp-{}", &Uuid::new_v4().to_string()[..8]);
            let client_id = args
                .get("client_id")
                .and_then(|c| c.as_str())
                .unwrap_or(&default_client_id);
            let retain = args.get("retain").and_then(|r| r.as_bool()).unwrap_or(false);

            let res = mqtt_publish_opts(broker_url, client_id, topic, payload, retain);
            let out = json!({
                "ok": res.ok,
                "topic": topic,
                "messages": res.messages,
                "elapsed_ms": res.elapsed_ms,
                "error": res.error
            });
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_mqtt_subscribe" => {
            let broker_url = args
                .get("broker_url")
                .and_then(|b| b.as_str())
                .ok_or_else(|| "Missing required parameter 'broker_url'".to_string())?;
            let topic = args
                .get("topic")
                .and_then(|t| t.as_str())
                .ok_or_else(|| "Missing required parameter 'topic'".to_string())?;
            let default_client_id = format!("blitz-mcp-{}", &Uuid::new_v4().to_string()[..8]);
            let client_id = args
                .get("client_id")
                .and_then(|c| c.as_str())
                .unwrap_or(&default_client_id);
            let timeout_secs = args
                .get("timeout_seconds")
                .and_then(|t| t.as_u64())
                .unwrap_or(3)
                .min(10);

            let res = mqtt_subscribe_once(
                broker_url,
                client_id,
                topic,
                Duration::from_secs(timeout_secs),
            );
            let out = json!({
                "ok": res.ok,
                "topic": topic,
                "messages": res.messages,
                "elapsed_ms": res.elapsed_ms,
                "error": res.error
            });
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_graphql_ws_subscribe" => {
            let ws_url = args
                .get("ws_url")
                .and_then(|u| u.as_str())
                .ok_or_else(|| "Missing required parameter 'ws_url'".to_string())?;
            let query = args
                .get("query")
                .and_then(|q| q.as_str())
                .ok_or_else(|| "Missing required parameter 'query'".to_string())?;
            let vars_str = match args.get("variables") {
                Some(JsonValue::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => "{}".to_string(),
            };
            let headers_vec = parse_headers_arg(args.get("headers"));
            let max_events = args
                .get("max_events")
                .and_then(|m| m.as_u64())
                .unwrap_or(3)
                .min(20) as usize;
            let timeout_ms = args
                .get("timeout_ms")
                .and_then(|t| t.as_u64())
                .unwrap_or(3000)
                .min(10_000);

            let events = graphql_ws_subscribe_with_headers(
                ws_url,
                query,
                &vars_str,
                &headers_vec,
                max_events,
                timeout_ms,
            )
            .map_err(|e| format!("GraphQL WS error: {e}"))?;

            let out = json!({
                "status": "ok",
                "ws_url": ws_url,
                "events": events,
                "count": events.len()
            });
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_socketio_emit" => {
            let url = args
                .get("url")
                .and_then(|u| u.as_str())
                .ok_or_else(|| "Missing required parameter 'url'".to_string())?;
            let emit_event = args
                .get("emit_event")
                .and_then(|e| e.as_str())
                .unwrap_or("");
            let emit_payload_str = match args.get("emit_payload") {
                Some(JsonValue::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => "{}".to_string(),
            };
            let listen_event = args
                .get("listen_event")
                .and_then(|l| l.as_str())
                .unwrap_or("message");
            let listen_ms = args
                .get("listen_ms")
                .and_then(|l| l.as_u64())
                .unwrap_or(2000)
                .min(10_000);

            let events = socketio_session(
                url,
                emit_event,
                &emit_payload_str,
                listen_event,
                listen_ms,
            )
            .map_err(|e| format!("Socket.IO error: {e}"))?;

            let out = json!({
                "status": "ok",
                "url": url,
                "events": events,
                "count": events.len()
            });
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_lint_collection" => {
            let col_opt = args.get("collection").and_then(|c| c.as_str());
            let target_cols: Vec<Collection> = if let Some(col_query) = col_opt {
                let col = snapshot
                    .collections
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(col_query) || c.id.to_string() == col_query)
                    .ok_or_else(|| format!("Collection '{col_query}' not found"))?;
                vec![col.clone()]
            } else {
                snapshot.collections.clone()
            };

            let mut ctx = snapshot.variables.clone();
            ctx.environment = resolve_env_map(&snapshot, args.get("environment").and_then(|e| e.as_str()))?;
            let issues = lint_collections(&target_cols, &ctx);
            let error_count = lint_error_count(&issues);
            let issue_list: Vec<JsonValue> = issues
                .iter()
                .map(|i| {
                    json!({
                        "collection": i.collection_name,
                        "request": i.request_name,
                        "field": i.field,
                        "severity": match i.severity {
                            LintSeverity::Error => "error",
                            LintSeverity::Warning => "warning",
                        },
                        "message": i.message,
                    })
                })
                .collect();
            let out = json!({
                "collections_checked": target_cols.len(),
                "error_count": error_count,
                "warning_count": issue_list.len().saturating_sub(error_count),
                "total_issues": issue_list.len(),
                "issues": issue_list,
            });
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_diff_environments" => {
            let env_a_q = args
                .get("environment_a")
                .and_then(|e| e.as_str())
                .ok_or_else(|| "Missing required parameter 'environment_a'".to_string())?;
            let env_b_q = args
                .get("environment_b")
                .and_then(|e| e.as_str())
                .ok_or_else(|| "Missing required parameter 'environment_b'".to_string())?;

            let env_a = find_env_by_name_or_id(&snapshot.environments, env_a_q)
                .ok_or_else(|| format!("Environment '{env_a_q}' not found"))?;
            let env_b = find_env_by_name_or_id(&snapshot.environments, env_b_q)
                .ok_or_else(|| format!("Environment '{env_b_q}' not found"))?;

            let report = compare_environments(env_a, env_b);
            let markdown = export_diff_markdown(&report);
            let rows: Vec<JsonValue> = report
                .rows
                .iter()
                .map(|r| {
                    json!({
                        "key": r.key,
                        "status": r.status.label(),
                        "baseline": r.baseline_var.as_ref().map(|v| v.effective()),
                        "target": r.target_var.as_ref().map(|v| v.effective()),
                    })
                })
                .collect();
            let out = json!({
                "baseline": report.baseline_name,
                "target": report.target_name,
                "total_keys": report.total_keys,
                "matching": report.matching_count,
                "drift": report.drift_count,
                "missing_in_target": report.missing_in_target_count,
                "extra_in_target": report.extra_in_target_count,
                "rows": rows,
                "markdown": markdown,
            });
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_set_collection_var" => {
            let col_query = args
                .get("collection")
                .and_then(|c| c.as_str())
                .ok_or_else(|| "Missing required parameter 'collection'".to_string())?;
            let key = args
                .get("key")
                .and_then(|k| k.as_str())
                .ok_or_else(|| "Missing required parameter 'key'".to_string())?;
            let delete = args.get("delete").and_then(|d| d.as_bool()).unwrap_or(false);
            let enabled = args.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true);

            let mut snapshot = snapshot;
            let col = snapshot
                .collections
                .iter_mut()
                .find(|c| c.name.eq_ignore_ascii_case(col_query) || c.id.to_string() == col_query)
                .ok_or_else(|| format!("Collection '{col_query}' not found"))?;

            let action_taken = if delete {
                col.variables.retain(|v| v.key != key);
                "deleted"
            } else {
                let value = args
                    .get("value")
                    .and_then(json_val_to_string)
                    .ok_or_else(|| "Missing required parameter 'value'".to_string())?;
                if let Some(existing) = col.variables.iter_mut().find(|v| v.key == key) {
                    existing.value = value;
                    existing.enabled = enabled;
                    "updated"
                } else {
                    col.variables.push(KeyValue {
                        key: key.to_string(),
                        value,
                        enabled,
                    });
                    "created"
                }
            };
            let col_name = col.name.clone();
            let var_count = col.variables.len();
            store.save(&snapshot).map_err(|e| format!("Save snapshot: {e}"))?;

            let out = json!({
                "status": "ok",
                "action": action_taken,
                "collection": col_name,
                "key": key,
                "variable_count": var_count,
            });
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_import_collection" => {
            let data_arg = args
                .get("data")
                .and_then(|d| d.as_str())
                .ok_or_else(|| "Missing required parameter 'data'".to_string())?;
            let custom_name = args.get("collection").and_then(|c| c.as_str());

            let content = if std::path::Path::new(data_arg).exists() {
                std::fs::read_to_string(data_arg)
                    .map_err(|e| format!("Failed to read file '{data_arg}': {e}"))?
            } else {
                data_arg.to_string()
            };

            let (mut col, detected_format) = if let Ok(c) = import_collection_v21(&content) {
                (c, "postman_v2.1")
            } else if let Ok(c) = import_openapi3(&content) {
                (c, "openapi_3")
            } else if let Ok(c) = serde_json::from_str::<Collection>(&content) {
                (c, "blitz_json")
            } else {
                return Err("Unsupported collection format (expected Postman v2.1, OpenAPI 3.0, or Blitz JSON)".to_string());
            };

            if let Some(name_override) = custom_name {
                col.name = name_override.to_string();
            }
            let col_name = col.name.clone();
            let col_id = col.id.to_string();
            let req_count = count_requests_in_items(&col.items);
            let mut snapshot = snapshot;
            let stats = crate::core::upsert_collection(&mut snapshot.collections, col);
            store.save(&snapshot).map_err(|e| format!("Save snapshot: {e}"))?;

            let out = json!({
                "status": "ok",
                "format": detected_format,
                "collection_id": col_id,
                "collection": col_name,
                "created_new": stats.created_new,
                "added_requests": stats.added_requests,
                "updated_requests": stats.updated_requests,
                "request_count": req_count,
            });
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_mcp_call" => {
            let cmd = args
                .get("command")
                .and_then(|c| c.as_str())
                .ok_or_else(|| "Missing required parameter 'command'".to_string())?;
            let argv: Vec<String> = match args.get("args") {
                Some(JsonValue::Array(arr)) => arr
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect(),
                Some(JsonValue::String(s)) => split_mcp_args(s),
                _ => Vec::new(),
            };
            if let Some(tool_name) = args.get("tool").and_then(|t| t.as_str()) {
                let tool_args = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
                let result = mcp_call_tool(cmd, &argv, tool_name, tool_args)
                    .map_err(|e| format!("MCP call failed: {e}"))?;
                let parsed: JsonValue =
                    serde_json::from_str(&result).unwrap_or(JsonValue::String(result));
                let out = json!({
                    "status": "ok",
                    "command": cmd,
                    "tool": tool_name,
                    "result": parsed,
                });
                serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
            } else {
                let tools = mcp_list_tools(cmd, &argv)
                    .map_err(|e| format!("MCP list_tools failed: {e}"))?;
                let list: Vec<JsonValue> = tools
                    .iter()
                    .map(|t| json!({ "name": t.name, "description": t.description }))
                    .collect();
                let out = json!({
                    "status": "ok",
                    "command": cmd,
                    "tool_count": list.len(),
                    "tools": list,
                });
                serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
            }
        }
        _ => Err(format!("Unknown tool: {name}")),
    }
}

fn resolve_env_map(
    snapshot: &AppSnapshot,
    explicit_env: Option<&str>,
) -> Result<HashMap<String, String>, String> {
    if let Some(env_name) = explicit_env {
        let env = find_env_by_name_or_id(&snapshot.environments, env_name)
            .ok_or_else(|| format!("Environment '{env_name}' not found"))?;
        return Ok(env.to_map());
    }
    if let Some(active_id) = snapshot.active_environment_id {
        if let Some(env) = snapshot.environments.iter().find(|e| e.id == active_id) {
            return Ok(env.to_map());
        }
    }
    if !snapshot.active_environment.is_empty()
        && !snapshot.active_environment.eq_ignore_ascii_case("no environment")
    {
        if let Some(env) = snapshot
            .environments
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(&snapshot.active_environment))
        {
            return Ok(env.to_map());
        }
    }
    Ok(snapshot.variables.environment.clone())
}

fn parse_headers_arg(val: Option<&JsonValue>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(map) = val.and_then(|h| h.as_object()) {
        for (k, v) in map {
            if let Some(s) = v.as_str() {
                out.push((k.clone(), s.to_string()));
            }
        }
    }
    out
}

fn build_mcp_var_ctx(
    snapshot: &AppSnapshot,
    environment: Option<&str>,
    override_vars: Option<&JsonValue>,
) -> Result<VariableContext, String> {
    let mut ctx = snapshot.variables.clone();
    ctx.environment = resolve_env_map(snapshot, environment)?;
    if let Some(obj) = override_vars.and_then(|v| v.as_object()) {
        for (k, v) in obj {
            let val = match v {
                JsonValue::String(s) => s.clone(),
                JsonValue::Null => continue,
                other => other.to_string(),
            };
            ctx.local.insert(k.clone(), val);
        }
    }
    Ok(ctx)
}

fn resolve_headers_with_ctx(
    headers: Vec<(String, String)>,
    ctx: &VariableContext,
) -> Vec<(String, String)> {
    headers
        .into_iter()
        .map(|(k, v)| (ctx.resolve_string(&k), ctx.resolve_string(&v)))
        .collect()
}

fn parse_form_data_arg(val: Option<&JsonValue>) -> Result<Vec<FormDataField>, String> {
    let Some(raw) = val else {
        return Ok(Vec::new());
    };
    let arr = raw
        .as_array()
        .ok_or_else(|| "'form_data' must be an array of field objects".to_string())?;
    let mut out = Vec::with_capacity(arr.len());
    for (idx, item) in arr.iter().enumerate() {
        let obj = item
            .as_object()
            .ok_or_else(|| format!("form_data[{idx}] must be an object"))?;
        let key = obj
            .get("key")
            .and_then(|k| k.as_str())
            .ok_or_else(|| format!("form_data[{idx}] missing string 'key'"))?
            .to_string();
        let enabled = obj.get("enabled").and_then(|b| b.as_bool()).unwrap_or(true);
        let file_path = obj.get("file_path").and_then(|p| p.as_str());
        let type_is_file = obj
            .get("type")
            .and_then(|t| t.as_str())
            .is_some_and(|t| t.eq_ignore_ascii_case("file"));
        let is_file = file_path.is_some() || type_is_file;
        let value = if let Some(fp) = file_path {
            fp.to_string()
        } else if let Some(v) = obj.get("value") {
            match v {
                JsonValue::String(s) => s.clone(),
                JsonValue::Null => String::new(),
                other => other.to_string(),
            }
        } else {
            String::new()
        };
        let content_type = obj
            .get("content_type")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());
        out.push(FormDataField {
            key,
            value,
            enabled,
            is_file,
            content_type,
        });
    }
    Ok(out)
}

fn parse_ws_messages_arg(
    args: &JsonValue,
    ctx: &VariableContext,
) -> Result<Vec<(String, u64)>, String> {
    let mut out = Vec::new();
    if let Some(msg) = args.get("message").and_then(|m| m.as_str()) {
        out.push((ctx.resolve_string(msg), 0));
    }
    if let Some(arr) = args.get("messages").and_then(|m| m.as_array()) {
        for (idx, item) in arr.iter().enumerate() {
            match item {
                JsonValue::String(s) => out.push((ctx.resolve_string(s), 0)),
                JsonValue::Object(obj) => {
                    let msg = obj
                        .get("message")
                        .and_then(|m| m.as_str())
                        .ok_or_else(|| format!("messages[{idx}] missing string 'message'"))?;
                    let delay_ms = obj
                        .get("delay_ms")
                        .and_then(|d| d.as_u64())
                        .unwrap_or(0)
                        .min(5000);
                    out.push((ctx.resolve_string(msg), delay_ms));
                }
                _ => {
                    return Err(format!(
                        "messages[{idx}] must be a string or {{message, delay_ms?}} object"
                    ))
                }
            }
        }
    }
    Ok(out)
}

fn ws_wait_handshake(sess: &WsSession, limit: Duration) -> Result<(), String> {
    let start = Instant::now();
    while start.elapsed() < limit {
        let evs = sess.events_snapshot();
        if evs.iter().any(|e| matches!(e, WsSessionEvent::Opened)) {
            return Ok(());
        }
        if let Some(err) = evs.iter().find_map(|e| match e {
            WsSessionEvent::Error(s) => Some(s.clone()),
            _ => None,
        }) {
            return Err(format!("WebSocket error: {err}"));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let evs = sess.events_snapshot();
    let err_msg = evs
        .into_iter()
        .find_map(|e| match e {
            WsSessionEvent::Error(s) => Some(s),
            _ => None,
        })
        .unwrap_or_else(|| "Connection timed out during handshake".to_string());
    Err(format!("WebSocket failed: {err_msg}"))
}

fn parse_protocol_arg(raw: &str) -> RequestProtocol {
    match raw.trim().to_ascii_lowercase().as_str() {
        "ws" | "websocket" => RequestProtocol::WebSocket,
        "grpc" => RequestProtocol::Grpc,
        "sse" => RequestProtocol::Sse,
        "mqtt" => RequestProtocol::Mqtt,
        "mcp" => RequestProtocol::Mcp,
        "socketio" | "socket.io" | "socket_io" => RequestProtocol::SocketIo,
        "graphql_ws" | "graphql-ws" | "gql_ws" => RequestProtocol::GraphqlWs,
        _ => RequestProtocol::Http,
    }
}

fn parse_auth_arg(val: &JsonValue) -> Option<AuthConfig> {
    let obj = val.as_object()?;
    let ty = obj.get("type").and_then(|t| t.as_str())?.trim().to_ascii_lowercase();
    let mut cfg = AuthConfig::default();
    match ty.as_str() {
        "inherit" => cfg.auth_type = AuthType::Inherit,
        "no_auth" | "none" => cfg.auth_type = AuthType::NoAuth,
        "bearer" => {
            cfg.auth_type = AuthType::Bearer;
            cfg.token = obj.get("token").and_then(|v| v.as_str()).unwrap_or("").to_string();
        }
        "basic" => {
            cfg.auth_type = AuthType::Basic;
            cfg.username = obj.get("username").and_then(|v| v.as_str()).unwrap_or("").to_string();
            cfg.password = obj.get("password").and_then(|v| v.as_str()).unwrap_or("").to_string();
        }
        "api_key" | "apikey" => {
            cfg.auth_type = AuthType::ApiKey;
            cfg.api_key_header = obj
                .get("header")
                .and_then(|v| v.as_str())
                .unwrap_or("X-API-Key")
                .to_string();
            cfg.token = obj
                .get("token")
                .or_else(|| obj.get("value"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }
        _ => return None,
    }
    Some(cfg)
}

fn parse_body_mode_arg(mode_opt: Option<&str>, body: &str) -> BodyMode {
    match mode_opt.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("none") => BodyMode::None,
        Some("raw") => BodyMode::Raw,
        Some("urlencoded") | Some("x-www-form-urlencoded") => BodyMode::Urlencoded,
        Some("form-data") | Some("formdata") | Some("form_data") => BodyMode::FormData,
        Some("graphql") => BodyMode::Graphql,
        Some("binary") => BodyMode::Binary,
        _ => {
            if body.trim().is_empty() {
                BodyMode::None
            } else {
                BodyMode::Raw
            }
        }
    }
}

fn upsert_folder_path_mut<'a>(items: &'a mut Vec<CollectionItem>, folder_path: &str) -> &'a mut Folder {
    if !folder_path.contains('/') {
        let found_idx = items.iter().position(|item| match item {
            CollectionItem::Folder(f) => f.name.eq_ignore_ascii_case(folder_path),
            _ => false,
        });
        let idx = match found_idx {
            Some(i) => i,
            None => {
                items.push(CollectionItem::Folder(Folder::new(folder_path)));
                items.len() - 1
            }
        };
        match &mut items[idx] {
            CollectionItem::Folder(f) => f,
            _ => unreachable!(),
        }
    } else {
        let mut cur_items = items;
        let mut segments: Vec<&str> = folder_path
            .split('/')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if segments.is_empty() {
            segments.push("General");
        }
        for (idx_seg, seg) in segments.iter().enumerate() {
            let found_idx = cur_items.iter().position(|item| match item {
                CollectionItem::Folder(f) => f.name.eq_ignore_ascii_case(seg),
                _ => false,
            });
            let pos = match found_idx {
                Some(i) => i,
                None => {
                    cur_items.push(CollectionItem::Folder(Folder::new(*seg)));
                    cur_items.len() - 1
                }
            };
            if idx_seg + 1 == segments.len() {
                return match &mut cur_items[pos] {
                    CollectionItem::Folder(f) => f,
                    _ => unreachable!(),
                };
            }
            cur_items = match &mut cur_items[pos] {
                CollectionItem::Folder(f) => &mut f.items,
                _ => unreachable!(),
            };
        }
        unreachable!("non-empty folder_path must produce at least one segment")
    }
}

fn find_request_in_items_mut<'a>(
    items: &'a mut [CollectionItem],
    id_or_name: &str,
) -> Option<&'a mut RequestDraft> {
    for item in items.iter_mut() {
        match item {
            CollectionItem::Request(r) => {
                if r.id.to_string() == id_or_name || r.name.eq_ignore_ascii_case(id_or_name) {
                    return Some(r);
                }
            }
            CollectionItem::Folder(f) => {
                if let Some(found) = find_request_in_items_mut(&mut f.items, id_or_name) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn find_folder_by_path_or_name_mut<'a>(
    items: &'a mut [CollectionItem],
    query: &str,
) -> Option<&'a mut Folder> {
    if query.contains('/') {
        let mut cur = items;
        let segments: Vec<&str> = query
            .split('/')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        for (idx_seg, seg) in segments.iter().enumerate() {
            let pos = cur.iter().position(|item| match item {
                CollectionItem::Folder(f) => f.name.eq_ignore_ascii_case(seg),
                _ => false,
            })?;
            if idx_seg + 1 == segments.len() {
                return match &mut cur[pos] {
                    CollectionItem::Folder(f) => Some(f),
                    _ => None,
                };
            }
            cur = match &mut cur[pos] {
                CollectionItem::Folder(f) => &mut f.items,
                _ => return None,
            };
        }
        return None;
    }
    for item in items.iter_mut() {
        if let CollectionItem::Folder(f) = item {
            if f.name.eq_ignore_ascii_case(query) {
                return Some(f);
            }
            if let Some(child) = find_folder_by_path_or_name_mut(&mut f.items, query) {
                return Some(child);
            }
        }
    }
    None
}

fn count_requests_in_items(items: &[CollectionItem]) -> usize {
    let mut count = 0;
    for item in items {
        match item {
            CollectionItem::Request(_) => count += 1,
            CollectionItem::Folder(f) => count += count_requests_in_items(&f.items),
        }
    }
    count
}

/// Resources exposed by Blitz to agents.
fn list_mcp_resources() -> Vec<JsonValue> {
    vec![
        json!({
            "uri": "blitz://collections",
            "name": "Blitz Collections Hierarchy",
            "description": "Full JSON export of all collections, folders, and request drafts in Blitz.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "blitz://environments",
            "name": "Blitz Environments",
            "description": "All configured Blitz environments and non-secret variables.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "blitz://active-env",
            "name": "Blitz Active Environment",
            "description": "The currently active Blitz environment and its resolved key-value pairs.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "blitz://history",
            "name": "Blitz Request History",
            "description": "Recent HTTP request execution history.",
            "mimeType": "application/json"
        })
    ]
}

/// Read a specific Blitz resource.
fn read_mcp_resource(uri: &str) -> Result<(&'static str, String), String> {
    let store = JsonStore::open_default().map_err(|e| e.to_string())?;
    let snapshot = store.load().map_err(|e| e.to_string())?;

    match uri {
        "blitz://collections" => {
            let s = serde_json::to_string_pretty(&snapshot.collections).map_err(|e| e.to_string())?;
            Ok(("application/json", s))
        }
        "blitz://environments" => {
            let s = serde_json::to_string_pretty(&snapshot.environments).map_err(|e| e.to_string())?;
            Ok(("application/json", s))
        }
        "blitz://active-env" => {
            let res = json!({
                "active_environment": snapshot.active_environment,
                "active_environment_id": snapshot.active_environment_id.map(|id| id.to_string()),
                "variables": snapshot.variables.environment
            });
            let s = serde_json::to_string_pretty(&res).map_err(|e| e.to_string())?;
            Ok(("application/json", s))
        }
        "blitz://history" => {
            let s = serde_json::to_string_pretty(&snapshot.history).map_err(|e| e.to_string())?;
            Ok(("application/json", s))
        }
        _ => Err(format!("Unknown resource URI: {uri}")),
    }
}

// Helpers
fn collect_request_summaries(
    items: &[CollectionItem],
    current_folder: &str,
    out: &mut Vec<JsonValue>,
) {
    for item in items {
        match item {
            CollectionItem::Request(r) => {
                let path = if current_folder.is_empty() {
                    r.name.clone()
                } else {
                    format!("{}/{}", current_folder, r.name)
                };
                let folder_val = if current_folder.is_empty() {
                    JsonValue::Null
                } else {
                    json!(current_folder)
                };
                out.push(json!({
                    "id": r.id.to_string(),
                    "name": r.name,
                    "method": r.method.as_str(),
                    "url": r.url,
                    "path": path,
                    "folder": folder_val
                }));
            }
            CollectionItem::Folder(f) => {
                let next_folder = if current_folder.is_empty() {
                    f.name.clone()
                } else {
                    format!("{}/{}", current_folder, f.name)
                };
                collect_request_summaries(&f.items, &next_folder, out);
            }
        }
    }
}

fn find_folder_by_path_or_name<'a>(items: &'a [CollectionItem], query: &str) -> Option<&'a Folder> {
    let clean = query.trim().trim_matches('/');
    if clean.is_empty() {
        return None;
    }

    // 1. Slash-separated path search e.g. "User/Account"
    let segments: Vec<&str> = clean
        .split('/')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if segments.len() > 1 {
        let mut curr_items = items;
        let mut matched = None;
        for (i, seg) in segments.iter().enumerate() {
            let found = curr_items.iter().find_map(|item| match item {
                CollectionItem::Folder(f) if f.name.eq_ignore_ascii_case(seg) => Some(f),
                _ => None,
            });
            if let Some(f) = found {
                if i == segments.len() - 1 {
                    return Some(f);
                }
                curr_items = &f.items;
                matched = Some(f);
            } else {
                matched = None;
                break;
            }
        }
        if matched.is_some() {
            return matched;
        }
    }

    // 2. Recursive name or UUID search
    for item in items {
        if let CollectionItem::Folder(f) = item {
            if f.name.eq_ignore_ascii_case(clean) || f.id.to_string() == clean {
                return Some(f);
            }
            if let Some(sub) = find_folder_by_path_or_name(&f.items, clean) {
                return Some(sub);
            }
        }
    }
    None
}

fn collect_folder_paths(items: &[CollectionItem], prefix: &str, out: &mut Vec<String>) {
    for item in items {
        if let CollectionItem::Folder(f) = item {
            let path = if prefix.is_empty() {
                f.name.clone()
            } else {
                format!("{}/{}", prefix, f.name)
            };
            out.push(path.clone());
            collect_folder_paths(&f.items, &path, out);
        }
    }
}

fn apply_saved_vars(
    snap: &mut AppSnapshot,
    save_to_env_arg: Option<&serde_json::Map<String, JsonValue>>,
    parsed_json: Option<&JsonValue>,
    headers: &[(String, String)],
    target_env_name: Option<&str>,
) -> HashMap<String, String> {
    let mut saved_vars = HashMap::new();
    let map = match save_to_env_arg {
        Some(m) => m,
        None => return saved_vars,
    };

    for (var_name, path_val) in map {
        let val_opt = if let Some(path_str) = path_val.as_str() {
            if let Some(hdr_key) = path_str
                .strip_prefix("headers.")
                .or_else(|| path_str.strip_prefix("header."))
            {
                headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(hdr_key))
                    .map(|(_, v)| v.clone())
            } else if let Some(j) = parsed_json {
                extract_json_path(j, path_str)
            } else {
                None
            }
        } else {
            None
        };

        if let Some(val) = val_opt {
            saved_vars.insert(var_name.clone(), val);
        }
    }

    if saved_vars.is_empty() {
        return saved_vars;
    }

    let env_idx = if let Some(target) = target_env_name {
        snap.environments.iter().position(|e| {
            e.name.eq_ignore_ascii_case(target) || e.id.to_string() == target
        })
    } else if let Some(active_id) = snap.active_environment_id {
        snap.environments.iter().position(|e| e.id == active_id)
    } else if !snap.environments.is_empty() {
        Some(0)
    } else {
        None
    };

    if let Some(idx) = env_idx {
        let env = &mut snap.environments[idx];
        let env_id = env.id;
        let env_name = env.name.clone();

        for (k, v) in &saved_vars {
            if let Some(existing) = env.values.iter_mut().find(|ev| ev.key == *k) {
                existing.current_value = v.clone();
                if existing.initial_value.is_empty() {
                    existing.initial_value = v.clone();
                }
                existing.enabled = true;
            } else {
                env.values.push(EnvVar::new(k, v));
            }
        }

        if snap.active_environment_id == Some(env_id)
            || snap.active_environment.eq_ignore_ascii_case(&env_name)
        {
            for (k, v) in &saved_vars {
                snap.variables.environment.insert(k.clone(), v.clone());
            }
        }
    }

    saved_vars
}

fn extract_json_path(root: &JsonValue, path: &str) -> Option<String> {
    let clean = path.trim();
    if clean.is_empty() {
        return None;
    }

    if clean.starts_with('/') {
        if let Some(val) = root.pointer(clean) {
            return json_val_to_string(val);
        }
    }

    let normalized = clean.replace('[', ".").replace(']', "");
    let segments: Vec<&str> = normalized.split('.').filter(|s| !s.is_empty()).collect();
    let mut curr = root;
    for seg in segments {
        match curr {
            JsonValue::Object(map) => {
                curr = map.get(seg)?;
            }
            JsonValue::Array(arr) => {
                let idx = seg.parse::<usize>().ok()?;
                curr = arr.get(idx)?;
            }
            _ => return None,
        }
    }
    json_val_to_string(curr)
}

fn json_val_to_string(v: &JsonValue) -> Option<String> {
    match v {
        JsonValue::String(s) => Some(s.clone()),
        JsonValue::Number(n) => Some(n.to_string()),
        JsonValue::Bool(b) => Some(b.to_string()),
        JsonValue::Null => Some("null".to_string()),
        other => Some(other.to_string()),
    }
}

fn find_request_by_id_or_name<'a>(
    cols: &'a [Collection],
    query: &str,
    target_col: Option<&str>,
) -> Result<(&'a Collection, RequestDraft), String> {
    // 1. Try UUID parse
    if let Ok(uid) = Uuid::parse_str(query) {
        if let Some((col_id, req)) = find_request_across(cols, uid) {
            if let Some(c) = cols.iter().find(|c| c.id == col_id) {
                return Ok((c, req.clone()));
            }
        }
    }

    // 2. Search by name
    let query_lower = query.to_lowercase();
    for col in cols {
        if let Some(tc) = target_col {
            if !col.name.eq_ignore_ascii_case(tc) && col.id.to_string() != tc {
                continue;
            }
        }
        for item in flatten_collections(std::slice::from_ref(col)) {
            if item.name.to_lowercase() == query_lower {
                if let Some((_, req)) = find_request_across(cols, item.id) {
                    return Ok((col, req.clone()));
                }
            }
        }
    }

    Err(format!("Request '{query}' not found in collections"))
}

fn parse_http_method(s: &str) -> HttpMethod {
    match s.trim().to_uppercase().as_str() {
        "POST" => HttpMethod::Post,
        "PUT" => HttpMethod::Put,
        "PATCH" => HttpMethod::Patch,
        "DELETE" => HttpMethod::Delete,
        "HEAD" => HttpMethod::Head,
        "OPTIONS" => HttpMethod::Options,
        _ => HttpMethod::Get,
    }
}

fn find_env_by_name_or_id<'a>(envs: &'a [Environment], query: &str) -> Option<&'a Environment> {
    envs.iter()
        .find(|e| e.name.eq_ignore_ascii_case(query) || e.id.to_string() == query)
}

#[cfg(test)]
mod tests {
    use super::*;
    static MCP_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct TempMcpStore {
        dir: std::path::PathBuf,
        store: JsonStore,
    }

    impl TempMcpStore {
        fn new(prefix: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("blitz_{prefix}_{}", Uuid::new_v4()));
            let _ = std::fs::create_dir_all(&dir);
            let store = JsonStore::open(&dir).expect("temp store");
            Self { dir, store }
        }
    }

    impl Drop for TempMcpStore {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn test_initialize_handshake() {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {}
            }
        });
        let resp = handle_mcp_message(&req).expect("response expected");
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 1);
        assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(resp["result"]["serverInfo"]["name"], "blitz");
    }

    #[test]
    fn test_tools_list_returns_tools() {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        });
        let resp = handle_mcp_message(&req).expect("response expected");
        let tools = resp["result"]["tools"].as_array().expect("tools array");
        assert!(tools.iter().any(|t| t["name"] == "blitz_list_collections"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_get_request"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_send_request"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_execute_http"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_list_environments"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_set_env_var"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_set_active_environment"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_upsert_request"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_import_curl"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_generate_code"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_list_history"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_run_collection"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_suggest_assertions"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_perf_test"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_sync_openapi"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_get_collection"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_create_environment"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_delete_request"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_start_mock"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_stop_mock"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_export_collection"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_diff_responses"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_delete_collection"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_generate_docs"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_create_folder"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_delete_folder"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_delete_environment"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_clear_history"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_import_har"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_manage_cookies"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_websocket_send"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_websocket_connect"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_websocket_session_send"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_websocket_read"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_websocket_close"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_grpc_unary"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_sse_listen"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_mqtt_publish"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_mqtt_subscribe"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_graphql_ws_subscribe"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_socketio_emit"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_lint_collection"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_diff_environments"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_set_collection_var"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_import_collection"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_mcp_call"));
        assert_eq!(tools.len(), 46);
    }

    #[test]
    fn test_resources_list_returns_resources() {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "resources/list"
        });
        let resp = handle_mcp_message(&req).expect("response expected");
        let res = resp["result"]["resources"].as_array().expect("resources array");
        assert!(res.iter().any(|r| r["uri"] == "blitz://collections"));
        assert!(res.iter().any(|r| r["uri"] == "blitz://environments"));
        assert!(res.iter().any(|r| r["uri"] == "blitz://active-env"));
        assert!(res.iter().any(|r| r["uri"] == "blitz://history"));
    }

    #[test]
    fn test_tool_generate_code() {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "blitz_generate_code",
                "arguments": {
                    "language": "python",
                    "url": "https://api.example.com/v1/users",
                    "method": "POST",
                    "body": "{\"name\": \"Alice\"}"
                }
            }
        });
        let resp = handle_mcp_message(&req).expect("response expected");
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 4);
        let content = &resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(content.contains("httpx") || content.contains("requests") || content.contains("Alice"));
    }

    #[test]
    fn test_tool_import_curl() {
        let temp = TempMcpStore::new("mcp_import_curl");
        let content = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_import_curl",
            &json!({
                "curl": "curl -X POST https://api.test.com/login -H 'Content-Type: application/json' -d '{\"user\":\"test\"}'"
            }),
        )
        .expect("import_curl succeeds");
        assert!(content.contains("https://api.test.com/login"));
        assert!(content.contains("POST"));
    }

    #[test]
    fn test_extract_json_path() {
        let sample = json!({
            "status": "success",
            "data": {
                "token": "secret_jwt_xyz",
                "user": {
                    "id": 42,
                    "name": "Alice"
                },
                "roles": ["admin", "editor"]
            }
        });

        assert_eq!(
            extract_json_path(&sample, "data.token"),
            Some("secret_jwt_xyz".to_string())
        );
        assert_eq!(
            extract_json_path(&sample, "data.user.id"),
            Some("42".to_string())
        );
        assert_eq!(
            extract_json_path(&sample, "data.roles[0]"),
            Some("admin".to_string())
        );
        assert_eq!(
            extract_json_path(&sample, "/data/user/name"),
            Some("Alice".to_string())
        );
        assert_eq!(extract_json_path(&sample, "data.missing"), None);
    }

    #[test]
    fn test_folder_path_breadcrumbs() {
        let mut f_sub = Folder::new("Account");
        f_sub.add_request(RequestDraft {
            name: "Login".into(),
            method: HttpMethod::Post,
            url: "https://api.com/login".into(),
            ..Default::default()
        });

        let mut f_parent = Folder::new("User");
        f_parent.items.push(CollectionItem::Folder(f_sub));

        let items = vec![CollectionItem::Folder(f_parent)];
        let mut summaries = Vec::new();
        collect_request_summaries(&items, "", &mut summaries);

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0]["name"], "Login");
        assert_eq!(summaries[0]["path"], "User/Account/Login");
        assert_eq!(summaries[0]["folder"], "User/Account");
    }

    #[test]
    fn test_find_folder_by_path_or_name() {
        let mut f_sub = Folder::new("Account");
        f_sub.add_request(RequestDraft {
            name: "Login".into(),
            ..Default::default()
        });

        let mut f_parent = Folder::new("User");
        f_parent.items.push(CollectionItem::Folder(f_sub));

        let items = vec![CollectionItem::Folder(f_parent)];

        assert!(find_folder_by_path_or_name(&items, "User").is_some());
        assert!(find_folder_by_path_or_name(&items, "Account").is_some());
        assert!(find_folder_by_path_or_name(&items, "User/Account").is_some());
        assert!(find_folder_by_path_or_name(&items, "NonExistent").is_none());
    }

    #[test]
    fn test_serde_resilience_missing_fields() {
        // Missing username, token, extra, oauth2 in AuthConfig
        let auth_json = r#"{"auth_type": "bearer"}"#;
        let auth: Result<crate::core::AuthConfig, _> = serde_json::from_str(auth_json);
        assert!(auth.is_ok(), "AuthConfig should deserialize with missing fields");

        // Missing params, headers, body, auth in RequestDraft
        let draft_json = r#"{
            "id": "00000000-0000-0000-0000-000000000001",
            "name": "Minimal Request"
        }"#;
        let draft: Result<RequestDraft, _> = serde_json::from_str(draft_json);
        assert!(draft.is_ok(), "RequestDraft should deserialize with missing fields");
    }

    #[test]
    fn test_tool_suggest_assertions() {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "tools/call",
            "params": {
                "name": "blitz_suggest_assertions",
                "arguments": {
                    "status": 200,
                    "body": "{\"user\": {\"id\": 42, \"name\": \"Bob\"}}",
                    "headers": {
                        "content-type": "application/json"
                    },
                    "elapsed_ms": 120
                }
            }
        });
        let resp = handle_mcp_message(&req).expect("response expected");
        assert_eq!(resp["id"], 10);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let val: JsonValue = serde_json::from_str(text).unwrap();
        assert_eq!(val["status"], 200);
        assert!(val["assertion_count"].as_u64().unwrap() >= 3);
        assert!(val["test_script"].as_str().unwrap().contains("blitz.test"));
    }

    #[test]
    fn test_tool_perf_test_missing_target() {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "tools/call",
            "params": {
                "name": "blitz_perf_test",
                "arguments": {}
            }
        });
        let resp = handle_mcp_message(&req).expect("response expected");
        assert_eq!(resp["id"], 11);
        assert_eq!(resp["result"]["isError"], true);
    }

    #[test]
    fn test_tool_sync_openapi_creates_collection() {
        let temp = TempMcpStore::new("mcp_sync_openapi");
        let unique_title = format!("Petstore Minimal {}", Uuid::new_v4());
        let minimal_spec = json!({
            "openapi": "3.0.0",
            "info": {
                "title": unique_title,
                "version": "1.0.0"
            },
            "paths": {
                "/pets": {
                    "get": {
                        "summary": "List all pets",
                        "responses": {
                            "200": { "description": "OK" }
                        }
                    }
                }
            }
        });
        let text = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_sync_openapi",
            &json!({
                "spec": serde_json::to_string(&minimal_spec).unwrap()
            }),
        )
        .expect("sync_openapi succeeds");
        let val: JsonValue = serde_json::from_str(&text).unwrap();
        assert_eq!(val["status"], "ok");
        assert_eq!(val["created_new"], true);
        assert_eq!(val["added_routes"], 1);
    }

    #[test]
    fn test_tool_diff_responses_direct_bodies() {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 13,
            "method": "tools/call",
            "params": {
                "name": "blitz_diff_responses",
                "arguments": {
                    "body_a": "{\"status\": \"ok\", \"count\": 1}",
                    "body_b": "{\"status\": \"ok\", \"count\": 2}"
                }
            }
        });
        let resp = handle_mcp_message(&req).expect("response expected");
        assert_eq!(resp["id"], 13);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let val: JsonValue = serde_json::from_str(text).unwrap();
        assert_eq!(val["stats"]["identical"], false);
        assert!(val["stats"]["added_lines"].as_u64().unwrap() > 0);
    }

    #[test]
    fn test_tool_start_and_stop_mock() {
        let _guard = MCP_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let temp = TempMcpStore::new("mcp_mock");
        let start_text = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_start_mock",
            &json!({ "port": 0 }),
        )
        .expect("start_mock succeeds");
        let start_val: JsonValue = serde_json::from_str(&start_text).unwrap();
        assert_eq!(start_val["status"], "running");
        assert!(start_val["port"].as_u64().unwrap() > 0);

        let stop_text = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_stop_mock",
            &json!({}),
        )
        .expect("stop_mock succeeds");
        let stop_val: JsonValue = serde_json::from_str(&stop_text).unwrap();
        assert_eq!(stop_val["status"], "stopped");
    }

    #[test]
    fn test_tool_folder_lifecycle_and_docs() {
        let temp = TempMcpStore::new("mcp_folder_docs");
        let unique_col = format!("DocFolderCol_{}", Uuid::new_v4());

        // 1. Create collection with blitz_import_curl
        execute_mcp_tool_with_store(
            &temp.store,
            "blitz_import_curl",
            &json!({
                "curl": "curl -X GET https://api.example.com/items",
                "collection": unique_col
            }),
        )
        .expect("import_curl succeeds");

        // 2. Create folder
        let fold_text = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_create_folder",
            &json!({
                "collection": unique_col,
                "name": "Auth API"
            }),
        )
        .expect("create_folder succeeds");
        let fold_val: JsonValue = serde_json::from_str(&fold_text).unwrap();
        assert_eq!(fold_val["status"], "ok");

        // 3. Generate Docs (static)
        let docs_text = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_generate_docs",
            &json!({
                "collection": unique_col,
                "format": "static"
            }),
        )
        .expect("generate_docs succeeds");
        assert!(docs_text.contains(&unique_col) || docs_text.contains("<!DOCTYPE html>"));

        // 4. Delete folder
        let del_text = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_delete_folder",
            &json!({
                "collection": unique_col,
                "folder": "Auth API"
            }),
        )
        .expect("delete_folder succeeds");
        let del_val: JsonValue = serde_json::from_str(&del_text).unwrap();
        assert_eq!(del_val["status"], "deleted");

        // 5. Cleanup collection
        let _ = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_delete_collection",
            &json!({ "collection": unique_col }),
        );
    }

    #[test]
    fn test_tool_manage_cookies_and_clear_history() {
        let temp = TempMcpStore::new("mcp_cookies_history");
        let cookie_text = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_manage_cookies",
            &json!({ "action": "clear" }),
        )
        .expect("manage_cookies succeeds");
        let cookie_val: JsonValue = serde_json::from_str(&cookie_text).unwrap();
        assert_eq!(cookie_val["status"], "cleared");

        let hist_text = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_clear_history",
            &json!({}),
        )
        .expect("clear_history succeeds");
        let hist_val: JsonValue = serde_json::from_str(&hist_text).unwrap();
        assert_eq!(hist_val["status"], "cleared");
    }

    #[test]
    fn test_protocol_tools_validation_and_execution() {
        // Missing required 'url' on blitz_websocket_send
        let ws_req = json!({
            "jsonrpc": "2.0",
            "id": 301,
            "method": "tools/call",
            "params": {
                "name": "blitz_websocket_send",
                "arguments": {}
            }
        });
        let ws_res = handle_mcp_message(&ws_req).expect("response");
        assert_eq!(ws_res["result"]["isError"], true);

        // Missing required 'endpoint' on blitz_grpc_unary
        let grpc_req = json!({
            "jsonrpc": "2.0",
            "id": 302,
            "method": "tools/call",
            "params": {
                "name": "blitz_grpc_unary",
                "arguments": {
                    "method": "test.Service/Method",
                    "body": "{}"
                }
            }
        });
        let grpc_res = handle_mcp_message(&grpc_req).expect("response");
        assert_eq!(grpc_res["result"]["isError"], true);

        // Missing required 'url' on blitz_sse_listen
        let sse_req = json!({
            "jsonrpc": "2.0",
            "id": 303,
            "method": "tools/call",
            "params": {
                "name": "blitz_sse_listen",
                "arguments": {}
            }
        });
        let sse_res = handle_mcp_message(&sse_req).expect("response");
        assert_eq!(sse_res["result"]["isError"], true);

        // Missing required 'topic' on blitz_mqtt_publish
        let mqtt_req = json!({
            "jsonrpc": "2.0",
            "id": 304,
            "method": "tools/call",
            "params": {
                "name": "blitz_mqtt_publish",
                "arguments": {
                    "broker_url": "127.0.0.1:1883"
                }
            }
        });
        let mqtt_res = handle_mcp_message(&mqtt_req).expect("response");
        assert_eq!(mqtt_res["result"]["isError"], true);

        // gRPC unary against non-existent port (handled cleanly without panic)
        let grpc_call_req = json!({
            "jsonrpc": "2.0",
            "id": 305,
            "method": "tools/call",
            "params": {
                "name": "blitz_grpc_unary",
                "arguments": {
                    "endpoint": "http://127.0.0.1:59999",
                    "method": "test.Service/Method",
                    "body": "{\"user_id\": 1}"
                }
            }
        });
        let grpc_call_res = handle_mcp_message(&grpc_call_req).expect("response");
        let grpc_text = grpc_call_res["result"]["content"][0]["text"].as_str().unwrap();
        let grpc_val: JsonValue = serde_json::from_str(grpc_text).unwrap();
        assert_eq!(grpc_val["status"], "ERROR");
    }

    #[test]
    fn test_unknown_method_error() {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "non_existent_method"
        });
        let resp = handle_mcp_message(&req).expect("response expected");
        assert_eq!(resp["error"]["code"], -32601);
    }

    #[test]
    fn test_tool_upsert_request_sets_body_mode_and_preserves_body_on_partial_update() {
        let dir = std::env::temp_dir().join(format!("blitz_mcp_upsert_{}", Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&dir);
        let store = JsonStore::open(&dir).expect("temp store");

        let mut col = Collection::new("Driver");
        let existing_id = Uuid::new_v4();
        let existing_req = RequestDraft {
            id: existing_id,
            name: "Create Login".into(),
            method: HttpMethod::Post,
            url: "https://api.example.com/login".into(),
            body_mode: BodyMode::Raw,
            body: "{\"email\":\"a@b.com\"}".into(),
            pre_request_script: "// keep me".into(),
            ..Default::default()
        };
        let mut f = Folder::new("Auth");
        f.add_request(existing_req);
        col.items.push(CollectionItem::Folder(f));
        let snap = AppSnapshot {
            collections: vec![col],
            ..Default::default()
        };
        store.save(&snap).expect("seed save");

        // Partial update by request_id omitting body must preserve body, body_mode, and scripts.
        let update_res = execute_mcp_tool_with_store(
            &store,
            "blitz_upsert_request",
            &json!({
                "collection": "Driver",
                "request_id": existing_id.to_string(),
                "name": "Create Login",
                "method": "POST",
                "url": "https://api.example.com/v2/login"
            }),
        )
        .expect("partial upsert succeeds");
        let parsed_upd: JsonValue = serde_json::from_str(&update_res).unwrap();
        assert_eq!(parsed_upd["updated"], true);
        assert_eq!(parsed_upd["body_mode"], "raw");

        // Creating a new POST request with body must automatically set body_mode = Raw.
        let create_res = execute_mcp_tool_with_store(
            &store,
            "blitz_upsert_request",
            &json!({
                "collection": "Driver",
                "folder": "Auth",
                "name": "Driver Smoke",
                "method": "POST",
                "url": "https://api.example.com/smoke",
                "body": "{\"ping\":true}"
            }),
        )
        .expect("create upsert succeeds");
        let parsed_new: JsonValue = serde_json::from_str(&create_res).unwrap();
        assert_eq!(parsed_new["updated"], false);
        assert_eq!(parsed_new["body_mode"], "raw");

        let reloaded = store.load().expect("reload");
        let (_, upd) = find_request_across(&reloaded.collections, existing_id).unwrap();
        assert_eq!(upd.url, "https://api.example.com/v2/login");
        assert_eq!(upd.body_mode, BodyMode::Raw);
        assert_eq!(upd.body, "{\"email\":\"a@b.com\"}");
        assert_eq!(upd.pre_request_script, "// keep me");

        let new_id = Uuid::parse_str(parsed_new["request_id"].as_str().unwrap()).unwrap();
        let (_, created) = find_request_across(&reloaded.collections, new_id).unwrap();
        assert_eq!(created.body_mode, BodyMode::Raw);
        assert_eq!(created.body, "{\"ping\":true}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_new_mcp_tools_and_bugfixes() {
        let temp = TempMcpStore::new("mcp_new_tools");

        // 1. blitz_create_environment with set_active populates snapshot.variables.environment
        let env_a_raw = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_create_environment",
            &json!({
                "name": "StageA",
                "variables": { "baseUrl": "http://127.0.0.1:18080", "shared": "same", "diff": "a" },
                "set_active": true
            }),
        )
        .expect("create StageA");
        let env_a: JsonValue = serde_json::from_str(&env_a_raw).unwrap();
        assert_eq!(env_a["is_active"], true);
        let reloaded = temp.store.load().unwrap();
        assert_eq!(
            reloaded.variables.environment.get("baseUrl").map(String::as_str),
            Some("http://127.0.0.1:18080")
        );

        execute_mcp_tool_with_store(
            &temp.store,
            "blitz_create_environment",
            &json!({
                "name": "StageB",
                "variables": { "shared": "same", "diff": "b", "onlyB": "extra" }
            }),
        )
        .expect("create StageB");

        // 2. blitz_diff_environments
        let diff_raw = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_diff_environments",
            &json!({ "environment_a": "StageA", "environment_b": "StageB" }),
        )
        .expect("diff envs");
        let diff: JsonValue = serde_json::from_str(&diff_raw).unwrap();
        assert_eq!(diff["matching"], 1);
        assert_eq!(diff["drift"], 1);
        assert_eq!(diff["missing_in_target"], 1);
        assert_eq!(diff["extra_in_target"], 1);

        // 3. blitz_upsert_request w/ protocol, auth, test_script, graphql_query + blitz_get_request
        let up_raw = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_upsert_request",
            &json!({
                "collection": "TestCol",
                "name": "GqlOp",
                "url": "{{baseUrl}}/graphql",
                "graphql_query": "{ hello }",
                "auth": { "type": "bearer", "token": "tok123" },
                "test_script": "blitz.test('ok', () => blitz.expect(200).to.equal(200));"
            }),
        )
        .expect("upsert graphql");
        let up: JsonValue = serde_json::from_str(&up_raw).unwrap();
        assert_eq!(up["method"], "POST");
        assert_eq!(up["body_mode"], "graphql");

        let get_raw = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_get_request",
            &json!({ "request_id": up["request_id"], "collection": "TestCol" }),
        )
        .expect("get request");
        let got: JsonValue = serde_json::from_str(&get_raw).unwrap();
        assert_eq!(got["request"]["auth_type"], "bearer");
        assert_eq!(got["request"]["graphql_query"], "{ hello }");

        // 4. blitz_set_collection_var + blitz_lint_collection
        execute_mcp_tool_with_store(
            &temp.store,
            "blitz_set_collection_var",
            &json!({ "collection": "TestCol", "key": "colKey", "value": "colVal" }),
        )
        .expect("set col var");
        execute_mcp_tool_with_store(
            &temp.store,
            "blitz_upsert_request",
            &json!({ "collection": "TestCol", "name": "UnresolvedReq", "url": "{{missingVar}}/x" }),
        )
        .expect("upsert unresolved");
        let lint_raw = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_lint_collection",
            &json!({ "collection": "TestCol", "environment": "StageA" }),
        )
        .expect("lint collection");
        let lint: JsonValue = serde_json::from_str(&lint_raw).unwrap();
        assert!(lint["total_issues"].as_u64().unwrap() >= 1);

        // 5. blitz_manage_cookies (set + domain-scoped clear)
        execute_mcp_tool_with_store(
            &temp.store,
            "blitz_manage_cookies",
            &json!({ "action": "set", "domain": "a.test", "name": "sid", "value": "111" }),
        )
        .expect("set cookie a");
        execute_mcp_tool_with_store(
            &temp.store,
            "blitz_manage_cookies",
            &json!({ "action": "set", "domain": "b.test", "name": "sid", "value": "222" }),
        )
        .expect("set cookie b");
        let clr_raw = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_manage_cookies",
            &json!({ "action": "clear", "domain": "a.test" }),
        )
        .expect("clear cookie a only");
        let clr: JsonValue = serde_json::from_str(&clr_raw).unwrap();
        assert_eq!(clr["cleared_count"], 1);
        assert_eq!(clr["remaining_count"], 1);

        // 6. blitz_import_collection (Postman v2.1)
        let pm = json!({
            "info": { "name": "ImportedPM", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
            "item": [{ "name": "Ping", "request": { "method": "GET", "url": { "raw": "http://127.0.0.1:18080/health" } } }]
        });
        let imp_raw = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_import_collection",
            &json!({ "data": pm.to_string() }),
        )
        .expect("import postman v2.1");
        let imp: JsonValue = serde_json::from_str(&imp_raw).unwrap();
        assert_eq!(imp["format"], "postman_v2.1");
        assert_eq!(imp["request_count"], 1);
    }

    #[test]
    #[ignore = "requires docker test-servers on localhost:18080..18085"]
    fn live_all_protocols_and_42_mcp_tools() {
        let _guard = MCP_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let temp = TempMcpStore::new("live_42_tools");
        let col_name = "__blitz_mcp_test_suite__";

        // Environments + vars + diff
        execute_mcp_tool_with_store(
            &temp.store,
            "blitz_create_environment",
            &json!({
                "name": "__test_env_a__",
                "variables": { "baseUrl": "http://127.0.0.1:18080", "envTag": "alpha" },
                "set_active": true
            }),
        )
        .unwrap();
        execute_mcp_tool_with_store(
            &temp.store,
            "blitz_create_environment",
            &json!({
                "name": "__test_env_b__",
                "variables": { "baseUrl": "http://127.0.0.1:18080", "envTag": "beta" }
            }),
        )
        .unwrap();
        execute_mcp_tool_with_store(
            &temp.store,
            "blitz_set_env_var",
            &json!({ "key": "dynamicCheck", "value": "ok", "environment": "__test_env_a__" }),
        )
        .unwrap();
        let diff_env: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_diff_environments",
                &json!({ "environment_a": "__test_env_a__", "environment_b": "__test_env_b__" }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(diff_env["drift"], 1);

        // Login -> stores Set-Cookie in jar & saves token to env
        let login: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_execute_http",
                &json!({
                    "method": "POST",
                    "url": "{{baseUrl}}/login",
                    "body": "{\"username\":\"LiveTester\"}",
                    "save_to_env": { "authToken": "token" }
                }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(login["status"], 200);
        assert_eq!(login["saved_to_env"]["authToken"], "bearer_xyz_123");

        // Protected endpoint uses persisted cookie
        let prot: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_execute_http",
                &json!({ "method": "GET", "url": "{{baseUrl}}/protected" }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(prot["status"], 200);
        assert_eq!(prot["json"]["authorized"], true);

        // GraphQL over HTTP
        let gql_http: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_execute_http",
                &json!({
                    "url": "{{baseUrl}}/graphql",
                    "graphql_query": "query($id: ID) { hello user(id: $id) { id name } }",
                    "graphql_variables": "{\"id\":\"42\",\"name\":\"Blitz\"}"
                }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(gql_http["status"], 200);
        assert_eq!(gql_http["json"]["data"]["user"]["name"], "TestUser");

        // Upsert saved HTTP request with test script, send, diff across envs, perf test, run collection
        let up: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_upsert_request",
                &json!({
                    "collection": col_name,
                    "folder": "HTTP",
                    "name": "EchoEnv",
                    "method": "GET",
                    "url": "{{baseUrl}}/echo?env={{envTag}}",
                    "test_script": "blitz.test('status 200', () => blitz.expect(blitz.response.code).to.equal(200));"
                }),
            )
            .unwrap(),
        )
        .unwrap();
        let req_id = up["request_id"].as_str().unwrap().to_string();

        let send_saved: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_send_request",
                &json!({ "request_id": &req_id, "collection": col_name }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(send_saved["status"], 200);
        assert_eq!(send_saved["tests"][0]["passed"], true);

        let diff_resp: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_diff_responses",
                &json!({
                    "request_id": &req_id,
                    "environment_a": "__test_env_a__",
                    "environment_b": "__test_env_b__"
                }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(diff_resp["status_comparison"]["status_match"], true);
        assert_eq!(diff_resp["stats"]["identical"], false);

        let run_col: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_run_collection",
                &json!({ "collection": col_name, "folder": "HTTP", "iterations": 2 }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(run_col["total_requests"], 2);
        assert_eq!(run_col["passed"], 2);

        let perf: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_perf_test",
                &json!({ "request_id": &req_id, "vus": 3, "duration_seconds": 1 }),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(perf["total_requests"].as_u64().unwrap() > 0);
        assert_eq!(perf["errors"], 0);

        // Local mock server start -> hit -> stop
        let mock: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_start_mock",
                &json!({ "collection": col_name }),
            )
            .unwrap(),
        )
        .unwrap();
        let mock_url = mock["mock_url"].as_str().unwrap();
        let mock_hit: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_execute_http",
                &json!({ "url": format!("{mock_url}/echo") }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(mock_hit["status"], 200);
        execute_mcp_tool_with_store(&temp.store, "blitz_stop_mock", &json!({})).unwrap();

        // SSE
        let sse: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_sse_listen",
                &json!({ "url": "http://127.0.0.1:18080/sse", "max_events": 3, "timeout_ms": 3000 }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(sse["count"], 3);

        // WebSocket
        let ws: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_websocket_send",
                &json!({
                    "url": "ws://127.0.0.1:18082/ws",
                    "message": "ping-from-blitz",
                    "max_messages": 2,
                    "timeout_ms": 3000
                }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(ws["count"], 2);
        assert!(ws["messages"][1].as_str().unwrap().contains("ping-from-blitz"));

        // GraphQL-WS
        let gql_ws: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_graphql_ws_subscribe",
                &json!({
                    "ws_url": "ws://127.0.0.1:18082/graphql",
                    "query": "subscription { tick }",
                    "max_events": 3,
                    "timeout_ms": 3000
                }),
            )
            .unwrap(),
        )
        .unwrap();
        let gql_events = gql_ws["events"].as_array().unwrap();
        assert!(gql_events.iter().any(|e| e.as_str().unwrap().contains("next")));
        assert!(!gql_events.iter().any(|e| e.as_str().unwrap().contains("4400")));

        // gRPC OK + gRPC Error trailers
        let grpc_ok: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_grpc_unary",
                &json!({
                    "endpoint": "http://127.0.0.1:18083",
                    "method": "test.EchoService/Echo",
                    "body": "{\"msg\":\"hello-grpc\",\"n\":7}",
                    "headers": { "authorization": "Bearer grpc-tok" }
                }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(grpc_ok["grpc_status"], 0, "grpc_ok: {grpc_ok}");
        assert_eq!(grpc_ok["response"]["ok"], true);
        assert_eq!(grpc_ok["response"]["echo"]["msg"], "hello-grpc");
        assert_eq!(grpc_ok["response"]["metadata_auth"], "Bearer grpc-tok");

        let grpc_err: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_grpc_unary",
                &json!({
                    "endpoint": "http://127.0.0.1:18083",
                    "method": "test.EchoService/Fail",
                    "body": "{}"
                }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(grpc_err["grpc_status"], 3, "grpc_err: {grpc_err}");
        assert_eq!(grpc_err["grpc_message"], "invalid_argument_test");

        // Socket.IO
        let sio: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_socketio_emit",
                &json!({
                    "url": "http://127.0.0.1:18084",
                    "emit_event": "chat",
                    "emit_payload": "{\"text\":\"hello-sio\"}",
                    "listen_event": "chat",
                    "listen_ms": 1200
                }),
            )
            .unwrap(),
        )
        .unwrap();
        let sio_evs = sio["events"].as_array().unwrap();
        assert!(
            sio_evs.iter().any(|e| e.as_str().unwrap().contains("hello-sio")),
            "sio_evs: {sio_evs:?}"
        );

        // MQTT publish (retain=true) + subscribe
        let mq_pub: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_mqtt_publish",
                &json!({
                    "broker_url": "mqtt://127.0.0.1:18085",
                    "topic": "blitz/test/topic",
                    "payload": "{\"temp\":22.5}",
                    "retain": true
                }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(mq_pub["ok"], true, "mq_pub: {mq_pub}");

        let mq_sub: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_mqtt_subscribe",
                &json!({
                    "broker_url": "mqtt://127.0.0.1:18085",
                    "topic": "blitz/test/topic",
                    "timeout_seconds": 3
                }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(mq_sub["ok"], true, "mq_sub: {mq_sub}");
        assert!(mq_sub["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m.as_str().unwrap().contains("temp")));

        // External STDIO MCP server via blitz_mcp_call
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let mcp_script = format!("{manifest_dir}/test-servers/mcp_stdio_server.js");
        let mcp_list: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_mcp_call",
                &json!({ "command": "node", "args": [&mcp_script] }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(mcp_list["tool_count"], 2);

        let mcp_call: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_mcp_call",
                &json!({
                    "command": "node",
                    "args": [&mcp_script],
                    "tool": "add_numbers",
                    "arguments": { "a": 19, "b": 23 }
                }),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(mcp_call["result"].to_string().contains("42"), "mcp_call: {mcp_call}");
    }

    #[test]
    fn test_execute_http_multipart_form_data_and_missing_file_error() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let temp = TempMcpStore::new("mcp_multipart");
        let upload_path = temp.dir.join("xss_probe.html");
        std::fs::write(&upload_path, b"<script>alert(1)</script>").unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&chunk[..n]);
                        if buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() > 200 {
                            // Give reqwest a tiny moment if body arrived in a second packet
                            if String::from_utf8_lossy(&buf).contains("--\r\n") {
                                break;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            let raw = String::from_utf8_lossy(&buf).into_owned();
            let resp_body = r#"{"uploaded":true}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                resp_body.len(),
                resp_body
            );
            let _ = stream.write_all(resp.as_bytes());
            raw
        });

        let out_str = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_execute_http",
            &json!({
                "method": "POST",
                "url": format!("http://{addr}/upload"),
                "override_vars": { "caption_val": "avatar-probe" },
                "form_data": [
                    { "key": "caption", "value": "{{caption_val}}" },
                    {
                        "key": "file",
                        "file_path": upload_path.to_string_lossy(),
                        "content_type": "text/html"
                    }
                ]
            }),
        )
        .expect("multipart upload should succeed");

        let out: JsonValue = serde_json::from_str(&out_str).unwrap();
        assert_eq!(out["status"], 200);
        assert_eq!(out["json"]["uploaded"], true);

        let raw_req = server_handle.join().unwrap();
        assert!(raw_req.to_ascii_lowercase().contains("multipart/form-data"));
        assert!(raw_req.contains("avatar-probe"), "raw_req: {raw_req}");
        assert!(raw_req.contains("xss_probe.html"), "raw_req: {raw_req}");
        assert!(raw_req.contains("text/html"), "raw_req: {raw_req}");
        assert!(raw_req.contains("<script>alert(1)</script>"), "raw_req: {raw_req}");

        let missing_err = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_execute_http",
            &json!({
                "method": "POST",
                "url": format!("http://{addr}/upload"),
                "form_data": [
                    { "key": "file", "file_path": "/nonexistent/blitz_missing_file.png" }
                ]
            }),
        )
        .unwrap_err();
        assert!(missing_err.contains("Multipart file not found"), "err: {missing_err}");
    }

    #[test]
    fn test_websocket_multi_message_var_interpolation_and_concurrent_sessions() {
        use futures_util::{SinkExt, StreamExt};
        use tokio::sync::broadcast;
        use tokio_tungstenite::tungstenite::Message as WsMsg;

        let temp = TempMcpStore::new("mcp_ws_multi");
        let mut snap = temp.store.load().unwrap();
        let mut env = crate::core::Environment::new("TestEnv");
        env.values.push(crate::core::EnvVar::new("ws_token", "jwt-abc-123"));
        snap.active_environment_id = Some(env.id);
        snap.active_environment = env.name.clone();
        snap.environments.push(env);
        temp.store.save(&snap).unwrap();

        let (port_tx, port_rx) = std::sync::mpsc::channel::<u16>();
        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
        let server_thread = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let port = listener.local_addr().unwrap().port();
                let _ = port_tx.send(port);
                let (btx, _) = broadcast::channel::<String>(64);

                loop {
                    tokio::select! {
                        _ = &mut stop_rx => break,
                        accept_res = listener.accept() => {
                            let Ok((stream, _)) = accept_res else { break; };
                            let btx = btx.clone();
                            let mut brx = btx.subscribe();
                            tokio::spawn(async move {
                                let mut auth_hdr = String::new();
                                let cb = |req: &tokio_tungstenite::tungstenite::handshake::server::Request,
                                          resp: tokio_tungstenite::tungstenite::handshake::server::Response| {
                                    if let Some(v) = req.headers().get("authorization") {
                                        auth_hdr = v.to_str().unwrap_or("").to_string();
                                    }
                                    Ok(resp)
                                };
                                let Ok(ws) = tokio_tungstenite::accept_hdr_async(stream, cb).await else { return; };
                                let (mut write, mut read) = ws.split();
                                let _ = write.send(WsMsg::Text(format!("welcome:{auth_hdr}").into())).await;
                                loop {
                                    tokio::select! {
                                        msg = read.next() => {
                                            match msg {
                                                Some(Ok(WsMsg::Text(t))) => {
                                                    let _ = btx.send(t.to_string());
                                                }
                                                Some(Ok(WsMsg::Close(_))) | None => break,
                                                _ => {}
                                            }
                                        }
                                        bcast = brx.recv() => {
                                            if let Ok(txt) = bcast {
                                                if write.send(WsMsg::Text(txt.into())).await.is_err() {
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            });
                        }
                    }
                }
            });
        });

        let port = port_rx.recv_timeout(Duration::from_secs(3)).unwrap();

        // 1. One-shot blitz_websocket_send with {{var}} interpolation + multi-step messages
        let oneshot_str = execute_mcp_tool_with_store(
            &temp.store,
            "blitz_websocket_send",
            &json!({
                "url": "ws://127.0.0.1:{{ws_port}}/ws",
                "override_vars": { "ws_port": port.to_string() },
                "headers": { "Authorization": "Bearer {{ws_token}}" },
                "messages": [
                    "step1:join",
                    { "message": "step2:{{ws_token}}", "delay_ms": 10 }
                ],
                "timeout_ms": 1000,
                "max_messages": 10
            }),
        )
        .unwrap();
        let oneshot: JsonValue = serde_json::from_str(&oneshot_str).unwrap();
        let oneshot_msgs = oneshot["messages"].as_array().unwrap();
        assert!(
            oneshot_msgs.iter().any(|m| m.as_str() == Some("welcome:Bearer jwt-abc-123")),
            "oneshot_msgs: {oneshot_msgs:?}"
        );
        assert!(
            oneshot_msgs.iter().any(|m| m.as_str() == Some("step1:join")),
            "oneshot_msgs: {oneshot_msgs:?}"
        );
        assert!(
            oneshot_msgs.iter().any(|m| m.as_str() == Some("step2:jwt-abc-123")),
            "oneshot_msgs: {oneshot_msgs:?}"
        );

        // 2. Concurrent persistent sessions: user_a sends -> user_b reads
        let conn_a: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_websocket_connect",
                &json!({
                    "session_id": "user_a",
                    "url": "ws://127.0.0.1:{{ws_port}}/ws",
                    "override_vars": { "ws_port": port.to_string() },
                    "headers": { "Authorization": "Bearer {{ws_token}}" }
                }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(conn_a["session_id"], "user_a");

        let conn_b: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_websocket_connect",
                &json!({
                    "session_id": "user_b",
                    "url": format!("ws://127.0.0.1:{port}/ws"),
                    "headers": { "Authorization": "Bearer user-b-jwt" }
                }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(conn_b["session_id"], "user_b");

        let send_a: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_websocket_session_send",
                &json!({
                    "session_id": "user_a",
                    "messages": ["dm:hello-user-b", "dm:second-msg"],
                    "wait_ms": 200
                }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(send_a["sent_count"], 2);

        let read_b: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_websocket_read",
                &json!({
                    "session_id": "user_b",
                    "timeout_ms": 1000,
                    "max_messages": 10
                }),
            )
            .unwrap(),
        )
        .unwrap();
        let b_msgs = read_b["messages"].as_array().unwrap();
        assert!(
            b_msgs.iter().any(|m| m.as_str() == Some("dm:hello-user-b"))
                && b_msgs.iter().any(|m| m.as_str() == Some("dm:second-msg")),
            "b_msgs: {b_msgs:?}"
        );

        let close_res: JsonValue = serde_json::from_str(
            &execute_mcp_tool_with_store(
                &temp.store,
                "blitz_websocket_close",
                &json!({ "all": true }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(close_res["count"], 2);

        let _ = stop_tx.send(());
        let _ = server_thread.join();
    }
}
