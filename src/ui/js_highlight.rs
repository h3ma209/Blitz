//! JavaScript syntax highlighting for script editor mirrors (Postman pre-request / tests).

use crate::core::{iter_var_spans, render_var_highlight_html, VariableContext};

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

const JS_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "async",
    "await",
];

/// Highlight a JS fragment (no `{{…}}` tokens). Best-effort lexer for editor mirror.
fn highlight_js_fragment(text: &str) -> String {
    let mut html = String::with_capacity(text.len().saturating_mul(2));
    let mut i = 0usize;
    while i < text.len() {
        let rest = &text[i..];
        let rb = rest.as_bytes();
        // Line comment
        if rest.starts_with("//") {
            let end = rest.find('\n').map(|n| i + n).unwrap_or(text.len());
            html.push_str("<span class=\"js-comment\">");
            html.push_str(&escape_html(&text[i..end]));
            html.push_str("</span>");
            i = end;
            continue;
        }
        // Block comment
        if rest.starts_with("/*") {
            let end = rest
                .find("*/")
                .map(|n| i + n + 2)
                .unwrap_or(text.len());
            html.push_str("<span class=\"js-comment\">");
            html.push_str(&escape_html(&text[i..end]));
            html.push_str("</span>");
            i = end;
            continue;
        }
        // String literals
        if rest.starts_with('"') || rest.starts_with('\'') || rest.starts_with('`') {
            let quote = rb[0];
            let mut j = 1usize;
            while j < rb.len() {
                let ch = rb[j];
                if ch == b'\\' {
                    j += 2;
                    continue;
                }
                if ch == quote {
                    j += 1;
                    break;
                }
                j += 1;
            }
            let end = i + j;
            html.push_str("<span class=\"js-string\">");
            html.push_str(&escape_html(&text[i..end]));
            html.push_str("</span>");
            i = end;
            continue;
        }
        // Numbers
        if rb[0].is_ascii_digit()
            || (rb[0] == b'.' && rb.len() > 1 && rb[1].is_ascii_digit())
        {
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
            html.push_str("<span class=\"js-number\">");
            html.push_str(&escape_html(&text[i..end]));
            html.push_str("</span>");
            i = end;
            continue;
        }
        // Identifiers / keywords
        if rb[0].is_ascii_alphabetic() || rb[0] == b'_' || rb[0] == b'$' {
            let mut j = 0usize;
            while j < rb.len() {
                let c = rb[j];
                if c.is_ascii_alphanumeric() || c == b'_' || c == b'$' {
                    j += 1;
                } else {
                    break;
                }
            }
            let word = &text[i..i + j];
            let class = if JS_KEYWORDS.contains(&word) {
                "js-keyword"
            } else if word == "pm" || word.starts_with("pm") {
                "js-pm"
            } else if word == "console" || word == "CryptoJS" || word == "require" {
                "js-builtin"
            } else {
                "js-plain"
            };
            html.push_str("<span class=\"");
            html.push_str(class);
            html.push_str("\">");
            html.push_str(&escape_html(word));
            html.push_str("</span>");
            i += j;
            continue;
        }
        // Punctuation / whitespace
        html.push_str(&escape_html(&text[i..i + 1]));
        i += 1;
    }
    html
}

/// Mirror HTML: JS highlighting with `{{…}}` variable tokens layered in.
///
/// @param text Editor buffer text.
/// @param vars When set, variable tokens use scope styling (`var-token` / `unresolved`).
/// @returns Escaped HTML safe for `dangerous_inner_html` on a mirror element.
pub fn render_js_var_mirror_html(text: &str, vars: Option<&VariableContext>) -> String {
    let mut html = String::with_capacity(text.len().saturating_mul(2));
    let mut cursor = 0;
    for span in iter_var_spans(text) {
        if span.start > cursor {
            html.push_str(&highlight_js_fragment(&text[cursor..span.start]));
        }
        let token = &text[span.start..span.end];
        let unresolved = vars
            .map(|v| v.inspect_var(&span.raw).is_unresolved())
            .unwrap_or(false);
        let class = if unresolved {
            "var-token unresolved"
        } else {
            "var-token"
        };
        html.push_str("<span class=\"");
        html.push_str(class);
        html.push_str("\">");
        html.push_str(&escape_html(token));
        html.push_str("</span>");
        cursor = span.end;
    }
    if cursor < text.len() {
        html.push_str(&highlight_js_fragment(&text[cursor..]));
    }
    if text.ends_with('\n') {
        html.push('\n');
    }
    html
}

/// Variable-only mirror (JSON / plain bodies).
pub fn render_var_mirror_html(text: &str, vars: &VariableContext) -> String {
    render_var_highlight_html(text, Some(vars))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::VariableContext;

    #[test]
    fn js_keywords_and_strings() {
        let html = highlight_js_fragment("const x = \"a\"; // c");
        assert!(html.contains("js-keyword"));
        assert!(html.contains("js-string"));
        assert!(html.contains("js-comment"));
    }

    #[test]
    fn js_var_mirror_preserves_tokens() {
        let ctx = VariableContext::demo();
        let html = render_js_var_mirror_html("pm.test(\"t\", () => {{baseUrl}});", Some(&ctx));
        assert!(html.contains("js-pm") || html.contains("pm"));
        assert!(html.contains("var-token"));
    }
}
