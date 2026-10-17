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

/// Seed environments used when snapshot has none.
pub fn demo_environments() -> Vec<Environment> {
    let mut dev = Environment::new("Development");
    dev.values.push(EnvVar::new("baseUrl", "https://httpbin.org"));
    let mut staging = Environment::new("Staging");
    staging
        .values
        .push(EnvVar::new("baseUrl", "https://httpbin.org"));
    vec![dev, staging]
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
}
