//! Newman-like CLI: `blitz run collection.json [--env env.json]`.

use std::path::Path;

use serde::Serialize;

use super::environment::{import_environment_json, Environment};
use super::impex::{import_collection_v21, import_har, import_openapi3};
use super::collection::{Collection, CollectionItem};
use super::request::RequestDraft;
use super::variables::VariableContext;
use crate::runtime::pipeline::{send_request_with_options, SendOptions};
use crate::runtime::runner::{RunnerItemResult, RunnerReport};
use crate::runtime::runner_export::to_junit_xml;

/// Parse argv after binary name. Returns Some(exit code) if CLI handled; None → launch GUI.
pub fn try_run_cli(args: &[String]) -> Option<i32> {
    if args.len() < 2 {
        return None;
    }
    match args[1].as_str() {
        "mcp" => {
            if args.len() > 2 {
                match args[2].as_str() {
                    "setup" | "install" | "scan" | "add" => {
                        Some(super::mcp_setup::run_mcp_setup_cli(&args[3..]))
                    }
                    "serve" => Some(super::mcp_server::run_stdio_server()),
                    "help" | "--help" | "-h" => {
                        print_mcp_help();
                        Some(0)
                    }
                    _ => Some(super::mcp_server::run_stdio_server()),
                }
            } else {
                Some(super::mcp_server::run_stdio_server())
            }
        }
        "run" => Some(run_collection_cli(&args[2..])),
        "help" | "--help" | "-h" => {
            print_help();
            Some(0)
        }
        _ => None,
    }
}

fn print_help() {
    eprintln!(
        "blitz — local API client\n\n\
         GUI:  blitz\n\
         CLI:  blitz run <collection.json> [--env <environment.json>] [--reporter json|junit] [--out report.xml]\n\
               blitz mcp\n\
               blitz mcp setup [--all] [--scan]\n\
               blitz help\n\n\
         MCP:  Model Context Protocol server & scanner for Cursor, Claude, Antigravity, Windsurf, VS Code.\n"
    );
}

fn print_mcp_help() {
    eprintln!(
        "blitz mcp — Model Context Protocol\n\n\
         USAGE:\n\
             blitz mcp                  Run stdio MCP server (for AI agents)\n\
             blitz mcp setup [OPTIONS]  Scan system and configure Blitz MCP in IDEs & AI agents\n\n\
         OPTIONS:\n\
             -a, --all                  Configure all detected IDEs without prompt\n\
             -s, --scan                 Scan and print detected IDEs without modifying files\n\
             -b, --bin <PATH>           Custom Blitz binary path\n"
    );
}

#[derive(Debug, Serialize)]
struct CliRunJson {
    collection: String,
    passed: usize,
    failed: usize,
    results: Vec<RunnerItemResult>,
}

fn run_collection_cli(args: &[String]) -> i32 {
    let mut collection_path: Option<&str> = None;
    let mut env_path: Option<&str> = None;
    let mut reporter = "text".to_string();
    let mut out_path: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--env" | "-e" => {
                i += 1;
                env_path = args.get(i).map(|s| s.as_str());
            }
            "--reporter" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    reporter = v.to_ascii_lowercase();
                }
            }
            "--out" | "-o" => {
                i += 1;
                out_path = args.get(i).cloned();
            }
            s if !s.starts_with('-') && collection_path.is_none() => {
                collection_path = Some(s);
            }
            other => {
                eprintln!("unknown arg: {other}");
                print_help();
                return 1;
            }
        }
        i += 1;
    }
    let Some(path) = collection_path else {
        eprintln!("missing collection path");
        print_help();
        return 1;
    };

    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("read {path}: {e}");
            return 1;
        }
    };

    let col = match load_collection_any(&text, path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("import: {e}");
            return 1;
        }
    };

    let mut ctx = VariableContext::default();
    if let Some(ep) = env_path {
        match std::fs::read_to_string(ep).and_then(|t| {
            import_environment_json(&t).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        }) {
            Ok(env) => {
                ctx.environment = env.to_map();
                eprintln!("env: {} ({} vars)", env.name, env.values.len());
            }
            Err(e) => {
                eprintln!("env load failed: {e}");
                return 1;
            }
        }
    }
    ctx.collection = col.variables_map();

    let drafts = flatten_requests(&col);
    if reporter == "text" {
        eprintln!("running {} request(s) from {}", drafts.len(), col.name);
    }
    let mut failed = 0usize;
    let mut jar = super::cookies::CookieJar::default();
    let mut inherited_pre = Vec::new();
    if !col.pre_request_script.is_empty() {
        inherited_pre.push(col.pre_request_script.as_str());
    }
    let mut inherited_post = Vec::new();
    if !col.test_script.is_empty() {
        inherited_post.push(col.test_script.as_str());
    }
    let mut results = Vec::with_capacity(drafts.len());
    for (idx, draft) in drafts.iter().enumerate() {
        if reporter == "text" {
            eprint!("[{}/{}] {} {} … ", idx + 1, drafts.len(), draft.method, draft.name);
        }
        let (passed, item) = match send_request_with_options(
            draft,
            &mut ctx,
            SendOptions {
                collection_auth: Some(&col.auth),
                cookie_jar: Some(&mut jar),
                inherited_pre: inherited_pre.clone(),
                inherited_post: inherited_post.clone(),
                ..Default::default()
            },
        ) {
            Ok(view) => {
                let tests_fail = view.test_results.iter().filter(|t| !t.passed).count();
                let ok = view.error.is_none() && view.status < 400 && tests_fail == 0;
                if !ok {
                    failed += 1;
                }
                if reporter == "text" {
                    if !ok {
                        eprintln!(
                            "FAIL status={} tests_fail={tests_fail} err={:?}",
                            view.status, view.error
                        );
                    } else {
                        eprintln!("OK {} ({} ms)", view.status, view.elapsed_ms);
                    }
                }
                (
                    ok,
                    RunnerItemResult {
                        request_id: draft.id,
                        name: draft.name.clone(),
                        method: draft.method.as_str().to_string(),
                        url: if view.resolved_url.is_empty() {
                            draft.url.clone()
                        } else {
                            view.resolved_url.clone()
                        },
                        status: if view.error.is_some() {
                            None
                        } else {
                            Some(view.status)
                        },
                        elapsed_ms: if view.error.is_some() {
                            None
                        } else {
                            Some(view.elapsed_ms)
                        },
                        passed: ok,
                        error: view.error.clone(),
                        test_results: view.test_results.clone(),
                        iteration: 1,
                    },
                )
            }
            Err(e) => {
                failed += 1;
                if reporter == "text" {
                    eprintln!("ERR {e}");
                }
                (
                    false,
                    RunnerItemResult {
                        request_id: draft.id,
                        name: draft.name.clone(),
                        method: draft.method.as_str().to_string(),
                        url: draft.url.clone(),
                        status: None,
                        elapsed_ms: None,
                        passed: false,
                        error: Some(e.to_string()),
                        test_results: Vec::new(),
                        iteration: 1,
                    },
                )
            }
        };
        let _ = passed;
        results.push(item);
    }

    let rep = RunnerReport {
        results,
        console: Vec::new(),
    };

    if reporter == "json" {
        let payload = CliRunJson {
            collection: col.name.clone(),
            passed: rep.passed_count(),
            failed: rep.failed_count(),
            results: rep.results.clone(),
        };
        let json = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into());
        if let Some(out) = out_path {
            if let Err(e) = std::fs::write(&out, &json) {
                eprintln!("write {out}: {e}");
                return 1;
            }
        } else {
            println!("{json}");
        }
    } else if reporter == "junit" {
        let xml = to_junit_xml(&col.name, &rep);
        if let Some(out) = out_path {
            if let Err(e) = std::fs::write(&out, &xml) {
                eprintln!("write {out}: {e}");
                return 1;
            }
        } else {
            println!("{xml}");
        }
    } else if reporter == "text" {
        eprintln!("done — {} failed of {}", failed, drafts.len());
    } else {
        eprintln!("unknown reporter: {reporter}");
        return 1;
    }

    if failed > 0 {
        1
    } else {
        0
    }
}

fn load_collection_any(text: &str, path: &str) -> Result<Collection, String> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "yaml" || ext == "yml" {
        return import_openapi3(text).map_err(|e| e.to_string());
    }
    if text.trim_start().starts_with('{') {
        if text.contains("\"log\"") && text.contains("\"entries\"") {
            return import_har(text).map_err(|e| e.to_string());
        }
        if text.contains("openapi") || text.contains("\"paths\"") {
            if let Ok(c) = import_openapi3(text) {
                return Ok(c);
            }
        }
    }
    import_collection_v21(text).map_err(|e| e.to_string())
}

fn flatten_requests(col: &Collection) -> Vec<RequestDraft> {
    let mut out = Vec::new();
    fn walk(items: &[CollectionItem], out: &mut Vec<RequestDraft>) {
        for item in items {
            match item {
                CollectionItem::Request(r) => out.push(r.clone()),
                CollectionItem::Folder(f) => walk(&f.items, out),
            }
        }
    }
    walk(&col.items, &mut out);
    out
}

#[allow(dead_code)]
fn _env_type(_: &Environment) {}
