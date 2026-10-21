//! Pure-Rust line diff for response bodies (with canonical JSON key-sorted formatting).

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    Same(String),
    Add(String),
    Del(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiffStats {
    pub added: usize,
    pub removed: usize,
    pub unchanged: usize,
}

/// Normalize a body string for diffing:
/// If valid JSON, recursively sort object keys and pretty-print so key reordering doesn't create noise.
pub fn normalize_body_for_diff(body: &str) -> String {
    if let Ok(val) = serde_json::from_str::<Value>(body) {
        let sorted = canonicalize_json(val);
        serde_json::to_string_pretty(&sorted).unwrap_or_else(|_| body.to_string())
    } else {
        body.to_string()
    }
}

fn canonicalize_json(val: Value) -> Value {
    match val {
        Value::Object(map) => {
            // serde_json::Map is backed by BTreeMap by default unless preserve_order is enabled,
            // but collecting and inserting in sorted key order guarantees canonical ordering.
            let mut entries: Vec<(String, Value)> = map
                .into_iter()
                .map(|(k, v)| (k, canonicalize_json(v)))
                .collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut out = serde_json::Map::new();
            for (k, v) in entries {
                out.insert(k, v);
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.into_iter().map(canonicalize_json).collect()),
        other => other,
    }
}

/// Compute line-by-line diff between `old_body` and `new_body`.
pub fn diff_bodies(old_body: &str, new_body: &str) -> (Vec<DiffLine>, DiffStats) {
    let norm_old = normalize_body_for_diff(old_body);
    let norm_new = normalize_body_for_diff(new_body);

    let old_lines: Vec<&str> = norm_old.lines().collect();
    let new_lines: Vec<&str> = norm_new.lines().collect();

    // Trim common prefix and suffix for speed on large payloads
    let mut prefix = 0usize;
    while prefix < old_lines.len()
        && prefix < new_lines.len()
        && old_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }

    let mut old_end = old_lines.len();
    let mut new_end = new_lines.len();
    while old_end > prefix
        && new_end > prefix
        && old_lines[old_end - 1] == new_lines[new_end - 1]
    {
        old_end -= 1;
        new_end -= 1;
    }

    let mut out = Vec::with_capacity(old_lines.len().max(new_lines.len()));
    let mut stats = DiffStats::default();

    for &line in &old_lines[..prefix] {
        out.push(DiffLine::Same(line.to_string()));
        stats.unchanged += 1;
    }

    let mid_old = &old_lines[prefix..old_end];
    let mid_new = &new_lines[prefix..new_end];

    let n = mid_old.len();
    let m = mid_new.len();

    if n == 0 {
        for &line in mid_new {
            out.push(DiffLine::Add(line.to_string()));
            stats.added += 1;
        }
    } else if m == 0 {
        for &line in mid_old {
            out.push(DiffLine::Del(line.to_string()));
            stats.removed += 1;
        }
    } else if n.saturating_mul(m) <= 2_500_000 {
        // Standard LCS DP table
        let width = m + 1;
        let mut dp = vec![0u32; (n + 1) * width];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                dp[i * width + j] = if mid_old[i] == mid_new[j] {
                    1 + dp[(i + 1) * width + (j + 1)]
                } else {
                    dp[(i + 1) * width + j].max(dp[i * width + (j + 1)])
                };
            }
        }
        let mut i = 0;
        let mut j = 0;
        while i < n && j < m {
            if mid_old[i] == mid_new[j] {
                out.push(DiffLine::Same(mid_old[i].to_string()));
                stats.unchanged += 1;
                i += 1;
                j += 1;
            } else if dp[(i + 1) * width + j] >= dp[i * width + (j + 1)] {
                out.push(DiffLine::Del(mid_old[i].to_string()));
                stats.removed += 1;
                i += 1;
            } else {
                out.push(DiffLine::Add(mid_new[j].to_string()));
                stats.added += 1;
                j += 1;
            }
        }
        while i < n {
            out.push(DiffLine::Del(mid_old[i].to_string()));
            stats.removed += 1;
            i += 1;
        }
        while j < m {
            out.push(DiffLine::Add(mid_new[j].to_string()));
            stats.added += 1;
            j += 1;
        }
    } else {
        // Fallback for huge middle chunks (>1500x1500 lines)
        for &line in mid_old {
            out.push(DiffLine::Del(line.to_string()));
            stats.removed += 1;
        }
        for &line in mid_new {
            out.push(DiffLine::Add(line.to_string()));
            stats.added += 1;
        }
    }

    for &line in &old_lines[old_end..] {
        out.push(DiffLine::Same(line.to_string()));
        stats.unchanged += 1;
    }

    (out, stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_key_order_is_ignored_in_diff() {
        let a = r#"{"b": 2, "a": 1}"#;
        let b = r#"{"a": 1, "b": 2}"#;
        let (lines, stats) = diff_bodies(a, b);
        assert_eq!(stats.added, 0);
        assert_eq!(stats.removed, 0);
        assert!(stats.unchanged > 0);
        assert!(lines.iter().all(|l| matches!(l, DiffLine::Same(_))));
    }

    #[test]
    fn detects_added_and_removed_lines() {
        let a = "{\"id\": 1, \"status\": \"pending\"}";
        let b = "{\"id\": 1, \"status\": \"active\"}";
        let (lines, stats) = diff_bodies(a, b);
        assert_eq!(stats.added, 1);
        assert_eq!(stats.removed, 1);
        assert!(lines
            .iter()
            .any(|l| matches!(l, DiffLine::Del(s) if s.contains("pending"))));
        assert!(lines
            .iter()
            .any(|l| matches!(l, DiffLine::Add(s) if s.contains("active"))));
    }
}
