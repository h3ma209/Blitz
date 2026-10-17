//! Variable scopes: global / collection / environment / local / vault.
//! Resolution order matches Postman product-docs convention (reports/04, 07).
//! `{{vault:key}}` resolved via vault map (file / SecretsStore) — see `vault.rs`.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

use super::vault::{resolve_vault_key, VaultStore};
use crate::secrets::SecretsStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableScope {
    Global,
    Collection,
    Environment,
    Local,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VariableContext {
    pub global: HashMap<String, String>,
    pub collection: HashMap<String, String>,
    pub environment: HashMap<String, String>,
    pub local: HashMap<String, String>,
    /// In-memory vault keys (loaded from `vault.json` at startup).
    #[serde(default, skip)]
    pub vault: VaultStore,
}

impl VariableContext {
    pub fn demo() -> Self {
        let mut ctx = Self::default();
        ctx.environment
            .insert("baseUrl".into(), "https://httpbin.org".into());
        ctx.global
            .insert("userAgent".into(), "notPostman/0.1".into());
        ctx
    }

    /// Resolve `{{name}}` with precedence: local > environment > collection > global.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.local
            .get(name)
            .or_else(|| self.environment.get(name))
            .or_else(|| self.collection.get(name))
            .or_else(|| self.global.get(name))
            .map(|s| s.as_str())
    }

    pub fn resolve_string(&self, input: &str) -> String {
        self.resolve_string_with_secrets(input, None)
    }

    /// Like [`resolve_string`], but `{{vault:key}}` may fall back to SecretsStore.
    pub fn resolve_string_with_secrets(
        &self,
        input: &str,
        secrets: Option<&dyn SecretsStore>,
    ) -> String {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| Regex::new(r"\{\{\s*([^}]+?)\s*\}\}").unwrap());

        re.replace_all(input, |caps: &regex::Captures| {
            let raw = caps[1].trim();
            if let Some(key) = raw.strip_prefix("vault:") {
                resolve_vault_key(&self.vault, secrets, key.trim())
                    .unwrap_or_default()
            } else {
                self.get(raw).unwrap_or("").to_string()
            }
        })
        .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::{service_name, InMemorySecrets};
    use crate::core::vault::VAULT_ACCOUNT_ID;

    #[test]
    fn resolve_env_var() {
        let ctx = VariableContext::demo();
        assert_eq!(
            ctx.resolve_string("{{baseUrl}}/get"),
            "https://httpbin.org/get"
        );
    }

    #[test]
    fn local_overrides_env() {
        let mut ctx = VariableContext::demo();
        ctx.local
            .insert("baseUrl".into(), "http://localhost".into());
        assert_eq!(ctx.resolve_string("{{baseUrl}}"), "http://localhost");
    }

    #[test]
    fn vault_prefix_from_store() {
        let mut ctx = VariableContext::default();
        ctx.vault.set("token", "v-123");
        assert_eq!(ctx.resolve_string("Bearer {{vault:token}}"), "Bearer v-123");
    }

    #[test]
    fn vault_falls_back_to_secrets() {
        let ctx = VariableContext::default();
        let store = InMemorySecrets::default();
        store
            .set(&service_name(VAULT_ACCOUNT_ID), "k", "from-store")
            .unwrap();
        assert_eq!(
            ctx.resolve_string_with_secrets("{{vault:k}}", Some(&store)),
            "from-store"
        );
    }
}
