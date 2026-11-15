//! Server-Sent Events (EventSource) client — GET/POST streams with AI-friendly event parsing.

use std::error::Error as _;
use std::io::{BufRead, BufReader, Read};
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use reqwest::Method;

#[derive(Debug, Clone)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
    #[allow(dead_code)]
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseStreamEvent {
    pub index: usize,
    pub event: String,
    pub data: String,
    pub id: String,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseStreamResult {
    pub status: u16,
    pub status_text: String,
    pub response_headers: Vec<(String, String)>,
    pub elapsed_ms: u128,
    pub ttfb_ms: Option<u128>,
    pub events: Vec<SseStreamEvent>,
    pub warning: Option<String>,
    pub log: Vec<String>,
}

/// Read SSE events until `max_events` or `timeout_ms`, returning log lines + parsed events.
pub fn sse_listen(
    url: &str,
    headers: &[(String, String)],
    max_events: usize,
    timeout_ms: u64,
) -> Result<(Vec<String>, Vec<SseEvent>), String> {
    let res = sse_stream_with_options("GET", url, headers, None, max_events, timeout_ms)?;
    let events = res
        .events
        .into_iter()
        .map(|ev| SseEvent {
            event: ev.event,
            data: ev.data,
            id: ev.id,
        })
        .collect();
    Ok((res.log, events))
}

/// Connect to an HTTP SSE endpoint (GET/POST/etc., optional body) and parse streamed events.
///
/// Disables HTTP compression negotiation (`identity`) and preserves partial/completed events if
/// a server closes cleanly or times out after emitting events, preventing opaque
/// `error decoding response body` failures.
pub fn sse_stream_with_options(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&str>,
    max_events: usize,
    timeout_ms: u64,
) -> Result<SseStreamResult, String> {
    let started = Instant::now();
    let max_events = max_events.max(1);
    let timeout = Duration::from_millis(timeout_ms.max(1_000));

    let client = Client::builder()
        .timeout(timeout)
        .no_gzip()
        .no_deflate()
        .no_brotli()
        .build()
        .map_err(|e| format!("SSE client build failed: {e}"))?;

    let req_method = Method::from_bytes(method.trim().to_ascii_uppercase().as_bytes())
        .unwrap_or(Method::GET);

    let mut builder = client.request(req_method.clone(), url);

    let has_accept = headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("accept"));
    if !has_accept {
        builder = builder.header("Accept", "text/event-stream");
    }
    let has_cache = headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("cache-control"));
    if !has_cache {
        builder = builder.header("Cache-Control", "no-cache");
    }
    let has_accept_encoding = headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("accept-encoding"));
    if !has_accept_encoding {
        builder = builder.header("Accept-Encoding", "identity");
    }

    for (k, v) in headers {
        let key = k.trim();
        let val = v.trim();
        if key.is_empty() || val.eq_ignore_ascii_case("<calculated at runtime>") {
            continue;
        }
        builder = builder.header(key, val);
    }

    if let Some(payload) = body {
        if !payload.is_empty() && req_method != Method::GET && req_method != Method::HEAD {
            let has_ct = headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
            if !has_ct {
                builder = builder.header("Content-Type", "application/json");
            }
            builder = builder.body(payload.to_string());
        }
    }

    let resp = builder.send().map_err(|e| describe_reqwest_error("connect", &e))?;
    let status = resp.status();
    let status_text = status.canonical_reason().unwrap_or("").to_string();
    let response_headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .filter_map(|(k, v)| v.to_str().ok().map(|s| (k.to_string(), s.to_string())))
        .collect();

    if !status.is_success() {
        let mut err_buf = Vec::new();
        let _ = resp.take(64 * 1024).read_to_end(&mut err_buf);
        let err_body = String::from_utf8_lossy(&err_buf).trim().to_string();
        if err_body.is_empty() {
            return Err(format!("SSE HTTP {} {}", status.as_u16(), status_text));
        } else {
            return Err(format!(
                "SSE HTTP {} {} — {}",
                status.as_u16(),
                status_text,
                truncate(&err_body, 600)
            ));
        }
    }

    let mut reader = BufReader::new(resp);
    let mut log = vec![format!(
        "sse: connected {} {} -> HTTP {} {}",
        req_method,
        url,
        status.as_u16(),
        status_text
    )];
    let mut events = Vec::new();
    let mut ttfb_ms: Option<u128> = None;
    let mut warning: Option<String> = None;

    let mut cur_event = String::new();
    let mut cur_data = String::new();
    let mut cur_id = String::new();
    let mut line_buf = Vec::with_capacity(1024);

    loop {
        line_buf.clear();
        match reader.read_until(b'\n', &mut line_buf) {
            Ok(0) => {
                // EOF: flush any trailing event even if server omitted final blank line.
                flush_sse_event(
                    &mut events,
                    &mut log,
                    &mut ttfb_ms,
                    started,
                    &mut cur_event,
                    &mut cur_data,
                    &mut cur_id,
                );
                break;
            }
            Ok(_) => {
                // Strip UTF-8 BOM on the very first chunk line if present
                let slice = if events.is_empty() && line_buf.starts_with(&[0xEF, 0xBB, 0xBF]) {
                    &line_buf[3..]
                } else {
                    &line_buf[..]
                };
                let raw_line = String::from_utf8_lossy(slice);
                let line = raw_line.trim_end_matches(['\r', '\n']);

                if line.is_empty() {
                    let flushed = flush_sse_event(
                        &mut events,
                        &mut log,
                        &mut ttfb_ms,
                        started,
                        &mut cur_event,
                        &mut cur_data,
                        &mut cur_id,
                    );
                    if flushed && events.len() >= max_events {
                        log.push(format!("sse: reached max_events limit ({max_events})"));
                        break;
                    }
                    continue;
                }

                if let Some(rest) = line.strip_prefix("event:") {
                    cur_event = rest.trim().to_string();
                } else if let Some(rest) = line.strip_prefix("data:") {
                    if !cur_data.is_empty() {
                        cur_data.push('\n');
                    }
                    cur_data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
                } else if let Some(rest) = line.strip_prefix("id:") {
                    cur_id = rest.trim().to_string();
                } else if line.starts_with(':') || line.starts_with("retry:") {
                    // SSE comment / keepalive / retry directive
                } else if cur_data.is_empty()
                    && cur_event.is_empty()
                    && (line.starts_with('{') || line.starts_with('['))
                {
                    // Helpful fallback when an endpoint returns NDJSON or non-`data:` JSON error line
                    cur_event = "message".into();
                    cur_data.push_str(line);
                }
            }
            Err(e) => {
                flush_sse_event(
                    &mut events,
                    &mut log,
                    &mut ttfb_ms,
                    started,
                    &mut cur_event,
                    &mut cur_data,
                    &mut cur_id,
                );
                let msg = describe_io_decode_error(&e);
                let is_benign_close = e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock
                    || (!events.is_empty()
                        && (e.kind() == std::io::ErrorKind::UnexpectedEof
                            || msg.contains("decoding response body")
                            || msg.contains("unexpected EOF")));

                if is_benign_close && !events.is_empty() {
                    let warn = if e.kind() == std::io::ErrorKind::TimedOut {
                        format!("Stream paused after {} events (timeout reached)", events.len())
                    } else {
                        format!("Stream ended early ({msg}); {} events preserved", events.len())
                    };
                    log.push(format!("sse: {warn}"));
                    warning = Some(warn);
                    break;
                } else if e.kind() == std::io::ErrorKind::TimedOut {
                    return Err(format!(
                        "SSE stream timed out after {} ms without receiving any events",
                        timeout.as_millis()
                    ));
                } else {
                    return Err(msg);
                }
            }
        }
    }

    log.push(format!(
        "sse: done ({} events in {} ms)",
        events.len(),
        started.elapsed().as_millis()
    ));

    Ok(SseStreamResult {
        status: status.as_u16(),
        status_text,
        response_headers,
        elapsed_ms: started.elapsed().as_millis(),
        ttfb_ms,
        events,
        warning,
        log,
    })
}

fn flush_sse_event(
    events: &mut Vec<SseStreamEvent>,
    log: &mut Vec<String>,
    ttfb_ms: &mut Option<u128>,
    started: Instant,
    cur_event: &mut String,
    cur_data: &mut String,
    cur_id: &mut String,
) -> bool {
    if cur_data.is_empty() && cur_event.is_empty() {
        return false;
    }
    let elapsed_ms = started.elapsed().as_millis();
    if ttfb_ms.is_none() {
        *ttfb_ms = Some(elapsed_ms);
    }
    let ev_name = if cur_event.trim().is_empty() {
        "message".to_string()
    } else {
        cur_event.clone()
    };
    let ev = SseStreamEvent {
        index: events.len() + 1,
        event: ev_name,
        data: cur_data.trim_end_matches('\n').to_string(),
        id: cur_id.clone(),
        elapsed_ms,
    };
    log.push(format!(
        "sse #{} [{}ms] event={} data={}",
        ev.index,
        ev.elapsed_ms,
        ev.event,
        truncate(&ev.data, 200)
    ));
    events.push(ev);
    cur_event.clear();
    cur_data.clear();
    cur_id.clear();
    true
}

fn describe_reqwest_error(stage: &str, err: &reqwest::Error) -> String {
    let mut parts = vec![format!("SSE {stage}: {err}")];
    let mut src = err.source();
    while let Some(cause) = src {
        let s = cause.to_string();
        if !parts.iter().any(|p| p.contains(&s)) {
            parts.push(s);
        }
        src = cause.source();
    }
    parts.join(" — ")
}

fn describe_io_decode_error(err: &std::io::Error) -> String {
    let mut parts = vec![err.to_string()];
    let mut src = err.source();
    while let Some(cause) = src {
        let s = cause.to_string();
        if !parts.iter().any(|p| p.contains(&s)) {
            parts.push(s);
        }
        src = cause.source();
    }
    parts.join(": ")
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let prefix: String = s.chars().take(n).collect();
        format!("{prefix}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    #[test]
    fn truncate_short() {
        assert_eq!(truncate("hi", 10), "hi");
    }

    #[test]
    fn truncate_multibyte_utf8_does_not_panic() {
        let s = "🔥".repeat(250);
        let out = truncate(&s, 200);
        assert!(out.ends_with('…'));
        assert_eq!(out.chars().count(), 201);
    }

    #[test]
    fn sse_post_stream_parses_events_and_trailing_chunk_without_blank_line() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut req_buf = [0u8; 2048];
            let n = stream.read(&mut req_buf).unwrap();
            let req_text = String::from_utf8_lossy(&req_buf[..n]);
            assert!(req_text.starts_with("POST /chat "));
            assert!(req_text.to_ascii_lowercase().contains("accept-encoding: identity"));

            let body = concat!(
                "id: 1\n",
                "event: delta\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
                "data: [DONE]\n"
            );
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).unwrap();
        });

        let res = sse_stream_with_options(
            "POST",
            &format!("http://{addr}/chat"),
            &[("Authorization".into(), "Bearer test".into())],
            Some(r#"{"stream":true}"#),
            20,
            5_000,
        )
        .unwrap();

        assert_eq!(res.status, 200);
        assert_eq!(res.events.len(), 3);
        assert_eq!(res.events[0].event, "delta");
        assert_eq!(res.events[0].id, "1");
        assert_eq!(res.events[1].event, "message");
        assert_eq!(res.events[2].data, "[DONE]");
        handle.join().unwrap();
    }

    #[test]
    fn sse_preserves_received_events_if_chunked_stream_drops_early() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut req_buf = [0u8; 1024];
            let _ = stream.read(&mut req_buf);

            // Chunked response that sends 1 complete SSE event then closes without terminating 0 chunk
            let ev1 = "event: token\ndata: hi\n\n";
            let chunk1 = format!("{:x}\r\n{}\r\n", ev1.len(), ev1);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{}",
                chunk1
            );
            stream.write_all(resp.as_bytes()).unwrap();
            let _ = stream.shutdown(std::net::Shutdown::Both);
        });

        let res = sse_stream_with_options(
            "GET",
            &format!("http://{addr}/events"),
            &[],
            None,
            20,
            3_000,
        )
        .unwrap();

        assert_eq!(res.events.len(), 1);
        assert_eq!(res.events[0].event, "token");
        assert_eq!(res.events[0].data, "hi");
        handle.join().unwrap();
    }
}
