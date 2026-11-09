use std::time::Duration;

use dioxus::prelude::*;

use crate::core::{
    graphql_ws_subscribe_with_headers, grpc_reflection_list, mcp_call_tool, mcp_list_tools,
    mqtt_publish, mqtt_subscribe_once, socketio_session, split_mcp_args, sse_listen,
    ws_connect_session_with_headers, MqttActionResult,
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
            let vars = st.vars.peek();
            let resolved_url = vars.resolve_string(&req.url);
            let mut headers_to_send: Vec<(String, String)> = Vec::new();
            for h in req.headers.iter().filter(|h| h.enabled && !h.key.is_empty()) {
                headers_to_send.push((
                    vars.resolve_string(&h.key),
                    vars.resolve_string(&h.value),
                ));
            }
            match &req.auth.auth_type {
                crate::core::AuthType::Bearer => {
                    let token = vars.resolve_string(&req.auth.token);
                    if !token.is_empty() {
                        headers_to_send.push(("Authorization".into(), format!("Bearer {token}")));
                    }
                }
                crate::core::AuthType::ApiKey => {
                    let key = vars.resolve_string(&req.auth.api_key_header);
                    let val = vars.resolve_string(&req.auth.token);
                    if !key.is_empty() && !val.is_empty() {
                        headers_to_send.push((key, val));
                    }
                }
                _ => {}
            }
            st.protocol_log.set(vec![format!("ws → {resolved_url} ({} headers)", headers_to_send.len())]);
            st.log_network(format!("⚡ [WS] Connecting to {resolved_url} ({} headers)", headers_to_send.len()));
            match ws_connect_session_with_headers(&resolved_url, &headers_to_send) {
                Ok(sess) => {
                    if let Ok(mut g) = WS_SESSION.lock() {
                        *g = Some(sess);
                    }
                    st.ws_connected.set(true);
                    st.status_msg.set("websocket connected".into());
                    st.log_network(format!("⚡ [WS] Connected to {resolved_url}"));
                    if st.app_settings.peek().ui.console_log_error_payloads {
                        let mut sent = format!("─── WS Handshake Sent ───\nURL: {resolved_url}");
                        if !headers_to_send.is_empty() {
                            sent.push_str("\nHeaders:");
                            for (k, v) in &headers_to_send {
                                sent.push_str(&format!("\n  {k}: {v}"));
                            }
                        }
                        st.log_network(sent);
                    }
                    spawn(async move {
                        let mut auto_ping_ticks = 0usize;
                        loop {
                            tokio::time::sleep(Duration::from_millis(150)).await;
                            if !(st.ws_connected)() {
                                break;
                            }
                            let (events, timeline, still_open) = {
                                let Ok(g) = WS_SESSION.lock() else { break; };
                                if let Some(s) = g.as_ref() {
                                    (s.events_snapshot(), s.timeline_snapshot(), true)
                                } else {
                                    (vec![], vec![], false)
                                }
                            };
                            if !still_open {
                                st.ws_connected.set(false);
                                break;
                            }

                            // Classic lines format for Postman mode
                            let lines: Vec<String> = events
                                .iter()
                                .map(|e| match e {
                                    crate::core::WsSessionEvent::Opened => "⚡ Connected".into(),
                                    crate::core::WsSessionEvent::Outbound(t) => format!("→ {t}"),
                                    crate::core::WsSessionEvent::Message(t) => format!("← {t}"),
                                    crate::core::WsSessionEvent::Closed(c) => format!("✗ Closed: {c}"),
                                    crate::core::WsSessionEvent::Error(e) => format!("! Error: {e}"),
                                })
                                .collect();
                            st.protocol_log.set(lines);
                            st.ws_timeline.set(timeline);

                            // Auto-ping if enabled every 15s (100 * 150ms)
                            if (st.ws_auto_ping)() {
                                auto_ping_ticks += 1;
                                if auto_ping_ticks >= 100 {
                                    auto_ping_ticks = 0;
                                    if let Ok(g) = WS_SESSION.lock() {
                                        if let Some(s) = g.as_ref() {
                                            let _ = s.send_text("{\"type\": \"ping\"}");
                                        }
                                    }
                                }
                            }

                            // If closed or error occurred, finish loop
                            if events.iter().any(|e| {
                                matches!(
                                    e,
                                    crate::core::WsSessionEvent::Closed(_)
                                        | crate::core::WsSessionEvent::Error(_)
                                )
                            }) {
                                tokio::time::sleep(Duration::from_millis(300)).await;
                                st.ws_connected.set(false);
                                break;
                            }
                        }
                    });
                }
                Err(e) => {
                    st.ws_connected.set(false);
                    st.log_error(format!("⚡ [WS] Connect error: {e}"));
                    if st.app_settings.peek().ui.console_log_error_payloads {
                        let mut sent = format!("─── WS Handshake Sent ───\nURL: {resolved_url}");
                        if !headers_to_send.is_empty() {
                            sent.push_str("\nHeaders:");
                            for (k, v) in &headers_to_send {
                                sent.push_str(&format!("\n  {k}: {v}"));
                            }
                        }
                        st.log_network(sent);
                        st.log_error(format!("─── Received on Error ───\nConnect error: {e}"));
                    }
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
                        if st.app_settings.peek().ui.console_log_error_payloads {
                            st.log_network(format!("─── WS Payload Sent ───\nPayload:\n{msg}"));
                            st.log_error(format!("─── Received on Error ───\nSend error: {e}"));
                        }
                        st.protocol_log.set(vec![e]);
                    } else {
                        st.log_network(format!("⚡ [WS] Sent ({} bytes)", msg.len()));
                        if st.app_settings.peek().ui.console_log_error_payloads {
                            st.log_network(format!("─── WS Frame Sent ───\nPayload:\n{msg}"));
                        }
                    }
                }
            }
        }
        "ws_clear" => {
            if let Ok(g) = WS_SESSION.lock() {
                if let Some(s) = g.as_ref() {
                    s.clear_timeline();
                }
            }
            st.protocol_log.set(Vec::new());
            st.ws_timeline.set(Vec::new());
        }
        "ws_toggle_ping" => {
            let curr = (st.ws_auto_ping)();
            st.ws_auto_ping.set(!curr);
        }
        "ws_disconnect" => {
            if let Ok(mut g) = WS_SESSION.lock() {
                if let Some(s) = g.take() {
                    s.close();
                }
            }
            st.ws_connected.set(false);
            st.log_network("⚡ [WS] Disconnected");
        }
        "mqtt_publish" => {
            st.sending.set(true);
            let url = req.url.clone();
            let cid = req.mqtt_client_id.clone();
            let topic = req.mqtt_topic.clone();
            let body = req.body.clone();
            st.log_network(format!("📡 [MQTT] Publish to topic '{topic}' ({url})"));
            let url_bg = url.clone();
            let cid_bg = cid.clone();
            let topic_bg = topic.clone();
            let body_bg = body.clone();
            spawn(async move {
                let r = tokio::task::spawn_blocking(move || {
                    mqtt_publish(&url_bg, &cid_bg, &topic_bg, &body_bg)
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
                    if st.app_settings.peek().ui.console_log_error_payloads {
                        st.log_network(format!(
                            "─── MQTT Payload Sent ───\nTopic: {topic}\nPayload:\n{body}"
                        ));
                        st.log_error(format!("─── Received on Error ───\nError: {e}"));
                    }
                    lines.push(e);
                } else {
                    st.log_network(format!("📡 [MQTT] Published in {} ms", r.elapsed_ms));
                    if st.app_settings.peek().ui.console_log_error_payloads {
                        st.log_network(format!(
                            "─── MQTT Payload Sent ───\nTopic: {topic}\nPayload:\n{body}"
                        ));
                    }
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
            let args_json_bg = args_json.clone();
            st.log_network(format!("🤖 [MCP] Calling tool '{tool}'"));
            spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    let v: serde_json::Value = serde_json::from_str(&args_json_bg)
                        .unwrap_or_else(|_| serde_json::json!({}));
                    mcp_call_tool(&cmd, &args, &tool_bg, v)
                })
                .await;
                match result {
                    Ok(Ok(body)) => {
                        st.log_network(format!("🤖 [MCP] Tool '{tool}' call succeeded"));
                        if st.app_settings.peek().ui.console_log_error_payloads {
                            st.log_network(format!(
                                "─── MCP Tool Call Sent ───\nTool: {tool}\nArgs:\n{args_json}\n─── Received Result ───\n{body}"
                            ));
                        }
                        st.protocol_log.set(vec![body]);
                    }
                    Ok(Err(e)) => {
                        st.log_error(format!("🤖 [MCP] Tool '{tool}' error: {e}"));
                        if st.app_settings.peek().ui.console_log_error_payloads {
                            st.log_network(format!(
                                "─── MCP Tool Call Sent ───\nTool: {tool}\nArgs:\n{args_json}"
                            ));
                            st.log_error(format!("─── Received on Error ───\nError: {e}"));
                        }
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
            let url_bg = url.clone();
            let ev_bg = ev.clone();
            let body_bg = body.clone();
            st.log_network(format!("⚡ [Socket.IO] Emitting '{ev}' to {url}"));
            spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    socketio_session(&url_bg, &ev_bg, &body_bg, &ev_bg, 3_000)
                })
                .await;
                match result {
                    Ok(Ok(lines)) => {
                        st.log_network("⚡ [Socket.IO] Event acknowledged");
                        if st.app_settings.peek().ui.console_log_error_payloads {
                            st.log_network(format!(
                                "─── Socket.IO Sent ───\nEvent: {ev}\nPayload:\n{body}"
                            ));
                        }
                        st.protocol_log.set(lines);
                    }
                    Ok(Err(e)) => {
                        st.log_error(format!("⚡ [Socket.IO] Error: {e}"));
                        if st.app_settings.peek().ui.console_log_error_payloads {
                            st.log_network(format!(
                                "─── Socket.IO Sent ───\nEvent: {ev}\nPayload:\n{body}"
                            ));
                            st.log_error(format!("─── Received on Error ───\nError: {e}"));
                        }
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
            let vars = st.vars.peek();
            let url = vars.resolve_string(&req.url);
            st.log_network(format!("🌊 [SSE] Connecting to {url}"));
            let mut headers: Vec<(String, String)> = req
                .headers
                .iter()
                .filter(|h| {
                    h.enabled
                        && !h.key.trim().is_empty()
                        && !h.value.trim().eq_ignore_ascii_case("<calculated at runtime>")
                })
                .map(|h| (vars.resolve_string(&h.key), vars.resolve_string(&h.value)))
                .collect();
            match &req.auth.auth_type {
                crate::core::AuthType::Bearer => {
                    let token = vars.resolve_string(&req.auth.token);
                    if !token.is_empty() {
                        headers.push(("Authorization".into(), format!("Bearer {token}")));
                    }
                }
                crate::core::AuthType::ApiKey => {
                    let key = vars.resolve_string(&req.auth.api_key_header);
                    let val = vars.resolve_string(&req.auth.token);
                    if !key.is_empty() && !val.is_empty() {
                        headers.push((key, val));
                    }
                }
                _ => {}
            }
            let url_bg = url.clone();
            let headers_bg = headers.clone();
            spawn(async move {
                let result =
                    tokio::task::spawn_blocking(move || sse_listen(&url_bg, &headers_bg, 20, 15_000))
                        .await;
                match result {
                    Ok(Ok((lines, _))) => {
                        st.log_network(format!("🌊 [SSE] Received {} events", lines.len()));
                        if st.app_settings.peek().ui.console_log_error_payloads {
                            let mut sent = format!("─── SSE Connection Sent ───\nURL: {url}");
                            if !headers.is_empty() {
                                sent.push_str("\nHeaders:");
                                for (k, v) in &headers {
                                    sent.push_str(&format!("\n  {k}: {v}"));
                                }
                            }
                            st.log_network(sent);
                        }
                        st.protocol_log.set(lines);
                    }
                    Ok(Err(e)) => {
                        st.log_error(format!("🌊 [SSE] Error: {e}"));
                        if st.app_settings.peek().ui.console_log_error_payloads {
                            let mut sent = format!("─── SSE Connection Sent ───\nURL: {url}");
                            if !headers.is_empty() {
                                sent.push_str("\nHeaders:");
                                for (k, v) in &headers {
                                    sent.push_str(&format!("\n  {k}: {v}"));
                                }
                            }
                            st.log_network(sent);
                            st.log_error(format!("─── Received on Error ───\nError: {e}"));
                        }
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
            let vars = st.vars.peek();
            let url = vars.resolve_string(&req.url);
            let raw_q = if req.graphql_ws_query.is_empty() {
                &req.graphql_query
            } else {
                &req.graphql_ws_query
            };
            let q = vars.resolve_string(raw_q);
            let q_vars = vars.resolve_string(&req.graphql_variables);
            let mut headers: Vec<(String, String)> = req
                .headers
                .iter()
                .filter(|h| {
                    h.enabled
                        && !h.key.trim().is_empty()
                        && !h.value.trim().eq_ignore_ascii_case("<calculated at runtime>")
                })
                .map(|h| (vars.resolve_string(&h.key), vars.resolve_string(&h.value)))
                .collect();
            match &req.auth.auth_type {
                crate::core::AuthType::Bearer => {
                    let token = vars.resolve_string(&req.auth.token);
                    if !token.is_empty() {
                        headers.push(("Authorization".into(), format!("Bearer {token}")));
                    }
                }
                crate::core::AuthType::ApiKey => {
                    let key = vars.resolve_string(&req.auth.api_key_header);
                    let val = vars.resolve_string(&req.auth.token);
                    if !key.is_empty() && !val.is_empty() {
                        headers.push((key, val));
                    }
                }
                _ => {}
            }
            let url_bg = url.clone();
            let q_bg = q.clone();
            let vars_bg = q_vars.clone();
            let headers_bg = headers.clone();
            st.log_network(format!(
                "◈ [GraphQL-WS] Subscribing to {url} ({} headers)",
                headers.len()
            ));
            spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    graphql_ws_subscribe_with_headers(
                        &url_bg,
                        &q_bg,
                        &vars_bg,
                        &headers_bg,
                        10,
                        15_000,
                    )
                })
                .await;
                match result {
                    Ok(Ok(lines)) => {
                        st.log_network(format!("◈ [GraphQL-WS] Received {} events", lines.len()));
                        if st.app_settings.peek().ui.console_log_error_payloads {
                            st.log_network(format!(
                                "─── GraphQL-WS Sent ───\nQuery:\n{q}\nVariables:\n{q_vars}"
                            ));
                        }
                        st.protocol_log.set(lines);
                    }
                    Ok(Err(e)) => {
                        st.log_error(format!("◈ [GraphQL-WS] Error: {e}"));
                        if st.app_settings.peek().ui.console_log_error_payloads {
                            st.log_network(format!(
                                "─── GraphQL-WS Sent ───\nQuery:\n{q}\nVariables:\n{q_vars}"
                            ));
                            st.log_error(format!("─── Received on Error ───\nError: {e}"));
                        }
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
