//! Lightweight JSONPath / expression query engine for filtering JSON responses.

use serde_json::Value;

/// Extract top-level keys if the value is an object, up to `limit` keys.
pub fn root_keys(value: &Value, limit: usize) -> Vec<String> {
    match value {
        Value::Object(map) => map.keys().take(limit).cloned().collect(),
        _ => Vec::new(),
    }
}

/// Evaluates a lightweight JSONPath / dot-expression against a `Value`.
///
/// Supported syntax:
/// - `""`, `"$"`, `"."`: identity (returns root)
/// - `"key"`, `".key"`, `"$.key"`: object field lookup
/// - `"key.subkey"`, `"$.key.subkey"`: nested dot notation
/// - `"[0]"`, `"key[0]"`, `"$.key[0].name"`: array indexing
/// - `"key[*]"` or `"key[*].name"`: wildcard array projection
/// - `"key.prop"` where `key` is an array: implicit projection over all items
pub fn query_json(root: &Value, query: &str) -> Result<Value, String> {
    let q = query.trim();
    if q.is_empty() || q == "$" || q == "." {
        return Ok(root.clone());
    }

    // Strip leading `$` or leading `.`
    let clean = q.strip_prefix('$').unwrap_or(q);
    let clean = clean.strip_prefix('.').unwrap_or(clean);

    let tokens = tokenize_path(clean)?;
    if tokens.is_empty() {
        return Ok(root.clone());
    }

    eval_tokens(root, &tokens)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Key(String),
    Index(isize),
    Wildcard,
}

fn tokenize_path(path: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = path.chars().peekable();
    let mut current_key = String::new();

    while let Some(&ch) = chars.peek() {
        match ch {
            '.' => {
                chars.next();
                if !current_key.is_empty() {
                    tokens.push(Token::Key(current_key.clone()));
                    current_key.clear();
                }
            }
            '[' => {
                chars.next();
                if !current_key.is_empty() {
                    tokens.push(Token::Key(current_key.clone()));
                    current_key.clear();
                }
                let mut index_str = String::new();
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == ']' {
                        closed = true;
                        break;
                    }
                    index_str.push(c);
                }
                if !closed {
                    return Err(format!("Unclosed bracket in query: '{path}'"));
                }
                let idx_trimmed = index_str.trim().trim_matches('\'').trim_matches('"');
                if idx_trimmed.is_empty() || idx_trimmed == "*" {
                    tokens.push(Token::Wildcard);
                } else if let Ok(idx) = idx_trimmed.parse::<isize>() {
                    tokens.push(Token::Index(idx));
                } else {
                    // String inside bracket like `['foo']` or `["prop.name"]`
                    tokens.push(Token::Key(idx_trimmed.to_string()));
                }
            }
            _ => {
                current_key.push(ch);
                chars.next();
            }
        }
    }

    if !current_key.is_empty() {
        tokens.push(Token::Key(current_key));
    }

    Ok(tokens)
}

fn eval_tokens(val: &Value, tokens: &[Token]) -> Result<Value, String> {
    if tokens.is_empty() {
        return Ok(val.clone());
    }

    let head = &tokens[0];
    let tail = &tokens[1..];

    match head {
        Token::Key(k) => match val {
            Value::Object(map) => {
                if let Some(child) = map.get(k) {
                    eval_tokens(child, tail)
                } else {
                    Err(format!("Key '{k}' not found"))
                }
            }
            Value::Array(arr) => {
                if arr.is_empty() {
                    return Ok(Value::Array(Vec::new()));
                }
                // Implicit wildcard map over array items
                let mut projected = Vec::new();
                for item in arr {
                    if let Value::Object(item_map) = item {
                        if let Some(child) = item_map.get(k) {
                            if let Ok(res) = eval_tokens(child, tail) {
                                projected.push(res);
                            }
                        }
                    }
                }
                if projected.is_empty() {
                    Err(format!("Field '{k}' not found in array elements"))
                } else {
                    Ok(Value::Array(projected))
                }
            }
            _ => Err(format!("Cannot read property '{k}' of non-object")),
        },
        Token::Index(idx) => match val {
            Value::Array(arr) => {
                let resolved_idx = if *idx < 0 {
                    let positive = arr.len() as isize + *idx;
                    if positive >= 0 {
                        positive as usize
                    } else {
                        return Err(format!(
                            "Negative index [{idx}] out of bounds (length {})",
                            arr.len()
                        ));
                    }
                } else {
                    *idx as usize
                };
                if let Some(child) = arr.get(resolved_idx) {
                    eval_tokens(child, tail)
                } else {
                    Err(format!("Index [{idx}] out of bounds (length {})", arr.len()))
                }
            }
            _ => Err(format!("Cannot index non-array with [{idx}]")),
        },
        Token::Wildcard => match val {
            Value::Array(arr) => {
                let mut projected = Vec::new();
                for item in arr {
                    if let Ok(res) = eval_tokens(item, tail) {
                        projected.push(res);
                    }
                }
                Ok(Value::Array(projected))
            }
            Value::Object(map) => {
                let mut projected = Vec::new();
                for (_, child) in map {
                    if let Ok(res) = eval_tokens(child, tail) {
                        projected.push(res);
                    }
                }
                Ok(Value::Array(projected))
            }
            _ => Err("Wildcard [*] cannot be used on scalar value".into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_identity_and_empty() {
        let v = json!({ "name": "blitz", "version": 1 });
        assert_eq!(query_json(&v, "").unwrap(), v);
        assert_eq!(query_json(&v, "$").unwrap(), v);
        assert_eq!(query_json(&v, ".").unwrap(), v);
    }

    #[test]
    fn test_dotted_and_array_index() {
        let v = json!({
            "data": {
                "users": [
                    { "id": 10, "name": "Alice" },
                    { "id": 20, "name": "Bob" }
                ]
            }
        });

        assert_eq!(query_json(&v, "data.users[0].name").unwrap(), json!("Alice"));
        assert_eq!(query_json(&v, "$.data.users[1].id").unwrap(), json!(20));
        assert_eq!(query_json(&v, "data.users[*].name").unwrap(), json!(["Alice", "Bob"]));
        assert_eq!(query_json(&v, "data.users.id").unwrap(), json!([10, 20]));
    }

    #[test]
    fn test_missing_key() {
        let v = json!({ "data": 123 });
        assert!(query_json(&v, "data.missing").is_err());
        assert!(query_json(&v, "nonexistent").is_err());
    }

    #[test]
    fn test_root_keys() {
        let v = json!({ "alpha": 1, "beta": 2, "gamma": 3 });
        let keys = root_keys(&v, 2);
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_negative_indexing_and_bracketed_keys() {
        let v = json!({
            "items": ["first", "middle", "last"],
            "complex": {
                "user.name": "Dave",
                "special-key": 42
            }
        });

        assert_eq!(query_json(&v, "items[-1]").unwrap(), json!("last"));
        assert_eq!(query_json(&v, "items[-2]").unwrap(), json!("middle"));
        assert_eq!(query_json(&v, "complex['user.name']").unwrap(), json!("Dave"));
        assert_eq!(query_json(&v, "complex[\"special-key\"]").unwrap(), json!(42));
        assert!(query_json(&v, "items[-10]").is_err());
    }

    #[test]
    fn test_empty_bracket_wildcard_and_empty_array_projection() {
        let v = json!({
            "users": [
                {"name": "Alice"},
                {"name": "Bob"}
            ],
            "empty": []
        });

        assert_eq!(
            query_json(&v, "users[].name").unwrap(),
            json!(["Alice", "Bob"])
        );
        assert_eq!(query_json(&v, "empty.name").unwrap(), json!([]));
    }
}
