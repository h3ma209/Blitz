//! Split workspace tree writer/reader (one file per request).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use uuid::Uuid;

use crate::core::collection::{Collection, CollectionItem, Folder};
use crate::core::environment::{import_environment_json, Environment};
use crate::core::impex::{
    export_request_item, import_collection_v21, import_request_item, ImpexError,
};
use crate::core::request::{AuthConfig, AuthType, KeyValue};

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
    #[serde(default)]
    pub default_env: String,
    #[serde(default)]
    pub remote: String,
}

pub const DEFAULT_GITIGNORE: &str = r#"# EvilPostman local / secrets — never commit
.evilpostman/local.json
.evilpostman/cache/
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

pub const DEFAULT_README: &str = r#"# EvilPostman cloud workspace

Collections and environments synced from [EvilPostman](https://github.com/h3ma209/evilpostman-cloud).

- **Environments = git branches** named `env/<sanitized-env-name>`
- Secret values are redacted in `environments/*.environment.json` (local vault / snapshot only)
- One request per `*.request.json` for merge-friendly diffs

Do not commit `vault.json`, cookies, or history.
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
pub fn branch_name_for_env(env_name: &str) -> String {
    let slug = sanitize_slug(env_name);
    if slug.is_empty() || slug == "unnamed" {
        "env/no-environment".into()
    } else {
        format!("env/{slug}")
    }
}

fn unique_slug(base: &str, used: &mut HashSet<String>) -> String {
    let slug = sanitize_slug(base);
    if used.insert(slug.clone()) {
        return slug;
    }
    let mut n = 2u32;
    loop {
        let candidate = format!("{slug}-{n}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
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
                "enabled": v.enabled,
                "type": if is_secret { "secret" } else { "default" },
            })
        })
        .collect();
    let doc = json!({
        "id": redacted.id.to_string(),
        "name": redacted.name,
        "values": values,
        "_postman_variable_scope": "environment",
        "_evilpostman_redacted": true,
    });
    Ok(serde_json::to_string_pretty(&doc)?)
}

/// Write full workspace tree under `root` (git working tree).
pub fn export_workspace_tree(
    root: &Path,
    project_name: &str,
    remote: &str,
    default_env: &str,
    collections: &[Collection],
    environments: &[Environment],
) -> Result<(), CloudTreeError> {
    let evil = root.join(".evilpostman");
    fs::create_dir_all(&evil)?;
    fs::create_dir_all(root.join("collections"))?;
    fs::create_dir_all(root.join("environments"))?;

    let meta = ProjectMeta {
        schema_version: 1,
        name: project_name.to_string(),
        default_env: default_env.to_string(),
        remote: remote.to_string(),
    };
    fs::write(
        evil.join("project.json"),
        serde_json::to_string_pretty(&meta)?,
    )?;

    let gi = root.join(".gitignore");
    if !gi.exists() {
        fs::write(&gi, DEFAULT_GITIGNORE)?;
    }
    let readme = root.join("README.md");
    if !readme.exists() {
        fs::write(&readme, DEFAULT_README)?;
    }

    // Wipe previous collection dirs for clean rewrite (same slug set).
    let cols_dir = root.join("collections");
    if cols_dir.is_dir() {
        for entry in fs::read_dir(&cols_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                fs::remove_dir_all(entry.path())?;
            }
        }
    }

    let mut used_col_slugs = HashSet::new();
    for col in collections {
        let slug = unique_slug(&col.name, &mut used_col_slugs);
        write_collection_split(&cols_dir.join(&slug), col)?;
    }

    // Environments
    let env_dir = root.join("environments");
    if env_dir.is_dir() {
        for entry in fs::read_dir(&env_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".environment.json") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    let mut used_env_slugs = HashSet::new();
    for env in environments {
        let slug = unique_slug(&env.name, &mut used_env_slugs);
        let path = env_dir.join(format!("{slug}.environment.json"));
        fs::write(path, export_env_redacted_json(env)?)?;
    }

    Ok(())
}

fn write_collection_split(dir: &Path, col: &Collection) -> Result<(), CloudTreeError> {
    fs::create_dir_all(dir)?;
    fs::create_dir_all(dir.join("folders"))?;
    fs::create_dir_all(dir.join("requests"))?;

    let mut used = HashSet::new();
    let order = write_items(dir, &col.items, Path::new(""), &mut used)?;

    let variables: Vec<Value> = col
        .variables
        .iter()
        .filter(|v| !v.key.is_empty())
        .map(|v| {
            json!({
                "key": v.key,
                "value": v.value,
                "disabled": !v.enabled,
            })
        })
        .collect();

    let auth = export_auth_json(&col.auth);
    let doc = json!({
        "info": {
            "name": col.name,
            "description": col.description,
            "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json",
            "_postman_id": col.id.to_string(),
        },
        "auth": auth,
        "variable": variables,
        "item": order,
        "_evilpostman": {
            "favorite": col.favorite,
            "preRequestScript": col.pre_request_script,
            "testScript": col.test_script,
            "split": true,
        }
    });
    fs::write(dir.join("collection.json"), serde_json::to_string_pretty(&doc)?)?;
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

/// Write items; return order refs for collection.json.
fn write_items(
    col_root: &Path,
    items: &[CollectionItem],
    folder_rel: &Path,
    used: &mut HashSet<String>,
) -> Result<Vec<Value>, CloudTreeError> {
    let mut order = Vec::new();
    for item in items {
        match item {
            CollectionItem::Request(r) => {
                let slug = unique_slug(&r.name, used);
                let file_name = format!("{slug}.request.json");
                let rel = if folder_rel.as_os_str().is_empty() {
                    PathBuf::from("requests").join(&file_name)
                } else {
                    PathBuf::from("folders")
                        .join(folder_rel)
                        .join(&file_name)
                };
                let abs = col_root.join(&rel);
                if let Some(parent) = abs.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut val = export_request_item(r);
                // Redact auth secrets in request file
                if let Some(req) = val.get_mut("request") {
                    if let Some(auth) = req.get_mut("auth") {
                        redact_auth_value(auth);
                    }
                }
                fs::write(abs, serde_json::to_string_pretty(&val)?)?;
                order.push(json!({
                    "kind": "request",
                    "path": path_to_str(&rel),
                    "id": r.id.to_string(),
                    "name": r.name,
                }));
            }
            CollectionItem::Folder(f) => {
                let slug = unique_slug(&f.name, used);
                let child_rel = if folder_rel.as_os_str().is_empty() {
                    PathBuf::from(&slug)
                } else {
                    folder_rel.join(&slug)
                };
                let folder_dir = col_root.join("folders").join(&child_rel);
                fs::create_dir_all(&folder_dir)?;
                let meta = json!({
                    "id": f.id.to_string(),
                    "name": f.name,
                    "preRequestScript": f.pre_request_script,
                    "testScript": f.test_script,
                    "sortKey": f.sort_key,
                });
                fs::write(
                    folder_dir.join("_folder.json"),
                    serde_json::to_string_pretty(&meta)?,
                )?;
                let mut child_used = HashSet::new();
                let children = write_items(col_root, &f.items, &child_rel, &mut child_used)?;
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

    Ok((collections, environments))
}

fn read_collection_split(dir: &Path) -> Result<Collection, CloudTreeError> {
    let text = fs::read_to_string(dir.join("collection.json"))?;
    let root: Value = serde_json::from_str(&text)?;

    // Prefer split order; fall back to full v2.1 item[] if present without refs.
    let item = root.get("item").cloned().unwrap_or(Value::Array(vec![]));
    let is_split = root
        .pointer("/_evilpostman/split")
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
            .pointer("/_evilpostman/favorite")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        col.pre_request_script = root
            .pointer("/_evilpostman/preRequestScript")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        col.test_script = root
            .pointer("/_evilpostman/testScript")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
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
        // Auth: leave Inherit/NoAuth; secrets stay local
        col.auth = AuthConfig {
            auth_type: AuthType::NoAuth,
            ..Default::default()
        };
        col.items = read_order_items(dir, &item)?;
        Ok(col)
    } else {
        // Full embedded collection.json
        Ok(import_collection_v21(&text)?)
    }
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
                let text = fs::read_to_string(col_root.join(path))?;
                out.push(CollectionItem::Request(import_request_item(&text)?));
            }
            "folder" => {
                let path = entry
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| CloudTreeError::Msg("folder missing path".into()))?;
                let folder_dir = col_root.join(path);
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
        std::env::temp_dir().join(format!("evilpostman-cloud-tree-{nanos}"))
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
        let mut req = RequestDraft::default();
        req.name = "Health".into();
        req.url = "https://example.com/health".into();
        let req_id = req.id;
        col.add_request(req);
        let mut folder = Folder::new("Auth");
        let mut login = RequestDraft::default();
        login.name = "Login".into();
        login.url = "https://example.com/login".into();
        let login_id = login.id;
        folder.add_request(login);
        col.add_folder(folder);

        let mut env = Environment::new("Development");
        env.values.push(EnvVar::new("baseUrl", "https://httpbin.org"));
        env.values.push(EnvVar::new("access_token", "do-not-leak"));

        export_workspace_tree(
            &root,
            "Demo",
            "https://github.com/h3ma209/evilpostman-cloud.git",
            "Development",
            &[col],
            &[env.clone()],
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
        assert!(cols[0].find_request(req_id).is_some());
        assert!(cols[0].find_request(login_id).is_some());
        assert_eq!(envs.len(), 1);

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

}
