//! JSON linting and beautification with support for embedded Blitz variables and comments.

/// Result of linting a JSON body: Ok(()) if valid (or empty),
/// Err((line, col, clean_message)) if invalid.
pub fn lint_json_body(raw: &str) -> Result<(), (usize, usize, String)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let clean = crate::runtime::postman_defaults::strip_json_comments(trimmed);

    // 1. Direct parse check
    if serde_json::from_str::<serde_json::Value>(&clean).is_ok() {
        return Ok(());
    }

    // 2. If it contains template variables `{{...}}`, mask them with null or string
    if clean.contains("{{") {
        let masked = mask_template_variables(&clean);
        if serde_json::from_str::<serde_json::Value>(&masked).is_ok() {
            return Ok(());
        }
    }

    // 3. Extract exact line, column, and descriptive message
    match serde_json::from_str::<serde_json::Value>(&clean) {
        Ok(_) => Ok(()),
        Err(e) => {
            let line = e.line();
            let col = e.column();
            let raw_msg = e.to_string();
            let clean_msg = raw_msg
                .split(" at line")
                .next()
                .unwrap_or(&raw_msg)
                .trim()
                .to_string();
            Err((line, col, clean_msg))
        }
    }
}

/// Mask `{{...}}` with `null` (or `"__VAR__"` if already inside a string) for JSON validation.
fn mask_template_variables(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(open) = text[cursor..].find("{{") {
        let start = cursor + open;
        out.push_str(&text[cursor..start]);
        if let Some(close) = text[start..].find("}}") {
            let end = start + close + 2;
            let before = out.trim_end();
            if before.ends_with('"') {
                out.push_str("__VAR__");
            } else {
                out.push_str("null");
            }
            cursor = end;
        } else {
            out.push_str(&text[start..]);
            cursor = text.len();
            break;
        }
    }
    out.push_str(&text[cursor..]);
    out
}

/// Beautify JSON with 2-space indentation while preserving Blitz template variables and comments.
pub fn beautify_json_body(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if lint_json_body(trimmed).is_err() {
        return None;
    }
    let tokens = tokenize(trimmed)?;
    Some(format_tokens(&tokens))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TokenKind {
    OpenBrace,
    CloseBrace,
    OpenBracket,
    CloseBracket,
    Colon,
    Comma,
    StringLit(String),
    TemplateVar(String),
    Literal(String),
    LineComment(String),
    BlockComment(String),
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    leading_newlines: usize,
}

fn tokenize(raw: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    let mut tokens = Vec::new();
    let mut leading_newlines = 0;

    while i < chars.len() {
        let c = chars[i];
        match c {
            '\n' => {
                leading_newlines += 1;
                i += 1;
            }
            '\r' | ' ' | '\t' => {
                i += 1;
            }
            '/' => {
                if i + 1 < chars.len() && chars[i + 1] == '/' {
                    let start = i;
                    i += 2;
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                    let mut end = i;
                    if end > start && chars[end - 1] == '\r' {
                        end -= 1;
                    }
                    let comment: String = chars[start..end].iter().collect();
                    tokens.push(Token {
                        kind: TokenKind::LineComment(comment),
                        leading_newlines,
                    });
                    leading_newlines = 0;
                } else if i + 1 < chars.len() && chars[i + 1] == '*' {
                    let start = i;
                    i += 2;
                    let mut closed = false;
                    while i + 1 < chars.len() {
                        if chars[i] == '*' && chars[i + 1] == '/' {
                            i += 2;
                            closed = true;
                            break;
                        }
                        i += 1;
                    }
                    if !closed {
                        return None;
                    }
                    let comment: String = chars[start..i].iter().collect();
                    tokens.push(Token {
                        kind: TokenKind::BlockComment(comment),
                        leading_newlines,
                    });
                    leading_newlines = 0;
                } else {
                    return None;
                }
            }
            '{' => {
                if i + 1 < chars.len() && chars[i + 1] == '{' {
                    let start = i;
                    i += 2;
                    let mut closed = false;
                    while i + 1 < chars.len() {
                        if chars[i] == '}' && chars[i + 1] == '}' {
                            i += 2;
                            closed = true;
                            break;
                        }
                        i += 1;
                    }
                    if !closed {
                        return None;
                    }
                    let var_str: String = chars[start..i].iter().collect();
                    tokens.push(Token {
                        kind: TokenKind::TemplateVar(var_str),
                        leading_newlines,
                    });
                    leading_newlines = 0;
                } else {
                    tokens.push(Token {
                        kind: TokenKind::OpenBrace,
                        leading_newlines,
                    });
                    leading_newlines = 0;
                    i += 1;
                }
            }
            '}' => {
                tokens.push(Token {
                    kind: TokenKind::CloseBrace,
                    leading_newlines,
                });
                leading_newlines = 0;
                i += 1;
            }
            '[' => {
                tokens.push(Token {
                    kind: TokenKind::OpenBracket,
                    leading_newlines,
                });
                leading_newlines = 0;
                i += 1;
            }
            ']' => {
                tokens.push(Token {
                    kind: TokenKind::CloseBracket,
                    leading_newlines,
                });
                leading_newlines = 0;
                i += 1;
            }
            ':' => {
                tokens.push(Token {
                    kind: TokenKind::Colon,
                    leading_newlines,
                });
                leading_newlines = 0;
                i += 1;
            }
            ',' => {
                tokens.push(Token {
                    kind: TokenKind::Comma,
                    leading_newlines,
                });
                leading_newlines = 0;
                i += 1;
            }
            '"' => {
                let start = i;
                i += 1;
                let mut escape = false;
                let mut closed = false;
                while i < chars.len() {
                    let ch = chars[i];
                    if escape {
                        escape = false;
                    } else if ch == '\\' {
                        escape = true;
                    } else if ch == '"' {
                        i += 1;
                        closed = true;
                        break;
                    }
                    i += 1;
                }
                if !closed {
                    return None;
                }
                let str_val: String = chars[start..i].iter().collect();
                tokens.push(Token {
                    kind: TokenKind::StringLit(str_val),
                    leading_newlines,
                });
                leading_newlines = 0;
            }
            _ => {
                let start = i;
                while i < chars.len() {
                    let ch = chars[i];
                    if ch.is_whitespace()
                        || ch == ','
                        || ch == '}'
                        || ch == ']'
                        || ch == ':'
                        || ch == '/'
                        || ch == '{'
                    {
                        break;
                    }
                    i += 1;
                }
                let lit: String = chars[start..i].iter().collect();
                if lit.is_empty() {
                    return None;
                }
                tokens.push(Token {
                    kind: TokenKind::Literal(lit),
                    leading_newlines,
                });
                leading_newlines = 0;
            }
        }
    }
    Some(tokens)
}

fn is_comment(kind: &TokenKind) -> bool {
    matches!(kind, TokenKind::LineComment(_) | TokenKind::BlockComment(_))
}

fn write_indent(out: &mut String, indent_level: usize, at_line_start: &mut bool) {
    if *at_line_start {
        for _ in 0..indent_level {
            out.push_str("  ");
        }
        *at_line_start = false;
    }
}

fn write_block_comment(
    out: &mut String,
    comment: &str,
    indent_level: usize,
    at_line_start: &mut bool,
) {
    let lines: Vec<&str> = comment.lines().collect();
    if lines.len() <= 1 {
        out.push_str(comment);
        *at_line_start = false;
        return;
    }
    for (idx, line) in lines.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
            *at_line_start = true;
            write_indent(out, indent_level, at_line_start);
            let trimmed = line.trim_start();
            if trimmed.starts_with('*') {
                out.push(' ');
            }
            out.push_str(trimmed);
        } else {
            out.push_str(line);
        }
    }
    *at_line_start = false;
}

fn format_tokens(tokens: &[Token]) -> String {
    let mut out = String::new();
    let mut indent_level = 0usize;
    let mut at_line_start = true;
    let mut idx = 0;

    while idx < tokens.len() {
        let token = &tokens[idx];
        let next_token = tokens.get(idx + 1);

        if token.leading_newlines >= 2
            && !out.is_empty()
            && !out.ends_with("{\n")
            && !out.ends_with("[\n")
            && !out.ends_with("\n\n")
        {
            if !at_line_start {
                out.push('\n');
            }
            out.push('\n');
            at_line_start = true;
        }

        match &token.kind {
            TokenKind::OpenBrace => {
                if let Some(next) = next_token {
                    if matches!(next.kind, TokenKind::CloseBrace) {
                        write_indent(&mut out, indent_level, &mut at_line_start);
                        out.push_str("{}");
                        idx += 2;
                        continue;
                    }
                }

                write_indent(&mut out, indent_level, &mut at_line_start);
                out.push('{');
                indent_level += 1;

                if let Some(next) = next_token {
                    if is_comment(&next.kind) && next.leading_newlines == 0 {
                        // comment follows on same line
                    } else {
                        out.push('\n');
                        at_line_start = true;
                    }
                } else {
                    out.push('\n');
                    at_line_start = true;
                }
            }
            TokenKind::CloseBrace => {
                indent_level = indent_level.saturating_sub(1);
                if !at_line_start {
                    out.push('\n');
                    at_line_start = true;
                }
                write_indent(&mut out, indent_level, &mut at_line_start);
                out.push('}');
            }
            TokenKind::OpenBracket => {
                if let Some(next) = next_token {
                    if matches!(next.kind, TokenKind::CloseBracket) {
                        write_indent(&mut out, indent_level, &mut at_line_start);
                        out.push_str("[]");
                        idx += 2;
                        continue;
                    }
                }

                write_indent(&mut out, indent_level, &mut at_line_start);
                out.push('[');
                indent_level += 1;

                if let Some(next) = next_token {
                    if is_comment(&next.kind) && next.leading_newlines == 0 {
                        // comment follows on same line
                    } else {
                        out.push('\n');
                        at_line_start = true;
                    }
                } else {
                    out.push('\n');
                    at_line_start = true;
                }
            }
            TokenKind::CloseBracket => {
                indent_level = indent_level.saturating_sub(1);
                if !at_line_start {
                    out.push('\n');
                    at_line_start = true;
                }
                write_indent(&mut out, indent_level, &mut at_line_start);
                out.push(']');
            }
            TokenKind::Colon => {
                out.push_str(": ");
                at_line_start = false;
            }
            TokenKind::Comma => {
                out.push(',');
                if let Some(next) = next_token {
                    if is_comment(&next.kind) && next.leading_newlines == 0 {
                        // trailing comment on same line
                    } else {
                        out.push('\n');
                        at_line_start = true;
                    }
                } else {
                    out.push('\n');
                    at_line_start = true;
                }
            }
            TokenKind::StringLit(s) | TokenKind::TemplateVar(s) | TokenKind::Literal(s) => {
                if !at_line_start && !out.ends_with(' ') && !out.ends_with(':') {
                    out.push(' ');
                }
                write_indent(&mut out, indent_level, &mut at_line_start);
                out.push_str(s);
                at_line_start = false;
            }
            TokenKind::LineComment(comment) => {
                if token.leading_newlines == 0 && !at_line_start {
                    out.push(' ');
                    out.push_str(comment);
                    out.push('\n');
                    at_line_start = true;
                } else {
                    if !at_line_start {
                        out.push('\n');
                        at_line_start = true;
                    }
                    write_indent(&mut out, indent_level, &mut at_line_start);
                    out.push_str(comment);
                    out.push('\n');
                    at_line_start = true;
                }
            }
            TokenKind::BlockComment(comment) => {
                if token.leading_newlines == 0 && !at_line_start {
                    out.push(' ');
                    write_block_comment(&mut out, comment, indent_level, &mut at_line_start);
                    if let Some(next) = next_token {
                        if next.leading_newlines > 0 {
                            out.push('\n');
                            at_line_start = true;
                        }
                    }
                } else {
                    if !at_line_start {
                        out.push('\n');
                        at_line_start = true;
                    }
                    write_indent(&mut out, indent_level, &mut at_line_start);
                    write_block_comment(&mut out, comment, indent_level, &mut at_line_start);
                    if let Some(next) = next_token {
                        if !matches!(next.kind, TokenKind::Comma | TokenKind::Colon) {
                            out.push('\n');
                            at_line_start = true;
                        }
                    }
                }
            }
        }
        idx += 1;
    }

    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lint_json_valid_and_empty() {
        assert!(lint_json_body("").is_ok());
        assert!(lint_json_body("   ").is_ok());
        assert!(lint_json_body(r#"{"hello": "world"}"#).is_ok());
        assert!(lint_json_body(r#"// comment
        {"hello": "world"}"#).is_ok());
    }

    #[test]
    fn test_lint_json_with_variables() {
        // Unquoted variable as value
        assert!(lint_json_body(r#"{"id": {{user_id}}, "name": "test"}"#).is_ok());
        // Quoted variable
        assert!(lint_json_body(r#"{"token": "{{token}}"}"#).is_ok());
    }

    #[test]
    fn test_lint_json_errors() {
        let err = lint_json_body(r#"{"name": "test""#).unwrap_err();
        assert_eq!(err.0, 1);
        assert!(err.2.contains("EOF") || err.2.contains("expected"));
    }

    #[test]
    fn test_beautify_json() {
        let raw = r#"{"a":1,"b":{{var}}}"#;
        let formatted = beautify_json_body(raw).unwrap();
        assert!(formatted.contains("{\n  \"a\": 1,\n  \"b\": {{var}}\n}"));
    }

    #[test]
    fn test_beautify_json_multiple_in_string_variables() {
        let raw = r#"{"url":"{{host}}/api/{{version}}/resource"}"#;
        let formatted = beautify_json_body(raw).unwrap();
        assert_eq!(
            formatted,
            "{\n  \"url\": \"{{host}}/api/{{version}}/resource\"\n}"
        );
    }

    #[test]
    fn test_beautify_json_preserves_line_comments() {
        let raw = r#"// Header comment
{
  // User ID
  "id": 1, // Inline ID comment
  "name": "Blitz",
  // Trailing note before brace
  "nested": {
    "key": "val" // Nested inline
  }
}
// Footer comment"#;
        let formatted = beautify_json_body(raw).unwrap();
        assert_eq!(
            formatted,
            "// Header comment\n{\n  // User ID\n  \"id\": 1, // Inline ID comment\n  \"name\": \"Blitz\",\n  // Trailing note before brace\n  \"nested\": {\n    \"key\": \"val\" // Nested inline\n  }\n}\n// Footer comment"
        );
    }

    #[test]
    fn test_beautify_json_preserves_block_comments() {
        let raw = r#"{
  /* inline comment */ "key": /* value comment */ "value",
  /*
   * multiline
   * description
   */
  "active": true
}"#;
        let formatted = beautify_json_body(raw).unwrap();
        assert!(formatted.contains("/* inline comment */"));
        assert!(formatted.contains("/* value comment */ \"value\""));
        assert!(formatted.contains("/*\n   * multiline\n   * description\n   */"));
    }

    #[test]
    fn test_beautify_json_preserves_array_comments_and_empty() {
        let raw = r#"// Array test
[
  // First item
  {"a": {}},
  // Second item
  [1, 2], // inline comment
  []
]"#;
        let formatted = beautify_json_body(raw).unwrap();
        assert_eq!(
            formatted,
            "// Array test\n[\n  // First item\n  {\n    \"a\": {}\n  },\n  // Second item\n  [\n    1,\n    2\n  ], // inline comment\n  []\n]"
        );
    }
}
