//! Git-backed team sync: workspace tree on disk + shell-out to system `git`.
//!
//! Layout (EvilPostman / `.evilpostman`):
//! ```text
//! .evilpostman/project.json
//! collections/<slug>/collection.json + folders/ + requests/
//! environments/<env>.environment.json   # secrets redacted
//! .gitignore / README.md
//! ```
//!
//! Environments map to branches: `env/<sanitized-name>`.
//! Vault, cookies, history, and secret values stay in local `snapshot.json` only.

mod git;
mod tree;

pub use git::{
    checkout_env_branch, ensure_clone, git_available, push_commit, status_summary, CloudGitError,
    SyncResult,
};
pub use tree::{
    branch_name_for_env, export_workspace_tree, import_workspace_tree, merge_secret_values,
    redact_environment, sanitize_slug, CloudTreeError, ProjectMeta, DEFAULT_GITIGNORE,
    DEFAULT_README,
};

use std::path::Path;

use super::{AppSettings, Collection, Environment};
use crate::storage::AppSnapshot;

/// High-level: ensure clone, checkout env branch, write tree, commit+push.
pub fn sync_on_save(
    data_root: &Path,
    settings: &mut AppSettings,
    collections: &[Collection],
    environments: &[Environment],
    active_env_name: &str,
) -> SyncResult {
    if !settings.cloud_sync.enabled {
        return SyncResult {
            ok: true,
            skipped: true,
            message: "cloud sync disabled".into(),
            branch: String::new(),
        };
    }

    let remote = settings.cloud_sync.remote_url.trim();
    if remote.is_empty() {
        let msg = "cloud sync: remote URL empty".to_string();
        settings.cloud_sync.last_status = msg.clone();
        return SyncResult {
            ok: false,
            skipped: false,
            message: msg,
            branch: String::new(),
        };
    }

    let clone = settings.cloud_sync.resolved_clone_path(data_root);
    match ensure_clone(remote, &clone) {
        Ok(()) => {}
        Err(e) => {
            let msg = format!("cloud sync: {e}");
            settings.cloud_sync.last_status = msg.clone();
            return SyncResult {
                ok: false,
                skipped: false,
                message: msg,
                branch: String::new(),
            };
        }
    }

    let branch = branch_name_for_env(active_env_name);
    if let Err(e) = checkout_env_branch(&clone, &branch) {
        let msg = format!("cloud sync checkout: {e}");
        settings.cloud_sync.last_status = msg.clone();
        return SyncResult {
            ok: false,
            skipped: false,
            message: msg,
            branch: branch.clone(),
        };
    }

    let project_name = collections
        .first()
        .map(|c| c.name.clone())
        .unwrap_or_else(|| "EvilPostman workspace".into());
    if let Err(e) = export_workspace_tree(
        &clone,
        &project_name,
        remote,
        active_env_name,
        collections,
        environments,
    ) {
        let msg = format!("cloud sync write: {e}");
        settings.cloud_sync.last_status = msg.clone();
        return SyncResult {
            ok: false,
            skipped: false,
            message: msg,
            branch: branch.clone(),
        };
    }

    let commit_msg = format!(
        "chore(evilpostman): sync {active_env_name} ({})",
        chrono::Local::now().format("%Y-%m-%d %H:%M")
    );
    match push_commit(&clone, &commit_msg) {
        Ok(r) => {
            settings.cloud_sync.last_branch = branch.clone();
            settings.cloud_sync.last_status = r.message.clone();
            SyncResult {
                ok: r.ok,
                skipped: r.skipped,
                message: r.message,
                branch,
            }
        }
        Err(e) => {
            let msg = format!("cloud sync git: {e}");
            settings.cloud_sync.last_status = msg.clone();
            SyncResult {
                ok: false,
                skipped: false,
                message: msg,
                branch,
            }
        }
    }
}

/// Explicit Sync for one environment: checkout `env/<slug>`, export tree, commit, push.
/// Does not require the env to already be active; caller should set active afterward.
pub fn sync_environment(
    data_root: &Path,
    settings: &mut AppSettings,
    collections: &[Collection],
    environments: &[Environment],
    env_name: &str,
) -> SyncResult {
    if !settings.cloud_sync.enabled {
        return SyncResult {
            ok: false,
            skipped: true,
            message: "cloud sync disabled — enable in Settings".into(),
            branch: String::new(),
        };
    }
    // Reuse save pipeline targeted at this env's branch.
    sync_on_save(data_root, settings, collections, environments, env_name)
}

/// Switch environment branch: commit current tree, checkout target, reload from files.
/// Returns updated collections/environments when reload succeeds.
pub fn switch_env_branch(
    data_root: &Path,
    settings: &mut AppSettings,
    collections: &[Collection],
    environments: &[Environment],
    from_env_name: &str,
    to_env_name: &str,
) -> Result<(Vec<Collection>, Vec<Environment>), String> {
    if !settings.cloud_sync.enabled {
        return Ok((collections.to_vec(), environments.to_vec()));
    }

    let remote = settings.cloud_sync.remote_url.trim();
    if remote.is_empty() {
        return Err("cloud sync: remote URL empty".into());
    }
    let clone = settings.cloud_sync.resolved_clone_path(data_root);
    ensure_clone(remote, &clone).map_err(|e| e.to_string())?;

    // Save current branch state first.
    let from_branch = branch_name_for_env(from_env_name);
    let _ = checkout_env_branch(&clone, &from_branch);
    let project_name = collections
        .first()
        .map(|c| c.name.clone())
        .unwrap_or_else(|| "EvilPostman workspace".into());
    export_workspace_tree(
        &clone,
        &project_name,
        remote,
        from_env_name,
        collections,
        environments,
    )
    .map_err(|e| e.to_string())?;
    let commit_msg = format!(
        "chore(evilpostman): before switch → {to_env_name}"
    );
    let _ = push_commit(&clone, &commit_msg);

    let to_branch = branch_name_for_env(to_env_name);
    checkout_env_branch(&clone, &to_branch).map_err(|e| e.to_string())?;

    let (mut cols, mut envs) = import_workspace_tree(&clone).map_err(|e| e.to_string())?;
    // Prefer local secret values over redacted git placeholders.
    merge_secret_values(&mut envs, environments);
    if cols.is_empty() {
        cols = collections.to_vec();
    }
    if envs.is_empty() {
        envs = environments.to_vec();
    }

    settings.cloud_sync.last_branch = to_branch;
    if let Ok(s) = status_summary(&clone) {
        settings.cloud_sync.last_status = s;
    } else {
        settings.cloud_sync.last_status =
            format!("checked out {}", branch_name_for_env(to_env_name));
    }

    Ok((cols, envs))
}

/// Apply sync result message onto a snapshot settings blob (for persist).
pub fn apply_sync_to_snapshot(snap: &mut AppSnapshot, result: &SyncResult) {
    snap.settings.cloud_sync.last_status = result.message.clone();
    if !result.branch.is_empty() {
        snap.settings.cloud_sync.last_branch = result.branch.clone();
    }
}
