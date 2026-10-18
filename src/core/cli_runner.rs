//! Newman-like CLI: `evil_postman run collection.json [--env env.json]`.

use std::path::Path;

use super::environment::{import_environment_json, Environment};
use super::impex::{import_collection_v21, import_har, import_openapi3};
use super::collection::{Collection, CollectionItem};
use super::request::RequestDraft;
use super::variables::VariableContext;
use crate::runtime::pipeline::{send_request_with_options, SendOptions};

/// Parse argv after binary name. Returns Some(exit code) if CLI handled; None → launch GUI.
pub fn try_run_cli(args: &[String]) -> Option<i32> {
    if args.len() < 2 {
        return None;
    }
    match args[1].as_str() {
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
        "evil_postman — local API client\n\n\
         GUI:  evil_postman\n\
         CLI:  evil_postman run <collection.json> [--env <environment.json>]\n\
               evil_postman help\n"
    );
}

fn run_collection_cli(args: &[String]) -> i32 {
    let mut collection_path: Option<&str> = None;
    let mut env_path: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--env" | "-e" => {
                i += 1;
                env_path = args.get(i).map(|s| s.as_str());
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
    eprintln!("running {} request(s) from {}", drafts.len(), col.name);
    let mut failed = 0usize;
    for (idx, draft) in drafts.iter().enumerate() {
        eprint!("[{}/{}] {} {} … ", idx + 1, drafts.len(), draft.method, draft.name);
        match send_request_with_options(draft, &mut ctx, SendOptions::default()) {
            Ok(view) => {
                let tests_fail = view.test_results.iter().filter(|t| !t.passed).count();
                if view.error.is_some() || view.status >= 400 || tests_fail > 0 {
                    failed += 1;
                    eprintln!(
                        "FAIL status={} tests_fail={tests_fail} err={:?}",
                        view.status, view.error
                    );
                } else {
                    eprintln!("OK {} ({} ms)", view.status, view.elapsed_ms);
                }
            }
            Err(e) => {
                failed += 1;
                eprintln!("ERR {e}");
            }
        }
    }
    eprintln!("done — {} failed of {}", failed, drafts.len());
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
