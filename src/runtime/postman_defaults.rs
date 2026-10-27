//! Postman Runtime default / system headers + JSON body prep.
//! Observed in Postman.app ASAR (`postman-runtime`): User-Agent PostmanRuntime/*,
//! Accept */*, Accept-Encoding, Cache-Control, Postman-Token, Connection, Host.

use uuid::Uuid;

use crate::core::{BodyMode, KeyValue, RequestDraft};

/// Mimic Postman desktop system headers (only fill gaps; never override user headers).
pub fn inject_postman_system_headers(draft: &mut RequestDraft, log: &mut Vec<String>) {
    let defaults = [
        ("User-Agent", "PostmanRuntime/7.43.0"),
        ("Accept", "*/*"),
        ("Accept-Encoding", "gzip, deflate, br"),
        ("Connection", "keep-alive"),
        ("Cache-Control", "no-cache"),
    ];
    for (key, value) in defaults {
        if !has_header(draft, key) {
            draft.headers.push(KeyValue::new(key, value));
            log.push(format!("headers: default {key}"));
        }
    }
    // Host header derived from URL if not already present
    if !has_header(draft, "Host") && !draft.url.is_empty() {
        if let Ok(parsed) = reqwest::Url::parse(&draft.url) {
            if let Some(host) = parsed.host_str() {
                let host_val = if let Some(port) = parsed.port() {
                    format!("{host}:{port}")
                } else {
                    host.to_string()
                };
                draft.headers.push(KeyValue::new("Host", &host_val));
                log.push(format!("headers: default Host {host_val}"));
            }
        } else {
            // Fallback parse without scheme
            let rest = draft.url.split("://").nth(1).unwrap_or(&draft.url);
            let host_part = rest.split('/').next().unwrap_or(rest);
            let host_only = host_part.split('?').next().unwrap_or(host_part).trim();
            if !host_only.is_empty() && !host_only.contains("{{") {
                draft.headers.push(KeyValue::new("Host", host_only));
                log.push(format!("headers: default Host {host_only}"));
            }
        }
    }
    if !has_header(draft, "Postman-Token") {
        let token = Uuid::new_v4().to_string();
        draft.headers.push(KeyValue::new("Postman-Token", &token));
        log.push("headers: default Postman-Token".into());
    }
}

fn has_header(draft: &RequestDraft, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    draft
        .headers
        .iter()
        .any(|h| h.enabled && h.key.eq_ignore_ascii_case(&lower) && !h.key.is_empty())
}

/// Raw body that looks like JSON (incl. Postman-style `//` comment scratchpads).
pub fn looks_like_json_body(raw: &str) -> bool {
    let t = raw.trim_start();
    t.starts_with('{')
        || t.starts_with('[')
        || t.starts_with("//")
        || t.starts_with("/*")
}

/// Strip `//` / `/* */` comments (string-aware) and return first parseable JSON value.
/// Fixes Hakki-style bodies where credential objects are line-commented.
pub fn prepare_json_raw_body(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return Ok(trimmed.to_string());
    }

    let stripped = strip_json_comments(trimmed);
    let stripped_trim = stripped.trim();
    if serde_json::from_str::<serde_json::Value>(stripped_trim).is_ok() {
        return Ok(stripped_trim.to_string());
    }

    // Multiple objects after uncomment (Hakki export) — take first `{...}` / `[...]`.
    if let Some(first) = first_json_value(stripped_trim) {
        if serde_json::from_str::<serde_json::Value>(&first).is_ok() {
            return Ok(first);
        }
    }

    // Line-uncomment then first value (full-line `//` only).
    let line_uncommented = uncomment_full_lines(trimmed);
    if let Some(first) = first_json_value(line_uncommented.trim()) {
        if serde_json::from_str::<serde_json::Value>(&first).is_ok() {
            return Ok(first);
        }
    }

    Err(
        "body is not valid JSON (remove // comments or leave one uncommented object)".into(),
    )
}

/// Ensure Content-Type + cleaned body for Raw JSON-ish payloads.
pub fn prepare_raw_body_for_send(draft: &mut RequestDraft, log: &mut Vec<String>) -> Result<(), String> {
    if draft.body_mode != BodyMode::Raw || draft.body.trim().is_empty() {
        return Ok(());
    }
    if !looks_like_json_body(&draft.body)
        && !has_header(draft, "Content-Type")
    {
        return Ok(());
    }
    let is_json_ct = draft
        .headers
        .iter()
        .find(|h| h.enabled && h.key.eq_ignore_ascii_case("Content-Type"))
        .map(|h| h.value.to_ascii_lowercase().contains("json"))
        .unwrap_or(false);

    if looks_like_json_body(&draft.body) || is_json_ct {
        match prepare_json_raw_body(&draft.body) {
            Ok(clean) if !clean.is_empty() => {
                if clean != draft.body {
                    log.push("body: stripped JSON comments / picked first object".into());
                    draft.body = clean;
                }
            }
            Ok(_) => {}
            Err(e) => {
                log.push(format!("body: JSON warn — {e}"));
                // Still send original; server may reject — surface in log.
            }
        }
        if !has_header(draft, "Content-Type") {
            draft
                .headers
                .push(KeyValue::new("Content-Type", "application/json"));
            log.push("headers: default Content-Type application/json".into());
        }
    }
    if draft.body_mode == BodyMode::Urlencoded && !has_header(draft, "Content-Type") {
        draft
            .headers
            .push(KeyValue::new("Content-Type", "application/x-www-form-urlencoded"));
        log.push("headers: default Content-Type application/x-www-form-urlencoded".into());
    } else if draft.body_mode == BodyMode::Graphql && !has_header(draft, "Content-Type") {
        draft
            .headers
            .push(KeyValue::new("Content-Type", "application/json"));
        log.push("headers: default Content-Type application/json".into());
    }
    Ok(())
}

fn uncomment_full_lines(raw: &str) -> String {
    raw.lines()
        .map(|line| {
            let t = line.trim_start();
            if let Some(rest) = t.strip_prefix("//") {
                // Preserve indent roughly
                let indent_len = line.len() - t.len();
                format!("{}{}", &line[..indent_len], rest.trim_start())
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_json_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut in_string = false;
    let mut escape = false;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '/' && i + 1 < chars.len() {
            if chars[i + 1] == '/' {
                i += 2;
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            if chars[i + 1] == '*' {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i = (i + 2).min(chars.len());
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

fn first_json_value(input: &str) -> Option<String> {
    let chars: Vec<char> = input.chars().collect();
    let start = chars.iter().position(|c| *c == '{' || *c == '[')?;
    let open = chars[start];
    let close = if open == '{' { '}' } else { ']' };
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for i in start..chars.len() {
        let c = chars[i];
        if in_string {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            c if c == open => depth += 1,
            c if c == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(chars[start..=i].iter().collect());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_inline_and_line_comments() {
        let raw = r#"{
    "username":"500110007",
    "password":"123456Aa",
    //phone: 10108772
    "app_secret":"poke7467384638746mm5465ds45sheisws" // hakki
}"#;
        let clean = prepare_json_raw_body(raw).unwrap();
        let v: serde_json::Value = serde_json::from_str(&clean).unwrap();
        assert_eq!(v["username"], "500110007");
        assert_eq!(v["app_secret"], "poke7467384638746mm5465ds45sheisws");
    }

    #[test]
    fn picks_first_fully_commented_object() {
        let raw = r#"// {
//     "username":"500110007",
//     "password":"123456Aa",
//     "app_secret":"secret1"
// }
// {
//     "username":"other",
//     "password":"x",
//     "app_secret":"secret2"
// }"#;
        let clean = prepare_json_raw_body(raw).unwrap();
        let v: serde_json::Value = serde_json::from_str(&clean).unwrap();
        assert_eq!(v["username"], "500110007");
        assert_eq!(v["app_secret"], "secret1");
    }

    #[test]
    fn injects_missing_system_headers() {
        let mut d = RequestDraft::default();
        let mut log = vec![];
        inject_postman_system_headers(&mut d, &mut log);
        assert!(has_header(&d, "User-Agent"));
        assert!(has_header(&d, "Accept"));
        assert!(has_header(&d, "Cache-Control"));
        assert!(has_header(&d, "Postman-Token"));
        assert!(has_header(&d, "Accept-Encoding"));
        assert!(has_header(&d, "Connection"));
        assert!(has_header(&d, "Host"));
        // Don't override
        d.headers
            .iter_mut()
            .find(|h| h.key == "User-Agent")
            .unwrap()
            .value = "CustomUA".into();
        let mut log2 = vec![];
        inject_postman_system_headers(&mut d, &mut log2);
        assert_eq!(
            d.headers
                .iter()
                .find(|h| h.key == "User-Agent")
                .unwrap()
                .value,
            "CustomUA"
        );
    }

    #[test]
    fn prepare_raw_adds_content_type() {
        let mut d = RequestDraft {
            body_mode: BodyMode::Raw,
            body: r#"{"a":1}"#.into(),
            ..Default::default()
        };
        let mut log = vec![];
        prepare_raw_body_for_send(&mut d, &mut log).unwrap();
        assert!(has_header(&d, "Content-Type"));
    }
}
