//! WebSocket client session (tokio-tungstenite).

use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WsDirection {
    Inbound,
    Outbound,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WsFrameItem {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub direction: WsDirection,
    pub text: String,
    pub is_binary: bool,
}

impl WsFrameItem {
    pub fn new(direction: WsDirection, text: impl Into<String>, is_binary: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            direction,
            text: text.into(),
            is_binary,
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum WsSessionEvent {
    Opened,
    Message(String),
    Outbound(String),
    Closed(String),
    Error(String),
}

pub struct WsSession {
    tx: mpsc::UnboundedSender<WsCmd>,
    events: Arc<Mutex<Vec<WsSessionEvent>>>,
    frames: Arc<Mutex<Vec<WsFrameItem>>>,
    join: Option<JoinHandle<()>>,
}

enum WsCmd {
    Send(String),
    Close,
}

impl WsSession {
    pub fn events_snapshot(&self) -> Vec<WsSessionEvent> {
        self.events.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn timeline_snapshot(&self) -> Vec<WsFrameItem> {
        self.frames.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn clear_timeline(&self) {
        if let Ok(mut f) = self.frames.lock() {
            f.clear();
        }
        if let Ok(mut e) = self.events.lock() {
            e.clear();
        }
    }

    pub fn send_text(&self, text: impl Into<String>) -> Result<(), String> {
        let msg = text.into();
        self.tx
            .send(WsCmd::Send(msg))
            .map_err(|e| format!("ws send: {e}"))
    }

    pub fn close(mut self) {
        let _ = self.tx.send(WsCmd::Close);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Connect in a background thread with its own Tokio runtime.
pub fn ws_connect_session(url: &str) -> Result<WsSession, String> {
    ws_connect_session_with_headers(url, &[])
}

/// Connect in a background thread with custom handshake headers.
pub fn ws_connect_session_with_headers(
    url: &str,
    headers: &[(String, String)],
) -> Result<WsSession, String> {
    let url = url.to_string();
    let headers_bg = headers.to_vec();
    let events = Arc::new(Mutex::new(Vec::<WsSessionEvent>::new()));
    let events_bg = Arc::clone(&events);
    let frames = Arc::new(Mutex::new(Vec::<WsFrameItem>::new()));
    let frames_bg = Arc::clone(&frames);
    let (tx, mut rx) = mpsc::unbounded_channel::<WsCmd>();

    let join = thread::Builder::new()
        .name("np-ws".into())
        .spawn(move || {
            let rt = match Runtime::new() {
                Ok(r) => r,
                Err(e) => {
                    push_both(
                        &events_bg,
                        &frames_bg,
                        WsSessionEvent::Error(format!("runtime: {e}")),
                        WsFrameItem::new(WsDirection::System, format!("Runtime error: {e}"), false),
                    );
                    return;
                }
            };
            rt.block_on(async move {
                let connect_res = if headers_bg.is_empty() {
                    connect_async(&url).await
                } else {
                    match url.clone().into_client_request() {
                        Ok(mut req) => {
                            for (k, v) in &headers_bg {
                                let v_trimmed = v.trim();
                                if v_trimmed.is_empty()
                                    || v_trimmed.eq_ignore_ascii_case("<calculated at runtime>")
                                {
                                    continue;
                                }
                                if let (Ok(hk), Ok(hv)) = (
                                    tokio_tungstenite::tungstenite::http::header::HeaderName::from_bytes(k.as_bytes()),
                                    tokio_tungstenite::tungstenite::http::header::HeaderValue::from_str(v_trimmed),
                                ) {
                                    req.headers_mut().insert(hk, hv);
                                }
                            }
                            connect_async(req).await
                        }
                        Err(e) => {
                            push_both(
                                &events_bg,
                                &frames_bg,
                                WsSessionEvent::Error(format!("request build: {e}")),
                                WsFrameItem::new(WsDirection::System, format!("Request error: {e}"), false),
                            );
                            return;
                        }
                    }
                };

                let (ws, _) = match connect_res {
                    Ok(p) => p,
                    Err(e) => {
                        push_both(
                            &events_bg,
                            &frames_bg,
                            WsSessionEvent::Error(format!("connect: {e}")),
                            WsFrameItem::new(WsDirection::System, format!("Connect error: {e}"), false),
                        );
                        return;
                    }
                };
                push_both(
                    &events_bg,
                    &frames_bg,
                    WsSessionEvent::Opened,
                    WsFrameItem::new(WsDirection::System, format!("Connected to {url}"), false),
                );
                let (mut write, mut read) = ws.split();

                loop {
                    tokio::select! {
                        cmd = rx.recv() => {
                            match cmd {
                                Some(WsCmd::Send(t)) => {
                                    push_both(
                                        &events_bg,
                                        &frames_bg,
                                        WsSessionEvent::Outbound(t.clone()),
                                        WsFrameItem::new(WsDirection::Outbound, t.clone(), false),
                                    );
                                    if let Err(e) = write.send(Message::Text(t.into())).await {
                                        push_both(
                                            &events_bg,
                                            &frames_bg,
                                            WsSessionEvent::Error(format!("send: {e}")),
                                            WsFrameItem::new(WsDirection::System, format!("Send error: {e}"), false),
                                        );
                                        break;
                                    }
                                }
                                Some(WsCmd::Close) | None => {
                                    let _ = write.close().await;
                                    push_both(
                                        &events_bg,
                                        &frames_bg,
                                        WsSessionEvent::Closed("local close".into()),
                                        WsFrameItem::new(WsDirection::System, "Disconnected (local close)", false),
                                    );
                                    break;
                                }
                            }
                        }
                        msg = read.next() => {
                            match msg {
                                Some(Ok(Message::Text(t))) => {
                                    push_both(
                                        &events_bg,
                                        &frames_bg,
                                        WsSessionEvent::Message(t.to_string()),
                                        WsFrameItem::new(WsDirection::Inbound, t.to_string(), false),
                                    );
                                }
                                Some(Ok(Message::Binary(b))) => {
                                    let label = format!("[binary {} bytes]", b.len());
                                    push_both(
                                        &events_bg,
                                        &frames_bg,
                                        WsSessionEvent::Message(label.clone()),
                                        WsFrameItem::new(WsDirection::Inbound, label, true),
                                    );
                                }
                                Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {}
                                Some(Ok(Message::Close(c))) => {
                                    let msg = format!("{c:?}");
                                    push_both(
                                        &events_bg,
                                        &frames_bg,
                                        WsSessionEvent::Closed(msg.clone()),
                                        WsFrameItem::new(WsDirection::System, format!("Closed by remote: {msg}"), false),
                                    );
                                    break;
                                }
                                Some(Err(e)) => {
                                    let msg = format!("read: {e}");
                                    push_both(
                                        &events_bg,
                                        &frames_bg,
                                        WsSessionEvent::Error(msg.clone()),
                                        WsFrameItem::new(WsDirection::System, format!("Read error: {msg}"), false),
                                    );
                                    break;
                                }
                                None => {
                                    push_both(
                                        &events_bg,
                                        &frames_bg,
                                        WsSessionEvent::Closed("eof".into()),
                                        WsFrameItem::new(WsDirection::System, "Disconnected (remote EOF)", false),
                                    );
                                    break;
                                }
                            }
                        }
                    }
                }
            });
        })
        .map_err(|e| format!("spawn: {e}"))?;

    Ok(WsSession {
        tx,
        events,
        frames,
        join: Some(join),
    })
}

fn push_both(
    events: &Arc<Mutex<Vec<WsSessionEvent>>>,
    frames: &Arc<Mutex<Vec<WsFrameItem>>>,
    ev: WsSessionEvent,
    frame: WsFrameItem,
) {
    if let Ok(mut g) = events.lock() {
        g.push(ev);
        if g.len() > 500 {
            let drain = g.len() - 500;
            g.drain(0..drain);
        }
    }
    if let Ok(mut f) = frames.lock() {
        f.push(frame);
        if f.len() > 500 {
            let drain = f.len() - 500;
            f.drain(0..drain);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_url_reports_error() {
        // Use an invalid scheme/domain so connection fails fast and deterministically across all OSes
        let sess = ws_connect_session("ws://invalid.domain.test:1/nope");
        assert!(sess.is_ok());
        let sess = sess.unwrap();
        let mut got_error = false;
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let ev = sess.events_snapshot();
            if ev.iter().any(|e| matches!(e, WsSessionEvent::Error(_))) {
                got_error = true;
                break;
            }
        }
        let ev = sess.events_snapshot();
        assert!(got_error, "{ev:?}");
        sess.close();
    }
}
