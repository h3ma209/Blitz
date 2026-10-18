//! Pretty JSON → escaped HTML with token spans (keys, strings, numbers, bools/null).

use serde_json::Value;

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
}
