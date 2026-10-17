//! Local mock HTTP server matching method + path to saved examples.
//! Matching inspired by mock-server-runtime literals (reports/11).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use uuid::Uuid;

use super::collection::Collection;
use super::request::{HttpMethod, RequestDraft, SavedExample};

#[derive(Debug, Clone)]
pub struct MockRoute {
    pub method: HttpMethod,
    pub path: String,
    pub example: SavedExample,
}

#[derive(Debug, Default)]
pub struct MockServerState {
    pub routes: Vec<MockRoute>,
    pub port: u16,
    pub running: bool,
}

/// Build routes from all requests that have examples (first example wins per method+path).
pub fn routes_from_collections(collections: &[Collection]) -> Vec<MockRoute> {
    let mut out = Vec::new();
    for col in collections {
        walk_items(&col.items, &mut out);
    }
    out
}

fn walk_items(items: &[super::collection::CollectionItem], out: &mut Vec<MockRoute>) {
    use super::collection::CollectionItem;
    for item in items {
        match item {
            CollectionItem::Request(r) => {
                if let Some(ex) = r.examples.first() {
                    out.push(MockRoute {
                        method: r.method,
                        path: path_only(&r.url),
                        example: ex.clone(),
                    });
                }
            }
            CollectionItem::Folder(f) => walk_items(&f.items, out),
        }
    }
}

fn path_only(url: &str) -> String {
    // Strip scheme/host and query; keep path.
    let rest = url.split("://").nth(1).unwrap_or(url);
    let path = if rest.contains('/') {
        let p = rest.split_once('/').map(|(_, p)| format!("/{p}")).unwrap_or_else(|| "/".into());
        p
    } else if url.starts_with('/') {
        url.to_string()
    } else {
        format!("/{url}")
    };
    path.split('?').next().unwrap_or(&path).to_string()
}

pub struct MockHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    pub port: u16,
}

impl MockHandle {
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake listener
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Start mock on `127.0.0.1:port` (port 0 = ephemeral). Returns bound port.
pub fn start_mock(routes: Vec<MockRoute>, port: u16) -> Result<MockHandle, String> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let addr: SocketAddr = listener.local_addr().map_err(|e| e.to_string())?;
    let bound = addr.port();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = stop.clone();
    let routes = Arc::new(Mutex::new(routes));

    let join = thread::spawn(move || {
        while !stop_t.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = handle_conn(&mut stream, &routes);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(_) => break,
            }
        }
    });

    Ok(MockHandle {
        stop,
        join: Some(join),
        port: bound,
    })
}

fn handle_conn(stream: &mut TcpStream, routes: &Arc<Mutex<Vec<MockRoute>>>) -> std::io::Result<()> {
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf)?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let first = req.lines().next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let path = parts.next().unwrap_or("/").split('?').next().unwrap_or("/");

    let routes = routes.lock().unwrap();
    let found = routes.iter().find(|r| {
        r.method.as_str().eq_ignore_ascii_case(method) && paths_match(&r.path, path)
    });

    let response = match found {
        Some(r) => {
            let mut headers = String::new();
            for (k, v) in &r.example.headers {
                if k.eq_ignore_ascii_case("content-length") || k.eq_ignore_ascii_case("transfer-encoding") {
                    continue;
                }
                headers.push_str(&format!("{k}: {v}\r\n"));
            }
            format!(
                "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{}",
                r.example.status,
                if r.example.status_text.is_empty() {
                    "OK"
                } else {
                    &r.example.status_text
                },
                r.example.body.len(),
                r.example.body
            )
        }
        None => {
            "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found"
                .to_string()
        }
    };
    stream.write_all(response.as_bytes())?;
    Ok(())
}

fn paths_match(template: &str, actual: &str) -> bool {
    // Exact or ignore trailing slash; simple `:id` segment wildcard.
    if template == actual {
        return true;
    }
    let t: Vec<_> = template.trim_matches('/').split('/').collect();
    let a: Vec<_> = actual.trim_matches('/').split('/').collect();
    if t.len() != a.len() {
        return false;
    }
    t.iter().zip(a.iter()).all(|(tp, ap)| tp.starts_with(':') || tp == ap)
}

/// Attach example to a request by id across collections.
pub fn save_example_to_request(
    collections: &mut [Collection],
    request_id: Uuid,
    example: SavedExample,
) -> bool {
    for col in collections.iter_mut() {
        if let Some(req) = col.find_request_mut(request_id) {
            req.examples.push(example);
            return true;
        }
    }
    false
}

/// Convenience: path from draft URL for display.
pub fn draft_mock_path(draft: &RequestDraft) -> String {
    path_only(&draft.url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_only_strips_host() {
        assert_eq!(path_only("https://api.example.com/v1/users"), "/v1/users");
        assert_eq!(path_only("/local"), "/local");
    }

    #[test]
    fn wildcard_match() {
        assert!(paths_match("/users/:id", "/users/42"));
        assert!(!paths_match("/users/:id", "/users/42/extra"));
    }
}
