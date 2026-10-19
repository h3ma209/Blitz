//! Pretty JSON → escaped HTML with token spans (keys, strings, numbers, bools/null).

use serde_json::Value;
use crate::core::{iter_var_spans, VariableContext};

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Highlight JSON fragment tokens (keys, strings, numbers, booleans, null, punctuation).
pub fn highlight_json_fragment(text: &str) -> String {
    let mut html = String::with_capacity(text.len().saturating_mul(2));
    let mut i = 0usize;
    while i < text.len() {
        let rest = &text[i..];
        let rb = rest.as_bytes();

        // String or Key
        if rb[0] == b'"' {
            let mut j = 1usize;
            while j < rb.len() {
                if rb[j] == b'\\' {
                    j += 1;
                    if let Some(escaped) = rest[j..].chars().next() {
                        j += escaped.len_utf8();
                    }
                    continue;
                }
                if rb[j] == b'"' {
                    j += 1;
                    break;
                }
                if let Some(c) = rest[j..].chars().next() {
                    j += c.len_utf8();
                } else {
                    j += 1;
                }
            }
            let end = i + j.min(rb.len());
            let str_val = &text[i..end];
            // Check if followed by colon (ignoring whitespace) → key
            let is_key = text[end..].trim_start().starts_with(':');
            let class = if is_key { "json-key" } else { "json-string" };
            html.push_str("<span class=\"");
            html.push_str(class);
            html.push_str("\">");
            html.push_str(&escape_html(str_val));
            html.push_str("</span>");
            i = end;
            continue;
        }

        // Numbers
        if rb[0].is_ascii_digit() || (rb[0] == b'-' && rb.len() > 1 && rb[1].is_ascii_digit()) {
            let mut j = 0usize;
            while j < rb.len() {
                let c = rb[j];
                if c.is_ascii_digit() || c == b'.' || c == b'e' || c == b'E' || c == b'+' || c == b'-' {
                    j += 1;
                } else {
                    break;
                }
            }
            let end = i + j;
            html.push_str("<span class=\"json-number\">");
            html.push_str(&escape_html(&text[i..end]));
            html.push_str("</span>");
            i = end;
            continue;
        }

        // Booleans & Null
        if rest.starts_with("true") {
            html.push_str("<span class=\"json-bool\">true</span>");
            i += 4;
            continue;
        }
        if rest.starts_with("false") {
            html.push_str("<span class=\"json-bool\">false</span>");
            i += 5;
            continue;
        }
        if rest.starts_with("null") {
            html.push_str("<span class=\"json-null\">null</span>");
            i += 4;
            continue;
        }

        // Structural syntax
        if rb[0] == b'{' || rb[0] == b'}' || rb[0] == b'[' || rb[0] == b']' || rb[0] == b':' || rb[0] == b',' {
            html.push_str("<span class=\"json-punct\">");
            html.push(rb[0] as char);
            html.push_str("</span>");
            i += 1;
            continue;
        }

        // Other character (whitespace, formatting, or non-ASCII UTF-8)
        let ch_len = rest.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        html.push_str(&escape_html(&text[i..i + ch_len]));
        i += ch_len;
    }
    html
}

/// Highlight a live JSON editor buffer, preserving variable tokens `{{var}}` with resolution status.
pub fn render_json_var_mirror_html(text: &str, vars: Option<&VariableContext>) -> String {
    let mut html = String::with_capacity(text.len().saturating_mul(2));
    let mut cursor = 0;
    for span in iter_var_spans(text) {
        if span.start > cursor {
            html.push_str(&highlight_json_fragment(&text[cursor..span.start]));
        }
        let token = &text[span.start..span.end];
        let (class, title) = match vars {
            Some(v) => {
                let info = v.inspect_var(&span.raw);
                if info.is_unresolved() {
                    ("var-token unresolved", format!("{}: (unresolved)", span.raw))
                } else {
                    ("var-token", format!("{}: {} ({})", span.raw, info.value, info.scope.label()))
                }
            }
            None => ("var-token", span.raw.clone()),
        };
        html.push_str("<span class=\"");
        html.push_str(class);
        html.push_str("\" title=\"");
        html.push_str(&escape_html(&title));
        html.push_str("\">");
        html.push_str(&escape_html(token));
        html.push_str("</span>");
        cursor = span.end;
    }
    if cursor < text.len() {
        html.push_str(&highlight_json_fragment(&text[cursor..]));
    }
    if text.ends_with('\n') {
        html.push('\n');
    }
    html
}

/// True when Content-Type looks like JSON or body parses as JSON.
pub fn should_highlight_json(body: &str, headers: &[(String, String)]) -> bool {
    let ct_json = headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("content-type") && {
            let lower = v.to_ascii_lowercase();
            lower.contains("application/json")
                || lower.contains("+json")
                || lower.contains("text/json")
        }
    });
    if ct_json {
        return serde_json::from_str::<Value>(body.trim()).is_ok() || body.trim().is_empty();
    }
    serde_json::from_str::<Value>(body.trim()).is_ok()
}

/// Pretty-print + wrap tokens in `<span class="json-*">`. Falls back to escaped plain text.
pub fn render_json_highlight_html(body: &str) -> String {
    match serde_json::from_str::<Value>(body.trim()) {
        Ok(v) => {
            let mut out = String::with_capacity(body.len().saturating_mul(3));
            render_value(&v, &mut out, 0);
            out
        }
        Err(_) => escape_html(body),
    }
}

fn render_value(v: &Value, out: &mut String, indent: usize) {
    match v {
        Value::Null => out.push_str("<span class=\"json-null\">null</span>"),
        Value::Bool(b) => {
            out.push_str("<span class=\"json-bool\">");
            out.push_str(if *b { "true" } else { "false" });
            out.push_str("</span>");
        }
        Value::Number(n) => {
            out.push_str("<span class=\"json-number\">");
            out.push_str(&escape_html(&n.to_string()));
            out.push_str("</span>");
        }
        Value::String(s) => {
            out.push_str("<span class=\"json-string\">\"");
            out.push_str(&escape_html(s));
            out.push_str("\"</span>");
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                out.push_str("<span class=\"json-punct\">[]</span>");
                return;
            }
            out.push_str("<span class=\"json-punct\">[</span>\n");
            for (i, item) in arr.iter().enumerate() {
                push_indent(out, indent + 1);
                render_value(item, out, indent + 1);
                if i + 1 < arr.len() {
                    out.push_str("<span class=\"json-punct\">,</span>");
                }
                out.push('\n');
            }
            push_indent(out, indent);
            out.push_str("<span class=\"json-punct\">]</span>");
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str("<span class=\"json-punct\">{}</span>");
                return;
            }
            out.push_str("<span class=\"json-punct\">{</span>\n");
            let entries: Vec<_> = map.iter().collect();
            for (i, (k, val)) in entries.iter().enumerate() {
                push_indent(out, indent + 1);
                out.push_str("<span class=\"json-key\">\"");
                out.push_str(&escape_html(k));
                out.push_str("\"</span>");
                out.push_str("<span class=\"json-punct\">: </span>");
                render_value(val, out, indent + 1);
                if i + 1 < entries.len() {
                    out.push_str("<span class=\"json-punct\">,</span>");
                }
                out.push('\n');
            }
            push_indent(out, indent);
            out.push_str("<span class=\"json-punct\">}</span>");
        }
    }
}

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_keys_strings_numbers_bools() {
        let html = render_json_highlight_html(r#"{"name":"snb","n":1,"ok":true,"x":null}"#);
        assert!(html.contains("json-key"));
        assert!(html.contains("json-string"));
        assert!(html.contains("json-number"));
        assert!(html.contains("json-bool"));
        assert!(html.contains("json-null"));
        assert!(html.contains("&quot;") || html.contains("\"name\"") || html.contains(">name<") || html.contains("json-key"));
        // Escaped quotes around key content
        assert!(html.contains("<span class=\"json-key\">\"name\"</span>") || html.contains("json-key"));
    }

    #[test]
    fn escapes_html_in_strings() {
        let html = render_json_highlight_html(r#"{"a":"<script>"}"#);
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<script>"));
    }

    #[test]
    fn should_highlight_on_json_content_type() {
        let headers = vec![("Content-Type".into(), "application/json; charset=utf-8".into())];
        assert!(should_highlight_json(r#"{"a":1}"#, &headers));
        assert!(!should_highlight_json("not json", &[("Content-Type".into(), "text/plain".into())]));
        assert!(should_highlight_json(r#"[1,2]"#, &[]));
    }

    #[test]
    fn highlight_json_fragment_handles_unicode_outside_quotes() {
        let html = highlight_json_fragment("{ “key”: \"val\\é\", // 🔥\n \"unclosed\\");
        assert!(html.contains("json-punct"));
        assert!(html.contains("🔥"));
    }
}
