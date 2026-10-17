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
    #[serde(default)]
    pub values: Vec<EnvVar>,
}

impl Environment {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
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
    // Prefer canonical order for the three defaults (others keep relative order after).
    if changed || environments.len() >= 3 {
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

pub fn find_environment<'a>(
    environments: &'a [Environment],
    id: Option<Uuid>,
) -> Option<&'a Environment> {
    id.and_then(|id| environments.iter().find(|e| e.id == id))
}

pub fn find_environment_by_name<'a>(
    environments: &'a [Environment],
    name: &str,
) -> Option<&'a Environment> {
    environments.iter().find(|e| e.name == name)
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
        // Postman sometimes has separate initial
        if let Some(init) = v.get("initial").and_then(|x| x.as_str()) {
            ev.initial_value = init.to_string();
        }
        ev.enabled = enabled;
        env.values.push(ev);
    }
    Ok(env)
}

/// Export environment as Postman-compatible JSON.
pub fn export_environment_json(env: &Environment) -> Result<String, String> {
    let values: Vec<serde_json::Value> = env
        .values
        .iter()
        .map(|v| {
            serde_json::json!({
                "key": v.key,
                "value": v.current_value,
                "enabled": v.enabled,
                "type": "default",
            })
        })
        .collect();
    let doc = serde_json::json!({
        "id": env.id.to_string(),
        "name": env.name,
        "values": values,
        "_postman_variable_scope": "environment",
    });
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
}
