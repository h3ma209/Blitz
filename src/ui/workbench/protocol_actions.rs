use std::time::Duration;

use dioxus::prelude::*;

use crate::core::{
    graphql_ws_subscribe, grpc_reflection_list, mcp_call_tool, mcp_list_tools, mqtt_publish,
    mqtt_subscribe_once, socketio_session, split_mcp_args, sse_listen, ws_connect_session,
    MqttActionResult,
};
use crate::ui::workbench::state::{WorkbenchState, WS_SESSION};

pub fn run_protocol_action(mut st: WorkbenchState, action: String) {
    let req = (st.draft)();
    match action.as_str() {
        "ws_connect" => {
            if let Ok(mut g) = WS_SESSION.lock() {
                if let Some(old) = g.take() {
                    old.close();
                }
            }
            st.protocol_log.set(vec![format!("ws → {}", req.url)]);
            st.log_network(format!("⚡ [WS] Connecting to {}", req.url));
            match ws_connect_session(&req.url) {
                Ok(sess) => {
                    if let Ok(mut g) = WS_SESSION.lock() {
                        *g = Some(sess);
                    }
                    st.ws_connected.set(true);
                    st.status_msg.set("websocket connecting…".into());
                    st.log_network(format!("⚡ [WS] Connected to {}", req.url));
                    spawn(async move {
                        for _ in 0..40 {
                            tokio::time::sleep(Duration::from_millis(250)).await;
                            let snap = WS_SESSION
                                .lock()
                                .ok()
                                .and_then(|g| g.as_ref().map(|s| s.events_snapshot()))
                                .unwrap_or_default();
                            let lines: Vec<String> =
                                snap.iter().map(|e| format!("{e:?}")).collect();
                            st.protocol_log.set(lines);
                            let still = WS_SESSION
                                .lock()
                                .ok()
                                .map(|g| g.is_some())
                                .unwrap_or(false);
                            if !still {
                                st.ws_connected.set(false);
                                break;
                            }
                        }
                    });
                }
                Err(e) => {
                    st.ws_connected.set(false);
                    st.log_error(format!("⚡ [WS] Connect error: {e}"));
                    st.protocol_log.set(vec![e]);
                }
            }
        }
        "ws_send" => {
            let msg = req.ws_message.clone();
            if let Ok(g) = WS_SESSION.lock() {
                if let Some(s) = g.as_ref() {
                    if let Err(e) = s.send_text(msg.clone()) {
                        st.log_error(format!("⚡ [WS] Send failed: {e}"));
                        st.protocol_log.set(vec![e]);
                    } else {
                        let mut lines = (st.protocol_log)();
                        lines.push("→ sent".into());
                        st.protocol_log.set(lines);
                        st.log_network(format!("⚡ [WS] Sent ({} bytes)", msg.len()));
                    }
                }
            }
        }
        "ws_disconnect" => {
            if let Ok(mut g) = WS_SESSION.lock() {
                if let Some(s) = g.take() {
                    s.close();
                }
            }
            st.ws_connected.set(false);
            let mut lines = (st.protocol_log)();
            lines.push("disconnected".into());
            st.protocol_log.set(lines);
            st.log_network("⚡ [WS] Disconnected");
        }
        "mqtt_publish" => {
            st.sending.set(true);
            let url = req.url.clone();
            let cid = req.mqtt_client_id.clone();
            let topic = req.mqtt_topic.clone();
            let body = req.body.clone();
            st.log_network(format!("📡 [MQTT] Publish to topic '{topic}' ({url})"));
            spawn(async move {
                let r = tokio::task::spawn_blocking(move || {
                    mqtt_publish(&url, &cid, &topic, &body)
                })
                .await
                .unwrap_or_else(|e| MqttActionResult {
                    ok: false,
                    messages: vec![],
                    error: Some(e.to_string()),
                    elapsed_ms: 0,
                });
                let mut lines = r.messages;
                if let Some(e) = r.error {
                    st.log_error(format!("📡 [MQTT] Publish error: {e}"));
                    lines.push(e);
                } else {
                    st.log_network(format!("📡 [MQTT] Published in {} ms", r.elapsed_ms));
                }
                lines.push(format!("{} ms", r.elapsed_ms));
                st.protocol_log.set(lines);
                st.sending.set(false);
            });
        }
        "mqtt_subscribe" => {
            st.sending.set(true);
            let url = req.url.clone();
            let cid = req.mqtt_client_id.clone();
            let topic = req.mqtt_topic.clone();
            st.log_network(format!("📡 [MQTT] Subscribing to topic '{topic}' ({url})"));
            spawn(async move {
                let r = tokio::task::spawn_blocking(move || {
                    mqtt_subscribe_once(&url, &cid, &topic, Duration::from_secs(8))
                })
                .await
                .unwrap_or_else(|e| MqttActionResult {
                    ok: false,
                    messages: vec![],
                    error: Some(e.to_string()),
                    elapsed_ms: 0,
                });
                let mut lines = r.messages;
                if let Some(e) = r.error {
                    st.log_error(format!("📡 [MQTT] Subscribe error: {e}"));
                    lines.push(e);
                } else {
                    st.log_network(format!("📡 [MQTT] Received message in {} ms", r.elapsed_ms));
                }
                st.protocol_log.set(lines);
                st.sending.set(false);
            });
        }
        "mcp_list" => {
            st.sending.set(true);
            let cmd = req.mcp_command.clone();
            let args = split_mcp_args(&req.mcp_args);
            st.log_network(format!("🤖 [MCP] Listing tools from '{cmd}'"));
            spawn(async move {
                let result =
                    tokio::task::spawn_blocking(move || mcp_list_tools(&cmd, &args)).await;
                match result {
                    Ok(Ok(tools)) => {
                        st.log_network(format!("🤖 [MCP] Discovered {} tools", tools.len()));
                        let lines: Vec<String> = tools
                            .into_iter()
                            .map(|t| format!("{} — {}", t.name, t.description))
                            .collect();
                        st.protocol_log.set(if lines.is_empty() {
                            vec!["(no tools)".into()]
                        } else {
                            lines
                        });
                    }
                    Ok(Err(e)) => {
                        st.log_error(format!("🤖 [MCP] List error: {e}"));
                        st.protocol_log.set(vec![format!("mcp: {e}")]);
                    }
                    Err(e) => {
                        st.log_error(format!("🤖 [MCP] Join error: {e}"));
                        st.protocol_log.set(vec![format!("mcp join: {e}")]);
                    }
                }
                st.sending.set(false);
            });
        }
        "mcp_call" => {
            st.sending.set(true);
            let cmd = req.mcp_command.clone();
            let args = split_mcp_args(&req.mcp_args);
            let tool = req.mcp_tool_name.clone();
            let tool_bg = tool.clone();
            let args_json = req.mcp_tool_args_json.clone();
            st.log_network(format!("🤖 [MCP] Calling tool '{tool}'"));
            spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    let v: serde_json::Value = serde_json::from_str(&args_json)
                        .unwrap_or_else(|_| serde_json::json!({}));
                    mcp_call_tool(&cmd, &args, &tool_bg, v)
                })
                .await;
                match result {
                    Ok(Ok(body)) => {
                        st.log_network(format!("🤖 [MCP] Tool '{tool}' call succeeded"));
                        st.protocol_log.set(vec![body]);
                    }
                    Ok(Err(e)) => {
                        st.log_error(format!("🤖 [MCP] Tool '{tool}' error: {e}"));
                        st.protocol_log.set(vec![e]);
                    }
                    Err(e) => {
                        st.log_error(format!("🤖 [MCP] Tool '{tool}' join error: {e}"));
                        st.protocol_log.set(vec![format!("join: {e}")]);
                    }
                }
                st.sending.set(false);
            });
        }
        "socketio_session" => {
            st.sending.set(true);
            let url = req.url.clone();
            let ev = req.socketio_event.clone();
            let body = req.body.clone();
            st.log_network(format!("⚡ [Socket.IO] Emitting '{ev}' to {url}"));
            spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    socketio_session(&url, &ev, &body, &ev, 3_000)
                })
                .await;
                match result {
                    Ok(Ok(lines)) => {
                        st.log_network("⚡ [Socket.IO] Event acknowledged");
                        st.protocol_log.set(lines);
                    }
                    Ok(Err(e)) => {
                        st.log_error(format!("⚡ [Socket.IO] Error: {e}"));
                        st.protocol_log.set(vec![e]);
                    }
                    Err(e) => {
                        st.log_error(format!("⚡ [Socket.IO] Join error: {e}"));
                        st.protocol_log.set(vec![format!("join: {e}")]);
                    }
                }
                st.sending.set(false);
            });
        }
        "sse_listen" => {
            st.sending.set(true);
            let url = req.url.clone();
            st.log_network(format!("🌊 [SSE] Connecting to {url}"));
            let headers: Vec<(String, String)> = req
                .headers
                .iter()
                .filter(|h| h.enabled && !h.key.is_empty())
                .map(|h| (h.key.clone(), h.value.clone()))
                .collect();
            spawn(async move {
                let result =
                    tokio::task::spawn_blocking(move || sse_listen(&url, &headers, 20, 15_000))
                        .await;
                match result {
                    Ok(Ok((lines, _))) => {
                        st.log_network(format!("🌊 [SSE] Received {} events", lines.len()));
                        st.protocol_log.set(lines);
                    }
                    Ok(Err(e)) => {
                        st.log_error(format!("🌊 [SSE] Error: {e}"));
                        st.protocol_log.set(vec![e]);
                    }
                    Err(e) => {
                        st.log_error(format!("🌊 [SSE] Join error: {e}"));
                        st.protocol_log.set(vec![format!("join: {e}")]);
                    }
                }
                st.sending.set(false);
            });
        }
        "graphql_ws_subscribe" => {
            st.sending.set(true);
            let url = req.url.clone();
            let q = if req.graphql_ws_query.is_empty() {
                req.graphql_query.clone()
            } else {
                req.graphql_ws_query.clone()
            };
            let vars = req.graphql_variables.clone();
            st.log_network(format!("◈ [GraphQL-WS] Subscribing to {url}"));
            spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    graphql_ws_subscribe(&url, &q, &vars, 10, 15_000)
                })
                .await;
                match result {
                    Ok(Ok(lines)) => {
                        st.log_network(format!("◈ [GraphQL-WS] Received {} events", lines.len()));
                        st.protocol_log.set(lines);
                    }
                    Ok(Err(e)) => {
                        st.log_error(format!("◈ [GraphQL-WS] Error: {e}"));
                        st.protocol_log.set(vec![e]);
                    }
                    Err(e) => {
                        st.log_error(format!("◈ [GraphQL-WS] Join error: {e}"));
                        st.protocol_log.set(vec![format!("join: {e}")]);
                    }
                }
                st.sending.set(false);
            });
        }
        "grpc_reflect" => {
            let url = req.url.clone();
            st.log_network(format!("🔌 [gRPC] Reflecting {url}"));
            st.protocol_log.set(grpc_reflection_list(&url));
        }
        _ => {}
    }
}
