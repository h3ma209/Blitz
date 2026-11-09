use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::core::{
    bundled_postman_collections, demo_environments, ensure_default_environments, flatten_collections,
    AppSettings, CookieJar, Environment, HistoryEntry, TreeRowKind, VariableContext, VaultStore,
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
    /// Workspace collections. Loaded from `collections.json` when that file exists.
    /// Still read from older `snapshot.json` files that embedded this list.
    #[serde(default, skip_serializing)]
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
    /// Cached responses per request id. Loaded from `responses.json`.
    /// Still read from older `snapshot.json` files that embedded this map.
    #[serde(default, skip_serializing)]
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

    pub fn open_for_settings(settings: &AppSettings) -> Result<Self, StorageError> {
        let custom = settings.storage_root.trim();
        if custom.is_empty() {
            Self::open_default()
        } else {
            Self::open(custom)
        }
    }

    pub fn open_path(path: &str) -> Result<Self, StorageError> {
        if path.trim().is_empty() {
            Self::open_default()
        } else {
            Self::open(path)
        }
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

    fn responses_path(&self) -> PathBuf {
        self.root.join("responses.json")
    }

    fn collections_path(&self) -> PathBuf {
        self.root.join("collections.json")
    }

    /// Write every file to a temp, then replace the destinations. If a replace fails,
/// destinations already swapped are moved back from their backups.
fn publish_files(files: &[(PathBuf, String)]) -> Result<(), StorageError> {
    let mut temps = Vec::with_capacity(files.len());
    for (path, text) in files {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "blob".into());
        let tmp = path.with_file_name(format!("{file_name}.tmp.{}", uuid::Uuid::new_v4()));
        if let Some(parent) = tmp.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Err(e) = fs::write(&tmp, text) {
            for tmp in &temps {
                let _ = fs::remove_file(tmp);
            }
            return Err(e.into());
        }
        temps.push(tmp);
    }
    let pairs: Vec<(PathBuf, PathBuf)> = temps
        .into_iter()
        .zip(files.iter().map(|(dest, _)| dest.clone()))
        .collect();
    Self::commit_replacements(&pairs)
}

/// `pairs` is `(temp, destination)`. Temps are removed. A failed replace restores earlier destinations.
fn commit_replacements(pairs: &[(PathBuf, PathBuf)]) -> Result<(), StorageError> {
    let mut backups: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (tmp, dest) in pairs {
        if dest.exists() {
            let file_name = dest
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "blob".into());
            let bak = dest.with_file_name(format!("{file_name}.bak.{}", uuid::Uuid::new_v4()));
            if let Err(e) = fs::rename(dest, &bak) {
                let _ = fs::rename(&bak, dest);
                Self::restore_backups(&backups);
                for (tmp, _) in pairs {
                    let _ = fs::remove_file(tmp);
                }
                return Err(e.into());
            }
            backups.push((dest.clone(), bak));
        }
        if let Err(e) = fs::rename(tmp, dest) {
            if let Some((backed_dest, bak)) = backups.last() {
                if backed_dest == dest && bak.exists() {
                    let _ = fs::rename(bak, dest);
                    backups.pop();
                }
            }
            Self::restore_backups(&backups);
            for (tmp, _) in pairs {
                let _ = fs::remove_file(tmp);
            }
            return Err(e.into());
        }
    }
    for (_, bak) in backups {
        let _ = fs::remove_file(bak);
    }
    Ok(())
}

fn restore_backups(backups: &[(PathBuf, PathBuf)]) {
    for (dest, bak) in backups.iter().rev() {
        if bak.exists() {
            let _ = fs::remove_file(dest);
            let _ = fs::rename(bak, dest);
        }
    }
}

    pub fn load(&self) -> Result<AppSnapshot, StorageError> {
        let path = self.snapshot_path();
        if !path.exists() {
            return Ok(Self::default_snapshot());
        }
        let text = fs::read_to_string(&path)?;
        let mut snap: AppSnapshot = match serde_json::from_str(&text) {
            Ok(s) => s,
            Err(e) => {
                let backup_path = self.root.join(format!(
                    "snapshot.json.corrupt.{}",
                    chrono::Utc::now().timestamp()
                ));
                let _ = fs::copy(&path, &backup_path);
                return Err(StorageError::Json(e));
            }
        };
        // Side file wins. Missing file keeps collections embedded in older snapshots.
        let collections_path = self.collections_path();
        if collections_path.exists() {
            if let Ok(cols) = fs::read_to_string(&collections_path).and_then(|text| {
                serde_json::from_str(&text)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            }) {
                snap.collections = cols;
            }
        }
        if snap.collections.is_empty() {
            snap.collections = bundled_postman_collections();
        }

        // Upgrade legacy HTTP requests that are actually non-HTTP (e.g. WebSocket room inside Hakki Websocket folder)
        for col in &mut snap.collections {
            for item in &mut col.items {
                migrate_item_protocols(item, None);
            }
        }
        for tab in &mut snap.open_tabs {
            tab.auto_detect_protocol(None);
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
        // Side file wins. Missing file keeps responses embedded in older snapshots.
        let responses_path = self.responses_path();
        if responses_path.exists() {
            if let Ok(map) = fs::read_to_string(&responses_path).and_then(|text| {
                serde_json::from_str(&text).map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, e)
                })
            }) {
                snap.responses = map;
            }
        }
        Ok(snap)
    }

    pub fn save(&self, snapshot: &AppSnapshot) -> Result<(), StorageError> {
        // shortcut: one collections.json rewrite, split per collection if this rewrite stays hot
        let collections = serde_json::to_string(&snapshot.collections)?;
        let text = serde_json::to_string_pretty(snapshot)?;
        let responses = serde_json::to_string(&snapshot.responses)?;
        // Serialize first. A later failure must not leave a new collection list beside old settings.
        Self::publish_files(&[
            (self.collections_path(), collections),
            (self.snapshot_path(), text),
            (self.responses_path(), responses),
        ])
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

fn migrate_item_protocols(item: &mut crate::core::CollectionItem, folder_hint: Option<&str>) {
    match item {
        crate::core::CollectionItem::Request(req) => {
            req.auto_detect_protocol(folder_hint);
        }
        crate::core::CollectionItem::Folder(folder) => {
            let name = folder.name.clone();
            for sub in &mut folder.items {
                migrate_item_protocols(sub, Some(&name));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::RequestDraft;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let id = uuid::Uuid::new_v4();
        std::env::temp_dir().join(format!("blitz-test-{id}"))
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
    fn responses_roundtrip_in_side_file_and_legacy_snapshot() {
        use crate::core::ResponseView;

        let root = temp_root();
        let store = JsonStore::open(&root).unwrap();
        let mut snap = JsonStore::default_snapshot();
        let id = uuid::Uuid::new_v4();
        snap.responses
            .insert(id, ResponseView::error("side", vec!["drop-me".into()]));
        store.save(&snap).unwrap();

        let main = fs::read_to_string(store.snapshot_path()).unwrap();
        assert!(!main.contains("\"responses\""));
        let loaded = store.load().unwrap();
        assert_eq!(
            loaded.responses.get(&id).and_then(|v| v.error.as_deref()),
            Some("side")
        );

        fs::remove_file(store.responses_path()).unwrap();
        let mut legacy = serde_json::to_value(&snap).unwrap();
        legacy["responses"] = serde_json::to_value(&snap.responses).unwrap();
        fs::write(store.snapshot_path(), serde_json::to_string(&legacy).unwrap()).unwrap();
        let from_legacy = store.load().unwrap();
        assert_eq!(
            from_legacy
                .responses
                .get(&id)
                .and_then(|v| v.error.as_deref()),
            Some("side")
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn collections_roundtrip_in_side_file_and_legacy_snapshot() {
        let root = temp_root();
        let store = JsonStore::open(&root).unwrap();
        let mut snap = JsonStore::default_snapshot();
        let marker = "https://example.com/side-collection";
        let draft = RequestDraft {
            name: "side-collection".into(),
            url: marker.into(),
            ..Default::default()
        };
        snap.collections[0].add_request(draft.clone());
        store.save(&snap).unwrap();

        let main = fs::read_to_string(store.snapshot_path()).unwrap();
        let side = fs::read_to_string(store.collections_path()).unwrap();
        assert!(!main.contains(marker));
        assert!(side.contains(marker));
        assert!(!side.contains('\n'));
        let loaded = store.load().unwrap();
        assert!(loaded
            .collections
            .iter()
            .any(|c| c.find_request(draft.id).is_some()));

        fs::remove_file(store.collections_path()).unwrap();
        let mut legacy = serde_json::to_value(&snap).unwrap();
        legacy["collections"] = serde_json::to_value(&snap.collections).unwrap();
        fs::write(store.snapshot_path(), serde_json::to_string(&legacy).unwrap()).unwrap();
        let from_legacy = store.load().unwrap();
        assert!(from_legacy
            .collections
            .iter()
            .any(|c| c.find_request(draft.id).is_some()));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn commit_replacements_restores_the_first_file_when_the_second_replace_fails() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let dest1 = root.join("collections.json");
        fs::write(&dest1, "old-collections").unwrap();
        let tmp1 = root.join("collections.tmp");
        fs::write(&tmp1, "new-collections").unwrap();
        let blocker = root.join("not-a-directory");
        fs::write(&blocker, "file").unwrap();
        let dest2 = blocker.join("snapshot.json");
        let tmp2 = root.join("snapshot.tmp");
        fs::write(&tmp2, "new-settings").unwrap();

        let err = JsonStore::commit_replacements(&[
            (tmp1, dest1.clone()),
            (tmp2, dest2),
        ]);
        assert!(err.is_err());
        assert_eq!(fs::read_to_string(&dest1).unwrap(), "old-collections");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn corrupt_snapshot_creates_backup_and_returns_error() {
        let root = temp_root();
        let store = JsonStore::open(&root).unwrap();
        let snap_path = store.snapshot_path();
        fs::write(&snap_path, "{ broken json content: 123,").unwrap();

        let load_res = store.load();
        assert!(load_res.is_err());

        // Verify that a backup copy was generated
        let entries = fs::read_dir(&root).unwrap();
        let has_corrupt_backup = entries
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().starts_with("snapshot.json.corrupt."));
        assert!(has_corrupt_backup);
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
