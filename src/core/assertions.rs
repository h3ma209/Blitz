//! No-code response assertions.
//!
//! Users build assertions in the UI ("status is 200", "$.data.id exists", ...).
//! They are compiled to a plain `blitz.test(...)` script at send time and run
//! through the same JS sandbox as hand-written tests, so results (`TestResult`)
//! look identical in the response pane, collection runner, and CLI.
//! The compiled script is never stored — only the assertion list is persisted.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssertionKind {
    StatusEquals { code: u16 },
    StatusInRange { min: u16, max: u16 },
    /// `path` is dotted / bracketed (`data.items[0].id`, optional leading `$`).
    /// `expected` is parsed as JSON when possible (`5`, `true`, `"5"`), else used as text.
    JsonPathEquals { path: String, expected: String },
    JsonPathExists { path: String },
    HeaderExists { name: String },
    HeaderEquals { name: String, value: String },
    BodyContains { text: String },
    TimeUnder { ms: u64 },
}

impl AssertionKind {
    /// Human-readable one-liner (also used as the test name).
    pub fn label(&self) -> String {
        match self {
            Self::StatusEquals { code } => format!("Status is {code}"),
            Self::StatusInRange { min, max } => format!("Status is {min}–{max}"),
            Self::JsonPathEquals { path, expected } => format!("{path} equals {expected}"),
            Self::JsonPathExists { path } => format!("{path} exists"),
            Self::HeaderExists { name } => format!("Header {name} exists"),
            Self::HeaderEquals { name, value } => format!("Header {name} = {value}"),
            Self::BodyContains { text } => format!("Body contains \"{text}\""),
            Self::TimeUnder { ms } => format!("Response time < {ms} ms"),
        }
    }

    /// Stable short key for UI pickers.
    pub fn type_key(&self) -> &'static str {
        match self {
            Self::StatusEquals { .. } => "status_equals",
            Self::StatusInRange { .. } => "status_in_range",
            Self::JsonPathEquals { .. } => "json_path_equals",
            Self::JsonPathExists { .. } => "json_path_exists",
            Self::HeaderExists { .. } => "header_exists",
            Self::HeaderEquals { .. } => "header_equals",
            Self::BodyContains { .. } => "body_contains",
            Self::TimeUnder { .. } => "time_under",
        }
    }

    /// Fresh assertion of the given picker key with sensible defaults.
    pub fn default_for(key: &str) -> Option<Self> {
        Some(match key {
            "status_equals" => Self::StatusEquals { code: 200 },
            "status_in_range" => Self::StatusInRange { min: 200, max: 299 },
            "json_path_equals" => Self::JsonPathEquals {
                path: "data.id".into(),
                expected: String::new(),
            },
            "json_path_exists" => Self::JsonPathExists {
                path: "data".into(),
            },
            "header_exists" => Self::HeaderExists {
                name: "content-type".into(),
            },
            "header_equals" => Self::HeaderEquals {
                name: "content-type".into(),
                value: "application/json".into(),
            },
            "body_contains" => Self::BodyContains {
                text: String::new(),
            },
            "time_under" => Self::TimeUnder { ms: 500 },
            _ => return None,
        })
    }

    /// (key, label) pairs for the "add assertion" picker.
    pub fn picker_options() -> &'static [(&'static str, &'static str)] {
        &[
            ("status_equals", "Status code equals"),
            ("status_in_range", "Status code in range"),
            ("json_path_equals", "JSON path equals"),
            ("json_path_exists", "JSON path exists"),
            ("header_exists", "Header exists"),
            ("header_equals", "Header equals"),
            ("body_contains", "Body contains text"),
            ("time_under", "Response time under (ms)"),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assertion {
    pub id: Uuid,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub kind: AssertionKind,
}

fn default_true() -> bool {
    true
}

impl Assertion {
    pub fn new(kind: AssertionKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            enabled: true,
            kind,
        }
    }
}

/// JS string literal (JSON-escaped, safe to embed in generated script).
fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Max auto-generated JSON-path assertions (keeps the list reviewable).
const SUGGEST_MAX_PATHS: usize = 10;

/// Build starter assertions from an observed response ("Auto-assert" button).
/// Status + content-type + time ceiling, then JSON-body paths (objects only, depth 2).
pub fn suggest_from_response(
    status: u16,
    headers: &[(String, String)],
    body: &str,
    elapsed_ms: u128,
) -> Vec<Assertion> {
    let mut out = vec![Assertion::new(AssertionKind::StatusEquals { code: status })];

    if headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
    {
        out.push(Assertion::new(AssertionKind::HeaderExists {
            name: "content-type".into(),
        }));
    }

    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(body) {
        let mut count = 0usize;
        suggest_object_paths(&map, "", 1, &mut count, &mut out);
    }

    // Generous ceiling: 3x observed, rounded up to 100 ms, never below 200.
    let ceiling = (((elapsed_ms.saturating_mul(3) + 99) / 100) * 100).max(200) as u64;
    out.push(Assertion::new(AssertionKind::TimeUnder { ms: ceiling }));
    out
}

fn suggest_object_paths(
    map: &serde_json::Map<String, serde_json::Value>,
    prefix: &str,
    depth: usize,
    count: &mut usize,
    out: &mut Vec<Assertion>,
) {
    use serde_json::Value;
    for (k, v) in map {
        if *count >= SUGGEST_MAX_PATHS {
            return;
        }
        // Keys needing quoting can't be expressed in our dotted path syntax; skip them.
        if k.is_empty() || k.contains(['.', '[', ']']) {
            continue;
        }
        let path = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            Value::Object(inner) if depth < 2 => {
                suggest_object_paths(inner, &path, depth + 1, count, out);
            }
            Value::Bool(_) | Value::Number(_) => {
                out.push(Assertion::new(AssertionKind::JsonPathEquals {
                    path,
                    expected: v.to_string(),
                }));
                *count += 1;
            }
            Value::String(s) if s.chars().count() <= 40 => {
                // Quote when the bare text would be re-read as non-string JSON (e.g. "5", "true").
                let expected = if serde_json::from_str::<Value>(s).is_ok() {
                    v.to_string()
                } else {
                    s.clone()
                };
                out.push(Assertion::new(AssertionKind::JsonPathEquals { path, expected }));
                *count += 1;
            }
            _ => {
                out.push(Assertion::new(AssertionKind::JsonPathExists { path }));
                *count += 1;
            }
        }
    }
}

/// Append `incoming` to `existing`, skipping assertions whose kind is already present.
/// Returns how many were added.
pub fn merge_assertions(existing: &mut Vec<Assertion>, incoming: Vec<Assertion>) -> usize {
    let mut added = 0;
    for a in incoming {
        if !existing.iter().any(|e| e.kind == a.kind) {
            existing.push(a);
            added += 1;
        }
    }
    added
}

const PATH_HELPER: &str = r#"function __bzPath(o, p) {
  var parts = String(p).replace(/^\$\.?/, "").replace(/\[(\d+)\]/g, ".$1").split(".").filter(function (x) { return x.length > 0; });
  var cur = o;
  for (var i = 0; i < parts.length; i++) {
    if (cur === null || cur === undefined) return undefined;
    cur = cur[parts[i]];
  }
  return cur;
}
function __bzJson() { return blitz.response.json(); }
"#;

fn compile_one(a: &AssertionKind) -> String {
    let name = js_str(&a.label());
    let body = match a {
        AssertionKind::StatusEquals { code } => {
            format!("blitz.expect(blitz.response.code).to.equal({code});")
        }
        AssertionKind::StatusInRange { min, max } => format!(
            "var c = blitz.response.code; if (c < {min} || c > {max}) throw new Error(\"expected status {min}-{max} but got \" + c);"
        ),
        AssertionKind::JsonPathEquals { path, expected } => format!(
            "var exp; try {{ exp = JSON.parse({e}); }} catch (e) {{ exp = {e}; }} \
             var act = __bzPath(__bzJson(), {p}); \
             if (act === undefined) throw new Error(\"path not found: \" + {p}); \
             blitz.expect(act).to.eql(exp);",
            e = js_str(expected),
            p = js_str(path)
        ),
        AssertionKind::JsonPathExists { path } => format!(
            "if (__bzPath(__bzJson(), {p}) === undefined) throw new Error(\"path not found: \" + {p});",
            p = js_str(path)
        ),
        AssertionKind::HeaderExists { name } => format!(
            "var v = blitz.response.headers.get({n}); if (v === undefined || v === null) throw new Error(\"header missing: \" + {n});",
            n = js_str(name)
        ),
        AssertionKind::HeaderEquals { name, value } => format!(
            "var v = blitz.response.headers.get({n}); if (v === undefined || v === null) throw new Error(\"header missing: \" + {n}); \
             blitz.expect(String(v)).to.equal({val});",
            n = js_str(name),
            val = js_str(value)
        ),
        AssertionKind::BodyContains { text } => {
            format!("blitz.expect(blitz.response.text()).to.include({});", js_str(text))
        }
        AssertionKind::TimeUnder { ms } => {
            format!("blitz.expect(blitz.response.responseTime).to.be.below({ms});")
        }
    };
    format!("blitz.test({name}, function () {{ {body} }});\n")
}

/// Compile enabled assertions into a sandbox script. Empty string when none enabled.
pub fn assertions_to_script(assertions: &[Assertion]) -> String {
    let enabled: Vec<&Assertion> = assertions.iter().filter(|a| a.enabled).collect();
    if enabled.is_empty() {
        return String::new();
    }
    let mut out = String::from(PATH_HELPER);
    for a in enabled {
        out.push_str(&compile_one(&a.kind));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::sandbox::{run_js_sandbox, SandboxResponse};
    use crate::core::VariableContext;

    fn resp() -> SandboxResponse {
        SandboxResponse {
            status: 201,
            status_text: "Created".into(),
            body: r#"{"data":{"id":7,"name":"ada","tags":["a","b"],"ok":true}}"#.into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            response_time: 40,
        }
    }

    fn run(kinds: Vec<AssertionKind>) -> Vec<(String, bool, String)> {
        let list: Vec<Assertion> = kinds.into_iter().map(Assertion::new).collect();
        let script = assertions_to_script(&list);
        let mut ctx = VariableContext::default();
        let out = run_js_sandbox(&script, "assertions", &mut ctx, Some(&resp()));
        assert!(out.error.is_none(), "script error: {:?}\n{script}", out.error);
        out.tests
            .into_iter()
            .map(|t| (t.name, t.passed, t.message))
            .collect()
    }

    #[test]
    fn empty_or_disabled_yields_empty_script() {
        assert_eq!(assertions_to_script(&[]), "");
        let mut a = Assertion::new(AssertionKind::StatusEquals { code: 200 });
        a.enabled = false;
        assert_eq!(assertions_to_script(&[a]), "");
    }

    #[test]
    fn status_assertions() {
        let r = run(vec![
            AssertionKind::StatusEquals { code: 201 },
            AssertionKind::StatusEquals { code: 200 },
            AssertionKind::StatusInRange { min: 200, max: 299 },
            AssertionKind::StatusInRange { min: 400, max: 499 },
        ]);
        assert_eq!(
            r.iter().map(|t| t.1).collect::<Vec<_>>(),
            vec![true, false, true, false]
        );
    }

    #[test]
    fn json_path_variants() {
        let r = run(vec![
            AssertionKind::JsonPathEquals { path: "data.id".into(), expected: "7".into() },
            AssertionKind::JsonPathEquals { path: "$.data.name".into(), expected: "ada".into() },
            AssertionKind::JsonPathEquals { path: "data.tags[1]".into(), expected: "b".into() },
            AssertionKind::JsonPathEquals { path: "data.ok".into(), expected: "true".into() },
            AssertionKind::JsonPathEquals { path: "data.id".into(), expected: "8".into() },
            AssertionKind::JsonPathExists { path: "data.tags[0]".into() },
            AssertionKind::JsonPathExists { path: "data.nope".into() },
            AssertionKind::JsonPathEquals { path: "data.nope".into(), expected: "x".into() },
        ]);
        let passed: Vec<bool> = r.iter().map(|t| t.1).collect();
        assert_eq!(passed, vec![true, true, true, true, false, true, false, false], "{r:?}");
        assert!(r[7].2.contains("path not found"));
    }

    #[test]
    fn json_string_vs_number_expected() {
        // `"7"` (quoted) is a string and must NOT equal numeric 7.
        let r = run(vec![AssertionKind::JsonPathEquals {
            path: "data.id".into(),
            expected: "\"7\"".into(),
        }]);
        assert!(!r[0].1);
    }

    #[test]
    fn header_body_time() {
        let r = run(vec![
            AssertionKind::HeaderExists { name: "Content-Type".into() },
            AssertionKind::HeaderExists { name: "x-missing".into() },
            AssertionKind::HeaderEquals { name: "content-type".into(), value: "application/json".into() },
            AssertionKind::HeaderEquals { name: "content-type".into(), value: "text/html".into() },
            AssertionKind::BodyContains { text: "\"ada\"".into() },
            AssertionKind::BodyContains { text: "grace".into() },
            AssertionKind::TimeUnder { ms: 100 },
            AssertionKind::TimeUnder { ms: 10 },
        ]);
        let passed: Vec<bool> = r.iter().map(|t| t.1).collect();
        assert_eq!(passed, vec![true, false, true, false, true, false, true, false], "{r:?}");
    }

    #[test]
    fn hostile_strings_are_escaped() {
        let r = run(vec![
            AssertionKind::BodyContains { text: "\"); throw new Error('pwn'); (\"".into() },
            AssertionKind::JsonPathEquals {
                path: "a\"b'c\\d".into(),
                expected: "e\nf".into(),
            },
        ]);
        assert_eq!(r.len(), 2);
        assert!(!r[0].1 && !r[1].1);
    }

    #[test]
    fn serde_roundtrip_and_defaults() {
        let a = Assertion::new(AssertionKind::JsonPathEquals {
            path: "a.b".into(),
            expected: "1".into(),
        });
        let s = serde_json::to_string(&a).unwrap();
        let back: Assertion = serde_json::from_str(&s).unwrap();
        assert_eq!(a, back);
        // `enabled` missing -> true
        let j = format!(
            r#"{{"id":"{}","kind":{{"type":"time_under","ms":5}}}}"#,
            Uuid::new_v4()
        );
        let parsed: Assertion = serde_json::from_str(&j).unwrap();
        assert!(parsed.enabled);
    }

    #[test]
    fn default_for_covers_picker() {
        for (k, _) in AssertionKind::picker_options() {
            let kind = AssertionKind::default_for(k).expect(k);
            assert_eq!(kind.type_key(), *k);
        }
        assert!(AssertionKind::default_for("nope").is_none());
    }

    #[test]
    fn suggested_assertions_pass_against_source_response() {
        let r = resp();
        let list = suggest_from_response(r.status, &r.headers, &r.body, r.response_time as u128);
        // status + header + (id, name, ok, tags->exists) + time
        assert!(list.len() >= 6, "{list:?}");
        assert!(matches!(list[0].kind, AssertionKind::StatusEquals { code: 201 }));
        let script = assertions_to_script(&list);
        let mut ctx = VariableContext::default();
        let out = run_js_sandbox(&script, "assertions", &mut ctx, Some(&r));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.tests.len(), list.len());
        assert!(out.tests.iter().all(|t| t.passed), "{:?}", out.tests);
    }

    #[test]
    fn suggest_quotes_numeric_looking_strings_and_caps_paths() {
        let body = r#"{"zip":"12345","flag":"true","plain":"hi"}"#;
        let list = suggest_from_response(200, &[], body, 10);
        let eq: Vec<(String, String)> = list
            .iter()
            .filter_map(|a| match &a.kind {
                AssertionKind::JsonPathEquals { path, expected } => {
                    Some((path.clone(), expected.clone()))
                }
                _ => None,
            })
            .collect();
        assert!(eq.contains(&("zip".into(), "\"12345\"".into())));
        assert!(eq.contains(&("flag".into(), "\"true\"".into())));
        assert!(eq.contains(&("plain".into(), "hi".into())));

        let mut big = serde_json::Map::new();
        for i in 0..50 {
            big.insert(format!("k{i:02}"), serde_json::json!(i));
        }
        let list = suggest_from_response(200, &[], &serde_json::Value::Object(big).to_string(), 10);
        let paths = list
            .iter()
            .filter(|a| matches!(a.kind, AssertionKind::JsonPathEquals { .. }))
            .count();
        assert_eq!(paths, SUGGEST_MAX_PATHS);
    }

    #[test]
    fn suggest_handles_non_json_and_slow_responses() {
        let list = suggest_from_response(404, &[], "<html>nope</html>", 1234);
        assert_eq!(list.len(), 2); // status + time only
        match list[1].kind {
            AssertionKind::TimeUnder { ms } => assert_eq!(ms, 3800),
            _ => panic!("expected time"),
        }
        let fast = suggest_from_response(200, &[], "", 1);
        assert!(matches!(fast[1].kind, AssertionKind::TimeUnder { ms: 200 }));
    }

    #[test]
    fn merge_skips_duplicates() {
        let mut existing = vec![Assertion::new(AssertionKind::StatusEquals { code: 200 })];
        let added = merge_assertions(
            &mut existing,
            vec![
                Assertion::new(AssertionKind::StatusEquals { code: 200 }),
                Assertion::new(AssertionKind::TimeUnder { ms: 500 }),
            ],
        );
        assert_eq!(added, 1);
        assert_eq!(existing.len(), 2);
    }
}
