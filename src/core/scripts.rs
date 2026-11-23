//! Script execution: boa JS sandbox (preferred) + legacy line helpers fallback.
//!
//! See `core/sandbox.rs` for supported `blitz.*` surface.

#[cfg(test)]
use regex::Regex;
#[cfg(test)]
use std::sync::OnceLock;

use super::request::TestResult;
use super::variables::VariableContext;
use super::sandbox::{
    run_js_sandbox_ex, SandboxExecution, SandboxExtras, SandboxResponse,
};
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct ScriptRunOutcome {
    pub messages: Vec<String>,
    pub tests: Vec<TestResult>,
    pub error: Option<String>,
    pub execution: SandboxExecution,
    pub visualizer_html: String,
    pub request_header_upserts: Vec<(String, String)>,
    pub request_header_removals: Vec<String>,
    pub request_url_override: Option<String>,
    pub request_body_raw_override: Option<String>,
    pub cookies: Option<HashMap<String, String>>,
}

/// Run pre-request or test script via JS sandbox.
#[cfg(test)]
pub fn run_script(
    script: &str,
    phase: &str,
    ctx: &mut VariableContext,
    response: Option<&SandboxResponse>,
) -> ScriptRunOutcome {
    run_script_ex(script, phase, ctx, response, &SandboxExtras::default())
}

pub fn run_script_ex(
    script: &str,
    phase: &str,
    ctx: &mut VariableContext,
    response: Option<&SandboxResponse>,
    extras: &SandboxExtras,
) -> ScriptRunOutcome {
    if script.trim().is_empty() {
        return ScriptRunOutcome {
            messages: vec![format!("{phase}: skipped (empty)")],
            ..Default::default()
        };
    }

    let js = run_js_sandbox_ex(script, phase, ctx, response, extras);
    ScriptRunOutcome {
        messages: js.messages,
        tests: js.tests,
        error: js.error,
        execution: js.execution,
        visualizer_html: js.visualizer_html,
        request_header_upserts: js.request_header_upserts,
        request_header_removals: js.request_header_removals,
        request_url_override: js.request_url_override,
        request_body_raw_override: js.request_body_raw_override,
        cookies: js.cookies,
    }
}

/// Normalize Postman / Bruno / Insomnia script globals (`pm.`, `bru.`, `bruno.`,
/// `epm.`, `insomnia.`) to canonical `blitz.` outside strings and comments.
pub fn normalize_script_to_blitz(script: &str) -> String {
    if script.is_empty() {
        return String::new();
    }
    let bytes = script.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0usize;

    let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';

    while i < len {
        let b = bytes[i];
        // Line comment
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            let start = i;
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            out.push_str(&script[start..i]);
            continue;
        }
        // Block comment
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(len);
            out.push_str(&script[start..i]);
            continue;
        }
        // String literals ('...', "...", `...`)
        if b == b'\'' || b == b'"' || b == b'`' {
            let quote = b;
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            out.push_str(&script[start..i]);
            continue;
        }
        // Identifier boundary check: must not be preceded by ident char or `.`
        let prev_ok = i == 0 || (!is_ident_char(bytes[i - 1]) && bytes[i - 1] != b'.');
        if prev_ok && is_ident_char(b) {
            let start = i;
            while i < len && is_ident_char(bytes[i]) {
                i += 1;
            }
            let ident = &script[start..i];
            if matches!(ident, "pm" | "bru" | "bruno" | "epm" | "insomnia") {
                // Skip optional whitespace before `.`
                let mut dot_pos = i;
                while dot_pos < len && (bytes[dot_pos] == b' ' || bytes[dot_pos] == b'\t') {
                    dot_pos += 1;
                }
                if dot_pos < len && bytes[dot_pos] == b'.' {
                    // Check Bruno-specific method shorthands after `.`
                    let after_dot = &script[dot_pos + 1..];
                    let bruno_map = [
                        ("setEnvVar(", "environment.set("),
                        ("getEnvVar(", "environment.get("),
                        ("setVar(", "variables.set("),
                        ("getVar(", "variables.get("),
                        ("setGlobalEnvVar(", "globals.set("),
                        ("getGlobalEnvVar(", "globals.get("),
                        ("setNextRequest(", "execution.setNextRequest("),
                    ];
                    if matches!(ident, "bru" | "bruno") {
                        if let Some((from, to)) = bruno_map
                            .iter()
                            .find(|(from, _)| after_dot.starts_with(from))
                        {
                            out.push_str("blitz.");
                            out.push_str(to);
                            i = dot_pos + 1 + from.len();
                            continue;
                        }
                    }
                    out.push_str("blitz.");
                    i = dot_pos + 1;
                    continue;
                }
            }
            out.push_str(ident);
            continue;
        }
        // Copy UTF-8 char
        let ch = script[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

/// Legacy regex helpers (kept for tests / tiny scripts without full JS needs).
#[cfg(test)]
pub fn run_script_helpers(
    script: &str,
    phase: &str,
    ctx: &mut VariableContext,
) -> ScriptRunOutcome {
    let mut out = ScriptRunOutcome::default();
    if script.trim().is_empty() {
        out.messages
            .push(format!("{phase}: skipped (empty)"));
        return out;
    }

    static SET_RE: OnceLock<Regex> = OnceLock::new();
    let set_re = SET_RE.get_or_init(|| {
        Regex::new(
            r#"(?:blitz|pm|epm)\.(environment|globals|variables)\.set\(\s*["']([^"']+)["']\s*,\s*["']([^"']*)["']\s*\)"#,
        )
        .unwrap()
    });

    static LOG_RE: OnceLock<Regex> = OnceLock::new();
    let log_re = LOG_RE.get_or_init(|| {
        Regex::new(r#"console\.log\(\s*["']([^"']*)["']\s*\)"#).unwrap()
    });

    static TEST_RE: OnceLock<Regex> = OnceLock::new();
    let test_re = TEST_RE.get_or_init(|| {
        Regex::new(r#"(?:blitz|pm|epm)\.test\(\s*["']([^"']+)["']"#).unwrap()
    });

    let mut applied = 0usize;
    for line in script.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if let Some(caps) = set_re.captures(trimmed) {
            let scope = &caps[1];
            let key = caps[2].to_string();
            let value = caps[3].to_string();
            match scope {
                "environment" => {
                    ctx.environment.insert(key.clone(), value.clone());
                }
                "globals" => {
                    ctx.global.insert(key.clone(), value.clone());
                }
                _ => {
                    ctx.local.insert(key.clone(), value.clone());
                }
            }
            out.messages
                .push(format!("{phase}: set {scope}.{key}={value}"));
            applied += 1;
            continue;
        }
        if let Some(caps) = log_re.captures(trimmed) {
            out.messages
                .push(format!("{phase}/console: {}", &caps[1]));
            applied += 1;
            continue;
        }
        if let Some(caps) = test_re.captures(trimmed) {
            out.tests.push(TestResult {
                name: caps[1].to_string(),
                passed: true,
                message: "helper stub (use JS sandbox for real assertions)".into(),
            });
            applied += 1;
            continue;
        }
    }

    out.messages
        .push(format!("{phase}: helpers applied={applied}"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_sets_environment_var() {
        let mut ctx = VariableContext::default();
        let out = run_script(
            r#"blitz.environment.set("baseUrl", "http://localhost");"#,
            "pre-request",
            &mut ctx,
            None,
        );
        assert_eq!(
            ctx.environment.get("baseUrl").map(String::as_str),
            Some("http://localhost")
        );
        assert!(out.error.is_none());
    }

    #[test]
    fn js_environment_set_applies_to_environment_values() {
        use crate::core::variables::apply_environment_map;
        use crate::core::Environment;

        let mut ctx = VariableContext::default();
        let out = run_script(
            r#"blitz.environment.set("endpoint_rcell_auth", "http://x");"#,
            "pre-request",
            &mut ctx,
            None,
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(
            ctx.environment.get("endpoint_rcell_auth").map(String::as_str),
            Some("http://x")
        );

        let mut env = Environment::new("Local");
        apply_environment_map(&mut env, &ctx.environment);
        assert_eq!(
            env.values
                .iter()
                .find(|v| v.key == "endpoint_rcell_auth")
                .map(|v| v.effective()),
            Some("http://x")
        );
    }

    #[test]
    fn helpers_still_work() {
        let mut ctx = VariableContext::default();
        let out = run_script_helpers(
            r#"blitz.environment.set("baseUrl", "http://localhost");"#,
            "pre-request",
            &mut ctx,
        );
        assert_eq!(
            ctx.environment.get("baseUrl").map(String::as_str),
            Some("http://localhost")
        );
        assert!(out.messages.iter().any(|m| m.contains("set environment")));
    }

    #[test]
    fn normalize_script_replaces_pm_bru_bruno_insomnia_with_blitz() {
        let input = r#"
// pm.test("ignored in comment")
const label = "pm.environment.get('ignored_in_string')";
pm.test("ok", function() {
  pm.expect(pm.response.code).to.equal(200);
});
bru.setEnvVar("tok", "abc");
bruno.getVar("x");
insomnia.environment.set("a", "1");
epm.globals.set("g", "2");
npm.install("keep");
my_pm.foo();
foo.pm.bar();
"#;
        let out = normalize_script_to_blitz(input);
        assert!(out.contains(r#"// pm.test("ignored in comment")"#));
        assert!(out.contains(r#""pm.environment.get('ignored_in_string')""#));
        assert!(out.contains("blitz.test(\"ok\""));
        assert!(out.contains("blitz.expect(blitz.response.code)"));
        assert!(out.contains("blitz.environment.set(\"tok\", \"abc\")"));
        assert!(out.contains("blitz.variables.get(\"x\")"));
        assert!(out.contains("blitz.environment.set(\"a\", \"1\")"));
        assert!(out.contains("blitz.globals.set(\"g\", \"2\")"));
        assert!(out.contains("npm.install(\"keep\")"));
        assert!(out.contains("my_pm.foo()"));
        assert!(out.contains("foo.pm.bar()"));
    }
}
