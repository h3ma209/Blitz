//! Socket.IO client (connect / emit / on) via `rust_socketio`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rust_socketio::{ClientBuilder, Payload, TransportType};
use serde_json::Value;

/// Connect, optionally emit one event, collect inbound events for `listen_ms`.
pub fn socketio_session(
    url: &str,
    emit_event: &str,
    emit_payload_json: &str,
    listen_event: &str,
    listen_ms: u64,
) -> Result<Vec<String>, String> {
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![format!(
        "socket.io: connecting {url}"
    )]));
    let log_c = log.clone();
    let listen = listen_event.to_string();

    let socket = ClientBuilder::new(url.to_string())
        .transport_type(TransportType::Websocket)
        .on("error", {
            let log = log.clone();
            move |err, _| {
                if let Ok(mut g) = log.lock() {
                    g.push(format!("socket.io error: {err:?}"));
                }
            }
        })
        .on(listen.as_str(), {
            let log = log_c.clone();
            move |payload, _| {
                let msg = match payload {
                    Payload::Text(v) => serde_json::to_string(&v).unwrap_or_else(|_| format!("{v:?}")),
                    Payload::Binary(b) => format!("<binary {} bytes>", b.len()),
                    other => format!("{other:?}"),
                };
                if let Ok(mut g) = log.lock() {
                    g.push(format!("socket.io ← {msg}"));
                }
            }
        })
        .on_any({
            let log = log.clone();
            move |event, payload, _| {
                if let Ok(mut g) = log.lock() {
                    g.push(format!("socket.io any {event}: {payload:?}"));
                }
            }
        })
        .connect()
        .map_err(|e| format!("socket.io connect: {e}"))?;

    {
        let mut g = log.lock().map_err(|e| e.to_string())?;
        g.push("socket.io: connected".into());
    }

    if !emit_event.is_empty() {
        let payload = if emit_payload_json.trim().is_empty() {
            Payload::from("{}")
        } else if let Ok(v) = serde_json::from_str::<Value>(emit_payload_json) {
            Payload::from(v.to_string())
        } else {
            Payload::from(emit_payload_json.to_string())
        };
        socket
            .emit(emit_event, payload)
            .map_err(|e| format!("emit: {e}"))?;
        let mut g = log.lock().map_err(|e| e.to_string())?;
        g.push(format!("socket.io → emit {emit_event}"));
    }

    std::thread::sleep(Duration::from_millis(listen_ms.max(200)));
    let _ = socket.disconnect();
    let mut g = log.lock().map_err(|e| e.to_string())?;
    g.push("socket.io: disconnected".into());
    Ok(g.clone())
}

#[cfg(test)]
mod tests {
    #[test]
    fn module_loads() {
        // Connect tests need a live server — skipped.
        assert!(true);
    }
}
