//! WebSocket client session (tokio-tungstenite).

use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use futures_util::{SinkExt, StreamExt};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Debug, Clone)]
pub enum WsSessionEvent {
    Opened,
    Message(String),
    Closed(String),
    Error(String),
}

pub struct WsSession {
    tx: mpsc::UnboundedSender<WsCmd>,
    events: Arc<Mutex<Vec<WsSessionEvent>>>,
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

    pub fn send_text(&self, text: impl Into<String>) -> Result<(), String> {
        self.tx
            .send(WsCmd::Send(text.into()))
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
    let url = url.to_string();
    let events = Arc::new(Mutex::new(Vec::<WsSessionEvent>::new()));
    let events_bg = Arc::clone(&events);
    let (tx, mut rx) = mpsc::unbounded_channel::<WsCmd>();

    let join = thread::Builder::new()
        .name("np-ws".into())
        .spawn(move || {
            let rt = match Runtime::new() {
                Ok(r) => r,
                Err(e) => {
                    push(&events_bg, WsSessionEvent::Error(format!("runtime: {e}")));
                    return;
                }
            };
            rt.block_on(async move {
                let (ws, _) = match connect_async(&url).await {
                    Ok(p) => p,
                    Err(e) => {
                        push(&events_bg, WsSessionEvent::Error(format!("connect: {e}")));
                        return;
                    }
                };
                push(&events_bg, WsSessionEvent::Opened);
                let (mut write, mut read) = ws.split();

                loop {
                    tokio::select! {
                        cmd = rx.recv() => {
                            match cmd {
                                Some(WsCmd::Send(t)) => {
                                    if let Err(e) = write.send(Message::Text(t.into())).await {
                                        push(&events_bg, WsSessionEvent::Error(format!("send: {e}")));
                                        break;
                                    }
                                }
                                Some(WsCmd::Close) | None => {
                                    let _ = write.close().await;
                                    push(&events_bg, WsSessionEvent::Closed("local close".into()));
                                    break;
                                }
                            }
                        }
                        msg = read.next() => {
                            match msg {
                                Some(Ok(Message::Text(t))) => {
                                    push(&events_bg, WsSessionEvent::Message(t.to_string()));
                                }
                                Some(Ok(Message::Binary(b))) => {
                                    push(
                                        &events_bg,
                                        WsSessionEvent::Message(format!("[binary {} bytes]", b.len())),
                                    );
                                }
                                Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {}
                                Some(Ok(Message::Close(c))) => {
                                    push(
                                        &events_bg,
                                        WsSessionEvent::Closed(format!("{c:?}")),
                                    );
                                    break;
                                }
                                Some(Err(e)) => {
                                    push(&events_bg, WsSessionEvent::Error(format!("read: {e}")));
                                    break;
                                }
                                None => {
                                    push(&events_bg, WsSessionEvent::Closed("eof".into()));
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
        join: Some(join),
    })
}

fn push(events: &Arc<Mutex<Vec<WsSessionEvent>>>, ev: WsSessionEvent) {
    if let Ok(mut g) = events.lock() {
        g.push(ev);
        if g.len() > 500 {
            let drain = g.len() - 500;
            g.drain(0..drain);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_url_reports_error() {
        let sess = ws_connect_session("ws://127.0.0.1:1/nope");
        assert!(sess.is_ok());
        let sess = sess.unwrap();
        std::thread::sleep(std::time::Duration::from_millis(400));
        let ev = sess.events_snapshot();
        assert!(
            ev.iter().any(|e| matches!(e, WsSessionEvent::Error(_))),
            "{ev:?}"
        );
        sess.close();
    }
}
