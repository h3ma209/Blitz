//! Local vault file + `{{vault:key}}` resolution (beside `secret://`).
//!
//! File: `{data_dir}/evilpostman/vault.json` — flat `{ "key": "value", … }`.
//! Also falls back to SecretsStore service `EvilPostman (vault)` / account=`key`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::secrets::{service_name, SecretsStore};

/// Keychain / store account_id used for vault entries.
pub const VAULT_ACCOUNT_ID: &str = "vault";

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultStore {
    #[serde(default)]
    pub entries: HashMap<String, String>,
}

impl VaultStore {
    pub fn path_under(root: &Path) -> PathBuf {
        root.join("vault.json")
    }

    pub fn load(root: &Path) -> Result<Self, VaultError> {
        let path = Self::path_under(root);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save(&self, root: &Path) -> Result<(), VaultError> {
        fs::create_dir_all(root)?;
        let path = Self::path_under(root);
        let text = serde_json::to_string_pretty(self)?;
        fs::write(path, text)?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(|s| s.as_str())
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.entries.insert(key.into(), value.into());
    }
}

/// Resolve a vault key: in-memory/file map first, then SecretsStore.
pub fn resolve_vault_key(
    vault: &VaultStore,
    store: Option<&dyn SecretsStore>,
    key: &str,
) -> Option<String> {
    if let Some(v) = vault.get(key) {
        return Some(v.to_string());
    }
    if let Some(store) = store {
        if let Ok(v) = store.get(&service_name(VAULT_ACCOUNT_ID), key) {
            return Some(v);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::InMemorySecrets;
    use std::env;

    #[test]
    fn vault_file_roundtrip() {
        let dir = env::temp_dir().join(format!("evilpostman-vault-{}", uuid::Uuid::new_v4()));
        let mut v = VaultStore::default();
        v.set("api_token", "tok-1");
        v.save(&dir).unwrap();
        let loaded = VaultStore::load(&dir).unwrap();
        assert_eq!(loaded.get("api_token"), Some("tok-1"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_falls_back_to_secrets() {
        let vault = VaultStore::default();
        let store = InMemorySecrets::default();
        store
            .set(&service_name(VAULT_ACCOUNT_ID), "secret_key", "from-kc")
            .unwrap();
        assert_eq!(
            resolve_vault_key(&vault, Some(&store), "secret_key").as_deref(),
            Some("from-kc")
        );
    }
}
