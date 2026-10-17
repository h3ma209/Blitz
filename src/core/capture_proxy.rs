//! Lightweight local HTTP capture proxy — logs requests into a shared buffer (history feed).

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use chrono::Utc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CapturedRequest {
    pub id: Uuid,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub elapsed_ms: Option<u128>,
    pub at: String,
    pub error: Option<String>,
}

pub struct CaptureProxyHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    pub port: u16,
    pub log: Arc<Mutex<Vec<CapturedRequest>>>,
}

impl CaptureProxyHandle {
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }

    pub fn drain_log(&self) -> Vec<CapturedRequest> {
        self.log.lock().map(|mut g| g.drain(..).collect()).unwrap_or_default()
    }

    pub fn snapshot(&self) -> Vec<CapturedRequest> {
        self.log.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

/// Bind `127.0.0.1:port` (0 = ephemeral). Absolute-form HTTP proxy only (no CONNECT MITM).
pub fn start_capture_proxy(port: u16) -> Result<CaptureProxyHandle, String> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let stop = Arc::new(AtomicBool::new(false));
    let log = Arc::new(Mutex::new(Vec::<CapturedRequest>::new()));
    let stop_bg = Arc::clone(&stop);
    let log_bg = Arc::clone(&log);

    let join = thread::Builder::new()
        .name("np-capture-proxy".into())
        .spawn(move || {
            while !stop_bg.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let log = Arc::clone(&log_bg);
                        thread::spawn(move || handle_client(stream, log));
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(25));
                    }
                    Err(_) => break,
                }
            }
        })
        .map_err(|e| e.to_string())?;

    Ok(CaptureProxyHandle {
        stop,
        join: Some(join),
        port,
        log,
    })
}

fn handle_client(mut client: TcpStream, log: Arc<Mutex<Vec<CapturedRequest>>>) {
    let _ = client.set_read_timeout(Some(std::time::Duration::from_secs(30)));
    let mut buf = [0u8; 65536];
    let n = match client.read(&mut buf) {
        Ok(0) | Err(_) => return,
        Ok(n) => n,
    };
    let raw = String::from_utf8_lossy(&buf[..n]);
    let mut lines = raw.lines();
    let request_line = match lines.next() {
        Some(l) => l.to_string(),
        None => return,
    };
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return;
    }
    let method = parts[0].to_string();
    let target = parts[1].to_string();

    if method.eq_ignore_ascii_case("CONNECT") {
        let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n");
        push(
            &log,
            CapturedRequest {
                id: Uuid::new_v4(),
                method,
                url: target,
                status: Some(502),
                elapsed_ms: Some(0),
                at: Utc::now().to_rfc3339(),
                error: Some("CONNECT/TLS MITM not supported".into()),
            },
        );
        return;
    }

    let url = if target.starts_with("http://") || target.starts_with("https://") {
        target.clone()
    } else {
        // Relative form — need Host header
        let host = raw
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("host:"))
            .and_then(|l| l.split_once(':').map(|(_, v)| v.trim().to_string()))
            .unwrap_or_else(|| "localhost".into());
        format!("http://{host}{target}")
    };

    let started = Instant::now();
    match forward_http(&url, &method, &buf[..n]) {
        Ok((status, resp_bytes)) => {
            let _ = client.write_all(&resp_bytes);
            push(
                &log,
                CapturedRequest {
                    id: Uuid::new_v4(),
                    method,
                    url,
                    status: Some(status),
                    elapsed_ms: Some(started.elapsed().as_millis()),
                    at: Utc::now().to_rfc3339(),
                    error: None,
                },
            );
        }
        Err(e) => {
            let body = format!("proxy error: {e}");
            let resp = format!(
                "HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            let _ = client.write_all(resp.as_bytes());
            push(
                &log,
                CapturedRequest {
                    id: Uuid::new_v4(),
                    method,
                    url,
                    status: Some(502),
                    elapsed_ms: Some(started.elapsed().as_millis()),
                    at: Utc::now().to_rfc3339(),
                    error: Some(e),
                },
            );
        }
    }
    let _ = client.shutdown(Shutdown::Both);
}

fn forward_http(url: &str, method: &str, original: &[u8]) -> Result<(u16, Vec<u8>), String> {
    // Re-issue with reqwest using original method + body after headers.
    let body_start = original
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .unwrap_or(original.len());
    let body = &original[body_start..];

    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;

    let mut builder = match method.to_ascii_uppercase().as_str() {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "PATCH" => client.patch(url),
        "DELETE" => client.delete(url),
        "HEAD" => client.head(url),
        other => client.request(
            reqwest::Method::from_bytes(other.as_bytes()).unwrap_or(reqwest::Method::GET),
            url,
        ),
    };
    if !body.is_empty() {
        builder = builder.body(body.to_vec());
    }
    let resp = builder.send().map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let headers = resp.headers().clone();
    let bytes = resp.bytes().map_err(|e| e.to_string())?;

    let mut out = format!("HTTP/1.1 {status} {}\r\n", reason(status));
    for (k, v) in headers.iter() {
        if let Ok(val) = v.to_str() {
            out.push_str(&format!("{k}: {val}\r\n"));
        }
    }
    out.push_str(&format!("Content-Length: {}\r\n\r\n", bytes.len()));
    let mut resp_bytes = out.into_bytes();
    resp_bytes.extend_from_slice(&bytes);
    Ok((status, resp_bytes))
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 | 302 => "Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        _ => "",
    }
}

fn push(log: &Arc<Mutex<Vec<CapturedRequest>>>, entry: CapturedRequest) {
    if let Ok(mut g) = log.lock() {
        g.push(entry);
        if g.len() > 200 {
            let n = g.len() - 200;
            g.drain(0..n);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_and_stops() {
        let h = start_capture_proxy(0).expect("bind");
        assert!(h.port > 0);
        h.stop();
    }
}
