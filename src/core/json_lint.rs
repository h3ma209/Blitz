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

/// Beautify JSON with 2-space indentation while preserving Blitz template variables.
pub fn beautify_json_body(raw: &str) -> Option<String> {
    let clean = crate::runtime::postman_defaults::strip_json_comments(raw);
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&clean) {
        return serde_json::to_string_pretty(&val).ok();
    }
    if clean.contains("{{") {
        let mut placeholders: Vec<(String, String)> = Vec::new();
        let mut substituted = String::with_capacity(clean.len());
        let mut cursor = 0;
        let mut counter = 0;
        while let Some(open) = clean[cursor..].find("{{") {
            let start = cursor + open;
            substituted.push_str(&clean[cursor..start]);
            if let Some(close) = clean[start..].find("}}") {
                let end = start + close + 2;
                let var_expr = &clean[start..end];
                let before = substituted.trim_end();
                if before.ends_with('"') {
                    let k_inner = format!("__BLITZ_VAR_{}__", counter);
                    counter += 1;
                    placeholders.push((k_inner.clone(), var_expr.to_string()));
                    substituted.push_str(&k_inner);
                } else {
                    let key = format!("\"__BLITZ_VAR_{}__\"", counter);
                    counter += 1;
                    placeholders.push((key.clone(), var_expr.to_string()));
                    substituted.push_str(&key);
                }
                cursor = end;
            } else {
                substituted.push_str(&clean[start..]);
                cursor = clean.len();
                break;
            }
        }
        substituted.push_str(&clean[cursor..]);
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&substituted) {
            if let Ok(mut pretty) = serde_json::to_string_pretty(&val) {
                for (placeholder, original) in placeholders {
                    pretty = pretty.replace(&placeholder, &original);
                }
                return Some(pretty);
            }
        }
    }
    None
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
}
