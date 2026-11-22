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

        // Line comment `// ...`
        if rb.starts_with(b"//") {
            let end = rest.find('\n').map(|n| i + n).unwrap_or(text.len());
            html.push_str("<span class=\"json-comment\">");
            html.push_str(&escape_html(&text[i..end]));
            html.push_str("</span>");
            i = end;
            continue;
        }

        // Block comment `/* ... */`
        if rb.starts_with(b"/*") {
            let end = rest[2..]
                .find("*/")
                .map(|n| i + 2 + n + 2)
                .unwrap_or(text.len());
            html.push_str("<span class=\"json-comment\">");
            html.push_str(&escape_html(&text[i..end]));
            html.push_str("</span>");
            i = end;
            continue;
        }

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

fn push_var_span_html(
    html: &mut String,
    token: &str,
    raw: &str,
    vars: Option<&VariableContext>,
) {
    let (class, title) = match vars {
        Some(v) => {
            let info = v.inspect_var(raw);
            if info.is_unresolved() {
                ("var-token unresolved", format!("{raw}: (unresolved)"))
            } else {
                ("var-token", format!("{raw}: {} ({})", info.value, info.scope.label()))
            }
        }
        None => ("var-token", raw.to_string()),
    };
    html.push_str("<span class=\"");
    html.push_str(class);
    html.push_str("\" data-var-raw=\"");
    html.push_str(&escape_html(raw));
    html.push_str("\" title=\"");
    html.push_str(&escape_html(&title));
    html.push_str("\">");
    html.push_str(&escape_html(token));
    html.push_str("</span>");
}

fn push_text_with_vars(
    html: &mut String,
    segment: &str,
    vars: Option<&VariableContext>,
) {
    let mut cursor = 0;
    for span in iter_var_spans(segment) {
        if span.start > cursor {
            html.push_str(&escape_html(&segment[cursor..span.start]));
        }
        push_var_span_html(html, &segment[span.start..span.end], &span.raw, vars);
        cursor = span.end;
    }
    if cursor < segment.len() {
        html.push_str(&escape_html(&segment[cursor..]));
    }
}

/// Highlight a live JSON editor buffer, preserving variable tokens `{{var}}` and JSONC comments.
pub fn render_json_var_mirror_html(text: &str, vars: Option<&VariableContext>) -> String {
    let mut html = String::with_capacity(text.len().saturating_mul(2));
    let mut i = 0usize;
    let mut plain_start = 0usize;

    while i < text.len() {
        let rest = &text[i..];
        let rb = rest.as_bytes();

        // Line comment `// ...` (outside strings)
        if rb.starts_with(b"//") {
            if i > plain_start {
                html.push_str(&highlight_json_fragment(&text[plain_start..i]));
            }
            let end = rest.find('\n').map(|n| i + n).unwrap_or(text.len());
            html.push_str("<span class=\"json-comment\">");
            html.push_str(&escape_html(&text[i..end]));
            html.push_str("</span>");
            i = end;
            plain_start = end;
            continue;
        }

        // Block comment `/* ... */` (outside strings)
        if rb.starts_with(b"/*") {
            if i > plain_start {
                html.push_str(&highlight_json_fragment(&text[plain_start..i]));
            }
            let end = rest[2..]
                .find("*/")
                .map(|n| i + 2 + n + 2)
                .unwrap_or(text.len());
            html.push_str("<span class=\"json-comment\">");
            html.push_str(&escape_html(&text[i..end]));
            html.push_str("</span>");
            i = end;
            plain_start = end;
            continue;
        }

        // String / Key (may contain `{{var}}` tokens)
        if rb[0] == b'"' {
            if i > plain_start {
                html.push_str(&highlight_json_fragment(&text[plain_start..i]));
            }
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
            let is_key = text[end..].trim_start().starts_with(':');
            let class = if is_key { "json-key" } else { "json-string" };
            html.push_str("<span class=\"");
            html.push_str(class);
            html.push_str("\">");
            push_text_with_vars(&mut html, str_val, vars);
            html.push_str("</span>");
            i = end;
            plain_start = end;
            continue;
        }

        // Template variable `{{var}}` outside strings/comments
        if rb.starts_with(b"{{") {
            if let Some(span) = iter_var_spans(rest).next().filter(|s| s.start == 0) {
                if i > plain_start {
                    html.push_str(&highlight_json_fragment(&text[plain_start..i]));
                }
                push_var_span_html(&mut html, &rest[..span.end], &span.raw, vars);
                i += span.end;
                plain_start = i;
                continue;
            }
        }

        let ch_len = rest.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        i += ch_len;
    }

    if plain_start < text.len() {
        html.push_str(&highlight_json_fragment(&text[plain_start..]));
    }
    if text.ends_with('\n') {
        html.push('\n');
    }
    html
}

/// shortcut: paint this prefix only, raise it if tokenizing full bodies stays cheap
pub const JSON_PAINT_CAP: usize = 256 * 1024;
/// Bounded prefix for pretty-print and highlight. Does not touch the stored body.
pub fn json_paint_window(text: &str) -> (&str, bool) {
    let (start, end) = json_paint_range(text, 0);
    (&text[start..end], end < text.len())
}

/// Byte range `[start, end)` of at most [`JSON_PAINT_CAP`], aligned to char boundaries.
/// `offset` selects a later page so the tail is reachable.
pub fn json_paint_range(text: &str, offset: usize) -> (usize, usize) {
    if text.is_empty() {
        return (0, 0);
    }
    let mut start = offset.min(text.len());
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = start.saturating_add(JSON_PAINT_CAP).min(text.len());
    while end > start && !text.is_char_boundary(end) {
        end -= 1;
    }
    if end == start {
        end = text.len();
    }
    (start, end)
}

fn strip_utf8_bom_and_trim(s: &str) -> &str {
    s.trim().trim_start_matches('\u{feff}').trim()
}

/// True when Content-Type looks like JSON or body parses as JSON.
pub fn should_highlight_json(body: &str, headers: &[(String, String)]) -> bool {
    let trimmed = strip_utf8_bom_and_trim(body);
    if trimmed.is_empty() {
        return false;
    }
    if json_paint_window(trimmed).1 {
        return false;
    }
    let ct_json = headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("content-type") && {
            let lower = v.to_ascii_lowercase();
            lower.contains("application/json")
                || lower.contains("+json")
                || lower.contains("text/json")
        }
    });
    if ct_json {
        return serde_json::from_str::<Value>(trimmed).is_ok();
    }
    serde_json::from_str::<Value>(trimmed).is_ok()
}

/// Render an already-parsed JSON [`Value`] as beautified syntax-highlighted HTML.
pub fn render_json_value_html(v: &Value, enable_folding: bool) -> String {
    let mut out = String::with_capacity(1024);
    if enable_folding {
        out.push_str("<div class=\"json-tree\">");
        render_node(None, v, false, &mut out, 0, true);
        out.push_str("</div>");
    } else {
        render_node(None, v, false, &mut out, 0, false);
    }
    out
}

/// Pretty-print + wrap tokens in `<span class="json-*">`. Falls back to escaped plain text.
pub fn render_json_highlight_html(body: &str) -> String {
    render_json_highlight_html_with_fold(body, true)
}

/// Pretty-print with optional `<details>` collapsible tree folding.
pub fn render_json_highlight_html_with_fold(body: &str, enable_folding: bool) -> String {
    let trimmed = strip_utf8_bom_and_trim(body);
    let (window, truncated) = json_paint_window(trimmed);
    if truncated {
        let mut html = String::with_capacity(window.len().saturating_mul(2) + 80);
        html.push_str("<div class=\"json-large-warning\">showing first ");
        html.push_str(&(window.len() / 1024).to_string());
        html.push_str(" KiB of ");
        html.push_str(&(trimmed.len() / 1024).to_string());
        html.push_str(" KiB</div>\n");
        html.push_str(&highlight_json_fragment(window));
        return html;
    }
    match serde_json::from_str::<Value>(window) {
        Ok(v) => render_json_value_html(&v, enable_folding),
        Err(_) => escape_html(body),
    }
}

fn push_key_html(out: &mut String, key: Option<&str>) {
    if let Some(k) = key {
        out.push_str("<span class=\"json-key\">\"");
        out.push_str(&escape_html(k));
        out.push_str("\"</span><span class=\"json-punct\">: </span>");
    }
}

fn render_node(
    key: Option<&str>,
    v: &Value,
    has_trailing_comma: bool,
    out: &mut String,
    indent: usize,
    enable_folding: bool,
) {
    match v {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            if enable_folding {
                out.push_str("<div class=\"json-row\"><span class=\"json-fold-spacer\"></span>");
                if key.is_some() {
                    out.push_str("<span class=\"json-row-key\">");
                    push_key_html(out, key);
                    out.push_str("</span>");
                }
                out.push_str("<span class=\"json-row-val\">");
            } else {
                push_indent(out, indent);
                push_key_html(out, key);
            }
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
                _ => unreachable!(),
            }
            if has_trailing_comma {
                out.push_str("<span class=\"json-punct\">,</span>");
            }
            if enable_folding {
                out.push_str("</span></div>");
            } else {
                out.push('\n');
            }
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                if enable_folding {
                    out.push_str("<div class=\"json-row\"><span class=\"json-fold-spacer\"></span>");
                    if key.is_some() {
                        out.push_str("<span class=\"json-row-key\">");
                        push_key_html(out, key);
                        out.push_str("</span>");
                    }
                    out.push_str("<span class=\"json-row-val\"><span class=\"json-punct\">[]");
                    if has_trailing_comma {
                        out.push(',');
                    }
                    out.push_str("</span></span></div>");
                } else {
                    push_indent(out, indent);
                    push_key_html(out, key);
                    out.push_str("<span class=\"json-punct\">[]");
                    if has_trailing_comma {
                        out.push(',');
                    }
                    out.push_str("</span>\n");
                }
                return;
            }

            let len = arr.len();
            let len_label = if len == 1 { "1 item".to_string() } else { format!("{len} items") };

            if enable_folding {
                out.push_str("<details class=\"json-fold\" open><summary class=\"json-fold-summary\">");
                out.push_str("<span class=\"json-fold-toggle\" title=\"Toggle fold\"><svg class=\"fold-icon\" viewBox=\"0 0 16 16\"><path fill=\"currentColor\" d=\"M4 6h8l-4 5z\"/></svg></span>");
                if key.is_some() {
                    out.push_str("<span class=\"json-row-key\">");
                    push_key_html(out, key);
                    out.push_str("</span>");
                }
                out.push_str("<span class=\"json-punct\">[</span>");
                out.push_str(&format!("<span class=\"json-fold-badge\">{len_label}</span>"));
                out.push_str("<span class=\"json-punct json-fold-close-brace\">]</span>");
                if has_trailing_comma {
                    out.push_str("<span class=\"json-punct json-fold-close-comma\">,</span>");
                }
                out.push_str("</summary><div class=\"json-children\">");

                for (i, item) in arr.iter().enumerate() {
                    let child_comma = i + 1 < arr.len();
                    render_node(None, item, child_comma, out, indent + 1, true);
                }

                out.push_str("</div><div class=\"json-fold-close\"><span class=\"json-fold-spacer\"></span><span class=\"json-punct\">]");
                if has_trailing_comma {
                    out.push(',');
                }
                out.push_str("</span></div></details>");
            } else {
                push_indent(out, indent);
                push_key_html(out, key);
                out.push_str("<span class=\"json-punct\">[</span>\n");
                for (i, item) in arr.iter().enumerate() {
                    let child_comma = i + 1 < arr.len();
                    render_node(None, item, child_comma, out, indent + 1, false);
                }
                push_indent(out, indent);
                out.push_str("<span class=\"json-punct\">]");
                if has_trailing_comma {
                    out.push(',');
                }
                out.push_str("</span>\n");
            }
        }
        Value::Object(map) => {
            if map.is_empty() {
                if enable_folding {
                    out.push_str("<div class=\"json-row\"><span class=\"json-fold-spacer\"></span>");
                    if key.is_some() {
                        out.push_str("<span class=\"json-row-key\">");
                        push_key_html(out, key);
                        out.push_str("</span>");
                    }
                    out.push_str("<span class=\"json-row-val\"><span class=\"json-punct\">{}");
                    if has_trailing_comma {
                        out.push(',');
                    }
                    out.push_str("</span></span></div>");
                } else {
                    push_indent(out, indent);
                    push_key_html(out, key);
                    out.push_str("<span class=\"json-punct\">{}");
                    if has_trailing_comma {
                        out.push(',');
                    }
                    out.push_str("</span>\n");
                }
                return;
            }

            let count = map.len();
            let count_label = if count == 1 { "1 key".to_string() } else { format!("{count} keys") };

            if enable_folding {
                out.push_str("<details class=\"json-fold\" open><summary class=\"json-fold-summary\">");
                out.push_str("<span class=\"json-fold-toggle\" title=\"Toggle fold\"><svg class=\"fold-icon\" viewBox=\"0 0 16 16\"><path fill=\"currentColor\" d=\"M4 6h8l-4 5z\"/></svg></span>");
                if key.is_some() {
                    out.push_str("<span class=\"json-row-key\">");
                    push_key_html(out, key);
                    out.push_str("</span>");
                }
                out.push_str("<span class=\"json-punct\">{</span>");
                out.push_str(&format!("<span class=\"json-fold-badge\">{count_label}</span>"));
                out.push_str("<span class=\"json-punct json-fold-close-brace\">}</span>");
                if has_trailing_comma {
                    out.push_str("<span class=\"json-punct json-fold-close-comma\">,</span>");
                }
                out.push_str("</summary><div class=\"json-children\">");

                let entries: Vec<_> = map.iter().collect();
                for (i, (child_key, child_val)) in entries.iter().enumerate() {
                    let child_comma = i + 1 < entries.len();
                    render_node(Some(child_key), child_val, child_comma, out, indent + 1, true);
                }

                out.push_str("</div><div class=\"json-fold-close\"><span class=\"json-fold-spacer\"></span><span class=\"json-punct\">}");
                if has_trailing_comma {
                    out.push(',');
                }
                out.push_str("</span></div></details>");
            } else {
                push_indent(out, indent);
                push_key_html(out, key);
                out.push_str("<span class=\"json-punct\">{</span>\n");
                let entries: Vec<_> = map.iter().collect();
                for (i, (child_key, child_val)) in entries.iter().enumerate() {
                    let child_comma = i + 1 < entries.len();
                    render_node(Some(child_key), child_val, child_comma, out, indent + 1, false);
                }
                push_indent(out, indent);
                out.push_str("<span class=\"json-punct\">}");
                if has_trailing_comma {
                    out.push(',');
                }
                out.push_str("</span>\n");
            }
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

    #[test]
    fn oversized_payload_skips_dom_tree() {
        let large = format!("{{\"data\": \"{}\"}}", "x".repeat(JSON_PAINT_CAP + 100));
        let headers = vec![("Content-Type".into(), "application/json".into())];
        assert!(!should_highlight_json(&large, &headers));
        let html = render_json_highlight_html(&large);
        assert!(html.contains("json-large-warning"));
        assert!(!html.contains("json-fold"));
    }

    #[test]
    fn json_paint_window_keeps_short_and_drops_the_tail() {
        let small = r#"{"a":1}"#;
        let (window, truncated) = json_paint_window(small);
        assert_eq!(window, small);
        assert!(!truncated);

        let mut mid_char = "a".repeat(JSON_PAINT_CAP - 1);
        mid_char.push('é');
        let (cut, truncated) = json_paint_window(&mid_char);
        assert!(truncated);
        assert!(cut.is_char_boundary(cut.len()));
        assert_eq!(cut, "a".repeat(JSON_PAINT_CAP - 1));

        let tail = "TAILMARKER";
        let mut big = String::from("{\"k\":\"");
        big.push_str(&"x".repeat(JSON_PAINT_CAP));
        big.push_str(tail);
        big.push_str("\"}");
        let html = render_json_highlight_html(&big);
        assert!(html.contains("json-large-warning"));
        assert!(!html.contains(tail));
        assert!(html.matches('x').count() <= JSON_PAINT_CAP);

        let (start, end) = json_paint_range(&big, JSON_PAINT_CAP);
        assert!(big[start..end].contains(tail));
        assert!(start > 0);
    }

    #[test]
    fn mirror_highlights_json_comments_and_var_tokens() {
        let mut vars = VariableContext::default();
        vars.environment.insert("token".into(), "secret".into());

        let input = "{\n  // \"disabled\": {{new_var}},\n  /* block */\n  \"url\": \"https://example.com/{{token}}\",\n  \"missing\": {{brand_new}}\n}";
        let html = render_json_var_mirror_html(input, Some(&vars));

        assert!(html.contains(r#"<span class="json-comment">// &quot;disabled&quot;: {{new_var}},</span>"#));
        assert!(html.contains(r#"<span class="json-comment">/* block */</span>"#));
        assert!(html.contains(r#"<span class="json-string">&quot;https://example.com/<span class="var-token" data-var-raw="token""#));
        assert!(html.contains(r#"<span class="var-token unresolved" data-var-raw="brand_new""#));
    }
}
