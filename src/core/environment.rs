//! Named environments with initial/current variable values.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvVar {
    pub key: String,
    #[serde(default)]
    pub initial_value: String,
    #[serde(default)]
    pub current_value: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl EnvVar {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            key: key.into(),
            initial_value: value.clone(),
            current_value: value,
            enabled: true,
        }
    }

    /// Prefer current; fall back to initial.
    pub fn effective(&self) -> &str {
        if !self.current_value.is_empty() {
            &self.current_value
        } else {
            &self.initial_value
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default)]
    pub values: Vec<EnvVar>,
}

impl Environment {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            icon: None,
            color: None,
            values: vec![],
        }
    }

    pub fn to_map(&self) -> HashMap<String, String> {
        self.values
            .iter()
            .filter(|v| v.enabled && !v.key.is_empty())
            .map(|v| (v.key.clone(), v.effective().to_string()))
            .collect()
    }
}

/// Canonical default environment names (also map to `env/<slug>` git branches).
pub const DEFAULT_ENV_NAMES: &[&str] = &["Local", "Development", "Production"];

fn seed_env(name: &str) -> Environment {
    let mut env = Environment::new(name);
    let base = match name {
        "Local" => "http://127.0.0.1:3000",
        "Production" => "https://api.example.com",
        _ => "https://httpbin.org",
    };
    env.values.push(EnvVar::new("baseUrl", base));
    env
}

/// Seed environments used when snapshot has none.
pub fn demo_environments() -> Vec<Environment> {
    DEFAULT_ENV_NAMES.iter().map(|n| seed_env(n)).collect()
}

/// Ensure Local / Development / Production exist. Does **not** remove other envs.
/// Returns `true` if any env was added.
pub fn ensure_default_environments(environments: &mut Vec<Environment>) -> bool {
    let mut changed = false;
    for &name in DEFAULT_ENV_NAMES {
        let exists = environments
            .iter()
            .any(|e| e.name.eq_ignore_ascii_case(name));
        if !exists {
            environments.push(seed_env(name));
            changed = true;
        }
    }
    // Only normalize default order when seeding missing defaults; preserve user drag-reorder.
    if changed {
        let mut ordered = Vec::with_capacity(environments.len());
        for &name in DEFAULT_ENV_NAMES {
            if let Some(pos) = environments
                .iter()
                .position(|e| e.name.eq_ignore_ascii_case(name))
            {
                ordered.push(environments.remove(pos));
            }
        }
        ordered.append(environments);
        *environments = ordered;
    }
    changed
}

fn merge_env_into(existing: &mut Environment, incoming: Environment) {
    if existing.name.trim().is_empty() && !incoming.name.trim().is_empty() {
        existing.name = incoming.name;
    }
    for iv in incoming.values {
        if iv.key.trim().is_empty() {
            continue;
        }
        if let Some(ev) = existing
            .values
            .iter_mut()
            .find(|v| v.key.trim() == iv.key.trim())
        {
            ev.initial_value = iv.initial_value;
            if !iv.current_value.is_empty() || ev.current_value.is_empty() {
                ev.current_value = iv.current_value;
            }
            ev.enabled = iv.enabled;
        } else {
            existing.values.push(iv);
        }
    }
}

/// Collapse duplicate environment entries sharing the same UUID by merging variables in place.
/// Returns the number of duplicate entries removed.
pub fn deduplicate_environments_by_id(envs: &mut Vec<Environment>) -> usize {
    let original_len = envs.len();
    let mut out: Vec<Environment> = Vec::with_capacity(original_len);
    for env in envs.drain(..) {
        if let Some(existing) = out.iter_mut().find(|e| e.id == env.id) {
            merge_env_into(existing, env);
        } else {
            out.push(env);
        }
    }
    *envs = out;
    original_len.saturating_sub(envs.len())
}

/// Upsert imported environments into `store` by matching UUID or case-insensitive name.
/// Updates existing variables in place (preserving local non-empty `current_value` if incoming
/// `current_value` is empty) instead of duplicating environments.
pub fn upsert_environments(
    store: &mut Vec<Environment>,
    incoming: Vec<Environment>,
) -> (Vec<Uuid>, usize, usize) {
    let mut ids = Vec::with_capacity(incoming.len());
    let mut created = 0usize;
    let mut updated = 0usize;

    for inc in incoming {
        let pos = store
            .iter()
            .position(|e| e.id == inc.id)
            .or_else(|| {
                let name = inc.name.trim();
                if name.is_empty() {
                    None
                } else {
                    store
                        .iter()
                        .position(|e| e.name.trim().eq_ignore_ascii_case(name))
                }
            });

        if let Some(idx) = pos {
            let existing = &mut store[idx];
            merge_env_into(existing, inc);
            ids.push(existing.id);
            updated += 1;
        } else {
            let id = inc.id;
            store.push(inc);
            ids.push(id);
            created += 1;
        }
    }

    (ids, created, updated)
}

/// Move an environment from `from_idx` to `to_idx` in place. Returns `true` if order changed.
pub fn reorder_environments(
    envs: &mut Vec<Environment>,
    from_idx: usize,
    to_idx: usize,
) -> bool {
    if from_idx >= envs.len() || to_idx >= envs.len() || from_idx == to_idx {
        return false;
    }
    let item = envs.remove(from_idx);
    envs.insert(to_idx, item);
    true
}

pub fn find_environment(
    environments: &[Environment],
    id: Option<Uuid>,
) -> Option<&Environment> {
    id.and_then(|id| environments.iter().find(|e| e.id == id))
}

/// Import Postman environment export JSON (`{ "name", "values": [{key,value,enabled,type}] }`).
pub fn import_environment_json(text: &str) -> Result<Environment, String> {
    let root: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("json: {e}"))?;
    // Detect collection mistakenly passed
    if root.pointer("/info/schema").is_some() {
        return Err("looks like a collection, not an environment".into());
    }
    let name = root
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Imported Environment")
        .to_string();
    let mut env = Environment::new(name);
    env.icon = root
        .get("_blitz_icon")
        .or_else(|| root.get("icon"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    env.color = root
        .get("_blitz_color")
        .or_else(|| root.get("color"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if let Some(id) = root.get("id").and_then(|v| v.as_str()) {
        if let Ok(u) = uuid::Uuid::parse_str(id) {
            env.id = u;
        }
    }
    let values = root
        .get("values")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for v in values {
        let key = v
            .get("key")
            .and_then(|k| k.as_str())
            .unwrap_or("")
            .to_string();
        if key.is_empty() {
            continue;
        }
        let value = v
            .get("value")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let enabled = v.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true);
        let mut ev = EnvVar::new(key, value);
        // Postman / Blitz redacted exports may supply separate `initialValue` or `initial`
        if let Some(init) = v
            .get("initialValue")
            .or_else(|| v.get("initial"))
            .and_then(|x| x.as_str())
        {
            ev.initial_value = init.to_string();
        }
        ev.enabled = enabled;
        env.values.push(ev);
    }
    Ok(env)
}

/// Import one or more Postman environment export JSON documents.
///
/// Each input is parsed independently; a failure on one file does not abort the
/// rest (the error is attached to that file's result). Returns the successfully
/// imported environments in input order.
pub fn import_environment_texts(texts: &[String]) -> Result<Vec<Environment>, String> {
    let mut out = Vec::with_capacity(texts.len());
    let mut first_err: Option<String> = None;
    for (i, text) in texts.iter().enumerate() {
        match import_environment_json(text) {
            Ok(env) => out.push(env),
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(format!("file {i}: {e}"));
                }
            }
        }
    }
    if out.is_empty() {
        return Err(first_err.unwrap_or_else(|| "no environments in selection".into()));
    }
    Ok(out)
}

/// Export environment as Postman-compatible JSON.
pub fn export_environment_json(env: &Environment) -> Result<String, String> {
    let values: Vec<serde_json::Value> = env
        .values
        .iter()
        .map(|v| {
            serde_json::json!({
                "key": v.key,
                "value": v.effective(),
                "enabled": v.enabled,
                "type": "default",
            })
        })
        .collect();
    let mut doc = serde_json::json!({
        "id": env.id.to_string(),
        "name": env.name,
        "values": values,
        "_postman_variable_scope": "environment",
    });
    if let Some(icon) = env.icon.as_deref().filter(|s| !s.trim().is_empty()) {
        if let Some(obj) = doc.as_object_mut() {
            obj.insert("_blitz_icon".into(), serde_json::json!(icon));
        }
    }
    if let Some(color) = env.color.as_deref().filter(|s| !s.trim().is_empty()) {
        if let Some(obj) = doc.as_object_mut() {
            obj.insert("_blitz_color".into(), serde_json::json!(color));
        }
    }
    serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_overrides_initial() {
        let mut v = EnvVar::new("x", "init");
        v.current_value = "now".into();
        assert_eq!(v.effective(), "now");
    }

    #[test]
    fn to_map_skips_disabled() {
        let mut env = Environment::new("t");
        env.values.push(EnvVar::new("a", "1"));
        let mut disabled = EnvVar::new("b", "2");
        disabled.enabled = false;
        env.values.push(disabled);
        let map = env.to_map();
        assert_eq!(map.get("a").map(String::as_str), Some("1"));
        assert!(!map.contains_key("b"));
    }

    #[test]
    fn ensure_defaults_adds_without_wipe() {
        let mut envs = vec![Environment::new("Custom")];
        assert!(ensure_default_environments(&mut envs));
        assert_eq!(envs.len(), 4);
        assert_eq!(envs[0].name, "Local");
        assert_eq!(envs[1].name, "Development");
        assert_eq!(envs[2].name, "Production");
        assert_eq!(envs[3].name, "Custom");
        assert!(!ensure_default_environments(&mut envs));
    }

    #[test]
    fn import_export_postman_env() {
        let sample = r#"{
            "id": "11111111-1111-1111-1111-111111111111",
            "name": "Initiate Envs",
            "values": [
                {"key": "baseUrl", "value": "https://api.example.com", "enabled": true},
                {"key": "token_hakki", "value": "", "enabled": true}
            ]
        }"#;
        let env = import_environment_json(sample).unwrap();
        assert_eq!(env.name, "Initiate Envs");
        assert_eq!(env.values.len(), 2);
        let json = export_environment_json(&env).unwrap();
        let again = import_environment_json(&json).unwrap();
        assert_eq!(again.values.len(), 2);
    }

    #[test]
    fn export_environment_preserves_initial_value_when_current_empty() {
        let mut env = Environment::new("TestEnv");
        let mut v = EnvVar::new("API_KEY", "secret_123");
        v.current_value = String::new(); // User didn't overwrite current
        env.values.push(v);

        let json = export_environment_json(&env).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["values"][0]["value"], "secret_123");
    }

    #[test]
    fn upsert_environments_updates_matching_without_duplicating() {
        let mut store = vec![Environment::new("Production")];
        store[0].values.push(EnvVar::new("baseUrl", "https://old.api"));
        store[0].values.push(EnvVar {
            key: "secret".into(),
            initial_value: "init".into(),
            current_value: "local-override".into(),
            enabled: true,
        });

        let mut inc = Environment::new("production");
        inc.values.push(EnvVar::new("baseUrl", "https://new.api"));
        inc.values.push(EnvVar::new("secret", ""));
        inc.values.push(EnvVar::new("apiKey", "k-1"));

        let (ids, created, updated) = upsert_environments(&mut store, vec![inc]);
        assert_eq!(store.len(), 1, "must not duplicate env");
        assert_eq!(created, 0);
        assert_eq!(updated, 1);
        assert_eq!(ids, vec![store[0].id]);
        assert_eq!(store[0].values.iter().find(|v| v.key == "baseUrl").unwrap().current_value, "https://new.api");
        assert_eq!(store[0].values.iter().find(|v| v.key == "secret").unwrap().current_value, "local-override");
        assert_eq!(store[0].values.iter().find(|v| v.key == "apiKey").unwrap().current_value, "k-1");
    }

    #[test]
    fn deduplicate_environments_by_id_merges_and_collapses() {
        let shared = Uuid::new_v4();
        let mut a = Environment::new("Cloud API");
        a.id = shared;
        a.values.push(EnvVar::new("token", "local"));

        let mut b = Environment::new("Cloud API");
        b.id = shared;
        b.values.push(EnvVar::new("token", ""));
        b.values.push(EnvVar::new("baseUrl", "https://api.example.com"));

        let mut list = vec![a, b, Environment::new("Other")];
        let removed = deduplicate_environments_by_id(&mut list);
        assert_eq!(removed, 1);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].values.len(), 2);
        assert_eq!(
            list[0].values.iter().find(|v| v.key == "token").unwrap().current_value,
            "local"
        );
    }

    #[test]
    fn environment_icon_serde_roundtrip() {
        let mut env = Environment::new("Production");
        let json_none = serde_json::to_string(&env).unwrap();
        assert!(!json_none.contains("\"icon\""));
        let decoded_none: Environment = serde_json::from_str(&json_none).unwrap();
        assert_eq!(decoded_none.icon, None);

        env.icon = Some("rocket".to_string());
        let json_some = serde_json::to_string(&env).unwrap();
        assert!(json_some.contains("\"icon\":\"rocket\""));
        let decoded_some: Environment = serde_json::from_str(&json_some).unwrap();
        assert_eq!(decoded_some.icon.as_deref(), Some("rocket"));
    }

    #[test]
    fn reorder_environments_moves_in_place() {
        let mut envs = vec![
            Environment::new("Local"),
            Environment::new("Staging"),
            Environment::new("Production"),
        ];
        assert!(reorder_environments(&mut envs, 2, 0));
        assert_eq!(
            envs.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            vec!["Production", "Local", "Staging"]
        );
        assert!(!reorder_environments(&mut envs, 1, 1));
        assert!(!reorder_environments(&mut envs, 9, 0));
    }
}
