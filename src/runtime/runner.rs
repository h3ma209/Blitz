//! Collection Runner — sequential requests, iterations, delay, data files.

use std::collections::HashMap;
use std::fs;
use std::thread;
use std::time::Duration;

use uuid::Uuid;

use crate::core::{
    flatten_collections, folder_path_to_request, AppSettings, AuthConfig, Collection, CookieJar,
    RequestDraft, ResponseView, TestResult, VariableContext,
};
use crate::secrets::SecretsStore;

use super::pipeline::{send_request_with_options, ExecutionHints, SendOptions};

#[derive(Debug, Clone, PartialEq)]
pub struct RunnerItemResult {
    pub request_id: Uuid,
    pub name: String,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub elapsed_ms: Option<u128>,
    pub passed: bool,
    pub error: Option<String>,
    pub test_results: Vec<TestResult>,
    pub iteration: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunnerReport {
    pub results: Vec<RunnerItemResult>,
    pub console: Vec<String>,
}

impl RunnerReport {
    pub fn passed_count(&self) -> usize {
        self.results.iter().filter(|r| r.passed).count()
    }

    pub fn failed_count(&self) -> usize {
        self.results.iter().filter(|r| !r.passed).count()
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunnerConfig {
    pub iterations: usize,
    pub delay_ms: u64,
    /// Path to CSV (header row) or JSON array of objects.
    pub data_file: String,
}

fn item_passed(view: &ResponseView) -> bool {
    if view.error.is_some() {
        return false;
    }
    if view.status_text == "SKIPPED" {
        return true;
    }
    if view.status == 0 || view.status >= 400 {
        return false;
    }
    if view.test_results.is_empty() {
        return true;
    }
    view.test_results.iter().all(|t| t.passed)
}

/// Collect requests from a collection in tree order (folders DFS).
pub fn ordered_requests(col: &Collection) -> Vec<RequestDraft> {
    flatten_collections(std::slice::from_ref(col))
        .into_iter()
        .filter_map(|row| match row.kind {
            crate::core::TreeRowKind::Request { .. } => find_draft(col, row.id),
            _ => None,
        })
        .collect()
}

fn find_draft(col: &Collection, id: Uuid) -> Option<RequestDraft> {
    fn walk(items: &[crate::core::CollectionItem], id: Uuid) -> Option<RequestDraft> {
        for item in items {
            match item {
                crate::core::CollectionItem::Request(r) if r.id == id => return Some(r.clone()),
                crate::core::CollectionItem::Folder(f) => {
                    if let Some(r) = walk(&f.items, id) {
                        return Some(r);
                    }
                }
                _ => {}
            }
        }
        None
    }
    walk(&col.items, id)
}

/// Load iteration rows from CSV or JSON file. Empty path → one empty row.
pub fn load_data_rows(path: &str) -> Result<Vec<HashMap<String, String>>, String> {
    let path = path.trim();
    if path.is_empty() {
        return Ok(vec![HashMap::new()]);
    }
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let trimmed = text.trim();
    if trimmed.starts_with('[') {
        let arr: Vec<serde_json::Value> =
            serde_json::from_str(trimmed).map_err(|e| e.to_string())?;
        let mut rows = Vec::new();
        for v in arr {
            let mut map = HashMap::new();
            if let Some(obj) = v.as_object() {
                for (k, val) in obj {
                    map.insert(
                        k.clone(),
                        match val {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        },
                    );
                }
            }
            rows.push(map);
        }
        if rows.is_empty() {
            rows.push(HashMap::new());
        }
        return Ok(rows);
    }
    // CSV
    let mut lines = trimmed.lines().filter(|l| !l.trim().is_empty());
    let header = lines
        .next()
        .ok_or_else(|| "CSV empty".to_string())?
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    for line in lines {
        let cols: Vec<_> = line
            .split(',')
            .map(|s| s.trim().trim_matches('"').to_string())
            .collect();
        let mut map = HashMap::new();
        for (i, h) in header.iter().enumerate() {
            map.insert(h.clone(), cols.get(i).cloned().unwrap_or_default());
        }
        rows.push(map);
    }
    if rows.is_empty() {
        rows.push(HashMap::new());
    }
    Ok(rows)
}

/// Run all requests in a collection sequentially (1 iteration, no delay).
///
/// Mutates `ctx` so script env/collection/global side-effects persist after the run.
pub fn run_collection(
    col: &Collection,
    ctx: &mut VariableContext,
    cookie_jar: &mut CookieJar,
    app_settings: Option<&AppSettings>,
    secrets_store: Option<&dyn SecretsStore>,
) -> RunnerReport {
    run_collection_with_config(
        col,
        ctx,
        cookie_jar,
        app_settings,
        secrets_store,
        &RunnerConfig {
            iterations: 1,
            delay_ms: 0,
            data_file: String::new(),
        },
    )
}

pub fn run_collection_with_config(
    col: &Collection,
    ctx: &mut VariableContext,
    cookie_jar: &mut CookieJar,
    app_settings: Option<&AppSettings>,
    secrets_store: Option<&dyn SecretsStore>,
    config: &RunnerConfig,
) -> RunnerReport {
    let mut report = RunnerReport::default();
    let data_rows = match load_data_rows(&config.data_file) {
        Ok(r) => r,
        Err(e) => {
            report.console.push(format!("runner: data file error: {e}"));
            vec![HashMap::new()]
        }
    };
    let iterations = config.iterations.max(1);
    let requests = ordered_requests(col);
    report.console.push(format!(
        "runner: {} request(s) × {iterations} iter in «{}»",
        requests.len(),
        col.name
    ));

    // Seed collection variables into the shared context (Postman: collection scope).
    ctx.collection = col.variables_map();

    let col_auth: AuthConfig = col.auth.clone();

    for iter in 0..iterations {
        let row = data_rows
            .get(iter % data_rows.len())
            .cloned()
            .unwrap_or_default();
        // Keep env/collection/globals across requests; only refresh iteration locals.
        ctx.local.clear();
        for (k, v) in &row {
            ctx.local.insert(k.clone(), v.clone());
        }
        let iter_json = serde_json::to_string(&row).unwrap_or_else(|_| "{}".into());
        report
            .console
            .push(format!("runner: iteration {}/{}", iter + 1, iterations));

        let mut i = 0usize;
        let mut next_override: Option<String> = None;
        while i < requests.len() {
            if let Some(name) = next_override.take() {
                if name.is_empty() {
                    report.console.push("runner: setNextRequest(null) — stop".into());
                    break;
                }
                if let Some(idx) = requests.iter().position(|r| r.name == name) {
                    i = idx;
                } else {
                    report
                        .console
                        .push(format!("runner: setNextRequest «{name}» not found"));
                    break;
                }
            }

            let req = &requests[i];
            let folders = folder_path_to_request(col, req.id);
            let mut inherited_pre: Vec<&str> = Vec::new();
            if !col.pre_request_script.is_empty() {
                inherited_pre.push(&col.pre_request_script);
            }
            for f in &folders {
                if !f.pre_request_script.is_empty() {
                    inherited_pre.push(&f.pre_request_script);
                }
            }
            let mut inherited_post: Vec<&str> = Vec::new();
            for f in folders.iter().rev() {
                if !f.test_script.is_empty() {
                    inherited_post.push(&f.test_script);
                }
            }
            if !col.test_script.is_empty() {
                inherited_post.push(&col.test_script);
            }

            let mut hints = ExecutionHints::default();
            report
                .console
                .push(format!("runner: → {} {}", req.method.as_str(), req.name));

            let result = send_request_with_options(
                req,
                ctx,
                SendOptions {
                    secrets_store,
                    collection_auth: Some(&col_auth),
                    cookie_jar: Some(cookie_jar),
                    app_settings,
                    inherited_pre: inherited_pre.clone(),
                    inherited_post: inherited_post.clone(),
                    iteration_data_json: Some(&iter_json),
                    execution: Some(&mut hints),
                    cancel: None,
                    ancestor_auth: None,
                    workspace_root: None,
                },
            );

            match result {
                Ok(view) => {
                    let passed = item_passed(&view);
                    report.console.extend(view.console_messages.iter().cloned());
                    report.results.push(RunnerItemResult {
                        request_id: req.id,
                        name: req.name.clone(),
                        method: req.method.as_str().into(),
                        url: view.resolved_url.clone(),
                        status: Some(view.status),
                        elapsed_ms: Some(view.elapsed_ms),
                        passed,
                        error: view.error.clone(),
                        test_results: view.test_results.clone(),
                        iteration: iter + 1,
                    });
                }
                Err(e) => {
                    report.console.push(format!("runner: error {e}"));
                    report.results.push(RunnerItemResult {
                        request_id: req.id,
                        name: req.name.clone(),
                        method: req.method.as_str().into(),
                        url: req.url.clone(),
                        status: None,
                        elapsed_ms: None,
                        passed: false,
                        error: Some(e.to_string()),
                        test_results: vec![],
                        iteration: iter + 1,
                    });
                }
            }

            if hints.skip_request {
                report.console.push("runner: skipRequest noted".into());
            }
            if let Some(nr) = hints.next_request {
                next_override = Some(nr);
                continue;
            }

            i += 1;
            if config.delay_ms > 0 && i < requests.len() {
                thread::sleep(Duration::from_millis(config.delay_ms));
            }
        }
        if config.delay_ms > 0 && iter + 1 < iterations {
            thread::sleep(Duration::from_millis(config.delay_ms));
        }
    }

    report.console.push(format!(
        "runner: done — {} passed, {} failed",
        report.passed_count(),
        report.failed_count()
    ));
    report
}
