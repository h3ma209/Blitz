//! Script execution: boa JS sandbox (preferred) + legacy line helpers fallback.
//!
//! See `runtime/sandbox.rs` for supported `pm.*` surface.

use regex::Regex;
use std::sync::OnceLock;

use super::request::TestResult;
use super::variables::VariableContext;
use super::sandbox::{
    run_js_sandbox_ex, SandboxExecution, SandboxExtras, SandboxResponse,
};

#[derive(Debug, Default)]
pub struct ScriptRunOutcome {
    pub messages: Vec<String>,
    pub tests: Vec<TestResult>,
    pub error: Option<String>,
    pub execution: SandboxExecution,
}

/// Run pre-request or test script via JS sandbox.
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
    }
}

/// Legacy regex helpers (kept for tests / tiny scripts without full JS needs).
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
            r#"pm\.(environment|globals|variables)\.set\(\s*["']([^"']+)["']\s*,\s*["']([^"']*)["']\s*\)"#,
        )
        .unwrap()
    });

    static LOG_RE: OnceLock<Regex> = OnceLock::new();
    let log_re = LOG_RE.get_or_init(|| {
        Regex::new(r#"console\.log\(\s*["']([^"']*)["']\s*\)"#).unwrap()
    });

    static TEST_RE: OnceLock<Regex> = OnceLock::new();
    let test_re = TEST_RE.get_or_init(|| {
        Regex::new(r#"pm\.test\(\s*["']([^"']+)["']"#).unwrap()
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
            r#"pm.environment.set("baseUrl", "http://localhost");"#,
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
    fn helpers_still_work() {
        let mut ctx = VariableContext::default();
        let out = run_script_helpers(
            r#"pm.environment.set("baseUrl", "http://localhost");"#,
            "pre-request",
            &mut ctx,
        );
        assert_eq!(
            ctx.environment.get("baseUrl").map(String::as_str),
            Some("http://localhost")
        );
        assert!(out.messages.iter().any(|m| m.contains("set environment")));
    }
}
