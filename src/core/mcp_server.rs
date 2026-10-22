//! Built-in Model Context Protocol (MCP) Server for Blitz.
//!
//! Enables AI agents (Antigravity, Gemini, Claude Desktop, Cursor, Windsurf, etc.)
//! to interact directly with Blitz collections, environments, and execution runtime
//! over standard I/O using JSON-RPC 2.0.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::{json, Value as JsonValue};
use uuid::Uuid;

use crate::core::collection::{find_request_across, flatten_collections, Collection, CollectionItem};
use crate::core::environment::Environment;
use crate::core::request::{HttpMethod, KeyValue, RequestDraft};
use crate::core::variables::VariableContext;
use crate::runtime::pipeline::{send_request_with_options, SendOptions};
use crate::runtime::runner::{run_collection_with_config, RunnerConfig};
use crate::storage::JsonStore;

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
            "description": "Executes a saved Blitz request by ID or name using the Blitz pipeline (resolving variables, running pre-request and test scripts, managing cookies), and returns status, headers, body, and test results.",
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
                    }
                },
                "required": ["url"]
            }
        }),
        json!({
            "name": "blitz_list_environments",
            "description": "Lists all configured environments in Blitz (e.g. Local, Development, Production) and their non-secret variable keys.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "blitz_run_collection",
            "description": "Runs all requests in a collection sequentially, executing assertions and returning a complete test pass/fail report.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "collection": {
                        "type": "string",
                        "description": "UUID or name of the collection to run."
                    },
                    "environment": {
                        "type": "string",
                        "description": "Optional environment name or UUID to use for variables."
                    }
                },
                "required": ["collection"]
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
                collect_request_summaries(&col.items, &mut requests_list);

                if !filter.is_empty() {
                    let col_match = col.name.to_lowercase().contains(&filter);
                    requests_list.retain(|r| {
                        col_match
                            || r["name"].as_str().unwrap_or("").to_lowercase().contains(&filter)
                            || r["url"].as_str().unwrap_or("").to_lowercase().contains(&filter)
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

            let out = json!({
                "status": view.status,
                "status_text": view.status_text,
                "elapsed_ms": view.elapsed_ms,
                "resolved_url": view.resolved_url,
                "headers": view.headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<HashMap<_, _>>(),
                "body": view.body,
                "error": view.error,
                "tests": tests_summary
            });

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

            let out = json!({
                "status": view.status,
                "status_text": view.status_text,
                "elapsed_ms": view.elapsed_ms,
                "resolved_url": view.resolved_url,
                "headers": view.headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<HashMap<_, _>>(),
                "body": view.body,
                "error": view.error
            });

            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        "blitz_list_environments" => {
            let envs: Vec<_> = snapshot
                .environments
                .iter()
                .map(|e| {
                    json!({
                        "id": e.id.to_string(),
                        "name": e.name,
                        "variable_keys": e.values.iter().filter(|v| v.enabled && !v.key.is_empty()).map(|v| &v.key).collect::<Vec<_>>()
                    })
                })
                .collect();

            let res = json!({
                "active_environment": snapshot.active_environment,
                "environments": envs
            });
            serde_json::to_string_pretty(&res).map_err(|e| e.to_string())
        }
        "blitz_run_collection" => {
            let col_name = args
                .get("collection")
                .and_then(|c| c.as_str())
                .ok_or_else(|| "Missing required parameter 'collection'".to_string())?;

            let target_col = snapshot
                .collections
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(col_name) || c.id.to_string() == col_name)
                .ok_or_else(|| format!("Collection '{col_name}' not found"))?;

            let mut ctx = snapshot.variables.clone();
            ctx.collection = target_col.variables_map();

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
                target_col,
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
                "total_requests": report.results.len(),
                "passed": report.passed_count(),
                "failed": report.failed_count(),
                "results": results
            });

            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
        }
        _ => Err(format!("Unknown tool: {name}")),
    }
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
        _ => Err(format!("Unknown resource URI: {uri}")),
    }
}

// Helpers
fn collect_request_summaries(items: &[CollectionItem], out: &mut Vec<JsonValue>) {
    for item in items {
        match item {
            CollectionItem::Request(r) => {
                out.push(json!({
                    "id": r.id.to_string(),
                    "name": r.name,
                    "method": r.method.as_str(),
                    "url": r.url
                }));
            }
            CollectionItem::Folder(f) => {
                collect_request_summaries(&f.items, out);
            }
        }
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
        assert!(tools.iter().any(|t| t["name"] == "blitz_send_request"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_execute_http"));
        assert!(tools.iter().any(|t| t["name"] == "blitz_run_collection"));
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
