//! Split workspace tree writer/reader (one file per request).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use uuid::Uuid;

use crate::core::collection::{
    Collection, CollectionEnvOverrides, CollectionItem, Folder,
};
use crate::core::environment::{deduplicate_environments_by_id, import_environment_json, Environment};
use crate::core::impex::{
    export_request_item, import_collection_v21, import_request_item, ImpexError,
};
use crate::core::request::{AuthConfig, AuthType, KeyValue, RequestDraft};
use crate::core::variables::VariableContext;

#[derive(Debug, Error)]
pub enum CloudTreeError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("impex: {0}")]
    Impex(#[from] ImpexError),
    #[error("{0}")]
    Msg(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMeta {
    pub schema_version: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default_env: String,
    #[serde(default)]
    pub remote: String,
}

pub const DEFAULT_GITIGNORE: &str = r#"# Blitz local / secrets — never commit
.blitz/local.json
.blitz/cache/
**/*.local.json
**/secrets/**
.env
.env.*
vault.json
*.pem
*.p12
*.key

# OS / editor
.DS_Store
.idea/
.vscode/*
!.vscode/extensions.json
"#;

pub const DEFAULT_README: &str = r#"# Blitz cloud workspace

Collections and environments synced from [Blitz](https://github.com/h3ma209/blitz-cloud).

- **Environments = git branches** named `env/<sanitized-env-name>`
- Secret values are redacted in `environments/*.environment.json` (local vault / snapshot only)
- One request per `*.request.json` for merge-friendly diffs
- **VariableContext.local** (script / request-local vars) is never synced — machine/session only

Do not commit `vault.json`, cookies, history, or `.blitz/local.json`.
"#;

/// Sanitize for filesystem / branch segment: lowercase, non-alnum → `-`.
pub fn sanitize_slug(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "unnamed".into()
    } else {
        trimmed
    }
}

/// Branch convention: `env/<slug>` (documented in design).
#[allow(dead_code)]
pub fn branch_name_for_env(env_name: &str) -> String {
    let slug = sanitize_slug(env_name);
    if slug.is_empty() || slug == "unnamed" {
        "env/no-environment".into()
    } else {
        format!("env/{slug}")
    }
}

fn unique_slug_with_id(base: &str, id: Uuid, used: &mut HashSet<String>) -> String {
    let slug = sanitize_slug(base);
    if used.insert(slug.clone()) {
        return slug;
    }
    let hex_id = id.simple().to_string();
    let short = &hex_id[..8.min(hex_id.len())];
    let det = format!("{slug}-{short}");
    if used.insert(det.clone()) {
        return det;
    }
    let mut n = 2u32;
    loop {
        let candidate = format!("{det}-{n}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

fn write_if_changed(path: &Path, content: &str) -> Result<(), CloudTreeError> {
    if let Ok(existing) = fs::read_to_string(path) {
        if existing == content {
            return Ok(());
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

fn prune_stale_files(dir: &Path, keep: &HashSet<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            prune_stale_files(&p, keep);
            let is_empty = fs::read_dir(&p)
                .map(|mut it| it.next().is_none())
                .unwrap_or(false);
            if is_empty {
                let _ = fs::remove_dir(&p);
            }
        } else if !keep.contains(&p) {
            let _ = fs::remove_file(&p);
        }
    }
}

/// Redact only secret credential fields inside an [`AuthConfig`], keeping non-secret config
/// (OAuth2 URLs/client_id/grant/scope, AWS region/service, Hawk algorithm, EdgeGrid host, usernames).
pub fn redact_auth_config(auth: &AuthConfig) -> AuthConfig {
    let mut out = auth.clone();
    out.token.clear();
    out.password.clear();
    out.oauth2.client_secret.clear();
    out.oauth2.access_token.clear();
    out.oauth2.refresh_token.clear();
    out.extra.secret.clear();
    out.extra.token.clear();
    if out.auth_type != AuthType::EdgeGrid {
        out.extra.token_secret.clear();
    }
    if matches!(out.auth_type, AuthType::AwsSigV4 | AuthType::EdgeGrid) {
        out.extra.key.clear();
    }
    out
}

fn merge_auth_secrets(imported: &mut AuthConfig, local: &AuthConfig) {
    if imported.auth_type != local.auth_type {
        return;
    }
    if imported.token.is_empty() && !local.token.is_empty() {
        imported.token = local.token.clone();
    }
    if imported.password.is_empty() && !local.password.is_empty() {
        imported.password = local.password.clone();
    }
    if imported.oauth2.client_secret.is_empty() && !local.oauth2.client_secret.is_empty() {
        imported.oauth2.client_secret = local.oauth2.client_secret.clone();
    }
    if imported.oauth2.access_token.is_empty() && !local.oauth2.access_token.is_empty() {
        imported.oauth2.access_token = local.oauth2.access_token.clone();
    }
    if imported.oauth2.refresh_token.is_empty() && !local.oauth2.refresh_token.is_empty() {
        imported.oauth2.refresh_token = local.oauth2.refresh_token.clone();
    }
    if imported.extra.secret.is_empty() && !local.extra.secret.is_empty() {
        imported.extra.secret = local.extra.secret.clone();
    }
    if imported.extra.token.is_empty() && !local.extra.token.is_empty() {
        imported.extra.token = local.extra.token.clone();
    }
    if imported.extra.token_secret.is_empty() && !local.extra.token_secret.is_empty() {
        imported.extra.token_secret = local.extra.token_secret.clone();
    }
    if imported.extra.key.is_empty() && !local.extra.key.is_empty() {
        imported.extra.key = local.extra.key.clone();
    }
}

fn redact_kv_secrets(vars: &[KeyValue]) -> Vec<KeyValue> {
    vars.iter()
        .map(|kv| {
            let mut copy = kv.clone();
            if looks_like_secret_key(&copy.key) {
                copy.value.clear();
            }
            copy
        })
        .collect()
}

fn merge_kv_secrets(imported: &mut [KeyValue], local: &[KeyValue]) {
    for kv in imported.iter_mut() {
        if looks_like_secret_key(&kv.key) && kv.value.is_empty() {
            if let Some(src) = local.iter().find(|l| l.key == kv.key && !l.value.is_empty()) {
                kv.value = src.value.clone();
            }
        }
    }
}

fn redact_env_overrides(ov: &CollectionEnvOverrides) -> CollectionEnvOverrides {
    let mut out = ov.clone();
    out.variables = redact_kv_secrets(&out.variables);
    if let Some(ref mut a) = out.auth {
        *a = redact_auth_config(a);
    }
    for req_ov in out.requests.values_mut() {
        req_ov.variables = redact_kv_secrets(&req_ov.variables);
        if let Some(ref mut a) = req_ov.auth {
            *a = redact_auth_config(a);
        }
    }
    out
}

fn merge_items_secrets(imported: &mut [CollectionItem], local: &[CollectionItem]) {
    for item in imported.iter_mut() {
        match item {
            CollectionItem::Request(r) => {
                if let Some(CollectionItem::Request(loc_r)) = local.iter().find(|li| match li {
                    CollectionItem::Request(lr) => lr.id == r.id || lr.name == r.name,
                    _ => false,
                }) {
                    merge_auth_secrets(&mut r.auth, &loc_r.auth);
                }
            }
            CollectionItem::Folder(f) => {
                if let Some(CollectionItem::Folder(loc_f)) = local.iter().find(|li| match li {
                    CollectionItem::Folder(lf) => lf.id == f.id || lf.name == f.name,
                    _ => false,
                }) {
                    merge_auth_secrets(&mut f.auth, &loc_f.auth);
                    merge_items_secrets(&mut f.items, &loc_f.items);
                }
            }
        }
    }
}

/// Restore local secret auth credentials, secret variables, and machine-local metadata on
/// collections after importing a redacted Git workspace tree.
pub fn merge_collection_secrets(imported: &mut [Collection], local: &[Collection]) {
    for col in imported.iter_mut() {
        let Some(loc) = local.iter().find(|c| c.id == col.id || c.name == col.name) else {
            continue;
        };
        merge_auth_secrets(&mut col.auth, &loc.auth);
        merge_kv_secrets(&mut col.variables, &loc.variables);
        if col.pinned_environment_id.is_none() {
            col.pinned_environment_id = loc.pinned_environment_id;
        }
        if col.openapi_spec_path.is_empty() && !loc.openapi_spec_path.is_empty() {
            col.openapi_spec_path = loc.openapi_spec_path.clone();
            col.openapi_synced_mtime = loc.openapi_synced_mtime;
        }
        merge_kv_secrets(
            &mut col.environment_overrides.variables,
            &loc.environment_overrides.variables,
        );
        if let (Some(ref mut imp_auth), Some(ref loc_auth)) =
            (&mut col.environment_overrides.auth, &loc.environment_overrides.auth)
        {
            merge_auth_secrets(imp_auth, loc_auth);
        }
        for (req_id, req_ov) in col.environment_overrides.requests.iter_mut() {
            if let Some(loc_ov) = loc.environment_overrides.requests.get(req_id) {
                merge_kv_secrets(&mut req_ov.variables, &loc_ov.variables);
                if let (Some(ref mut ia), Some(ref la)) = (&mut req_ov.auth, &loc_ov.auth) {
                    merge_auth_secrets(ia, la);
                }
            }
        }
        merge_items_secrets(&mut col.items, &loc.items);
    }
}

/// Read synced workspace globals from `.blitz/globals.json`.
pub fn import_workspace_globals(root: &Path) -> HashMap<String, String> {
    let path = root.join(".blitz").join("globals.json");
    let Ok(text) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    serde_json::from_str::<HashMap<String, String>>(&text).unwrap_or_default()
}

/// Restore local secret values for globals whose key looks like a secret.
pub fn merge_global_secrets(
    imported: &mut HashMap<String, String>,
    local: &HashMap<String, String>,
) {
    for (k, v) in imported.iter_mut() {
        if looks_like_secret_key(k) && v.is_empty() {
            if let Some(lv) = local.get(k) {
                if !lv.is_empty() {
                    *v = lv.clone();
                }
            }
        }
    }
    for (k, lv) in local {
        if looks_like_secret_key(k) && !lv.is_empty() && !imported.contains_key(k) {
            imported.insert(k.clone(), lv.clone());
        }
    }
}

/// Keys that look like secrets → values emptied + `type: secret` in export.
pub fn looks_like_secret_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    [
        "password",
        "passwd",
        "secret",
        "token",
        "apikey",
        "api_key",
        "access_key",
        "private_key",
        "client_secret",
        "refresh_token",
        "auth_key",
        "credential",
        "bearer",
    ]
    .iter()
    .any(|s| k.contains(s))
}

/// Redact secret values for git; keep keys / enabled flags.
pub fn redact_environment(env: &Environment) -> Environment {
    let mut out = env.clone();
    for v in &mut out.values {
        if looks_like_secret_key(&v.key) {
            v.initial_value.clear();
            v.current_value.clear();
        }
    }
    out
}

/// After import from git, restore secret values from local memory by env name + key.
pub fn merge_secret_values(imported: &mut [Environment], local: &[Environment]) {
    for env in imported.iter_mut() {
        let Some(prev) = local.iter().find(|e| e.name == env.name || e.id == env.id) else {
            continue;
        };
        for v in env.values.iter_mut() {
            if !looks_like_secret_key(&v.key) {
                continue;
            }
            if !v.current_value.is_empty() || !v.initial_value.is_empty() {
                continue;
            }
            if let Some(src) = prev.values.iter().find(|x| x.key == v.key) {
                v.initial_value = src.initial_value.clone();
                v.current_value = src.current_value.clone();
            }
        }
        // Also carry over local-only secret keys missing from redacted file
        for src in &prev.values {
            if looks_like_secret_key(&src.key)
                && !env.values.iter().any(|v| v.key == src.key)
                && (!src.current_value.is_empty() || !src.initial_value.is_empty())
            {
                env.values.push(src.clone());
            }
        }
    }
}

fn export_env_redacted_json(env: &Environment) -> Result<String, CloudTreeError> {
    let redacted = redact_environment(env);
    let values: Vec<Value> = redacted
        .values
        .iter()
        .filter(|v| !v.key.is_empty())
        .map(|v| {
            let is_secret = looks_like_secret_key(&v.key);
            json!({
                "key": v.key,
                "value": if is_secret { "" } else { v.current_value.as_str() },
                "initialValue": if is_secret { "" } else { v.initial_value.as_str() },
                "enabled": v.enabled,
                "type": if is_secret { "secret" } else { "default" },
            })
        })
        .collect();
    let mut doc = serde_json::Map::new();
    doc.insert("id".into(), json!(redacted.id.to_string()));
    doc.insert("name".into(), json!(redacted.name));
    doc.insert("values".into(), json!(values));
    doc.insert("_postman_variable_scope".into(), json!("environment"));
    doc.insert("_blitz_redacted".into(), json!(true));
    if let Some(icon) = redacted.icon.as_deref().filter(|s| !s.trim().is_empty()) {
        doc.insert("_blitz_icon".into(), json!(icon));
    }
    if let Some(color) = redacted.color.as_deref().filter(|s| !s.trim().is_empty()) {
        doc.insert("_blitz_color".into(), json!(color));
    }
    Ok(serde_json::to_string_pretty(&Value::Object(doc))?)
}

/// Write full workspace tree under `root` (git working tree).
///
/// Syncs collections, collection variables, globals, and shared environments (all secrets redacted).
/// **Never** writes [`VariableContext::local`] or the machine-only `"Local"` environment.
/// Any stale `.blitz/local.json` is removed so it cannot be committed.
pub fn export_workspace_tree(
    root: &Path,
    project_name: &str,
    remote: &str,
    _default_env: &str,
    collections: &[Collection],
    environments: &[Environment],
    variables: &VariableContext,
) -> Result<(), CloudTreeError> {
    let blitz = root.join(".blitz");
    fs::create_dir_all(&blitz)?;
    fs::create_dir_all(root.join("collections"))?;
    fs::create_dir_all(root.join("environments"))?;

    // Local scope must never land in the cloud tree (even as a gitignored overlay).
    let _ = &variables.local;
    let local_overlay = blitz.join("local.json");
    if local_overlay.exists() {
        fs::remove_file(&local_overlay)?;
    }

    // Omit volatile per-user active env from project.json to avoid cross-teammate merge conflicts.
    let meta = ProjectMeta {
        schema_version: 1,
        name: project_name.to_string(),
        default_env: String::new(),
        remote: super::git::mask_remote_url(remote),
    };
    write_if_changed(
        &blitz.join("project.json"),
        &serde_json::to_string_pretty(&meta)?,
    )?;

    // Globals may sync with secret keys redacted; local never does.
    let globals_path = blitz.join("globals.json");
    if variables.global.is_empty() {
        if globals_path.exists() {
            fs::remove_file(&globals_path)?;
        }
    } else {
        let mut sorted_globals = BTreeMap::new();
        for (k, v) in &variables.global {
            if k.is_empty() {
                continue;
            }
            let safe_val = if looks_like_secret_key(k) {
                String::new()
            } else {
                v.clone()
            };
            sorted_globals.insert(k.clone(), safe_val);
        }
        write_if_changed(&globals_path, &serde_json::to_string_pretty(&sorted_globals)?)?;
    }

    let gi = root.join(".gitignore");
    if !gi.exists() {
        fs::write(&gi, DEFAULT_GITIGNORE)?;
    }
    let readme = root.join("README.md");
    if !readme.exists() {
        fs::write(&readme, DEFAULT_README)?;
    }

    // Differential write under collections/ + prune only removed files.
    let cols_dir = root.join("collections");
    let mut written_col_files = HashSet::new();
    let mut used_col_slugs = HashSet::new();
    for col in collections {
        let slug = unique_slug_with_id(&col.name, col.id, &mut used_col_slugs);
        write_collection_split(&cols_dir.join(&slug), col, &mut written_col_files)?;
    }
    if cols_dir.is_dir() {
        prune_stale_files(&cols_dir, &written_col_files);
    }

    // Shared environments (skip machine-local "Local" env); prune stale .environment.json files.
    let env_dir = root.join("environments");
    let mut written_env_files = HashSet::new();
    let mut used_env_slugs = HashSet::new();
    for env in environments {
        if env.name.trim().eq_ignore_ascii_case("Local") {
            continue;
        }
        let slug = unique_slug_with_id(&env.name, env.id, &mut used_env_slugs);
        let path = env_dir.join(format!("{slug}.environment.json"));
        write_if_changed(&path, &export_env_redacted_json(env)?)?;
        written_env_files.insert(path);
    }
    if env_dir.is_dir() {
        prune_stale_files(&env_dir, &written_env_files);
    }

    Ok(())
}

fn write_collection_split(
    dir: &Path,
    col: &Collection,
    written: &mut HashSet<PathBuf>,
) -> Result<(), CloudTreeError> {
    fs::create_dir_all(dir)?;
    fs::create_dir_all(dir.join("folders"))?;
    fs::create_dir_all(dir.join("requests"))?;

    let mut used = HashSet::new();
    let order = write_items(dir, &col.items, Path::new(""), &mut used, written)?;

    let variables: Vec<Value> = col
        .variables
        .iter()
        .filter(|v| !v.key.is_empty())
        .map(|v| {
            let is_secret = looks_like_secret_key(&v.key);
            json!({
                "key": v.key,
                "value": if is_secret { "" } else { v.value.as_str() },
                "type": if is_secret { "secret" } else { "default" },
                "disabled": !v.enabled,
            })
        })
        .collect();

    let auth = export_auth_json(&col.auth);
    let redacted_auth_cfg = redact_auth_config(&col.auth);
    let redacted_ov = redact_env_overrides(&col.environment_overrides);
    let has_env_overrides = !redacted_ov.is_empty();
    let mut blitz_meta = serde_json::Map::new();
    blitz_meta.insert("favorite".into(), json!(col.favorite));
    blitz_meta.insert("preRequestScript".into(), json!(col.pre_request_script));
    blitz_meta.insert("testScript".into(), json!(col.test_script));
    blitz_meta.insert("split".into(), json!(true));
    if redacted_auth_cfg.auth_type != AuthType::Inherit {
        if let Ok(v) = serde_json::to_value(&redacted_auth_cfg) {
            blitz_meta.insert("auth".into(), v);
        }
    }
    if let Some(pid) = col.pinned_environment_id {
        blitz_meta.insert("pinnedEnvironmentId".into(), json!(pid.to_string()));
    }
    if let Some(icon) = col.icon.as_deref().filter(|s| !s.trim().is_empty()) {
        blitz_meta.insert("icon".into(), json!(icon));
    }
    if let Some(color) = col.color.as_deref().filter(|s| !s.trim().is_empty()) {
        blitz_meta.insert("color".into(), json!(color));
    }
    let mut doc = json!({
        "info": {
            "name": col.name,
            "description": col.description,
            "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json",
            "_postman_id": col.id.to_string(),
        },
        "auth": auth,
        "variable": variables,
        "item": order,
        "_blitz": Value::Object(blitz_meta),
    });
    if has_env_overrides {
        if let Some(obj) = doc.as_object_mut() {
            obj.insert("environmentOverrides".into(), serde_json::to_value(redacted_ov)?);
        }
    }
    let col_path = dir.join("collection.json");
    write_if_changed(&col_path, &serde_json::to_string_pretty(&doc)?)?;
    written.insert(col_path);
    Ok(())
}

fn export_auth_json(auth: &AuthConfig) -> Value {
    // Always redact credential fields in cloud tree (secrets stay local).
    match auth.auth_type {
        AuthType::NoAuth => json!({ "type": "noauth" }),
        AuthType::Inherit => Value::Null,
        AuthType::Bearer => json!({
            "type": "bearer",
            "bearer": [{ "key": "token", "value": "", "type": "string" }]
        }),
        AuthType::Basic => json!({
            "type": "basic",
            "basic": [
                { "key": "username", "value": auth.username, "type": "string" },
                { "key": "password", "value": "", "type": "string" },
            ]
        }),
        AuthType::ApiKey => json!({
            "type": "apikey",
            "apikey": [
                { "key": "key", "value": auth.api_key_header, "type": "string" },
                { "key": "value", "value": "", "type": "string" },
                { "key": "in", "value": "header", "type": "string" },
            ]
        }),
        AuthType::Digest => json!({
            "type": "digest",
            "digest": [
                { "key": "username", "value": auth.username, "type": "string" },
                { "key": "password", "value": "", "type": "string" },
            ]
        }),
        AuthType::Oauth2 => json!({
            "type": "oauth2",
            "oauth2": [
                { "key": "accessToken", "value": "", "type": "string" },
                { "key": "tokenUrl", "value": auth.oauth2.token_url, "type": "string" },
            ]
        }),
        AuthType::Oauth1 => json!({ "type": "oauth1" }),
        AuthType::Hawk => json!({ "type": "hawk" }),
        AuthType::Ntlm => json!({ "type": "ntlm" }),
        AuthType::AwsSigV4 => json!({ "type": "awsv4" }),
        AuthType::EdgeGrid => json!({ "type": "edgegrid" }),
    }
}

fn import_auth_json(auth: Option<&Value>) -> AuthConfig {
    let Some(auth) = auth else {
        return AuthConfig {
            auth_type: AuthType::NoAuth,
            ..Default::default()
        };
    };
    if auth.is_null() {
        return AuthConfig {
            auth_type: AuthType::Inherit,
            ..Default::default()
        };
    }
    let ty = auth.get("type").and_then(|v| v.as_str()).unwrap_or("noauth");
    let kv = |section: &str, key: &str| -> String {
        auth.get(section)
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .find(|e| e.get("key").and_then(|k| k.as_str()) == Some(key))
            .and_then(|e| e.get("value"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let mut cfg = AuthConfig::default();
    match ty {
        "noauth" => cfg.auth_type = AuthType::NoAuth,
        "bearer" => {
            cfg.auth_type = AuthType::Bearer;
            cfg.token = kv("bearer", "token");
        }
        "basic" => {
            cfg.auth_type = AuthType::Basic;
            cfg.username = kv("basic", "username");
            cfg.password = kv("basic", "password");
        }
        "apikey" => {
            cfg.auth_type = AuthType::ApiKey;
            cfg.api_key_header = kv("apikey", "key");
            cfg.token = kv("apikey", "value");
        }
        "digest" => {
            cfg.auth_type = AuthType::Digest;
            cfg.username = kv("digest", "username");
            cfg.password = kv("digest", "password");
        }
        "oauth2" => {
            cfg.auth_type = AuthType::Oauth2;
            cfg.oauth2.access_token = kv("oauth2", "accessToken");
            cfg.oauth2.token_url = kv("oauth2", "tokenUrl");
        }
        "oauth1" => cfg.auth_type = AuthType::Oauth1,
        "hawk" => cfg.auth_type = AuthType::Hawk,
        "ntlm" => cfg.auth_type = AuthType::Ntlm,
        "awsv4" => cfg.auth_type = AuthType::AwsSigV4,
        "edgegrid" => cfg.auth_type = AuthType::EdgeGrid,
        _ => cfg.auth_type = AuthType::NoAuth,
    }
    cfg
}

/// Write items; return order refs for collection.json.
fn write_items(
    col_root: &Path,
    items: &[CollectionItem],
    folder_rel: &Path,
    used: &mut HashSet<String>,
    written: &mut HashSet<PathBuf>,
) -> Result<Vec<Value>, CloudTreeError> {
    let mut order = Vec::new();
    for item in items {
        match item {
            CollectionItem::Request(r) => {
                let slug = unique_slug_with_id(&r.name, r.id, used);
                let file_name = format!("{slug}.request.json");
                let rel = if folder_rel.as_os_str().is_empty() {
                    PathBuf::from("requests").join(&file_name)
                } else {
                    PathBuf::from("folders")
                        .join(folder_rel)
                        .join(&file_name)
                };
                let abs = col_root.join(&rel);
                let mut val = export_request_item(r);
                // Redact auth secrets in request file, while preserving non-secret AuthConfig fields
                if let Some(req) = val.get_mut("request") {
                    if let Some(auth) = req.get_mut("auth") {
                        redact_auth_value(auth);
                    }
                }
                if r.auth.auth_type != AuthType::Inherit {
                    let redacted_cfg = redact_auth_config(&r.auth);
                    if let Ok(auth_v) = serde_json::to_value(&redacted_cfg) {
                        if let Some(obj) = val.as_object_mut() {
                            let xb = obj
                                .entry("x-blitz")
                                .or_insert_with(|| json!({}));
                            if let Some(xb_obj) = xb.as_object_mut() {
                                xb_obj.insert("auth".into(), auth_v);
                            }
                        }
                    }
                }
                write_if_changed(&abs, &serde_json::to_string_pretty(&val)?)?;
                written.insert(abs);
                order.push(json!({
                    "kind": "request",
                    "path": path_to_str(&rel),
                    "id": r.id.to_string(),
                    "name": r.name,
                }));
            }
            CollectionItem::Folder(f) => {
                let slug = unique_slug_with_id(&f.name, f.id, used);
                let child_rel = if folder_rel.as_os_str().is_empty() {
                    PathBuf::from(&slug)
                } else {
                    folder_rel.join(&slug)
                };
                let folder_dir = col_root.join("folders").join(&child_rel);
                fs::create_dir_all(&folder_dir)?;
                let redacted_folder_auth = redact_auth_config(&f.auth);
                let mut meta = json!({
                    "id": f.id.to_string(),
                    "name": f.name,
                    "description": f.description,
                    "auth": export_auth_json(&redacted_folder_auth),
                    "preRequestScript": f.pre_request_script,
                    "testScript": f.test_script,
                    "sortKey": f.sort_key,
                });
                if redacted_folder_auth.auth_type != AuthType::Inherit {
                    if let Ok(av) = serde_json::to_value(&redacted_folder_auth) {
                        if let Some(obj) = meta.as_object_mut() {
                            obj.insert("blitzAuth".into(), av);
                        }
                    }
                }
                let meta_path = folder_dir.join("_folder.json");
                write_if_changed(&meta_path, &serde_json::to_string_pretty(&meta)?)?;
                written.insert(meta_path);
                let mut child_used = HashSet::new();
                let children =
                    write_items(col_root, &f.items, &child_rel, &mut child_used, written)?;
                order.push(json!({
                    "kind": "folder",
                    "path": format!("folders/{}", path_to_str(&child_rel)),
                    "id": f.id.to_string(),
                    "name": f.name,
                    "item": children,
                }));
            }
        }
    }
    Ok(order)
}

fn path_to_str(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn redact_auth_value(auth: &mut Value) {
    if let Some(obj) = auth.as_object_mut() {
        for (_k, v) in obj.iter_mut() {
            if let Some(arr) = v.as_array_mut() {
                for entry in arr {
                    if let Some(key) = entry.get("key").and_then(|x| x.as_str()) {
                        let kl = key.to_ascii_lowercase();
                        if kl.contains("password")
                            || kl.contains("token")
                            || kl.contains("secret")
                            || kl == "value"
                        {
                            if let Some(val) = entry.get_mut("value") {
                                *val = json!("");
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Load collections + environments from a cloud working tree.
pub fn import_workspace_tree(
    root: &Path,
) -> Result<(Vec<Collection>, Vec<Environment>), CloudTreeError> {
    let mut collections = Vec::new();
    let cols_dir = root.join("collections");
    if cols_dir.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(&cols_dir)?.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let path = entry.path();
            let col_json = path.join("collection.json");
            if !col_json.exists() {
                continue;
            }
            collections.push(read_collection_split(&path)?);
        }
    }

    let mut environments = Vec::new();
    let env_dir = root.join("environments");
    if env_dir.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(&env_dir)?.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".environment.json") {
                continue;
            }
            let text = fs::read_to_string(entry.path())?;
            match import_environment_json(&text) {
                Ok(env) => environments.push(env),
                Err(e) => return Err(CloudTreeError::Msg(e)),
            }
        }
    }

    deduplicate_environments_by_id(&mut environments);
    Ok((collections, environments))
}

fn read_collection_split(dir: &Path) -> Result<Collection, CloudTreeError> {
    let text = fs::read_to_string(dir.join("collection.json"))?;
    let root: Value = serde_json::from_str(&text)?;

    // Prefer split order; fall back to full v2.1 item[] if present without refs.
    let item = root.get("item").cloned().unwrap_or(Value::Array(vec![]));
    let is_split = root
        .pointer("/_blitz/split")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || item
            .as_array()
            .map(|a| {
                a.iter()
                    .any(|i| i.get("kind").is_some() && i.get("path").is_some())
            })
            .unwrap_or(false);

    if is_split {
        let name = root
            .pointer("/info/name")
            .and_then(|v| v.as_str())
            .unwrap_or("Imported")
            .to_string();
        let description = root
            .pointer("/info/description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mut col = Collection::new(name);
        col.description = description;
        if let Some(id) = root
            .pointer("/info/_postman_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
        {
            col.id = id;
        }
        col.favorite = root
            .pointer("/_blitz/favorite")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        col.pre_request_script = root
            .pointer("/_blitz/preRequestScript")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        col.test_script = root
            .pointer("/_blitz/testScript")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        col.icon = root
            .pointer("/_blitz/icon")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        col.color = root
            .pointer("/_blitz/color")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if let Some(vars) = root.get("variable").and_then(|v| v.as_array()) {
            col.variables = vars
                .iter()
                .filter_map(|v| {
                    Some(KeyValue {
                        key: v.get("key")?.as_str()?.to_string(),
                        value: v
                            .get("value")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        enabled: !v.get("disabled").and_then(|d| d.as_bool()).unwrap_or(false),
                    })
                })
                .collect();
        }
        // Auth: prefer rich structured `_blitz.auth`, else Postman `auth`
        if let Some(b_auth) = root
            .pointer("/_blitz/auth")
            .and_then(|v| serde_json::from_value::<AuthConfig>(v.clone()).ok())
        {
            col.auth = b_auth;
        } else {
            col.auth = import_auth_json(root.get("auth"));
        }
        if let Some(pid) = root
            .pointer("/_blitz/pinnedEnvironmentId")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
        {
            col.pinned_environment_id = Some(pid);
        }
        // Import environment overrides if present
        if let Some(env_overrides) = root.get("environmentOverrides") {
            col.environment_overrides = serde_json::from_value(env_overrides.clone())
                .unwrap_or_default();
        }
        col.items = read_order_items(dir, &item)?;
        recover_orphaned_requests(dir, &mut col);
        Ok(col)
    } else {
        // Full embedded collection.json
        Ok(import_collection_v21(&text)?)
    }
}

fn collect_request_ids(items: &[CollectionItem], ids: &mut HashSet<Uuid>) {
    for item in items {
        match item {
            CollectionItem::Request(r) => {
                ids.insert(r.id);
            }
            CollectionItem::Folder(f) => collect_request_ids(&f.items, ids),
        }
    }
}

fn parse_request_with_blitz_auth(text: &str) -> Result<RequestDraft, CloudTreeError> {
    let mut req = import_request_item(text)?;
    if let Ok(raw) = serde_json::from_str::<Value>(text) {
        if let Some(b_auth) = raw
            .pointer("/x-blitz/auth")
            .and_then(|v| serde_json::from_value::<AuthConfig>(v.clone()).ok())
        {
            req.auth = b_auth;
        }
    }
    Ok(req)
}

fn collect_request_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut list: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    list.sort();
    for p in list {
        if p.is_dir() {
            collect_request_files_recursive(&p, out);
        } else if p
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|n| n.ends_with(".request.json"))
        {
            out.push(p);
        }
    }
}

/// Recover any `*.request.json` files on disk not listed in `collection.json` (e.g. after resolving
/// a `collection.json` merge conflict) so neither teammate's newly added requests are orphaned.
fn recover_orphaned_requests(col_root: &Path, col: &mut Collection) {
    let mut seen_ids = HashSet::new();
    collect_request_ids(&col.items, &mut seen_ids);

    let mut disk_files = Vec::new();
    collect_request_files_recursive(&col_root.join("requests"), &mut disk_files);
    collect_request_files_recursive(&col_root.join("folders"), &mut disk_files);

    for path in disk_files {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(req) = parse_request_with_blitz_auth(&text) else {
            continue;
        };
        if !seen_ids.insert(req.id) {
            continue;
        }
        col.items.push(CollectionItem::Request(req));
    }
}

fn safe_child_path(root: &Path, rel: &str) -> Result<PathBuf, CloudTreeError> {
    if rel.trim().is_empty() {
        return Err(CloudTreeError::Msg("empty relative path".into()));
    }
    let rel_path = Path::new(rel);
    for comp in rel_path.components() {
        match comp {
            std::path::Component::Normal(_) | std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Err(CloudTreeError::Msg(format!(
                    "unsafe path escapes collection root: {rel}"
                )));
            }
        }
    }
    Ok(root.join(rel_path))
}

fn read_order_items(col_root: &Path, item: &Value) -> Result<Vec<CollectionItem>, CloudTreeError> {
    let arr = item
        .as_array()
        .ok_or_else(|| CloudTreeError::Msg("item must be array".into()))?;
    let mut out = Vec::new();
    for entry in arr {
        let kind = entry.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        match kind {
            "request" => {
                let path = entry
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| CloudTreeError::Msg("request missing path".into()))?;
                let req_path = safe_child_path(col_root, path)?;
                let text = fs::read_to_string(req_path)?;
                out.push(CollectionItem::Request(parse_request_with_blitz_auth(&text)?));
            }
            "folder" => {
                let path = entry
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| CloudTreeError::Msg("folder missing path".into()))?;
                let folder_dir = safe_child_path(col_root, path)?;
                let mut folder = Folder::new(
                    entry
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Folder"),
                );
                if let Some(id) = entry
                    .get("id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok())
                {
                    folder.id = id;
                }
                let meta_path = folder_dir.join("_folder.json");
                if meta_path.exists() {
                    let meta: Value = serde_json::from_str(&fs::read_to_string(meta_path)?)?;
                    if let Some(id) = meta
                        .get("id")
                        .and_then(|v| v.as_str())
                        .and_then(|s| Uuid::parse_str(s).ok())
                    {
                        folder.id = id;
                    }
                    if let Some(n) = meta.get("name").and_then(|v| v.as_str()) {
                        folder.name = n.to_string();
                    }
                    folder.description = meta
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if let Some(b_auth) = meta
                        .get("blitzAuth")
                        .and_then(|v| serde_json::from_value::<AuthConfig>(v.clone()).ok())
                    {
                        folder.auth = b_auth;
                    } else if let Some(auth_val) = meta.get("auth") {
                        folder.auth = import_auth_json(Some(auth_val));
                    }
                    folder.pre_request_script = meta
                        .get("preRequestScript")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    folder.test_script = meta
                        .get("testScript")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    folder.sort_key = meta
                        .get("sortKey")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                }
                let children = entry
                    .get("item")
                    .cloned()
                    .unwrap_or(Value::Array(vec![]));
                folder.items = read_order_items(col_root, &children)?;
                out.push(CollectionItem::Folder(folder));
            }
            _ => {
                // Unknown — skip
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::environment::EnvVar;
    use crate::core::request::RequestDraft;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("blitz-cloud-tree-{nanos}"))
    }

    #[test]
    fn slug_and_branch() {
        assert_eq!(sanitize_slug("Development"), "development");
        assert_eq!(sanitize_slug("My Env!"), "my-env");
        assert_eq!(branch_name_for_env("Staging"), "env/staging");
    }

    #[test]
    fn redact_secrets() {
        let mut env = Environment::new("Dev");
        env.values.push(EnvVar::new("baseUrl", "https://api.example.com"));
        env.values.push(EnvVar::new("api_token", "super-secret"));
        let r = redact_environment(&env);
        assert_eq!(r.values[0].current_value, "https://api.example.com");
        assert!(r.values[1].current_value.is_empty());
    }

    #[test]
    fn roundtrip_split_collection() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();
        let mut col = Collection::new("Demo");
        col.icon = Some("rocket".into());
        col.color = Some("#3b82f6".into());
        let req = RequestDraft {
            name: "Health".into(),
            url: "https://example.com/health".into(),
            ..Default::default()
        };
        let req_id = req.id;
        col.add_request(req);
        let mut folder = Folder::new("Auth");
        let login = RequestDraft {
            name: "Login".into(),
            url: "https://example.com/login".into(),
            ..Default::default()
        };
        let login_id = login.id;
        folder.add_request(login);
        col.add_folder(folder);

        let mut env = Environment::new("Development");
        env.icon = Some("server".into());
        env.color = Some("#f59e0b".into());
        env.values.push(EnvVar::new("baseUrl", "https://httpbin.org"));
        env.values.push(EnvVar::new("access_token", "do-not-leak"));

        export_workspace_tree(
            &root,
            "Demo",
            "https://github.com/h3ma209/blitz-cloud.git",
            "Development",
            &[col],
            &[env.clone()],
            &VariableContext::default(),
        )
        .unwrap();

        // Secret must not appear on disk
        let env_text = fs::read_to_string(
            root.join("environments")
                .join("development.environment.json"),
        )
        .unwrap();
        assert!(!env_text.contains("do-not-leak"));

        let (cols, envs) = import_workspace_tree(&root).unwrap();
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].icon.as_deref(), Some("rocket"));
        assert_eq!(cols[0].color.as_deref(), Some("#3b82f6"));
        assert!(cols[0].find_request(req_id).is_some());
        assert!(cols[0].find_request(login_id).is_some());
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].icon.as_deref(), Some("server"));
        assert_eq!(envs[0].color.as_deref(), Some("#f59e0b"));

        let mut imported = envs;
        merge_secret_values(&mut imported, &[env]);
        let tok = imported[0]
            .values
            .iter()
            .find(|v| v.key == "access_token")
            .unwrap();
        assert_eq!(tok.current_value, "do-not-leak");

        let _ = fs::remove_dir_all(&root);
    }

    fn concat_tree_text(root: &Path) -> String {
        let mut out = String::new();
        fn walk(dir: &Path, out: &mut String) {
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if let Ok(text) = fs::read_to_string(&path) {
                    out.push_str(&text);
                    out.push('\n');
                }
            }
        }
        walk(root, &mut out);
        out
    }

    #[test]
    fn export_workspace_omits_local_variable_map_keys() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();

        // Stale overlay from a buggy older build — export must scrub it.
        let evil = root.join(".blitz");
        fs::create_dir_all(&evil).unwrap();
        fs::write(
            evil.join("local.json"),
            r#"{"should_not_survive":"x"}"#,
        )
        .unwrap();

        let mut col = Collection::new("Demo");
        col.icon = Some("rocket".into());
        col.color = Some("#3b82f6".into());
        col.variables.push(KeyValue::new("colKey", "colVal"));
        let mut env = Environment::new("Development");
        env.icon = Some("server".into());
        env.color = Some("#f59e0b".into());
        env.values.push(EnvVar::new("baseUrl", "https://httpbin.org"));

        let mut vars = VariableContext::default();
        vars.local.insert(
            "zz_local_only_sync_probe".into(),
            "NEVER_SYNC_THIS_LOCAL_VALUE_9f3a".into(),
        );
        vars.global
            .insert("userAgent".into(), "Blitz/test".into());

        export_workspace_tree(
            &root,
            "Demo",
            "https://github.com/h3ma209/blitz-cloud.git",
            "Development",
            &[col],
            &[env],
            &vars,
        )
        .unwrap();

        assert!(!root.join(".blitz/local.json").exists());
        let blob = concat_tree_text(&root);
        assert!(
            !blob.contains("zz_local_only_sync_probe"),
            "local map keys must not appear in cloud tree"
        );
        assert!(
            !blob.contains("NEVER_SYNC_THIS_LOCAL_VALUE_9f3a"),
            "local map values must not appear in cloud tree"
        );
        // Globals may sync.
        let globals = fs::read_to_string(root.join(".blitz/globals.json")).unwrap();
        assert!(globals.contains("userAgent"));
        assert!(globals.contains("Blitz/test"));

        let _ = fs::remove_dir_all(&root);
    }

}
