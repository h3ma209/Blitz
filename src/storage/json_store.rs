use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::core::{
    bundled_postman_collections, demo_environments, ensure_default_environments,
    flatten_collections, AppSettings, CookieJar, Environment, HistoryEntry, TreeRowKind,
    VariableContext, VaultStore,
};

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSnapshot {
    #[serde(default)]
    pub collections: Vec<CollectionCompat>,
    #[serde(default)]
    pub variables: VariableContext,
    /// Legacy display name (kept for older snapshots).
    #[serde(default)]
    pub active_environment: String,
    #[serde(default)]
    pub environments: Vec<Environment>,
    #[serde(default)]
    pub active_environment_id: Option<Uuid>,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
    #[serde(default)]
    pub cookies: CookieJar,
    /// Last selected collection (sidebar).
    #[serde(default)]
    pub active_collection_id: Option<Uuid>,
    /// Last opened request in the workbench.
    #[serde(default)]
    pub active_request_id: Option<Uuid>,
    /// Proxy / client-cert settings.
    #[serde(default)]
    pub settings: AppSettings,
    /// Soft-deleted collections (Trash).
    #[serde(default)]
    pub trash: Vec<CollectionCompat>,
    /// Open tabs with unsaved drafts.
    #[serde(default)]
    pub open_tabs: Vec<crate::core::RequestDraft>,
    /// Cached responses per request id for restoration.
    #[serde(default)]
    pub responses: std::collections::HashMap<Uuid, crate::core::ResponseView>,
    /// Panel open states restored on startup.
    #[serde(default)]
    pub show_vars: bool,
    #[serde(default)]
    pub show_settings: bool,
    #[serde(default)]
    pub show_runner: bool,
    #[serde(default)]
    pub show_perf: bool,
    #[serde(default)]
    pub show_env_manager: bool,
    #[serde(default)]
    pub show_col_vars: bool,
}

/// Alias so serde still deserializes `collections` as our Collection type.
pub type CollectionCompat = crate::core::Collection;

/// File-backed store under `~/.local/share/blitz` (or platform equivalent).
pub struct JsonStore {
    root: PathBuf,
}

impl JsonStore {
    pub fn open_default() -> Result<Self, StorageError> {
        let root = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("blitz");
        Self::open(root)
    }

    pub fn open(root: impl AsRef<Path>) -> Result<Self, StorageError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn snapshot_path(&self) -> PathBuf {
        self.root.join("snapshot.json")
    }

    pub fn load(&self) -> Result<AppSnapshot, StorageError> {
        let path = self.snapshot_path();
        if !path.exists() {
            return Ok(Self::default_snapshot());
        }
        let text = fs::read_to_string(&path)?;
        let mut snap: AppSnapshot = serde_json::from_str(&text)?;
        if snap.collections.is_empty() {
            snap.collections = bundled_postman_collections();
        }
        if snap.environments.is_empty() {
            snap.environments = demo_environments();
            // Migrate flat environment map into Development if present.
            if !snap.variables.environment.is_empty() {
                if let Some(dev) = snap.environments.iter_mut().find(|e| e.name == "Development")
                {
                    for (k, v) in snap.variables.environment.iter() {
                        if !dev.values.iter().any(|ev| ev.key == *k) {
                            dev.values.push(crate::core::EnvVar::new(k, v));
                        }
                    }
                }
            }
        } else {
            // Always keep Local / Development / Production present (don't wipe others).
            let _ = ensure_default_environments(&mut snap.environments);
        }
        if snap.active_environment_id.is_none() {
            if !snap.active_environment.is_empty() {
                snap.active_environment_id = snap
                    .environments
                    .iter()
                    .find(|e| e.name == snap.active_environment)
                    .map(|e| e.id);
            }
            if snap.active_environment_id.is_none() {
                snap.active_environment_id = snap.environments.first().map(|e| e.id);
            }
        }
        if let Some(id) = snap.active_environment_id {
            if let Some(env) = snap.environments.iter().find(|e| e.id == id) {
                snap.active_environment = env.name.clone();
                snap.variables.environment = env.to_map();
            }
        } else if snap.active_environment.is_empty() {
            snap.active_environment = "No Environment".into();
        }
        if snap.variables.global.is_empty() && snap.variables.environment.is_empty() {
            let demo = VariableContext::demo();
            if snap.variables.global.is_empty() {
                snap.variables.global = demo.global;
            }
        }
        // Load local vault.json into variable context (not serialized in snapshot).
        if let Ok(vault) = VaultStore::load(&self.root) {
            snap.variables.vault = vault;
        }
        Ok(snap)
    }

    pub fn save(&self, snapshot: &AppSnapshot) -> Result<(), StorageError> {
        let path = self.snapshot_path();
        let tmp_path = self.root.join("snapshot.json.tmp");
        let text = serde_json::to_string_pretty(snapshot)?;
        fs::write(&tmp_path, text)?;
        fs::rename(&tmp_path, &path)?;
        Ok(())
    }

    pub fn default_snapshot() -> AppSnapshot {
        let collections = bundled_postman_collections();
        let environments = demo_environments();
        let active_environment_id = environments.first().map(|e| e.id);
        let active_environment = environments
            .first()
            .map(|e| e.name.clone())
            .unwrap_or_else(|| "Development".into());
        let mut variables = VariableContext::demo();
        if let Some(env) = environments.first() {
            variables.environment = env.to_map();
        }
        let active_collection_id = collections.first().map(|c| c.id);
        let active_request_id = flatten_collections(&collections)
            .into_iter()
            .find(|row| matches!(row.kind, TreeRowKind::Request { .. }))
            .map(|row| row.id);
        AppSnapshot {
            collections,
            variables,
            active_environment,
            environments,
            active_environment_id,
            history: vec![],
            cookies: CookieJar::default(),
            active_collection_id,
            active_request_id,
            settings: AppSettings::default(),
            trash: vec![],
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::RequestDraft;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("blitz-test-{nanos}"))
    }

    #[test]
    fn roundtrip_collections_schema() {
        let root = temp_root();
        let store = JsonStore::open(&root).unwrap();
        let mut snap = JsonStore::default_snapshot();
        let draft = RequestDraft {
            name: "persisted".into(),
            url: "https://example.com/x".into(),
            ..Default::default()
        };
        snap.collections[0].add_request(draft.clone());
        snap.active_request_id = Some(draft.id);
        store.save(&snap).unwrap();

        let loaded = store.load().unwrap();
        assert!(loaded
            .collections
            .iter()
            .any(|c| c.find_request(draft.id).is_some()));
        assert_eq!(loaded.active_request_id, Some(draft.id));
        assert!(!loaded.environments.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn load_missing_file_seeds_demo() {
        let root = temp_root();
        let store = JsonStore::open(&root).unwrap();
        let snap = store.load().unwrap();
        assert!(!snap.collections.is_empty());
        assert!(!snap.environments.is_empty());
        let _ = fs::remove_dir_all(&root);
    }
}
