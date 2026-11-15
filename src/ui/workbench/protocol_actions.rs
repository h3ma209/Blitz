use std::sync::Arc;
use std::time::Duration;

use dioxus::prelude::*;

use crate::core::{
    build_url_with_params, graphql_ws_subscribe_with_headers, grpc_reflection_list,
    mcp_call_tool, mcp_list_tools, mqtt_publish, mqtt_subscribe_once, socketio_session,
    split_mcp_args, sse_stream_with_options, substitute_path_params,
    ws_connect_session_with_headers, BodyMode, HttpMethod, MqttActionResult, ResponseView,
    TimingBreakdown,
};
use crate::ui::workbench::state::{WorkbenchState, WS_SESSION};

fn parse_sse_action_params(action: &str) -> Option<(usize, u64)> {
    if action == "sse_listen" {
        return Some((40, 20_000));
    }
    let rest = action.strip_prefix("sse_listen:")?;
    let mut parts = rest.split(':');
    let max_events = parts
        .next()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(40)
        .clamp(1, 500);
    let timeout_ms = parts
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(20_000)
        .clamp(1_000, 300_000);
    Some((max_events, timeout_ms))
}

pub fn run_protocol_action(mut st: WorkbenchState, action: String) {
    let req = (st.draft)();
    if let Some((max_events, timeout_ms)) = parse_sse_action_params(&action) {
        st.sending.set(true);
        let vars = st.vars.peek();
        let resolved_url = vars.resolve_string(&req.url);
        let resolved_url = substitute_path_params(&resolved_url, &req.path_params);
        let url = build_url_with_params(&resolved_url, &req.params);
        let method_str = req.method.as_str().to_string();
        let body_opt = if req.method != HttpMethod::Get
            && req.method != HttpMethod::Head
            && req.body_mode == BodyMode::Raw
        {
            let resolved_body = vars.resolve_string(&req.body);
            if resolved_body.trim().is_empty() {
                None
            } else {
                Some(resolved_body)
            }
        } else {
            None
        };
        st.log_network(format!(
            "🌊 [SSE] Connecting ({method_str}) to {url} (max {max_events} events, {}s timeout)",
            timeout_ms / 1_000
        ));
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
                if !token.is_empty()
                    && !headers
                        .iter()
                        .any(|(k, _)| k.eq_ignore_ascii_case("authorization"))
                {
                    headers.push(("Authorization".into(), format!("Bearer {token}")));
                }
            }
            crate::core::AuthType::ApiKey => {
                let key = vars.resolve_string(&req.auth.api_key_header);
                let val = vars.resolve_string(&req.auth.token);
                let hdr_name = if key.trim().is_empty() {
                    "X-Api-Key".to_string()
                } else {
                    key
                };
                if !val.is_empty()
                    && !headers
                        .iter()
                        .any(|(k, _)| k.eq_ignore_ascii_case(&hdr_name))
                {
                    headers.push((hdr_name, val));
                }
            }
            _ => {}
        }
        let req_id = req.id;
        let url_bg = url.clone();
        let method_bg = method_str.clone();
        let body_bg = body_opt.clone();
        let headers_bg = headers.clone();
        spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                sse_stream_with_options(
                    &method_bg,
                    &url_bg,
                    &headers_bg,
                    body_bg.as_deref(),
                    max_events,
                    timeout_ms,
                )
            })
            .await;
            match result {
                Ok(Ok(stream_res)) => {
                    st.log_network(format!(
                        "🌊 [SSE] {} {} -> HTTP {} ({} events in {} ms)",
                        method_str,
                        url,
                        stream_res.status,
                        stream_res.events.len(),
                        stream_res.elapsed_ms
                    ));
                    if let Some(ref w) = stream_res.warning {
                        st.log_warn(format!("🌊 [SSE] {w}"));
                    }
                    if st.app_settings.peek().ui.console_log_error_payloads {
                        let mut sent = format!("─── SSE Connection Sent ───\n{method_str} {url}");
                        if !headers.is_empty() {
                            sent.push_str("\nHeaders:");
                            for (k, v) in &headers {
                                sent.push_str(&format!("\n  {k}: {v}"));
                            }
                        }
                        if let Some(ref b) = body_opt {
                            sent.push_str(&format!("\nBody:\n{b}"));
                        }
                        st.log_network(sent);
                    }
                    let mut structured_log = Vec::with_capacity(stream_res.events.len() + 1);
                    let meta_json = serde_json::json!({
                        "status": stream_res.status,
                        "status_text": stream_res.status_text,
                        "elapsed_ms": stream_res.elapsed_ms,
                        "ttfb_ms": stream_res.ttfb_ms,
                        "count": stream_res.events.len(),
                        "warning": stream_res.warning,
                        "method": method_str,
                        "url": url,
                    });
                    structured_log.push(format!("SSE_META:{meta_json}"));
                    let mut reconstructed = String::new();
                    for ev in &stream_res.events {
                        let ev_json = serde_json::json!({
                            "index": ev.index,
                            "event": ev.event,
                            "id": ev.id,
                            "elapsed_ms": ev.elapsed_ms,
                            "data": ev.data,
                        });
                        structured_log.push(format!("SSE_EVENT:{ev_json}"));
                        if !reconstructed.is_empty() {
                            reconstructed.push('\n');
                        }
                        if !ev.id.is_empty() {
                            reconstructed.push_str(&format!("id: {}\n", ev.id));
                        }
                        reconstructed.push_str(&format!("event: {}\n", ev.event));
                        for line in ev.data.lines() {
                            reconstructed.push_str(&format!("data: {line}\n"));
                        }
                    }
                    let resp_view = Arc::new(ResponseView {
                        status: stream_res.status,
                        status_text: stream_res.status_text,
                        headers: stream_res.response_headers,
                        body: reconstructed,
                        elapsed_ms: stream_res.elapsed_ms,
                        error: None,
                        pipeline_log: stream_res.log,
                        console_messages: Vec::new(),
                        test_results: Vec::new(),
                        resolved_url: url,
                        timing: TimingBreakdown {
                            ttfb_ms: stream_res.ttfb_ms.unwrap_or(0),
                            download_ms: stream_res
                                .elapsed_ms
                                .saturating_sub(stream_res.ttfb_ms.unwrap_or(0)),
                            ..Default::default()
                        },
                        visualizer_html: String::new(),
                        sent_headers: headers,
                        sent_body: body_opt,
                    });
                    st.response.set(Some(resp_view.clone()));
                    st.responses.write().insert(req_id, resp_view);
                    st.protocol_log.set(structured_log);
                }
                Ok(Err(e)) => {
                    st.log_error(format!("🌊 [SSE] Error: {e}"));
                    if st.app_settings.peek().ui.console_log_error_payloads {
                        let mut sent = format!("─── SSE Connection Sent ───\n{method_str} {url}");
                        if !headers.is_empty() {
                            sent.push_str("\nHeaders:");
                            for (k, v) in &headers {
                                sent.push_str(&format!("\n  {k}: {v}"));
                            }
                        }
                        if let Some(ref b) = body_opt {
                            sent.push_str(&format!("\nBody:\n{b}"));
                        }
                        st.log_network(sent);
                        st.log_error(format!("─── Received on Error ───\nError: {e}"));
                    }
                    let err_json = serde_json::json!({ "error": e, "method": method_str, "url": url });
                    st.protocol_log.set(vec![format!("SSE_ERR:{err_json}")]);
                }
                Err(e) => {
                    st.log_error(format!("🌊 [SSE] Join error: {e}"));
                    st.protocol_log.set(vec![format!("SSE_ERR:{}", serde_json::json!({ "error": format!("join: {e}") }))]);
                }
            }
            st.sending.set(false);
        });
        return;
    }
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
                            if *st.protocol_log.peek() != lines {
                                st.protocol_log.set(lines);
                            }
                            if *st.ws_timeline.peek() != timeline {
                                st.ws_timeline.set(timeline);
                            }

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
