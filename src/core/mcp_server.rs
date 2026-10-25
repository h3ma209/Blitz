//! Built-in Model Context Protocol (MCP) Server for Blitz.
//!
//! Enables AI agents (Antigravity, Gemini, Claude Desktop, Cursor, Windsurf, etc.)
//! to interact directly with Blitz collections, environments, and execution runtime
//! over standard I/O using JSON-RPC 2.0.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};

use serde_json::{json, Value as JsonValue};
use uuid::Uuid;

use crate::core::assertions::{assertions_to_script, suggest_from_response};
use crate::core::codegen::{generate_snippet_resolved, snippet_languages};
use crate::core::collection::{
    find_request_across, flatten_collections, remove_across, Collection, CollectionItem, Folder,
};
use crate::core::environment::{EnvVar, Environment};
use crate::core::history::{push_history, HistoryEntry, HISTORY_MAX};
use crate::core::impex::{import_curl, import_openapi3, reconcile_openapi3_collection};
use crate::core::perf_runner::run_performance;
use crate::core::request::{HttpMethod, KeyValue, RequestDraft};
use crate::runtime::pipeline::{send_request_with_options, SendOptions};
use crate::runtime::runner::{run_collection_with_config, RunnerConfig};
use crate::storage::{AppSnapshot, JsonStore};

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

                // Handle optional Content-Length header prefix if client sends HTTP-like MCP frames
                let json_str = if trimmed.starts_with('{') {
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
                    "environment": {
                        "type": "string",
                        "description": "Optional environment name to resolve {{variables}} against."
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
                        "description": "Optional raw request body."
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
        })
    ]
}

/// Execute a specific Blitz tool call.
fn execute_mcp_tool(name: &str, args: &JsonValue) -> Result<String, String> {
    let store = JsonStore::open_default().map_err(|e| format!("Open store: {e}"))?;
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
                    "method": found_req.method.as_str(),
                    "url": found_req.url,
                    "headers": found_req.headers.iter().filter(|h| h.enabled && !h.key.is_empty()).map(|h| (&h.key, &h.value)).collect::<HashMap<_, _>>(),
                    "params": found_req.params.iter().filter(|p| p.enabled && !p.key.is_empty()).map(|p| (&p.key, &p.value)).collect::<HashMap<_, _>>(),
                    "body_mode": format!("{:?}", found_req.body_mode),
                    "body": found_req.body,
                    "description": found_req.description,
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

            let (col, req) = find_request_by_id_or_name(&snapshot.collections, req_id_str, None)?;

            let mut ctx = snapshot.variables.clone();
            ctx.collection = col.variables_map();

            // Apply environment if specified
            if let Some(env_name) = args.get("environment").and_then(|e| e.as_str()) {
                if let Some(env) = find_env_by_name_or_id(&snapshot.environments, env_name) {
                    ctx.environment = env.to_map();
                } else {
                    return Err(format!("Environment '{env_name}' not found"));
                }
            }

            // Apply override vars
            if let Some(overrides) = args.get("override_vars").and_then(|o| o.as_object()) {
                for (k, v) in overrides {
                    if let Some(val_str) = v.as_str() {
                        ctx.collection.insert(k.clone(), val_str.to_string());
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

            // Record to history & response cache so Blitz desktop UI reflects this run
            let mut snap_for_save = snapshot;
            push_history(
                &mut snap_for_save.history,
                HistoryEntry::from_send(
                    &req,
                    &view.resolved_url,
                    Some(view.status),
                    Some(view.elapsed_ms),
                ),
                HISTORY_MAX,
            );
            snap_for_save.responses.insert(req.id, view.clone());

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

            let _ = store.save(&snap_for_save);

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

            let method_str = args
                .get("method")
                .and_then(|m| m.as_str())
                .unwrap_or("GET");
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

            let draft = RequestDraft {
                name: "Ad-hoc Request".into(),
                method,
                url: url.into(),
                headers: headers_vec,
                body: body_str,
                ..Default::default()
            };

            let mut ctx = snapshot.variables.clone();
            if let Some(env_name) = args.get("environment").and_then(|e| e.as_str()) {
                if let Some(env) = find_env_by_name_or_id(&snapshot.environments, env_name) {
                    ctx.environment = env.to_map();
                } else {
                    return Err(format!("Environment '{env_name}' not found"));
                }
            }

            let mut jar = snapshot.cookies.clone();
            let send_opts = SendOptions {
                cookie_jar: Some(&mut jar),
                ..Default::default()
            };

            let view = send_request_with_options(&draft, &mut ctx, send_opts)
                .map_err(|e| format!("Execution error: {e}"))?;

            let parsed_json: Option<JsonValue> = serde_json::from_str(&view.body).ok();

            // Record to history
            let mut snap_for_save = snapshot;
            push_history(
                &mut snap_for_save.history,
                HistoryEntry::from_send(
                    &draft,
                    &view.resolved_url,
                    Some(view.status),
                    Some(view.elapsed_ms),
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

            let _ = store.save(&snap_for_save);

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
            let method_str = args
                .get("method")
                .and_then(|m| m.as_str())
                .unwrap_or("GET");
            let method = parse_http_method(method_str);

            let mut headers = Vec::new();
            if let Some(h_obj) = args.get("headers").and_then(|h| h.as_object()) {
                for (k, v) in h_obj {
                    if let Some(val) = v.as_str() {
                        headers.push(KeyValue::new(k, val));
                    }
                }
            }

            let mut params = Vec::new();
            if let Some(p_obj) = args.get("params").and_then(|p| p.as_object()) {
                for (k, v) in p_obj {
                    if let Some(val) = v.as_str() {
                        params.push(KeyValue::new(k, val));
                    }
                }
            }

            let body = args
                .get("body")
                .and_then(|b| b.as_str())
                .unwrap_or("")
                .to_string();
            let description = args
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let folder_name = args.get("folder").and_then(|f| f.as_str());
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

            let col = &mut snapshot.collections[col_idx];

            let draft = RequestDraft {
                id: req_id_opt.unwrap_or_else(Uuid::new_v4),
                name: req_name.to_string(),
                method,
                url: url.to_string(),
                headers,
                params,
                body,
                description,
                ..Default::default()
            };

            let final_id = draft.id;

            if let Some(f_name) = folder_name {
                let folder_item = col.items.iter_mut().find_map(|item| match item {
                    CollectionItem::Folder(f) if f.name.eq_ignore_ascii_case(f_name) => Some(f),
                    _ => None,
                });

                if let Some(folder) = folder_item {
                    if let Some(existing) = folder.items.iter_mut().find_map(|item| match item {
                        CollectionItem::Request(r)
                            if r.id == final_id || r.name.eq_ignore_ascii_case(req_name) =>
                        {
                            Some(r)
                        }
                        _ => None,
                    }) {
                        *existing = draft;
                    } else {
                        folder.add_request(draft);
                    }
                } else {
                    let mut new_folder = Folder::new(f_name);
                    new_folder.add_request(draft);
                    col.items.push(CollectionItem::Folder(new_folder));
                }
            } else if let Some(existing) = col.items.iter_mut().find_map(|item| match item {
                CollectionItem::Request(r)
                    if r.id == final_id || r.name.eq_ignore_ascii_case(req_name) =>
                {
                    Some(r)
                }
                _ => None,
            }) {
                *existing = draft;
            } else {
                col.add_request(draft);
            }

            let col_id_str = col.id.to_string();
            let col_name_str = col.name.clone();

            store
                .save(&snapshot)
                .map_err(|e| format!("Save snapshot: {e}"))?;

            let res = json!({
                "status": "ok",
                "collection_id": col_id_str,
                "collection_name": col_name_str,
                "request_id": final_id.to_string(),
                "name": req_name,
                "method": method_str,
                "url": url
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
                RequestDraft {
                    name: "Snippet Request".into(),
                    method,
                    url: url.into(),
                    headers,
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

            if let Some(env_name) = args.get("environment").and_then(|e| e.as_str()) {
                if let Some(env) = find_env_by_name_or_id(&snapshot.environments, env_name) {
                    ctx.environment = env.to_map();
                }
            }

            let cfg = RunnerConfig {
                iterations: 1,
                delay_ms: 0,
                data_file: String::new(),
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
            let var_count = new_env.values.len();
            snapshot.environments.push(new_env);

            if set_active {
                snapshot.active_environment_id = Some(env_id);
                snapshot.active_environment = env_name_str.clone();
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
        _ => Err(format!("Unknown tool: {name}")),
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
        assert_eq!(tools.len(), 18);
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
        let req = json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "blitz_import_curl",
                "arguments": {
                    "curl": "curl -X POST https://api.test.com/login -H 'Content-Type: application/json' -d '{\"user\":\"test\"}'"
                }
            }
        });
        let resp = handle_mcp_message(&req).expect("response expected");
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 5);
        let content = resp["result"]["content"][0]["text"].as_str().unwrap();
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
        let minimal_spec = json!({
            "openapi": "3.0.0",
            "info": {
                "title": "Petstore Minimal",
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
        let req = json!({
            "jsonrpc": "2.0",
            "id": 12,
            "method": "tools/call",
            "params": {
                "name": "blitz_sync_openapi",
                "arguments": {
                    "spec": serde_json::to_string(&minimal_spec).unwrap()
                }
            }
        });
        let resp = handle_mcp_message(&req).expect("response expected");
        assert_eq!(resp["id"], 12);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let val: JsonValue = serde_json::from_str(text).unwrap();
        assert_eq!(val["status"], "ok");
        assert_eq!(val["created_new"], true);
        assert_eq!(val["added_routes"], 1);
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
}
