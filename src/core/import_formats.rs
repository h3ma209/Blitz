//! Unified multi-format importer for Postman, Bruno, Yaak, OpenAPI 3, HAR, and cURL.
//!
//! Automatically classifies and separates imported artifacts into their Blitz categories:
//! - Collections (folders, HTTP/WS/gRPC/GraphQL requests, collection vars, auth, scripts)
//! - Environments
//! - Globals
//! - Vault secrets

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::Value;
use uuid::Uuid;

use super::collection::{Collection, CollectionItem, Folder};
use super::environment::{import_environment_json, EnvVar, Environment};
use super::impex::{
    import_collection_v21, import_curl, import_har, import_openapi3, ImpexError,
};
use super::request::{
    AuthConfig, AuthType, BodyMode, FormDataField, HttpMethod, KeyValue, RequestDraft,
    RequestProtocol,
};

/// Categorized import output separating collections, environments, globals, and vault secrets.
#[derive(Debug, Clone, Default)]
pub struct ImportBundle {
    pub collections: Vec<Collection>,
    pub environments: Vec<Environment>,
    pub globals: Vec<(String, String)>,
    pub vault_secrets: Vec<(String, String)>,
    pub source_format: String,
}

impl ImportBundle {
    pub fn is_empty(&self) -> bool {
        self.collections.is_empty()
            && self.environments.is_empty()
            && self.globals.is_empty()
            && self.vault_secrets.is_empty()
    }

    pub fn merge(&mut self, other: ImportBundle) {
        self.collections.extend(other.collections);
        self.environments.extend(other.environments);
        self.globals.extend(other.globals);
        self.vault_secrets.extend(other.vault_secrets);
        if self.source_format.is_empty() {
            self.source_format = other.source_format;
        } else if !other.source_format.is_empty() && !self.source_format.contains(&other.source_format)
        {
            self.source_format = format!("{}, {}", self.source_format, other.source_format);
        }
    }

    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.collections.is_empty() {
            parts.push(format!("{} collection(s)", self.collections.len()));
        }
        if !self.environments.is_empty() {
            parts.push(format!("{} env(s)", self.environments.len()));
        }
        if !self.globals.is_empty() {
            parts.push(format!("{} global(s)", self.globals.len()));
        }
        if !self.vault_secrets.is_empty() {
            parts.push(format!("{} vault secret(s)", self.vault_secrets.len()));
        }
        if parts.is_empty() {
            "nothing imported".to_string()
        } else if self.source_format.is_empty() {
            format!("imported {}", parts.join(", "))
        } else {
            format!("imported ({}): {}", self.source_format, parts.join(", "))
        }
    }
}

/// Auto-detect format (Yaak, Bruno JSON, `.bru`, Postman Collection/Env/Globals/Dump,
/// OpenAPI 3, HAR, cURL) and route each item into its Blitz category.
pub fn import_auto_bundle(text: &str) -> Result<ImportBundle, ImpexError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(ImpexError::Unsupported("empty input".into()));
    }

    let lower_head = trimmed.chars().take(64).collect::<String>().to_ascii_lowercase();
    if lower_head.starts_with("curl ") || lower_head.starts_with("curl\n") || lower_head == "curl" {
        let req = import_curl(trimmed)?;
        let mut col = Collection::new("cURL import");
        col.add_request(req);
        return Ok(ImportBundle {
            collections: vec![col],
            source_format: "cURL".into(),
            ..Default::default()
        });
    }

    if trimmed.starts_with('{') {
        let root: Value =
            serde_json::from_str(trimmed).map_err(|e| ImpexError::Json(e.to_string()))?;

        // 1. Yaak JSON export
        if is_yaak_json(&root) {
            return import_yaak_json(&root);
        }

        // 2. Bruno JSON export
        if is_bruno_json(&root) {
            return import_bruno_json(&root);
        }

        // 3. HAR
        if root.pointer("/log/entries").is_some() {
            let col = import_har(trimmed)?;
            return Ok(ImportBundle {
                collections: vec![col],
                source_format: "HAR".into(),
                ..Default::default()
            });
        }

        // 4. OpenAPI 3 / Swagger
        if root.get("openapi").is_some()
            || root.get("swagger").is_some()
            || root.get("paths").is_some()
        {
            let col = import_openapi3(trimmed)?;
            return Ok(ImportBundle {
                collections: vec![col],
                source_format: "OpenAPI".into(),
                ..Default::default()
            });
        }

        // 5. Postman Data Dump (collections + environments + globals in one JSON)
        if root.get("collections").is_some()
            || root.get("environments").is_some()
            || root.get("globals").is_some()
        {
            return import_postman_dump(&root);
        }

        // 6. Postman Globals JSON
        if is_postman_globals(&root) {
            return import_postman_globals(&root);
        }

        // 7. Postman Environment JSON
        if is_postman_environment(&root) {
            return import_postman_env_bundle(trimmed, &root);
        }

        // 8. Postman Collection v2.1
        if root.pointer("/info/schema").is_some() || root.get("item").is_some() {
            let col = import_collection_v21(trimmed)?;
            return Ok(ImportBundle {
                collections: vec![col],
                source_format: "Postman".into(),
                ..Default::default()
            });
        }

        return Err(ImpexError::Unsupported(
            "unrecognized JSON format (expected Postman, Bruno, Yaak, OpenAPI, or HAR)".into(),
        ));
    }

    // YAML OpenAPI
    if trimmed.contains("openapi:") || trimmed.contains("paths:") {
        let col = import_openapi3(trimmed)?;
        return Ok(ImportBundle {
            collections: vec![col],
            source_format: "OpenAPI".into(),
            ..Default::default()
        });
    }

    // Single `.bru` file
    if is_bru_text(trimmed) {
        return import_single_bru(trimmed, "Imported Bruno");
    }

    Err(ImpexError::Unsupported(
        "unrecognized format (expected Postman, Bruno, Yaak, OpenAPI, HAR, or cURL)".into(),
    ))
}

/// Import an entire Bruno workspace directory (`bruno.json`, `collection.bru`,
/// `environments/*.bru`, nested folders with `folder.bru`, and request `*.bru` files).
pub fn import_bruno_dir(dir: &Path) -> Result<ImportBundle, ImpexError> {
    if !dir.is_dir() {
        return Err(ImpexError::Unsupported(format!(
            "not a directory: {}",
            dir.display()
        )));
    }

    let mut bundle = ImportBundle {
        source_format: "Bruno".into(),
        ..Default::default()
    };

    let mut col_name = dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Bruno Collection")
        .to_string();

    let bruno_json_path = dir.join("bruno.json");
    if bruno_json_path.is_file() {
        if let Ok(raw) = std::fs::read_to_string(&bruno_json_path) {
            if let Ok(val) = serde_json::from_str::<Value>(&raw) {
                if let Some(n) = val.get("name").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    col_name = n.to_string();
                }
            }
        }
    }

    let mut col = Collection::new(col_name);

    let collection_bru = dir.join("collection.bru");
    if collection_bru.is_file() {
        if let Ok(raw) = std::fs::read_to_string(&collection_bru) {
            let parsed = parse_bru_blocks(&raw);
            apply_bru_scope_to_collection(&parsed, &mut col, &mut bundle.vault_secrets);
        }
    }

    // Environments directory
    let envs_dir = dir.join("environments");
    if envs_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&envs_dir) {
            let mut env_paths: Vec<_> = entries
                .filter_map(|e| e.ok().map(|x| x.path()))
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("bru"))
                .collect();
            env_paths.sort();
            for p in env_paths {
                if let Ok(raw) = std::fs::read_to_string(&p) {
                    let env_name = p
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Environment")
                        .to_string();
                    let (env, secrets) = parse_bru_environment(&env_name, &raw);
                    bundle.environments.push(env);
                    bundle.vault_secrets.extend(secrets);
                }
            }
        }
    }

    col.items = read_bru_items_in_dir(dir, &mut bundle.vault_secrets);
    bundle.collections.push(col);
    Ok(bundle)
}

// ---------------------------------------------------------------------------
// Postman Helpers (Globals, Environment + Secrets, Data Dump)
// ---------------------------------------------------------------------------

fn is_postman_globals(root: &Value) -> bool {
    root.get("_postman_variable_scope")
        .and_then(|v| v.as_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("globals"))
}

fn is_postman_environment(root: &Value) -> bool {
    if root.pointer("/info/schema").is_some() || root.get("item").is_some() {
        return false;
    }
    if root
        .get("_postman_variable_scope")
        .and_then(|v| v.as_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("environment"))
    {
        return true;
    }
    root.get("values").and_then(|v| v.as_array()).is_some()
        && root.get("name").and_then(|v| v.as_str()).is_some()
}

fn import_postman_globals(root: &Value) -> Result<ImportBundle, ImpexError> {
    let mut bundle = ImportBundle {
        source_format: "Postman".into(),
        ..Default::default()
    };
    let values = root
        .get("values")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ImpexError::Unsupported("missing 'values' array in Postman globals".into()))?;
    for v in values {
        let enabled = v.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true);
        if !enabled {
            continue;
        }
        let Some(key) = v.get("key").and_then(|k| k.as_str()).filter(|s| !s.is_empty()) else {
            continue;
        };
        let val = value_to_string(v.get("value"));
        let is_secret = v
            .get("type")
            .and_then(|t| t.as_str())
            .is_some_and(|t| t.eq_ignore_ascii_case("secret"));
        if is_secret {
            bundle.vault_secrets.push((key.to_string(), val));
        } else {
            bundle.globals.push((key.to_string(), val));
        }
    }
    Ok(bundle)
}

fn import_postman_env_bundle(text: &str, root: &Value) -> Result<ImportBundle, ImpexError> {
    let mut env = import_environment_json(text).map_err(ImpexError::Unsupported)?;
    let mut vault_secrets = Vec::new();
    let mut secret_keys = HashSet::new();
    if let Some(values) = root.get("values").and_then(|v| v.as_array()) {
        for v in values {
            let is_secret = v
                .get("type")
                .and_then(|t| t.as_str())
                .is_some_and(|t| t.eq_ignore_ascii_case("secret"));
            if is_secret {
                if let Some(key) = v.get("key").and_then(|k| k.as_str()).filter(|s| !s.is_empty()) {
                    let val = value_to_string(v.get("value"));
                    vault_secrets.push((key.to_string(), val));
                    secret_keys.insert(key.to_string());
                }
            }
        }
    }
    if !secret_keys.is_empty() {
        env.values.retain(|ev| !secret_keys.contains(&ev.key));
    }
    Ok(ImportBundle {
        environments: vec![env],
        vault_secrets,
        source_format: "Postman".into(),
        ..Default::default()
    })
}

fn import_postman_dump(root: &Value) -> Result<ImportBundle, ImpexError> {
    let mut bundle = ImportBundle {
        source_format: "Postman".into(),
        ..Default::default()
    };
    if let Some(cols) = root.get("collections").and_then(|v| v.as_array()) {
        for c in cols {
            if let Ok(raw) = serde_json::to_string(c) {
                if let Ok(col) = import_collection_v21(&raw) {
                    bundle.collections.push(col);
                }
            }
        }
    }
    if let Some(envs) = root.get("environments").and_then(|v| v.as_array()) {
        for e in envs {
            if let Ok(raw) = serde_json::to_string(e) {
                if let Ok(sub) = import_postman_env_bundle(&raw, e) {
                    bundle.merge(sub);
                }
            }
        }
    }
    if let Some(g) = root.get("globals") {
        if let Ok(sub) = import_postman_globals(g) {
            bundle.merge(sub);
        }
    }
    if bundle.is_empty() {
        return Err(ImpexError::Unsupported("empty Postman dump".into()));
    }
    Ok(bundle)
}

// ---------------------------------------------------------------------------
// Yaak JSON Importer
// ---------------------------------------------------------------------------

fn is_yaak_json(root: &Value) -> bool {
    root.get("yaakVersion").is_some()
        || root.get("yaakSchema").is_some()
        || root.pointer("/resources/workspaces").is_some()
        || root.pointer("/resources/httpRequests").is_some()
}

fn import_yaak_json(root: &Value) -> Result<ImportBundle, ImpexError> {
    let resources = root
        .get("resources")
        .unwrap_or(root);
    let mut bundle = ImportBundle {
        source_format: "Yaak".into(),
        ..Default::default()
    };

    let workspaces = resources
        .get("workspaces")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let folders_arr = resources
        .get("folders")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let http_reqs = resources
        .get("httpRequests")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let ws_reqs = resources
        .get("websocketRequests")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let grpc_reqs = resources
        .get("grpcRequests")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let envs_arr = resources
        .get("environments")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // Separate Yaak environments: `base: true` / "Global Variables" → Globals; others → Environments
    for env_val in &envs_arr {
        let name = env_val
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Environment")
            .to_string();
        let is_base = env_val
            .get("base")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
            || env_val
                .get("parentModel")
                .and_then(|v| v.as_str())
                .is_some_and(|p| p == "workspace" && name.eq_ignore_ascii_case("Global Variables"));

        let vars = env_val
            .get("variables")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if is_base {
            for var in vars {
                if !var.get("enabled").and_then(|b| b.as_bool()).unwrap_or(true) {
                    continue;
                }
                let key = var
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                if key.is_empty() {
                    continue;
                }
                let val = value_to_string(var.get("value"));
                let is_secret = var.get("secret").and_then(|b| b.as_bool()).unwrap_or(false)
                    || var.get("encrypted").and_then(|b| b.as_bool()).unwrap_or(false);
                if is_secret {
                    bundle.vault_secrets.push((key.to_string(), val));
                } else {
                    bundle.globals.push((key.to_string(), val));
                }
            }
        } else {
            let mut env = Environment::new(name);
            for var in vars {
                let enabled = var.get("enabled").and_then(|b| b.as_bool()).unwrap_or(true);
                let key = var
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                if key.is_empty() {
                    continue;
                }
                let val = value_to_string(var.get("value"));
                let is_secret = var.get("secret").and_then(|b| b.as_bool()).unwrap_or(false)
                    || var.get("encrypted").and_then(|b| b.as_bool()).unwrap_or(false);
                if is_secret && enabled {
                    bundle.vault_secrets.push((key.to_string(), val));
                } else {
                    let mut ev = EnvVar::new(key, val);
                    ev.enabled = enabled;
                    env.values.push(ev);
                }
            }
            bundle.environments.push(env);
        }
    }

    // Build a Collection per workspace (or a single default one if workspaces is empty)
    let ws_specs: Vec<(Option<String>, String, String)> = if workspaces.is_empty() {
        vec![(None, "Yaak Import".to_string(), String::new())]
    } else {
        workspaces
            .iter()
            .map(|w| {
                (
                    w.get("id").and_then(|v| v.as_str()).map(str::to_string),
                    w.get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Yaak Workspace")
                        .to_string(),
                    w.get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                )
            })
            .collect()
    };

    for (ws_id_opt, ws_name, ws_desc) in ws_specs {
        let mut col = Collection::new(ws_name);
        col.description = ws_desc;

        let matches_ws = |item: &Value| -> bool {
            match (&ws_id_opt, item.get("workspaceId").and_then(|v| v.as_str())) {
                (Some(ws_id), Some(item_ws)) => ws_id == item_ws,
                (None, _) => true,
                (Some(_), None) => workspaces.len() <= 1,
            }
        };

        // Map Yaak folder id -> Folder
        let mut folder_map: HashMap<String, Folder> = HashMap::new();
        let mut folder_parent: HashMap<String, Option<String>> = HashMap::new();
        let mut folder_order: Vec<(f64, String)> = Vec::new();
        for f in &folders_arr {
            if !matches_ws(f) {
                continue;
            }
            let Some(fid) = f.get("id").and_then(|v| v.as_str()).map(str::to_string) else {
                continue;
            };
            let fname = f
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Folder")
                .to_string();
            let parent = f
                .get("folderId")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let prio = f.get("sortPriority").and_then(|v| v.as_f64()).unwrap_or(0.0);
            folder_map.insert(fid.clone(), Folder::new(fname));
            folder_parent.insert(fid.clone(), parent);
            folder_order.push((prio, fid));
        }
        folder_order.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        // Attach requests into folders or root
        let mut root_items: Vec<(f64, CollectionItem)> = Vec::new();
        let mut folder_children: HashMap<String, Vec<(f64, CollectionItem)>> = HashMap::new();

        for req in &http_reqs {
            if !matches_ws(req) {
                continue;
            }
            let prio = req.get("sortPriority").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let draft = yaak_http_to_draft(req);
            if let Some(fid) = req
                .get("folderId")
                .and_then(|v| v.as_str())
                .filter(|s| folder_map.contains_key(*s))
            {
                folder_children
                    .entry(fid.to_string())
                    .or_default()
                    .push((prio, CollectionItem::Request(draft)));
            } else {
                root_items.push((prio, CollectionItem::Request(draft)));
            }
        }
        for req in &ws_reqs {
            if !matches_ws(req) {
                continue;
            }
            let prio = req.get("sortPriority").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let mut draft = yaak_http_to_draft(req);
            draft.protocol = RequestProtocol::WebSocket;
            if let Some(fid) = req
                .get("folderId")
                .and_then(|v| v.as_str())
                .filter(|s| folder_map.contains_key(*s))
            {
                folder_children
                    .entry(fid.to_string())
                    .or_default()
                    .push((prio, CollectionItem::Request(draft)));
            } else {
                root_items.push((prio, CollectionItem::Request(draft)));
            }
        }
        for req in &grpc_reqs {
            if !matches_ws(req) {
                continue;
            }
            let prio = req.get("sortPriority").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let mut draft = yaak_http_to_draft(req);
            draft.protocol = RequestProtocol::Grpc;
            draft.method = HttpMethod::Post;
            if let Some(fid) = req
                .get("folderId")
                .and_then(|v| v.as_str())
                .filter(|s| folder_map.contains_key(*s))
            {
                folder_children
                    .entry(fid.to_string())
                    .or_default()
                    .push((prio, CollectionItem::Request(draft)));
            } else {
                root_items.push((prio, CollectionItem::Request(draft)));
            }
        }

        // Recursively assemble folder hierarchy
        fn build_yaak_folder(
            fid: &str,
            folder_map: &HashMap<String, Folder>,
            folder_parent: &HashMap<String, Option<String>>,
            folder_order: &[(f64, String)],
            folder_children: &HashMap<String, Vec<(f64, CollectionItem)>>,
            visited: &mut HashSet<String>,
        ) -> Option<Folder> {
            if !visited.insert(fid.to_string()) {
                return None;
            }
            let mut f = folder_map.get(fid)?.clone();
            let mut items: Vec<(f64, CollectionItem)> =
                folder_children.get(fid).cloned().unwrap_or_default();
            for (prio, child_id) in folder_order {
                if folder_parent.get(child_id).and_then(|p| p.as_deref()) == Some(fid) {
                    if let Some(sub) = build_yaak_folder(
                        child_id,
                        folder_map,
                        folder_parent,
                        folder_order,
                        folder_children,
                        visited,
                    ) {
                        items.push((*prio, CollectionItem::Folder(sub)));
                    }
                }
            }
            items.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            f.items = items.into_iter().map(|(_, it)| it).collect();
            Some(f)
        }

        let mut visited = HashSet::new();
        for (prio, fid) in &folder_order {
            let parent = folder_parent.get(fid).and_then(|p| p.as_deref());
            if parent.is_none() || parent.is_some_and(|p| !folder_map.contains_key(p)) {
                if let Some(f) = build_yaak_folder(
                    fid,
                    &folder_map,
                    &folder_parent,
                    &folder_order,
                    &folder_children,
                    &mut visited,
                ) {
                    root_items.push((*prio, CollectionItem::Folder(f)));
                }
            }
        }
        root_items.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        col.items = root_items.into_iter().map(|(_, it)| it).collect();
        bundle.collections.push(col);
    }

    Ok(bundle)
}

fn yaak_http_to_draft(req: &Value) -> RequestDraft {
    let name = req
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Request")
        .to_string();
    let method = parse_http_method_str(req.get("method").and_then(|v| v.as_str()).unwrap_or("GET"));
    let url = req
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let headers = extract_kv_array(req.get("headers"), "name", "value");
    let params = extract_kv_array(req.get("urlParameters"), "name", "value");

    let body_type = req
        .get("bodyType")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let body_obj = req.get("body");

    let mut body_mode = BodyMode::None;
    let mut body = String::new();
    let mut body_urlencoded = Vec::new();
    let mut body_formdata = Vec::new();
    let mut graphql_query = String::new();
    let mut graphql_variables = String::new();

    match body_type.as_str() {
        "application/json" | "text/plain" | "application/xml" | "other" => {
            body = body_obj
                .and_then(|b| b.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !body.is_empty() {
                body_mode = BodyMode::Raw;
            }
        }
        "application/x-www-form-urlencoded" => {
            body_mode = BodyMode::Urlencoded;
            body_urlencoded =
                extract_kv_array(body_obj.and_then(|b| b.get("form")), "name", "value");
        }
        "multipart/form-data" => {
            body_mode = BodyMode::FormData;
            if let Some(arr) = body_obj.and_then(|b| b.get("form")).and_then(|v| v.as_array()) {
                for row in arr {
                    let key = row
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if key.is_empty() {
                        continue;
                    }
                    let enabled = row.get("enabled").and_then(|b| b.as_bool()).unwrap_or(true);
                    let file = row.get("file").and_then(|v| v.as_str()).filter(|s| !s.is_empty());
                    body_formdata.push(FormDataField {
                        key,
                        value: file
                            .map(str::to_string)
                            .unwrap_or_else(|| value_to_string(row.get("value"))),
                        enabled,
                        is_file: file.is_some(),
                        content_type: row
                            .get("contentType")
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                    });
                }
            }
        }
        "graphql" => {
            body_mode = BodyMode::Graphql;
            if let Some(txt) = body_obj.and_then(|b| b.get("text")).and_then(|v| v.as_str()) {
                if let Ok(g) = serde_json::from_str::<Value>(txt) {
                    graphql_query = g
                        .get("query")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    graphql_variables = g
                        .get("variables")
                        .map(|v| v.to_string())
                        .unwrap_or_default();
                } else {
                    graphql_query = txt.to_string();
                }
            }
        }
        _ => {
            if let Some(txt) = body_obj.and_then(|b| b.get("text")).and_then(|v| v.as_str()) {
                if !txt.is_empty() {
                    body = txt.to_string();
                    body_mode = BodyMode::Raw;
                }
            }
        }
    }

    let auth_type = req
        .get("authenticationType")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let auth_obj = req.get("authentication");
    let auth = match auth_type.as_str() {
        "bearer" => AuthConfig {
            auth_type: AuthType::Bearer,
            token: auth_obj
                .and_then(|a| a.get("token"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            ..Default::default()
        },
        "basic" => AuthConfig {
            auth_type: AuthType::Basic,
            username: auth_obj
                .and_then(|a| a.get("username"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            password: auth_obj
                .and_then(|a| a.get("password"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            ..Default::default()
        },
        _ => AuthConfig::default(),
    };

    RequestDraft {
        id: Uuid::new_v4(),
        name,
        method,
        url,
        headers,
        params,
        body_mode,
        body,
        body_urlencoded,
        body_formdata,
        graphql_query,
        graphql_variables,
        auth,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Bruno JSON & `.bru` Importer
// ---------------------------------------------------------------------------

fn is_bruno_json(root: &Value) -> bool {
    root.get("items").and_then(|v| v.as_array()).is_some()
        && (root.get("uid").is_some()
            || root.get("version").is_some()
            || root.pointer("/root/request").is_some()
            || root.get("environments").is_some())
}

fn import_bruno_json(root: &Value) -> Result<ImportBundle, ImpexError> {
    let mut bundle = ImportBundle {
        source_format: "Bruno".into(),
        ..Default::default()
    };

    let name = root
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Bruno Collection")
        .to_string();
    let mut col = Collection::new(name);

    if let Some(root_req) = root.pointer("/root/request") {
        col.auth = import_bruno_json_auth(root_req.get("auth"));
        if let Some(vars) = root_req.pointer("/vars/req").and_then(|v| v.as_array()) {
            for v in vars {
                let key = v.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
                if key.is_empty() {
                    continue;
                }
                let val = value_to_string(v.get("value"));
                let enabled = v.get("enabled").and_then(|b| b.as_bool()).unwrap_or(true);
                col.variables.push(KeyValue {
                    key,
                    value: val,
                    enabled,
                });
            }
        }
        if let Some(pre) = root_req.pointer("/script/req").and_then(|v| v.as_str()) {
            col.pre_request_script = crate::core::normalize_script_to_blitz(pre);
        }
        if let Some(tests) = root_req.get("tests").and_then(|v| v.as_str()) {
            col.test_script = crate::core::normalize_script_to_blitz(tests);
        }
    }

    if let Some(items) = root.get("items").and_then(|v| v.as_array()) {
        col.items = import_bruno_json_items(items);
    }

    if let Some(envs) = root.get("environments").and_then(|v| v.as_array()) {
        for env_val in envs {
            let env_name = env_val
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Environment")
                .to_string();
            let mut env = Environment::new(env_name);
            if let Some(vars) = env_val.get("variables").and_then(|v| v.as_array()) {
                for v in vars {
                    let key = v
                        .get("name")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .trim();
                    if key.is_empty() {
                        continue;
                    }
                    let val = value_to_string(v.get("value"));
                    let enabled = v.get("enabled").and_then(|b| b.as_bool()).unwrap_or(true);
                    let secret = v.get("secret").and_then(|b| b.as_bool()).unwrap_or(false);
                    if secret && enabled {
                        bundle.vault_secrets.push((key.to_string(), val));
                    } else {
                        let mut ev = EnvVar::new(key, val);
                        ev.enabled = enabled;
                        env.values.push(ev);
                    }
                }
            }
            bundle.environments.push(env);
        }
    }

    bundle.collections.push(col);
    Ok(bundle)
}

fn import_bruno_json_items(items: &[Value]) -> Vec<CollectionItem> {
    let mut out = Vec::new();
    for item in items {
        let kind = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Item")
            .to_string();
        if kind == "folder" || item.get("items").is_some() {
            let mut folder = Folder::new(name);
            if let Some(sub) = item.get("items").and_then(|v| v.as_array()) {
                folder.items = import_bruno_json_items(sub);
            }
            out.push(CollectionItem::Folder(folder));
            continue;
        }

        let req = item.get("request").unwrap_or(item);
        let method = parse_http_method_str(req.get("method").and_then(|v| v.as_str()).unwrap_or("GET"));
        let url = req
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let headers = extract_kv_array(req.get("headers"), "name", "value");
        let params = extract_kv_array(req.get("params"), "name", "value");
        let auth = import_bruno_json_auth(req.get("auth"));

        let body_obj = req.get("body");
        let body_mode_str = body_obj
            .and_then(|b| b.get("mode"))
            .and_then(|v| v.as_str())
            .unwrap_or("none");
        let mut body_mode = BodyMode::None;
        let mut body = String::new();
        let mut body_urlencoded = Vec::new();
        let mut body_formdata = Vec::new();
        let mut graphql_query = String::new();
        let mut graphql_variables = String::new();

        match body_mode_str {
            "json" => {
                body_mode = BodyMode::Raw;
                body = body_obj
                    .and_then(|b| b.get("json"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
            }
            "text" | "xml" | "sparql" => {
                body_mode = BodyMode::Raw;
                body = body_obj
                    .and_then(|b| b.get(body_mode_str))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
            }
            "formUrlEncoded" => {
                body_mode = BodyMode::Urlencoded;
                body_urlencoded = extract_kv_array(
                    body_obj.and_then(|b| b.get("formUrlEncoded")),
                    "name",
                    "value",
                );
            }
            "multipartForm" => {
                body_mode = BodyMode::FormData;
                if let Some(arr) = body_obj
                    .and_then(|b| b.get("multipartForm"))
                    .and_then(|v| v.as_array())
                {
                    for row in arr {
                        let key = row
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if key.is_empty() {
                            continue;
                        }
                        let is_file = row
                            .get("type")
                            .and_then(|v| v.as_str())
                            .is_some_and(|t| t == "file");
                        body_formdata.push(FormDataField {
                            key,
                            value: value_to_string(row.get("value")),
                            enabled: row.get("enabled").and_then(|b| b.as_bool()).unwrap_or(true),
                            is_file,
                            content_type: row
                                .get("contentType")
                                .and_then(|v| v.as_str())
                                .map(str::to_string),
                        });
                    }
                }
            }
            "graphql" => {
                body_mode = BodyMode::Graphql;
                graphql_query = body_obj
                    .and_then(|b| b.pointer("/graphql/query"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                graphql_variables = body_obj
                    .and_then(|b| b.pointer("/graphql/variables"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
            }
            _ => {}
        }

        let pre_request_script = crate::core::normalize_script_to_blitz(
            req.pointer("/script/req")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
        );
        let test_script = crate::core::normalize_script_to_blitz(
            req.get("tests")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
        );

        out.push(CollectionItem::Request(RequestDraft {
            id: Uuid::new_v4(),
            name,
            protocol: if kind == "grpc" {
                RequestProtocol::Grpc
            } else if kind == "ws" {
                RequestProtocol::WebSocket
            } else {
                RequestProtocol::Http
            },
            method,
            url,
            headers,
            params,
            body_mode,
            body,
            body_urlencoded,
            body_formdata,
            graphql_query,
            graphql_variables,
            auth,
            pre_request_script,
            test_script,
            ..Default::default()
        }));
    }
    out
}

fn import_bruno_json_auth(auth: Option<&Value>) -> AuthConfig {
    let Some(a) = auth else {
        return AuthConfig::default();
    };
    match a.get("mode").and_then(|v| v.as_str()).unwrap_or("none") {
        "bearer" => AuthConfig {
            auth_type: AuthType::Bearer,
            token: a
                .pointer("/bearer/token")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            ..Default::default()
        },
        "basic" => AuthConfig {
            auth_type: AuthType::Basic,
            username: a
                .pointer("/basic/username")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            password: a
                .pointer("/basic/password")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            ..Default::default()
        },
        "apikey" => AuthConfig {
            auth_type: AuthType::ApiKey,
            api_key_header: a
                .pointer("/apikey/key")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            token: a
                .pointer("/apikey/value")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            ..Default::default()
        },
        "inherit" => AuthConfig {
            auth_type: AuthType::Inherit,
            ..Default::default()
        },
        _ => AuthConfig {
            auth_type: AuthType::NoAuth,
            ..Default::default()
        },
    }
}

// ---------------------------------------------------------------------------
// `.bru` Parser (Directory + Single File)
// ---------------------------------------------------------------------------

fn is_bru_text(text: &str) -> bool {
    let blocks = parse_bru_blocks(text);
    blocks.iter().any(|(name, _)| {
        matches!(
            name.as_str(),
            "meta"
                | "get"
                | "post"
                | "put"
                | "delete"
                | "patch"
                | "head"
                | "options"
                | "vars"
                | "headers"
        )
    })
}

fn import_single_bru(text: &str, default_name: &str) -> Result<ImportBundle, ImpexError> {
    let blocks = parse_bru_blocks(text);
    let mut bundle = ImportBundle {
        source_format: "Bruno".into(),
        ..Default::default()
    };
    if blocks.iter().any(|(k, _)| k == "vars")
        && !blocks.iter().any(|(k, _)| {
            matches!(
                k.as_str(),
                "get" | "post" | "put" | "delete" | "patch" | "head" | "options"
            )
        })
    {
        let (env, secrets) = parse_bru_environment(default_name, text);
        bundle.environments.push(env);
        bundle.vault_secrets.extend(secrets);
        return Ok(bundle);
    }
    let req = bru_blocks_to_request(&blocks, default_name);
    let mut col = Collection::new(default_name);
    col.add_request(req);
    bundle.collections.push(col);
    Ok(bundle)
}

fn read_bru_items_in_dir(dir: &Path, vault_secrets: &mut Vec<(String, String)>) -> Vec<CollectionItem> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries.filter_map(|e| e.ok().map(|x| x.path())).collect();
    paths.sort();

    let mut out = Vec::new();
    for path in paths {
        let Some(fname) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if path.is_dir() {
            if fname == "environments" || fname.starts_with('.') {
                continue;
            }
            let mut folder = Folder::new(fname);
            let folder_bru = path.join("folder.bru");
            if folder_bru.is_file() {
                if let Ok(raw) = std::fs::read_to_string(&folder_bru) {
                    let blocks = parse_bru_blocks(&raw);
                    for (bname, body) in &blocks {
                        if bname == "auth:bearer" {
                            folder.auth = AuthConfig {
                                auth_type: AuthType::Bearer,
                                token: bru_kv_get(body, "token"),
                                ..Default::default()
                            };
                        } else if bname == "script:pre-request" {
                            folder.pre_request_script =
                                crate::core::normalize_script_to_blitz(body.trim());
                        } else if bname == "tests" || bname == "script:post-response" {
                            folder.test_script =
                                crate::core::normalize_script_to_blitz(body.trim());
                        }
                    }
                    let _ = vault_secrets;
                }
            }
            folder.items = read_bru_items_in_dir(&path, vault_secrets);
            out.push(CollectionItem::Folder(folder));
        } else if path.extension().and_then(|e| e.to_str()) == Some("bru")
            && fname != "collection.bru"
            && fname != "folder.bru"
        {
            if let Ok(raw) = std::fs::read_to_string(&path) {
                let blocks = parse_bru_blocks(&raw);
                let fallback = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Request");
                out.push(CollectionItem::Request(bru_blocks_to_request(
                    &blocks, fallback,
                )));
            }
        }
    }
    out
}

/// Parse top-level `.bru` blocks (`name { ... }` or `name [ ... ]`).
fn parse_bru_blocks(text: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0usize;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            i += 1;
            continue;
        }
        let open_ch = if trimmed.ends_with('{') {
            Some(('{', '}'))
        } else if trimmed.ends_with('[') {
            Some(('[', ']'))
        } else {
            None
        };
        if let Some((op, cl)) = open_ch {
            let name = trimmed[..trimmed.len() - 1].trim().to_string();
            let mut depth = 1i32;
            let mut body_lines = Vec::new();
            i += 1;
            while i < lines.len() && depth > 0 {
                let l = lines[i];
                for ch in l.chars() {
                    if ch == op {
                        depth += 1;
                    } else if ch == cl {
                        depth -= 1;
                    }
                }
                if depth > 0 {
                    body_lines.push(l);
                }
                i += 1;
            }
            blocks.push((name, body_lines.join("\n")));
        } else {
            i += 1;
        }
    }
    blocks
}

fn parse_bru_kv(body: &str) -> Vec<KeyValue> {
    body.lines()
        .filter_map(|line| {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                return None;
            }
            let (enabled, clean) = if let Some(rest) = t.strip_prefix('~') {
                (false, rest.trim())
            } else {
                (true, t)
            };
            let (k, v) = clean.split_once(':')?;
            Some(KeyValue {
                key: k.trim().to_string(),
                value: v.trim().to_string(),
                enabled,
            })
        })
        .collect()
}

fn bru_kv_get(body: &str, target: &str) -> String {
    parse_bru_kv(body)
        .into_iter()
        .find(|kv| kv.key == target)
        .map(|kv| kv.value)
        .unwrap_or_default()
}

fn parse_bru_environment(name: &str, text: &str) -> (Environment, Vec<(String, String)>) {
    let blocks = parse_bru_blocks(text);
    let mut env = Environment::new(name);
    let mut secrets = Vec::new();
    for (bname, body) in blocks {
        if bname == "vars" {
            for kv in parse_bru_kv(&body) {
                let mut ev = EnvVar::new(kv.key, kv.value);
                ev.enabled = kv.enabled;
                env.values.push(ev);
            }
        } else if bname == "vars:secret" {
            for line in body.lines() {
                let k = line.trim().trim_end_matches(',');
                if !k.is_empty() && !k.starts_with('#') {
                    secrets.push((k.to_string(), String::new()));
                }
            }
        }
    }
    (env, secrets)
}

fn apply_bru_scope_to_collection(
    blocks: &[(String, String)],
    col: &mut Collection,
    vault_secrets: &mut Vec<(String, String)>,
) {
    for (bname, body) in blocks {
        match bname.as_str() {
            "vars:pre-request" | "vars" => col.variables.extend(parse_bru_kv(body)),
            "vars:secret" => {
                for line in body.lines() {
                    let k = line.trim().trim_end_matches(',');
                    if !k.is_empty() {
                        vault_secrets.push((k.to_string(), String::new()));
                    }
                }
            }
            "auth:bearer" => {
                col.auth = AuthConfig {
                    auth_type: AuthType::Bearer,
                    token: bru_kv_get(body, "token"),
                    ..Default::default()
                };
            }
            "auth:basic" => {
                col.auth = AuthConfig {
                    auth_type: AuthType::Basic,
                    username: bru_kv_get(body, "username"),
                    password: bru_kv_get(body, "password"),
                    ..Default::default()
                };
            }
            "script:pre-request" => {
                col.pre_request_script = crate::core::normalize_script_to_blitz(body.trim())
            }
            "tests" | "script:post-response" => {
                col.test_script = crate::core::normalize_script_to_blitz(body.trim())
            }
            _ => {}
        }
    }
}

fn bru_blocks_to_request(blocks: &[(String, String)], fallback_name: &str) -> RequestDraft {
    let mut req = RequestDraft {
        id: Uuid::new_v4(),
        name: fallback_name.to_string(),
        ..Default::default()
    };
    for (bname, body) in blocks {
        match bname.as_str() {
            "meta" => {
                let n = bru_kv_get(body, "name");
                if !n.is_empty() {
                    req.name = n;
                }
            }
            "get" | "post" | "put" | "delete" | "patch" | "head" | "options" => {
                req.method = parse_http_method_str(bname);
                req.url = bru_kv_get(body, "url");
            }
            "headers" => req.headers = parse_bru_kv(body),
            "params:query" => req.params = parse_bru_kv(body),
            "params:path" => req.path_params = parse_bru_kv(body),
            "body:json" | "body:text" | "body:xml" => {
                req.body_mode = BodyMode::Raw;
                req.body = body.trim().to_string();
            }
            "body:form-urlencoded" => {
                req.body_mode = BodyMode::Urlencoded;
                req.body_urlencoded = parse_bru_kv(body);
            }
            "body:multipart-form" => {
                req.body_mode = BodyMode::FormData;
                req.body_formdata = parse_bru_kv(body)
                    .into_iter()
                    .map(|kv| {
                        let is_file = kv.value.starts_with("@file(");
                        let val = if is_file {
                            kv.value
                                .trim_start_matches("@file(")
                                .trim_end_matches(')')
                                .to_string()
                        } else {
                            kv.value
                        };
                        FormDataField {
                            key: kv.key,
                            value: val,
                            enabled: kv.enabled,
                            is_file,
                            content_type: None,
                        }
                    })
                    .collect();
            }
            "body:graphql" => {
                req.body_mode = BodyMode::Graphql;
                req.graphql_query = body.trim().to_string();
            }
            "body:graphql:vars" => {
                req.graphql_variables = body.trim().to_string();
            }
            "auth:bearer" => {
                req.auth = AuthConfig {
                    auth_type: AuthType::Bearer,
                    token: bru_kv_get(body, "token"),
                    ..Default::default()
                };
            }
            "auth:basic" => {
                req.auth = AuthConfig {
                    auth_type: AuthType::Basic,
                    username: bru_kv_get(body, "username"),
                    password: bru_kv_get(body, "password"),
                    ..Default::default()
                };
            }
            "auth:apikey" => {
                req.auth = AuthConfig {
                    auth_type: AuthType::ApiKey,
                    api_key_header: bru_kv_get(body, "key"),
                    token: bru_kv_get(body, "value"),
                    ..Default::default()
                };
            }
            "script:pre-request" => {
                req.pre_request_script = crate::core::normalize_script_to_blitz(body.trim())
            }
            "tests" | "script:post-response" => {
                req.test_script = crate::core::normalize_script_to_blitz(body.trim())
            }
            _ => {}
        }
    }
    req
}

// ---------------------------------------------------------------------------
// Shared JSON Utilities
// ---------------------------------------------------------------------------

fn value_to_string(val: Option<&Value>) -> String {
    match val {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn parse_http_method_str(s: &str) -> HttpMethod {
    match s.trim().to_ascii_uppercase().as_str() {
        "POST" => HttpMethod::Post,
        "PUT" => HttpMethod::Put,
        "DELETE" => HttpMethod::Delete,
        "PATCH" => HttpMethod::Patch,
        "HEAD" => HttpMethod::Head,
        "OPTIONS" => HttpMethod::Options,
        _ => HttpMethod::Get,
    }
}

fn extract_kv_array(arr_val: Option<&Value>, key_field: &str, val_field: &str) -> Vec<KeyValue> {
    let Some(arr) = arr_val.and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| {
            let key = item.get(key_field).and_then(|v| v.as_str())?.to_string();
            if key.is_empty() {
                return None;
            }
            let value = value_to_string(item.get(val_field));
            let enabled = item.get("enabled").and_then(|b| b.as_bool()).unwrap_or(true);
            Some(KeyValue {
                key,
                value,
                enabled,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postman_globals_and_env_secrets_are_separated_by_category() {
        let globals_json = r#"{
            "name": "My Workspace Globals",
            "_postman_variable_scope": "globals",
            "values": [
                { "key": "base_url", "value": "https://api.example.com", "enabled": true, "type": "default" },
                { "key": "master_secret", "value": "s3cr3t", "enabled": true, "type": "secret" },
                { "key": "disabled_var", "value": "skip", "enabled": false, "type": "default" }
            ]
        }"#;
        let bundle = import_auto_bundle(globals_json).unwrap();
        assert_eq!(bundle.source_format, "Postman");
        assert!(bundle.collections.is_empty());
        assert!(bundle.environments.is_empty());
        assert_eq!(
            bundle.globals,
            vec![("base_url".into(), "https://api.example.com".into())]
        );
        assert_eq!(
            bundle.vault_secrets,
            vec![("master_secret".into(), "s3cr3t".into())]
        );
    }

    #[test]
    fn yaak_export_separates_workspaces_envs_globals_and_protocols() {
        let yaak = r#"{
            "yaakVersion": "2025.1.0",
            "yaakSchema": 4,
            "resources": {
                "workspaces": [{ "id": "ws1", "name": "Chat API" }],
                "environments": [
                    {
                        "id": "env_base",
                        "workspaceId": "ws1",
                        "name": "Global Variables",
                        "base": true,
                        "variables": [
                            { "name": "app_name", "value": "chat", "enabled": true },
                            { "name": "signing_key", "value": "k-123", "enabled": true, "secret": true }
                        ]
                    },
                    {
                        "id": "env_prod",
                        "workspaceId": "ws1",
                        "name": "Production",
                        "base": false,
                        "variables": [
                            { "name": "host", "value": "https://prod.chat", "enabled": true }
                        ]
                    }
                ],
                "folders": [
                    { "id": "f1", "workspaceId": "ws1", "name": "Auth", "sortPriority": 1 }
                ],
                "httpRequests": [
                    {
                        "id": "r1",
                        "workspaceId": "ws1",
                        "folderId": "f1",
                        "name": "Login",
                        "method": "POST",
                        "url": "{{host}}/login",
                        "bodyType": "application/json",
                        "body": { "text": "{\"user\":\"a\"}" }
                    }
                ],
                "websocketRequests": [
                    { "id": "wsr1", "workspaceId": "ws1", "name": "Live Chat", "url": "wss://prod.chat/ws" }
                ]
            }
        }"#;
        let bundle = import_auto_bundle(yaak).unwrap();
        assert_eq!(bundle.source_format, "Yaak");
        assert_eq!(bundle.collections.len(), 1);
        assert_eq!(bundle.collections[0].name, "Chat API");
        assert_eq!(bundle.collections[0].items.len(), 2); // Auth folder + Live Chat WS
        assert_eq!(bundle.environments.len(), 1);
        assert_eq!(bundle.environments[0].name, "Production");
        assert_eq!(bundle.globals, vec![("app_name".into(), "chat".into())]);
        assert_eq!(bundle.vault_secrets, vec![("signing_key".into(), "k-123".into())]);
    }

    #[test]
    fn bruno_json_and_bru_dir_separate_collections_envs_and_secrets() {
        let bruno_json = r#"{
            "name": "Payments",
            "version": "1",
            "root": {
                "request": {
                    "vars": { "req": [{ "name": "currency", "value": "USD", "enabled": true }] }
                }
            },
            "items": [
                {
                    "type": "folder",
                    "name": "Charges",
                    "items": [
                        {
                            "type": "http-request",
                            "name": "Create Charge",
                            "request": {
                                "method": "POST",
                                "url": "{{base}}/charges",
                                "body": { "mode": "json", "json": "{\"amount\":100}" }
                            }
                        }
                    ]
                }
            ],
            "environments": [
                {
                    "name": "Sandbox",
                    "variables": [
                        { "name": "base", "value": "https://sbx.pay", "enabled": true },
                        { "name": "api_secret", "value": "sk_test", "enabled": true, "secret": true }
                    ]
                }
            ]
        }"#;
        let bundle = import_auto_bundle(bruno_json).unwrap();
        assert_eq!(bundle.source_format, "Bruno");
        assert_eq!(bundle.collections.len(), 1);
        assert_eq!(bundle.collections[0].variables[0].key, "currency");
        assert_eq!(bundle.environments.len(), 1);
        assert_eq!(bundle.environments[0].name, "Sandbox");
        assert_eq!(bundle.vault_secrets, vec![("api_secret".into(), "sk_test".into())]);

        // Test Bruno .bru directory
        let dir = std::env::temp_dir().join(format!("blitz_bru_{}", Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("environments")).unwrap();
        std::fs::create_dir_all(dir.join("Users")).unwrap();
        std::fs::write(dir.join("bruno.json"), r#"{"name":"BruWorkspace","type":"collection"}"#).unwrap();
        std::fs::write(
            dir.join("environments/Staging.bru"),
            "vars {\n  host: https://stg.test\n}\nvars:secret [\n  jwt_secret\n]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("Users/Get Profile.bru"),
            "meta {\n  name: Get Profile\n  type: http\n}\nget {\n  url: {{host}}/me\n}\n",
        )
        .unwrap();

        let dir_bundle = import_bruno_dir(&dir).unwrap();
        assert_eq!(dir_bundle.collections[0].name, "BruWorkspace");
        assert_eq!(dir_bundle.environments.len(), 1);
        assert_eq!(dir_bundle.environments[0].name, "Staging");
        assert_eq!(dir_bundle.vault_secrets[0].0, "jwt_secret");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
