//! Collection Runner — sequential requests, iterations, delay, data files.

use std::collections::HashMap;
use std::fs;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

use uuid::Uuid;

use crate::core::{
    folder_path_to_request, grpc_unary_with_headers, resolve_ancestor_auth, sse_listen,
    AppSettings, AuthConfig, AuthType, Collection, CollectionItem, CookieJar, RequestDraft,
    RequestProtocol, ResponseView, TestResult, VariableContext,
};
use crate::secrets::SecretsStore;

use super::pipeline::{send_request_with_options, ExecutionHints, SendOptions};

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RunnerItemResult {
    pub request_id: Uuid,
    pub name: String,
    pub folder_path: String,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub elapsed_ms: Option<u128>,
    pub passed: bool,
    pub error: Option<String>,
    pub test_results: Vec<TestResult>,
    pub response_headers: Vec<(String, String)>,
    pub response_preview: String,
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

    pub fn total_duration_ms(&self) -> u128 {
        self.results.iter().filter_map(|r| r.elapsed_ms).sum()
    }

    pub fn total_assertions(&self) -> usize {
        self.results.iter().map(|r| r.test_results.len()).sum()
    }

    pub fn passed_assertions(&self) -> usize {
        self.results
            .iter()
            .flat_map(|r| r.test_results.iter())
            .filter(|t| t.passed)
            .count()
    }

    #[allow(dead_code)]
    pub fn failed_assertions(&self) -> usize {
        self.results
            .iter()
            .flat_map(|r| r.test_results.iter())
            .filter(|t| !t.passed)
            .count()
    }

    pub fn avg_duration_ms(&self) -> u128 {
        let times: Vec<u128> = self.results.iter().filter_map(|r| r.elapsed_ms).collect();
        if times.is_empty() {
            0
        } else {
            times.iter().sum::<u128>() / times.len() as u128
        }
    }

    pub fn p95_duration_ms(&self) -> u128 {
        let mut times: Vec<u128> = self.results.iter().filter_map(|r| r.elapsed_ms).collect();
        if times.is_empty() {
            return 0;
        }
        times.sort_unstable();
        let idx = ((times.len() as f64) * 0.95).ceil() as usize;
        times[idx.saturating_sub(1).min(times.len() - 1)]
    }

    pub fn max_duration_ms(&self) -> Option<(u128, &str)> {
        self.results
            .iter()
            .filter_map(|r| r.elapsed_ms.map(|ms| (ms, r.name.as_str())))
            .max_by_key(|(ms, _)| *ms)
    }
}

#[derive(Debug, Default)]
pub struct RunnerProgress {
    pub completed: AtomicUsize,
    pub total: AtomicUsize,
    pub passed: AtomicUsize,
    pub failed: AtomicUsize,
    pub current_label: Mutex<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunnerProgressSnapshot {
    pub completed: usize,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub current_label: String,
}

impl RunnerProgress {
    pub fn snapshot(&self) -> RunnerProgressSnapshot {
        RunnerProgressSnapshot {
            completed: self.completed.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
            passed: self.passed.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            current_label: self
                .current_label
                .lock()
                .map(|s| s.clone())
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunnerConfig {
    pub iterations: usize,
    pub delay_ms: u64,
    /// Path to CSV (header row) or JSON array of objects.
    pub data_file: String,
    /// Optional folder name or breadcrumb path to restrict execution scope.
    pub folder_filter: Option<String>,
    /// Optional explicit subset of request IDs (in exact execution order; empty = all in tree order).
    pub selected_request_ids: Vec<Uuid>,
    /// Stop collection run immediately on the first failed request/assertion.
    pub stop_on_error: bool,
    /// Optional cancellation flag checked between requests and passed to HTTP sends.
    pub cancel: Option<Arc<AtomicBool>>,
    /// Optional live progress tracker polled by the UI during execution.
    pub progress: Option<Arc<RunnerProgress>>,
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

/// Collect `(folder_breadcrumb, RequestDraft)` pairs from a collection in tree order (DFS).
pub fn ordered_requests_with_folders(col: &Collection) -> Vec<(String, RequestDraft)> {
    fn walk(items: &[CollectionItem], prefix: &str, out: &mut Vec<(String, RequestDraft)>) {
        for item in items {
            match item {
                CollectionItem::Request(r) => out.push((prefix.to_string(), r.clone())),
                CollectionItem::Folder(f) => {
                    let next = if prefix.is_empty() {
                        f.name.clone()
                    } else {
                        format!("{prefix}/{}", f.name)
                    };
                    walk(&f.items, &next, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&col.items, "", &mut out);
    out
}

/// Collect requests from a collection in tree order (folders DFS).
pub fn ordered_requests(col: &Collection) -> Vec<RequestDraft> {
    ordered_requests_with_folders(col)
        .into_iter()
        .map(|(_, r)| r)
        .collect()
}

fn preview_snippet(s: &str, max_chars: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= max_chars {
        trimmed.to_string()
    } else {
        let mut out: String = trimmed.chars().take(max_chars).collect();
        out.push('…');
        out
    }
}

fn resolve_protocol_headers(req: &RequestDraft, ctx: &VariableContext) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = req
        .headers
        .iter()
        .filter(|h| h.enabled && !h.key.trim().is_empty())
        .map(|h| (ctx.resolve_string(&h.key), ctx.resolve_string(&h.value)))
        .collect();
    match &req.auth.auth_type {
        AuthType::Bearer => {
            let tok = ctx.resolve_string(&req.auth.token);
            if !tok.is_empty() {
                headers.push(("Authorization".into(), format!("Bearer {tok}")));
            }
        }
        AuthType::ApiKey => {
            let k = ctx.resolve_string(&req.auth.api_key_header);
            let v = ctx.resolve_string(&req.auth.token);
            if !k.trim().is_empty() && !v.is_empty() {
                headers.push((k, v));
            }
        }
        _ => {}
    }
    headers
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
    let mut scoped_requests = ordered_requests_with_folders(col);
    if let Some(filter) = config
        .folder_filter
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let filter_lower = filter.to_ascii_lowercase();
        let prefix_lower = format!("{filter_lower}/");
        scoped_requests.retain(|(folder, _)| {
            let f_low = folder.to_ascii_lowercase();
            f_low == filter_lower
                || f_low.starts_with(&prefix_lower)
                || folder
                    .rsplit('/')
                    .next()
                    .is_some_and(|leaf| leaf.eq_ignore_ascii_case(filter))
        });
    }
    if !config.selected_request_ids.is_empty() {
        let mut by_id: HashMap<Uuid, (String, RequestDraft)> = scoped_requests
            .into_iter()
            .map(|(fp, r)| (r.id, (fp, r)))
            .collect();
        scoped_requests = config
            .selected_request_ids
            .iter()
            .filter_map(|id| by_id.remove(id))
            .collect();
    }

    report.console.push(format!(
        "runner: {} request(s) × {iterations} iter in «{}»",
        scoped_requests.len(),
        col.name
    ));

    if let Some(p) = &config.progress {
        p.total
            .store(scoped_requests.len().saturating_mul(iterations), Ordering::Relaxed);
        p.completed.store(0, Ordering::Relaxed);
        p.passed.store(0, Ordering::Relaxed);
        p.failed.store(0, Ordering::Relaxed);
    }

    // Seed collection variables into the shared context (Postman: collection scope).
    ctx.collection = col.variables_map();

    let col_auth: AuthConfig = col.auth.clone();
    let mut aborted = false;

    let is_cancelled = || {
        config
            .cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::SeqCst))
    };

    for iter in 0..iterations {
        if aborted || is_cancelled() {
            if is_cancelled() {
                report.console.push("runner: cancelled by user".into());
            }
            break;
        }
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
        let max_steps = scoped_requests.len().saturating_mul(10).max(100);
        let mut steps = 0usize;
        while i < scoped_requests.len() {
            if is_cancelled() {
                report.console.push("runner: cancelled by user".into());
                aborted = true;
                break;
            }
            if steps >= max_steps {
                report.console.push(format!(
                    "runner: max step limit ({max_steps}) reached in iteration — aborting cycle"
                ));
                break;
            }
            steps += 1;
            if let Some(name) = next_override.take() {
                if name.is_empty() {
                    report.console.push("runner: setNextRequest(null) — stop".into());
                    break;
                }
                if let Some(idx) = scoped_requests.iter().position(|(_, r)| {
                    r.name == name
                        || r.name.eq_ignore_ascii_case(&name)
                        || r.id.to_string().eq_ignore_ascii_case(&name)
                }) {
                    i = idx;
                } else {
                    report
                        .console
                        .push(format!("runner: setNextRequest «{name}» not found"));
                    break;
                }
            }

            let (folder_path, req) = &scoped_requests[i];
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
            let req_badge = if req.protocol == RequestProtocol::Http {
                req.method.as_str()
            } else {
                req.protocol.badge_label()
            };
            if let Some(p) = &config.progress {
                if let Ok(mut guard) = p.current_label.lock() {
                    *guard = format!(
                        "[Iter {}/{}] {} {}",
                        iter + 1,
                        iterations,
                        req_badge,
                        req.name
                    );
                }
            }
            report.console.push(format!(
                "runner: → {} {}{}",
                req_badge,
                if folder_path.is_empty() {
                    String::new()
                } else {
                    format!("{folder_path}/")
                },
                req.name
            ));

            let ancestor = resolve_ancestor_auth(std::slice::from_ref(col), req.id);
            let ancestor_label = ancestor.as_ref().map(|(_, origin)| {
                if origin.is_collection {
                    format!("collection '{}'", origin.name)
                } else {
                    format!("folder '{}'", origin.name)
                }
            });
            let ancestor_auth = match (ancestor.as_ref(), ancestor_label.as_deref()) {
                (Some((auth, _)), Some(label)) => Some((auth, label)),
                _ => None,
            };

            let item_passed_flag: bool;
            if req.protocol == RequestProtocol::Grpc {
                let endpoint = ctx.resolve_string(&req.url);
                let grpc_method = ctx.resolve_string(&req.grpc_method);
                let body = ctx.resolve_string(&req.body);
                let hdrs = resolve_protocol_headers(req, ctx);
                let res = grpc_unary_with_headers(&endpoint, &grpc_method, &body, &hdrs);
                item_passed_flag = res.grpc_status == 0 && res.error.is_none();
                report.console.extend(res.log.iter().cloned());
                report.results.push(RunnerItemResult {
                    request_id: req.id,
                    name: req.name.clone(),
                    folder_path: folder_path.clone(),
                    method: "gRPC".into(),
                    url: format!("{endpoint}/{grpc_method}"),
                    status: if res.grpc_status >= 0 {
                        Some(if res.grpc_status == 0 { 200 } else { 500 })
                    } else {
                        None
                    },
                    elapsed_ms: Some(res.elapsed_ms),
                    passed: item_passed_flag,
                    error: res.error,
                    test_results: vec![],
                    response_headers: vec![],
                    response_preview: preview_snippet(&res.body_json, 2000),
                    iteration: iter + 1,
                });
            } else if req.protocol == RequestProtocol::Sse {
                let url = ctx.resolve_string(&req.url);
                let hdrs = resolve_protocol_headers(req, ctx);
                let started = std::time::Instant::now();
                match sse_listen(&url, &hdrs, 5, 3000) {
                    Ok((log, events)) => {
                        item_passed_flag = true;
                        report.console.extend(log);
                        let summary = events
                            .iter()
                            .map(|e| format!("{}: {}", e.event, e.data))
                            .collect::<Vec<_>>()
                            .join("\n");
                        report.results.push(RunnerItemResult {
                            request_id: req.id,
                            name: req.name.clone(),
                            folder_path: folder_path.clone(),
                            method: "SSE".into(),
                            url,
                            status: Some(200),
                            elapsed_ms: Some(started.elapsed().as_millis()),
                            passed: true,
                            error: None,
                            test_results: vec![],
                            response_headers: vec![],
                            response_preview: preview_snippet(&summary, 2000),
                            iteration: iter + 1,
                        });
                    }
                    Err(e) => {
                        item_passed_flag = false;
                        report.console.push(format!("runner: sse error {e}"));
                        report.results.push(RunnerItemResult {
                            request_id: req.id,
                            name: req.name.clone(),
                            folder_path: folder_path.clone(),
                            method: "SSE".into(),
                            url,
                            status: None,
                            elapsed_ms: Some(started.elapsed().as_millis()),
                            passed: false,
                            error: Some(e),
                            test_results: vec![],
                            response_headers: vec![],
                            response_preview: String::new(),
                            iteration: iter + 1,
                        });
                    }
                }
            } else if req.protocol != RequestProtocol::Http {
                let label = format!("{:?}", req.protocol);
                report.console.push(format!(
                    "runner: skipped interactive protocol {label} for «{}»",
                    req.name
                ));
                item_passed_flag = true;
                report.results.push(RunnerItemResult {
                    request_id: req.id,
                    name: req.name.clone(),
                    folder_path: folder_path.clone(),
                    method: label.to_uppercase(),
                    url: req.url.clone(),
                    status: None,
                    elapsed_ms: Some(0),
                    passed: true,
                    error: None,
                    test_results: vec![],
                    response_headers: vec![],
                    response_preview: format!("Skipped interactive/session protocol ({label}) in batch runner."),
                    iteration: iter + 1,
                });
            } else {
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
                        iteration: iter,
                        execution: Some(&mut hints),
                        cancel: config.cancel.clone(),
                        ancestor_auth,
                        workspace_root: None,
                    },
                );

                match result {
                    Ok(view) => {
                        let passed = item_passed(&view);
                        item_passed_flag = passed;
                        report.console.extend(view.console_messages.iter().cloned());
                        report.results.push(RunnerItemResult {
                            request_id: req.id,
                            name: req.name.clone(),
                            folder_path: folder_path.clone(),
                            method: req.method.as_str().into(),
                            url: view.resolved_url.clone(),
                            status: Some(view.status),
                            elapsed_ms: Some(view.elapsed_ms),
                            passed,
                            error: view.error.clone(),
                            test_results: view.test_results.clone(),
                            response_headers: view.headers.clone(),
                            response_preview: preview_snippet(&view.body, 2000),
                            iteration: iter + 1,
                        });
                    }
                    Err(e) => {
                        item_passed_flag = false;
                        report.console.push(format!("runner: error {e}"));
                        report.results.push(RunnerItemResult {
                            request_id: req.id,
                            name: req.name.clone(),
                            folder_path: folder_path.clone(),
                            method: req.method.as_str().into(),
                            url: req.url.clone(),
                            status: None,
                            elapsed_ms: None,
                            passed: false,
                            error: Some(e.to_string()),
                            test_results: vec![],
                            response_headers: vec![],
                            response_preview: String::new(),
                            iteration: iter + 1,
                        });
                    }
                }
            }

            if let Some(p) = &config.progress {
                p.completed.fetch_add(1, Ordering::Relaxed);
                if item_passed_flag {
                    p.passed.fetch_add(1, Ordering::Relaxed);
                } else {
                    p.failed.fetch_add(1, Ordering::Relaxed);
                }
            }

            if config.stop_on_error && !item_passed_flag {
                report
                    .console
                    .push("runner: stop_on_error triggered — aborting run".into());
                aborted = true;
                break;
            }

            if hints.skip_request {
                report.console.push("runner: skipRequest noted".into());
            }
            if let Some(nr) = hints.next_request {
                next_override = Some(nr);
                if config.delay_ms > 0 {
                    thread::sleep(Duration::from_millis(config.delay_ms));
                }
                continue;
            }

            i += 1;
            if config.delay_ms > 0 && i < scoped_requests.len() {
                thread::sleep(Duration::from_millis(config.delay_ms));
            }
        }
        if !aborted && config.delay_ms > 0 && iter + 1 < iterations {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::collection::{CollectionItem, Folder};
    use crate::core::request::HttpMethod;

    #[test]
    fn ordered_requests_with_folders_preserves_hierarchy() {
        let mut col = Collection::new("Hierarchy");
        col.add_request(RequestDraft {
            name: "RootReq".into(),
            ..Default::default()
        });
        let mut sub = Folder::new("V1");
        sub.add_request(RequestDraft {
            name: "Nested".into(),
            ..Default::default()
        });
        let mut parent = Folder::new("Users");
        parent.items.push(CollectionItem::Folder(sub));
        col.items.push(CollectionItem::Folder(parent));

        let rows = ordered_requests_with_folders(&col);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "");
        assert_eq!(rows[0].1.name, "RootReq");
        assert_eq!(rows[1].0, "Users/V1");
        assert_eq!(rows[1].1.name, "Nested");
    }

    #[test]
    fn runner_report_assertion_and_duration_metrics() {
        let rep = RunnerReport {
            results: vec![
                RunnerItemResult {
                    request_id: Uuid::new_v4(),
                    name: "A".into(),
                    folder_path: "Auth".into(),
                    method: "GET".into(),
                    url: "http://localhost".into(),
                    status: Some(200),
                    elapsed_ms: Some(15),
                    passed: true,
                    error: None,
                    test_results: vec![
                        TestResult { name: "t1".into(), passed: true, message: "ok".into() },
                        TestResult { name: "t2".into(), passed: true, message: "ok".into() },
                    ],
                    response_headers: vec![],
                    response_preview: "{}".into(),
                    iteration: 1,
                },
                RunnerItemResult {
                    request_id: Uuid::new_v4(),
                    name: "B".into(),
                    folder_path: String::new(),
                    method: "POST".into(),
                    url: "http://localhost".into(),
                    status: Some(500),
                    elapsed_ms: Some(25),
                    passed: false,
                    error: None,
                    test_results: vec![
                        TestResult { name: "t3".into(), passed: false, message: "500".into() },
                    ],
                    response_headers: vec![],
                    response_preview: "err".into(),
                    iteration: 1,
                },
            ],
            console: vec![],
        };
        assert_eq!(rep.total_duration_ms(), 40);
        assert_eq!(rep.avg_duration_ms(), 20);
        assert_eq!(rep.p95_duration_ms(), 25);
        assert_eq!(rep.max_duration_ms(), Some((25, "B")));
        assert_eq!(rep.total_assertions(), 3);
        assert_eq!(rep.passed_assertions(), 2);
        assert_eq!(rep.failed_assertions(), 1);
    }

    #[test]
    fn custom_order_and_cancellation_work() {
        let mut col = Collection::new("CustomOrder");
        let r_a = RequestDraft {
            name: "First".into(),
            protocol: RequestProtocol::WebSocket,
            url: "ws://127.0.0.1:1".into(),
            ..Default::default()
        };
        let r_b = RequestDraft {
            name: "Second".into(),
            protocol: RequestProtocol::WebSocket,
            url: "ws://127.0.0.1:2".into(),
            ..Default::default()
        };
        let r_c = RequestDraft {
            name: "Third".into(),
            protocol: RequestProtocol::WebSocket,
            url: "ws://127.0.0.1:3".into(),
            ..Default::default()
        };
        let id_a = r_a.id;
        let id_c = r_c.id;
        col.add_request(r_a);
        col.add_request(r_b);
        col.add_request(r_c);

        let mut ctx = VariableContext::default();
        let mut jar = CookieJar::default();
        // Run C then A (skipping B)
        let rep = run_collection_with_config(
            &col,
            &mut ctx,
            &mut jar,
            None,
            None,
            &RunnerConfig {
                selected_request_ids: vec![id_c, id_a],
                ..Default::default()
            },
        );
        assert_eq!(rep.results.len(), 2);
        assert_eq!(rep.results[0].name, "Third");
        assert_eq!(rep.results[1].name, "First");

        // Pre-set cancel stops immediately
        let cancel_flag = Arc::new(AtomicBool::new(true));
        let rep_c = run_collection_with_config(
            &col,
            &mut ctx,
            &mut jar,
            None,
            None,
            &RunnerConfig {
                cancel: Some(cancel_flag),
                ..Default::default()
            },
        );
        assert!(rep_c.results.is_empty());
        assert!(rep_c.console.iter().any(|l| l.contains("cancelled")));
    }

    #[test]
    fn stop_on_error_and_folder_filter_short_circuit() {
        let mut col = Collection::new("FilterStop");
        let r_skip = RequestDraft {
            name: "SkipOther".into(),
            protocol: RequestProtocol::WebSocket,
            url: "ws://127.0.0.1:1".into(),
            ..Default::default()
        };
        let mut f_target = Folder::new("API");
        let r_fail = RequestDraft {
            name: "FirstFail".into(),
            method: HttpMethod::Get,
            url: "http://127.0.0.1:1/nope".into(),
            ..Default::default()
        };
        let r_after = RequestDraft {
            name: "ShouldNotRun".into(),
            protocol: RequestProtocol::WebSocket,
            url: "ws://127.0.0.1:1".into(),
            ..Default::default()
        };
        f_target.add_request(r_fail.clone());
        f_target.add_request(r_after);
        col.add_request(r_skip);
        col.items.push(CollectionItem::Folder(f_target));

        let mut ctx = VariableContext::default();
        let mut jar = CookieJar::default();
        let rep = run_collection_with_config(
            &col,
            &mut ctx,
            &mut jar,
            None,
            None,
            &RunnerConfig {
                iterations: 2,
                folder_filter: Some("API".into()),
                stop_on_error: true,
                ..Default::default()
            },
        );
        // Folder filter drops SkipOther, stop_on_error stops after FirstFail in iteration 1!
        assert_eq!(rep.results.len(), 1);
        assert_eq!(rep.results[0].name, "FirstFail");
        assert_eq!(rep.results[0].folder_path, "API");
        assert!(!rep.results[0].passed);
    }
}
