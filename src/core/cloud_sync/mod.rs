//! Git-backed team sync: workspace tree on disk + shell-out to system `git`.
//!
//! Layout (Blitz / `.blitz`):
//! ```text
//! .blitz/project.json
//! collections/<slug>/collection.json + folders/ + requests/
//! environments/<env>.environment.json   # secrets redacted
//! .gitignore / README.md
//! ```
//!
//! Environments map to branches: `env/<sanitized-name>`.
//! Vault, cookies, history, secret values, and [`VariableContext::local`] stay in
//! local `snapshot.json` only — never written into the git working tree.
//!
//! **Save** → local `snapshot.json` + (if cloud on + clone + active env) export → commit only.
//! **Sync** → pull --rebase → export/commit → push → reload tree. Never force-push.

mod git;
mod tree;

pub use git::{
    build_commit_message, checkout_env_branch, commit_local_changes, detect_git_conflicts,
    ensure_clone, get_conflict_file_versions, get_team_activity, git_available,
    open_in_file_manager, pull_rebase, push_commit, push_only, resolve_commit_message,
    resolve_git_conflicts, status_detail, status_summary, sync_pull_push, ChangedFile,
    CloudGitError, GitStatus, SyncResult, TeamActivity, TeamCommit, TeamMember,
};
pub use tree::{
    branch_name_for_env, export_workspace_tree, import_workspace_tree, merge_secret_values,
    redact_environment, sanitize_slug, CloudTreeError, ProjectMeta, DEFAULT_GITIGNORE,
    DEFAULT_README,
};

use std::path::Path;

use super::{AppSettings, Collection, Environment, VariableContext};
use crate::storage::AppSnapshot;

fn is_no_environment(name: &str) -> bool {
    let t = name.trim();
    t.is_empty() || t.eq_ignore_ascii_case("No Environment")
}

/// Local is machine-only (like No Environment) — not a team/git Sync target.
fn is_local_environment(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case("Local")
}

fn export_to_clone(
    clone: &Path,
    remote: &str,
    active_env_name: &str,
    collections: &[Collection],
    environments: &[Environment],
    variables: &VariableContext,
) -> Result<(), String> {
    let project_name = collections
        .first()
        .map(|c| c.name.clone())
        .unwrap_or_else(|| "Blitz workspace".into());
    export_workspace_tree(
        clone,
        &project_name,
        remote,
        active_env_name,
        collections,
        environments,
        variables,
    )
    .map_err(|e| e.to_string())
}

/// On Save: export tree → `git add` → **commit only** (never push).
/// Skips when cloud disabled, no linked clone, or no active env ("No Environment").
/// `variables.local` is never exported.
pub fn sync_on_save(
    data_root: &Path,
    settings: &mut AppSettings,
    collections: &[Collection],
    environments: &[Environment],
    active_env_name: &str,
    variables: &VariableContext,
) -> SyncResult {
    if !settings.cloud_sync.enabled {
        return SyncResult {
            ok: true,
            skipped: true,
            message: "cloud sync disabled".into(),
            branch: String::new(),
        };
    }

    if is_no_environment(active_env_name) {
        return SyncResult {
            ok: true,
            skipped: true,
            message: "no environment — local save only".into(),
            branch: String::new(),
        };
    }
    if is_local_environment(active_env_name) {
        return SyncResult {
            ok: true,
            skipped: true,
            message: "Local environment — local save only (not synced)".into(),
            branch: String::new(),
        };
    }

    let clone = settings.cloud_sync.resolved_clone_path(data_root);
    if !clone.join(".git").is_dir() {
        return SyncResult {
            ok: true,
            skipped: true,
            message: "no clone — local save only".into(),
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

    let branch = "main".to_string();
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

    if let Err(e) = export_to_clone(
        &clone,
        remote,
        active_env_name,
        collections,
        environments,
        variables,
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

    let override_msg = {
        let custom = settings.cloud_sync.commit_message.trim();
        if custom.is_empty() {
            None
        } else {
            Some(custom)
        }
    };
    match commit_local_changes(&clone, override_msg) {
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

/// Explicit Sync for one environment (primary “share with team” action):
/// 1. checkout `env/<slug>`
/// 2. export + commit any local changes (so pull can rebase)
/// 3. `git pull --rebase`
/// 4. import tree (teammates’ changes)
/// 5. `git push` (never force)
///
/// Returns reloaded collections/environments on success (empty vecs if pull failed early).
/// Never syncs [`VariableContext::local`].
pub fn sync_environment(
    data_root: &Path,
    settings: &mut AppSettings,
    collections: &[Collection],
    environments: &[Environment],
    env_name: &str,
    variables: &VariableContext,
) -> Result<(Vec<Collection>, Vec<Environment>, SyncResult), String> {
    if is_no_environment(env_name) {
        return Err("pick an environment before Sync".into());
    }
    if is_local_environment(env_name) {
        return Err("Local environment is not synced — pick Development/Production (or another team env)".into());
    }
    if !settings.cloud_sync.enabled {
        return Err("cloud sync disabled — enable in Settings or Cloud menu".into());
    }

    let remote = settings.cloud_sync.remote_url.trim();
    if remote.is_empty() {
        return Err("cloud sync: remote URL empty".into());
    }

    let clone = settings.cloud_sync.resolved_clone_path(data_root);
    ensure_clone(remote, &clone).map_err(|e| e.to_string())?;

    let branch = "main";
    checkout_env_branch(&clone, branch).map_err(|e| e.to_string())?;

    export_to_clone(
        &clone,
        remote,
        env_name,
        collections,
        environments,
        variables,
    )?;

    let override_msg = {
        let custom = settings.cloud_sync.commit_message.trim();
        if custom.is_empty() {
            None
        } else {
            Some(custom)
        }
    };
    let commit = commit_local_changes(&clone, override_msg).map_err(|e| e.to_string())?;
    if !commit.ok {
        settings.cloud_sync.last_status = commit.message.clone();
        return Ok((Vec::new(), Vec::new(), commit));
    }

    let pull = pull_rebase(&clone).map_err(|e| e.to_string())?;
    if !pull.ok {
        settings.cloud_sync.last_status = pull.message.clone();
        settings.cloud_sync.last_branch = branch.into();
        return Ok((Vec::new(), Vec::new(), pull));
    }

    let (mut cols, mut envs) = import_workspace_tree(&clone).map_err(|e| e.to_string())?;
    merge_secret_values(&mut envs, environments);
    if cols.is_empty() {
        cols = collections.to_vec();
    }
    if envs.is_empty() {
        envs = environments.to_vec();
    }

    let push = push_only(&clone).map_err(|e| e.to_string())?;
    let status = status_detail(&clone)
        .map(|s| s.summary_line())
        .unwrap_or_else(|_| branch.into());

    let message = if push.ok {
        let commit_note = if commit.skipped {
            "no local commit"
        } else {
            "committed"
        };
        format!("synced · {status} · {commit_note}")
    } else {
        format!("{} · {}", pull.message, push.message)
    };

    settings.cloud_sync.last_branch = branch.into();
    settings.cloud_sync.last_status = message.clone();

    Ok((
        cols,
        envs,
        SyncResult {
            ok: push.ok,
            skipped: false,
            message,
            branch: branch.into(),
        },
    ))
}

/// Switch environment: commit current tree to main branch, reload environment variables from main.
/// Returns updated collections/environments when reload succeeds.
/// With per-collection environment overrides, no git branch switching is needed.
pub fn switch_env_branch(
    data_root: &Path,
    settings: &mut AppSettings,
    collections: &[Collection],
    environments: &[Environment],
    from_env_name: &str,
    to_env_name: &str,
    variables: &VariableContext,
) -> Result<(Vec<Collection>, Vec<Environment>), String> {
    if !settings.cloud_sync.enabled {
        return Ok((collections.to_vec(), environments.to_vec()));
    }

    let remote = settings.cloud_sync.remote_url.trim();
    if remote.is_empty() {
        return Err("cloud sync: remote URL empty".into());
    }
    let clone = settings.cloud_sync.resolved_clone_path(data_root);
    if !clone.join(".git").is_dir() {
        // No linked clone yet — local env switch only.
        return Ok((collections.to_vec(), environments.to_vec()));
    }

    // Save current state to main branch (commit only — Sync pushes).
    let main_branch = "main";
    let _ = checkout_env_branch(&clone, main_branch);
    export_to_clone(
        &clone,
        remote,
        from_env_name,
        collections,
        environments,
        variables,
    )?;
    let _ = commit_local_changes(&clone, None);

    if is_no_environment(to_env_name) {
        settings.cloud_sync.last_status = "local (no environment)".into();
        return Ok((collections.to_vec(), environments.to_vec()));
    }

    // Checkout main branch (contains all collections with environment overrides)
    checkout_env_branch(&clone, main_branch).map_err(|e| e.to_string())?;

    let (mut cols, mut envs) = import_workspace_tree(&clone).map_err(|e| e.to_string())?;
    // Prefer local secret values over redacted git placeholders.
    merge_secret_values(&mut envs, environments);
    if cols.is_empty() {
        cols = collections.to_vec();
    }
    if envs.is_empty() {
        envs = environments.to_vec();
    }

    settings.cloud_sync.last_branch = main_branch.into();
    if let Ok(s) = status_summary(&clone) {
        settings.cloud_sync.last_status = s;
    } else {
        settings.cloud_sync.last_status = format!("switched to {}", to_env_name);
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

/// Clone remote into configured path (or verify existing `.git`).
pub fn clone_or_ensure(
    data_root: &Path,
    settings: &mut AppSettings,
) -> Result<SyncResult, String> {
    let remote = settings.cloud_sync.remote_url.trim();
    if remote.is_empty() {
        return Err("cloud sync: remote URL empty".into());
    }
    let clone = settings.cloud_sync.resolved_clone_path(data_root);
    ensure_clone(remote, &clone).map_err(|e| e.to_string())?;
    settings.cloud_sync.enabled = true;
    let status = status_detail(&clone)
        .map(|s| s.summary_line())
        .unwrap_or_else(|_| "cloned".into());
    settings.cloud_sync.last_status = status.clone();
    if let Ok(s) = status_detail(&clone) {
        settings.cloud_sync.last_branch = s.branch;
    }
    Ok(SyncResult {
        ok: true,
        skipped: false,
        message: format!("linked · {} · {status}", clone.display()),
        branch: settings.cloud_sync.last_branch.clone(),
    })
}

/// Link an existing folder (must contain `.git` and preferably `.blitz/`).
pub fn link_folder(
    data_root: &Path,
    settings: &mut AppSettings,
    folder: &Path,
) -> Result<(Vec<Collection>, Vec<Environment>, SyncResult), String> {
    if !folder.join(".git").is_dir() {
        return Err(format!(
            "not a git repo (missing .git): {}",
            folder.display()
        ));
    }
    settings.cloud_sync.enabled = true;
    settings.cloud_sync.clone_path = folder.display().to_string();
    if settings.cloud_sync.remote_url.trim().is_empty() {
        settings.cloud_sync.remote_url = crate::core::settings::DEFAULT_CLOUD_REMOTE.to_string();
    }
    let _ = data_root; // link uses absolute folder; data_root unused
    let (cols, envs) = import_workspace_tree(folder).unwrap_or_default();
    let status = status_detail(folder)
        .map(|s| {
            settings.cloud_sync.last_branch = s.branch.clone();
            s.summary_line()
        })
        .unwrap_or_else(|_| "linked".into());
    settings.cloud_sync.last_status = status.clone();
    Ok((
        cols,
        envs,
        SyncResult {
            ok: true,
            skipped: false,
            message: format!("linked folder · {status}"),
            branch: settings.cloud_sync.last_branch.clone(),
        },
    ))
}

/// Refresh git status into settings (no network mutate beyond fetch).
pub fn refresh_status(data_root: &Path, settings: &mut AppSettings) -> SyncResult {
    if !settings.cloud_sync.enabled {
        return SyncResult {
            ok: false,
            skipped: true,
            message: "cloud sync disabled".into(),
            branch: String::new(),
        };
    }
    let clone = settings.cloud_sync.resolved_clone_path(data_root);
    if !clone.join(".git").is_dir() {
        let msg = format!("no clone yet — use Cloud → Clone ({})", clone.display());
        settings.cloud_sync.last_status = msg.clone();
        return SyncResult {
            ok: false,
            skipped: false,
            message: msg,
            branch: String::new(),
        };
    }
    match status_detail(&clone) {
        Ok(s) => {
            let line = s.summary_line();
            settings.cloud_sync.last_branch = s.branch.clone();
            settings.cloud_sync.last_status = line.clone();
            SyncResult {
                ok: true,
                skipped: false,
                message: line,
                branch: s.branch,
            }
        }
        Err(e) => {
            let msg = format!("status: {e}");
            settings.cloud_sync.last_status = msg.clone();
            SyncResult {
                ok: false,
                skipped: false,
                message: msg,
                branch: settings.cloud_sync.last_branch.clone(),
            }
        }
    }
}

/// Pull with rebase, then import tree into app (merge local secrets).
/// Uses main branch which contains all collections with environment overrides.
pub fn pull_workspace(
    data_root: &Path,
    settings: &mut AppSettings,
    local_envs: &[Environment],
) -> Result<(Vec<Collection>, Vec<Environment>, SyncResult), String> {
    if !settings.cloud_sync.enabled {
        return Err("cloud sync disabled — enable in Settings or Cloud menu".into());
    }
    let remote = settings.cloud_sync.remote_url.trim();
    if remote.is_empty() {
        return Err("cloud sync: remote URL empty".into());
    }
    let clone = settings.cloud_sync.resolved_clone_path(data_root);
    ensure_clone(remote, &clone).map_err(|e| e.to_string())?;
    checkout_env_branch(&clone, "main").map_err(|e| e.to_string())?;
    let result = pull_rebase(&clone).map_err(|e| e.to_string())?;
    settings.cloud_sync.last_status = result.message.clone();
    if !result.branch.is_empty() {
        settings.cloud_sync.last_branch = result.branch.clone();
    }
    if !result.ok {
        return Ok((Vec::new(), Vec::new(), result));
    }
    let (cols, mut envs) = import_workspace_tree(&clone).map_err(|e| e.to_string())?;
    merge_secret_values(&mut envs, local_envs);
    Ok((cols, envs, result))
}

/// Export current workspace, commit (optional message), push. Never force.
/// Cloud menu advanced Push — not used on Save.
/// Never syncs [`VariableContext::local`].
pub fn push_workspace(
    data_root: &Path,
    settings: &mut AppSettings,
    collections: &[Collection],
    environments: &[Environment],
    active_env_name: &str,
    commit_message: Option<&str>,
    variables: &VariableContext,
) -> SyncResult {
    if !settings.cloud_sync.enabled {
        return SyncResult {
            ok: false,
            skipped: true,
            message: "cloud sync disabled".into(),
            branch: String::new(),
        };
    }
    if is_no_environment(active_env_name) {
        return SyncResult {
            ok: false,
            skipped: true,
            message: "pick an environment before Push".into(),
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
    if let Err(e) = ensure_clone(remote, &clone) {
        let msg = format!("cloud sync: {e}");
        settings.cloud_sync.last_status = msg.clone();
        return SyncResult {
            ok: false,
            skipped: false,
            message: msg,
            branch: String::new(),
        };
    }
    let branch = "main";
    if let Err(e) = checkout_env_branch(&clone, branch) {
        let msg = format!("cloud sync checkout: {e}");
        settings.cloud_sync.last_status = msg.clone();
        return SyncResult {
            ok: false,
            skipped: false,
            message: msg,
            branch: branch.into(),
        };
    }
    if let Err(e) = export_to_clone(
        &clone,
        remote,
        active_env_name,
        collections,
        environments,
        variables,
    ) {
        let msg = format!("cloud sync write: {e}");
        settings.cloud_sync.last_status = msg.clone();
        return SyncResult {
            ok: false,
            skipped: false,
            message: msg,
            branch: branch.into(),
        };
    }
    let msg = commit_message
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    match push_commit(&clone, msg) {
        Ok(r) => {
            settings.cloud_sync.last_branch = branch.into();
            settings.cloud_sync.last_status = r.message.clone();
            SyncResult {
                ok: r.ok,
                skipped: r.skipped,
                message: r.message,
                branch: branch.into(),
            }
        }
        Err(e) => {
            let msg = format!("cloud sync git: {e}");
            settings.cloud_sync.last_status = msg.clone();
            SyncResult {
                ok: false,
                skipped: false,
                message: msg,
                branch: branch.into(),
            }
        }
    }
}
