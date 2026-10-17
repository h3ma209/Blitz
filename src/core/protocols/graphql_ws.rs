//! GraphQL subscriptions over WebSocket (`graphql-ws` / `graphql-transport-ws` protocol).

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::runtime::Runtime;
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// Subscribe and collect up to `max_events` payload messages (or until timeout).
pub fn graphql_ws_subscribe(
    ws_url: &str,
    query: &str,
    variables_json: &str,
    max_events: usize,
    timeout_ms: u64,
) -> Result<Vec<String>, String> {
    let rt = Runtime::new().map_err(|e| e.to_string())?;
    rt.block_on(subscribe_async(
        ws_url,
        query,
        variables_json,
        max_events,
        timeout_ms,
    ))
}

async fn subscribe_async(
    ws_url: &str,
    query: &str,
    variables_json: &str,
    max_events: usize,
    timeout_ms: u64,
) -> Result<Vec<String>, String> {
    let mut log = vec![format!("graphql-ws: connect {ws_url}")];
    let (mut ws, _) = connect_async(ws_url)
        .await
        .map_err(|e| format!("connect: {e}"))?;

    // graphql-transport-ws connection_init
    let init = json!({"type": "connection_init", "payload": {}});
    ws.send(Message::Text(init.to_string().into()))
        .await
        .map_err(|e| e.to_string())?;

    let vars: Value = serde_json::from_str(if variables_json.trim().is_empty() {
        "{}"
    } else {
        variables_json
    })
    .unwrap_or(json!({}));

    let sub_id = "1";
    let subscribe = json!({
        "id": sub_id,
        "type": "subscribe",
        "payload": {
            "query": query,
            "variables": vars,
        }
    });
    // Also try legacy graphql-ws start after ack
    let legacy_start = json!({
        "id": sub_id,
        "type": "start",
        "payload": {
            "query": query,
            "variables": vars,
        }
    });

    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms.max(1_000));
    let mut events = 0usize;
    let mut started = false;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            log.push("graphql-ws: timeout".into());
            break;
        }
        let msg = tokio::time::timeout(remaining, ws.next())
            .await
            .map_err(|_| "timeout".to_string())?
            .ok_or_else(|| "ws closed".to_string())?
            .map_err(|e| e.to_string())?;

        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Ping(p) => {
                let _ = ws.send(Message::Pong(p)).await;
                continue;
            }
            Message::Close(_) => {
                log.push("graphql-ws: closed".into());
                break;
            }
            _ => continue,
        };

        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        log.push(format!("graphql-ws ← {ty}: {}", truncate(&text, 300)));

        match ty {
            "connection_ack" => {
                if !started {
                    let _ = ws.send(Message::Text(subscribe.to_string().into())).await;
                    let _ = ws
                        .send(Message::Text(legacy_start.to_string().into()))
                        .await;
                    started = true;
                    log.push("graphql-ws: subscribe sent".into());
                }
            }
            "next" | "data" | "error" | "complete" => {
                events += 1;
                if ty == "complete" || events >= max_events {
                    let stop = json!({"id": sub_id, "type": "complete"});
                    let _ = ws.send(Message::Text(stop.to_string().into())).await;
                    break;
                }
            }
            "ka" | "ping" => {
                let _ = ws
                    .send(Message::Text(json!({"type": "pong"}).to_string().into()))
                    .await;
            }
            _ => {}
        }
    }
    log.push(format!("graphql-ws: done ({events} payloads)"));
    Ok(log)
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}
