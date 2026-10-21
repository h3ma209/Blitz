//! Server-Sent Events (EventSource) client — GET stream, log events.

use std::io::{BufRead, BufReader};
use std::time::Duration;

use reqwest::blocking::Client;

#[derive(Debug, Clone)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
    #[allow(dead_code)]
    pub id: String,
}

/// Read SSE events until `max_events` or `timeout_ms`, returning log lines + parsed events.
pub fn sse_listen(
    url: &str,
    headers: &[(String, String)],
    max_events: usize,
    timeout_ms: u64,
) -> Result<(Vec<String>, Vec<SseEvent>), String> {
    let client = Client::builder()
        .timeout(Duration::from_millis(timeout_ms.max(1_000)))
        .build()
        .map_err(|e| e.to_string())?;

    let mut builder = client.get(url).header("Accept", "text/event-stream");
    for (k, v) in headers {
        if !k.is_empty() {
            builder = builder.header(k, v);
        }
    }

    let resp = builder.send().map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("SSE HTTP {status}"));
    }

    let reader = BufReader::new(resp);
    let mut log = vec![format!("sse: connected {url}")];
    let mut events = Vec::new();
    let mut cur_event = String::from("message");
    let mut cur_data = String::new();
    let mut cur_id = String::new();

    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.is_empty() {
            if !cur_data.is_empty() || !cur_event.is_empty() {
                let ev = SseEvent {
                    event: cur_event.clone(),
                    data: cur_data.trim_end_matches('\n').to_string(),
                    id: cur_id.clone(),
                };
                log.push(format!("sse: event={} data={}", ev.event, truncate(&ev.data, 200)));
                events.push(ev);
                if events.len() >= max_events {
                    break;
                }
            }
            cur_event = "message".into();
            cur_data.clear();
            cur_id.clear();
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            cur_event = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("data:") {
            if !cur_data.is_empty() {
                cur_data.push('\n');
            }
            cur_data.push_str(rest.trim_start());
        } else if let Some(rest) = line.strip_prefix("id:") {
            cur_id = rest.trim().to_string();
        } else if line.starts_with(':') {
            // comment
        }
    }
    log.push(format!("sse: done ({} events)", events.len()));
    Ok((log, events))
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
}
