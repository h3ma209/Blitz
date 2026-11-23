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
    abort_git_rebase, build_commit_message, checkout_env_branch, commit_local_changes,
    default_branch, detect_git_conflicts, ensure_clone, get_conflict_file_versions,
    get_team_activity, get_team_activity_ex, is_git_repo, is_merge_in_progress, is_noreply_user,
    is_rebase_in_progress, mask_remote_url, normalize_remote_for_compare, open_in_file_manager,
    pull_rebase, push_commit, push_only, read_origin_url, remote_storage_dir_name,
    resolve_commit_message, resolve_git_conflicts, status_detail, status_summary, sync_pull_push,
    ChangedFile, CloudGitError, CommitDiffLine, CommitDiffLineKind, CommitFileDiff, GitStatus,
    SyncResult, TeamActivity, TeamCommit, TeamMember,
};
pub use tree::{
    branch_name_for_env, export_workspace_tree, import_workspace_globals, import_workspace_tree,
    merge_collection_secrets, merge_global_secrets, merge_secret_values, redact_auth_config,
    redact_environment, sanitize_slug, CloudTreeError, ProjectMeta, DEFAULT_GITIGNORE,
    DEFAULT_README,
};

use std::collections::HashMap;
use std::path::Path;

use super::{AppSettings, Collection, Environment, VariableContext};

/// Result of a pull / sync / link operation including merged local secrets & globals.
#[derive(Debug, Clone)]
pub struct SyncedWorkspace {
    pub collections: Vec<Collection>,
    pub environments: Vec<Environment>,
    pub globals: HashMap<String, String>,
    pub result: SyncResult,
}

fn is_no_environment(name: &str) -> bool {
    let t = name.trim();
    t.is_empty() || t.eq_ignore_ascii_case("No Environment")
}

/// Local is machine-only — never pushed to remote.
fn is_local_environment(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case("Local")
}

/// Preserve the user's machine-local `"Local"` environment (excluded from Git exports)
/// when importing shared environments from the remote tree.
fn preserve_local_environment(imported: &mut Vec<Environment>, local_envs: &[Environment]) {
    if let Some((idx, local_env)) = local_envs
        .iter()
        .enumerate()
        .find(|(_, e)| is_local_environment(&e.name))
    {
        if !imported.iter().any(|e| is_local_environment(&e.name)) {
            let pos = idx.min(imported.len());
            imported.insert(pos, local_env.clone());
        }
    }
}

pub type LoadedWorkspaceTree = (Vec<Collection>, Vec<Environment>, HashMap<String, String>);

/// Import the Git workspace tree & globals and restore all local secret credentials,
/// secret variables, machine-local metadata, and the local-only `"Local"` environment.
pub fn load_synced_tree_with_secrets(
    clone: &Path,
    local_cols: &[Collection],
    local_envs: &[Environment],
    local_globals: &HashMap<String, String>,
) -> Result<LoadedWorkspaceTree, String> {
    let (mut cols, mut envs) = import_workspace_tree(clone).map_err(|e| e.to_string())?;
    merge_collection_secrets(&mut cols, local_cols);
    merge_secret_values(&mut envs, local_envs);
    preserve_local_environment(&mut envs, local_envs);
    let mut globals = import_workspace_globals(clone);
    merge_global_secrets(&mut globals, local_globals);
    Ok((cols, envs, globals))
}

/// Switch to a configured Git repository (clone if needed, fetch/pull latest from remote if set)
/// and load ONLY the files present in that repository with zero carryover from any previous git repo.
pub fn switch_git_workspace(
    data_root: &Path,
    settings: &mut AppSettings,
) -> Result<SyncedWorkspace, String> {
    let remote = settings.cloud_sync.remote_url.trim().to_string();
    let clone = settings.cloud_sync.resolved_clone_path(data_root);

    if !remote.is_empty() {
        ensure_clone(&remote, &clone).map_err(|e| e.to_string())?;
        let branch = default_branch(&clone);
        if is_rebase_in_progress(&clone) || is_merge_in_progress(&clone) {
            let msg = "unfinished merge/rebase conflicts in repository — resolve or abort first".to_string();
            settings.cloud_sync.last_status = msg.clone();
            settings.cloud_sync.last_branch = branch.clone();
            settings.cloud_sync.record_recent_repo();
            return Ok(SyncedWorkspace {
                collections: Vec::new(),
                environments: Vec::new(),
                globals: HashMap::new(),
                result: SyncResult {
                    ok: false,
                    skipped: false,
                    message: msg,
                    branch,
                },
            });
        }
        let _ = checkout_env_branch(&clone, &branch);
        if let Ok(pull) = pull_rebase(&clone) {
            if !pull.ok {
                settings.cloud_sync.last_status = pull.message.clone();
                settings.cloud_sync.last_branch = branch.clone();
                settings.cloud_sync.record_recent_repo();
                return Ok(SyncedWorkspace {
                    collections: Vec::new(),
                    environments: Vec::new(),
                    globals: HashMap::new(),
                    result: pull,
                });
            }
        }
    } else if is_git_repo(&clone) {
        if let Some(origin) = read_origin_url(&clone) {
            settings.cloud_sync.remote_url = origin;
        }
    } else {
        let msg = "no git repository configured".to_string();
        settings.cloud_sync.last_status = msg.clone();
        return Ok(SyncedWorkspace {
            collections: Vec::new(),
            environments: Vec::new(),
            globals: HashMap::new(),
            result: SyncResult {
                ok: true,
                skipped: true,
                message: msg,
                branch: String::new(),
            },
        });
    }

    let (cols, envs) = import_workspace_tree(&clone).map_err(|e| e.to_string())?;
    let globals = import_workspace_globals(&clone);
    let branch = default_branch(&clone);
    let status = status_detail(&clone)
        .map(|s| {
            let b = if s.branch.is_empty() { branch.clone() } else { s.branch.clone() };
            (b, s.summary_line())
        })
        .unwrap_or_else(|_| (branch.clone(), "loaded".into()));
    settings.cloud_sync.enabled = true;
    settings.cloud_sync.last_branch = status.0.clone();
    let msg = format!(
        "switched git · {} collection(s), {} env(s) · {}",
        cols.len(),
        envs.len(),
        status.1
    );
    settings.cloud_sync.last_status = msg.clone();
    settings.cloud_sync.record_recent_repo();

    Ok(SyncedWorkspace {
        collections: cols,
        environments: envs,
        globals,
        result: SyncResult {
            ok: true,
            skipped: false,
            message: msg,
            branch: status.0,
        },
    })
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

    let clone = settings.cloud_sync.resolved_clone_path(data_root);
    if !is_git_repo(&clone) {
        return SyncResult {
            ok: true,
            skipped: true,
            message: "no clone — local save only".into(),
            branch: String::new(),
        };
    }

    if is_rebase_in_progress(&clone) || is_merge_in_progress(&clone) {
        let msg = "cloud sync paused: resolve or abort merge conflicts first".to_string();
        settings.cloud_sync.last_status = msg.clone();
        return SyncResult {
            ok: false,
            skipped: true,
            message: msg,
            branch: settings.cloud_sync.last_branch.clone(),
        };
    }

    let remote = settings.cloud_sync.remote_url.trim();
    let branch = default_branch(&clone);
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

    if is_local_environment(active_env_name) {
        return SyncResult {
            ok: true,
            skipped: true,
            message: "Local env — local save only".into(),
            branch,
        };
    }

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

/// Explicit Sync for one shared environment (primary “share with team” action):
/// 1. ensure clone & checkout default branch (`main` / `master`)
/// 2. export + commit any local changes (so pull can rebase)
/// 3. `git pull --rebase`
/// 4. import tree & globals with local secret restoration
/// 5. `git push` (never force)
///
/// Never syncs [`VariableContext::local`] or the `"Local"` environment.
pub fn sync_environment(
    data_root: &Path,
    settings: &mut AppSettings,
    collections: &[Collection],
    environments: &[Environment],
    env_name: &str,
    variables: &VariableContext,
) -> Result<SyncedWorkspace, String> {
    if is_no_environment(env_name) {
        return Err("pick an environment before Sync".into());
    }
    if is_local_environment(env_name) {
        return Err("Local environment stays local-only — switch to a shared environment (e.g. Development) to Sync".into());
    }
    if !settings.cloud_sync.enabled {
        return Err("cloud sync disabled — enable in Settings or Cloud menu".into());
    }

    let remote = settings.cloud_sync.remote_url.trim().to_string();
    if remote.is_empty() {
        return Err("cloud sync: remote URL empty — configure in Cloud Settings".into());
    }

    let clone = settings.cloud_sync.resolved_clone_path(data_root);
    let existed_before = is_git_repo(&clone);
    ensure_clone(&remote, &clone).map_err(|e| e.to_string())?;

    if is_rebase_in_progress(&clone) || is_merge_in_progress(&clone) {
        let branch = default_branch(&clone);
        let msg = "unfinished merge/rebase conflicts in clone — resolve or abort first".to_string();
        settings.cloud_sync.last_status = msg.clone();
        settings.cloud_sync.last_branch = branch.clone();
        settings.cloud_sync.record_recent_repo();
        return Ok(SyncedWorkspace {
            collections: Vec::new(),
            environments: Vec::new(),
            globals: HashMap::new(),
            result: SyncResult {
                ok: false,
                skipped: false,
                message: msg,
                branch,
            },
        });
    }

    let branch = default_branch(&clone);
    checkout_env_branch(&clone, &branch).map_err(|e| e.to_string())?;

    let fresh_nonempty_clone = !existed_before
        && collections.is_empty()
        && (clone.join("collections").is_dir()
            || clone.join("environments").is_dir()
            || clone.join(".blitz").is_dir());

    let commit = if fresh_nonempty_clone {
        SyncResult {
            ok: true,
            skipped: true,
            message: "fresh clone loaded".into(),
            branch: branch.clone(),
        }
    } else {
        export_to_clone(
            &clone,
            &remote,
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
        let c = commit_local_changes(&clone, override_msg).map_err(|e| e.to_string())?;
        if !c.ok {
            settings.cloud_sync.last_status = c.message.clone();
            settings.cloud_sync.record_recent_repo();
            return Ok(SyncedWorkspace {
                collections: Vec::new(),
                environments: Vec::new(),
                globals: HashMap::new(),
                result: c,
            });
        }
        c
    };

    let pull = pull_rebase(&clone).map_err(|e| e.to_string())?;
    if !pull.ok {
        settings.cloud_sync.last_status = pull.message.clone();
        settings.cloud_sync.last_branch = branch;
        settings.cloud_sync.record_recent_repo();
        return Ok(SyncedWorkspace {
            collections: Vec::new(),
            environments: Vec::new(),
            globals: HashMap::new(),
            result: pull,
        });
    }

    let (cols, envs, globals) =
        load_synced_tree_with_secrets(&clone, collections, environments, &variables.global)?;

    let push = push_only(&clone).map_err(|e| e.to_string())?;
    let status = status_detail(&clone)
        .map(|s| s.summary_line())
        .unwrap_or_else(|_| branch.clone());

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

    settings.cloud_sync.last_branch = branch.clone();
    settings.cloud_sync.last_status = message.clone();
    settings.cloud_sync.record_recent_repo();

    Ok(SyncedWorkspace {
        collections: cols,
        environments: envs,
        globals,
        result: SyncResult {
            ok: push.ok,
            skipped: false,
            message,
            branch,
        },
    })
}

/// Environment switch no longer swaps Git branches (`local` ↔ `main`) or creates noisy commits.
/// Shared collections live on the repo's default branch while environment profiles switch locally.
#[allow(dead_code)]
pub fn switch_env_branch(
    _data_root: &Path,
    settings: &mut AppSettings,
    collections: &[Collection],
    environments: &[Environment],
    _from_env_name: &str,
    to_env_name: &str,
    _variables: &VariableContext,
) -> Result<(Vec<Collection>, Vec<Environment>), String> {
    if is_no_environment(to_env_name) {
        settings.cloud_sync.last_status = "local (no environment)".into();
    }
    Ok((collections.to_vec(), environments.to_vec()))
}

/// Clone remote into configured path (or verify existing `.git`).
pub fn clone_or_ensure(
    data_root: &Path,
    settings: &mut AppSettings,
) -> Result<SyncResult, String> {
    let remote = settings.cloud_sync.remote_url.trim().to_string();
    if remote.is_empty() {
        return Err("cloud sync: remote URL empty".into());
    }
    let clone = settings.cloud_sync.resolved_clone_path(data_root);
    ensure_clone(&remote, &clone).map_err(|e| e.to_string())?;
    settings.cloud_sync.enabled = true;
    let status = status_detail(&clone)
        .map(|s| s.summary_line())
        .unwrap_or_else(|_| "cloned".into());
    settings.cloud_sync.last_status = status.clone();
    if let Ok(s) = status_detail(&clone) {
        settings.cloud_sync.last_branch = s.branch;
    }
    settings.cloud_sync.record_recent_repo();
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
) -> Result<SyncedWorkspace, String> {
    if !is_git_repo(folder) {
        return Err(format!(
            "not a git repo (missing .git): {}",
            folder.display()
        ));
    }
    settings.cloud_sync.enabled = true;
    settings.cloud_sync.clone_path = folder.display().to_string();
    settings.cloud_sync.remote_url = read_origin_url(folder).unwrap_or_default();
    let _ = data_root; // link uses absolute folder; data_root unused
    let (cols, envs) = import_workspace_tree(folder).unwrap_or_default();
    let globals = import_workspace_globals(folder);
    let status = status_detail(folder)
        .map(|s| {
            settings.cloud_sync.last_branch = s.branch.clone();
            s.summary_line()
        })
        .unwrap_or_else(|_| "linked".into());
    settings.cloud_sync.last_status = status.clone();
    settings.cloud_sync.record_recent_repo();
    Ok(SyncedWorkspace {
        collections: cols,
        environments: envs,
        globals,
        result: SyncResult {
            ok: true,
            skipped: false,
            message: format!("linked folder · {status}"),
            branch: settings.cloud_sync.last_branch.clone(),
        },
    })
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
    if !is_git_repo(&clone) {
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

/// Pull with rebase, then import tree into app (merging local secrets for auth, vars, globals & envs).
pub fn pull_workspace(
    data_root: &Path,
    settings: &mut AppSettings,
    local_cols: &[Collection],
    local_envs: &[Environment],
    local_globals: &HashMap<String, String>,
) -> Result<SyncedWorkspace, String> {
    if !settings.cloud_sync.enabled {
        return Err("cloud sync disabled — enable in Settings or Cloud menu".into());
    }
    let remote = settings.cloud_sync.remote_url.trim();
    if remote.is_empty() {
        return Err("cloud sync: remote URL empty — configure in Cloud Settings".into());
    }
    let clone = settings.cloud_sync.resolved_clone_path(data_root);
    ensure_clone(remote, &clone).map_err(|e| e.to_string())?;
    let branch = default_branch(&clone);
    if is_rebase_in_progress(&clone) || is_merge_in_progress(&clone) {
        let msg = "unfinished merge/rebase conflicts — resolve or abort before pulling".to_string();
        settings.cloud_sync.last_status = msg.clone();
        settings.cloud_sync.last_branch = branch.clone();
        return Ok(SyncedWorkspace {
            collections: Vec::new(),
            environments: Vec::new(),
            globals: HashMap::new(),
            result: SyncResult {
                ok: false,
                skipped: false,
                message: msg,
                branch,
            },
        });
    }
    checkout_env_branch(&clone, &branch).map_err(|e| e.to_string())?;
    let result = pull_rebase(&clone).map_err(|e| e.to_string())?;
    settings.cloud_sync.last_status = result.message.clone();
    if !result.branch.is_empty() {
        settings.cloud_sync.last_branch = result.branch.clone();
    }
    if !result.ok {
        settings.cloud_sync.record_recent_repo();
        return Ok(SyncedWorkspace {
            collections: Vec::new(),
            environments: Vec::new(),
            globals: HashMap::new(),
            result,
        });
    }
    let (cols, envs, globals) =
        load_synced_tree_with_secrets(&clone, local_cols, local_envs, local_globals)?;
    settings.cloud_sync.record_recent_repo();
    Ok(SyncedWorkspace {
        collections: cols,
        environments: envs,
        globals,
        result,
    })
}

/// Export current workspace, commit (optional message), pull --rebase if needed, and push. Never force.
/// Refuses when active environment is `"Local"`. Never syncs [`VariableContext::local`].
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
    if is_local_environment(active_env_name) {
        return SyncResult {
            ok: false,
            skipped: true,
            message: "Local environment stays local-only — switch to a shared environment before Push".into(),
            branch: String::new(),
        };
    }
    let remote = settings.cloud_sync.remote_url.trim();
    if remote.is_empty() {
        let msg = "cloud sync: remote URL empty — configure in Cloud Settings".to_string();
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
    let branch = default_branch(&clone);
    if is_rebase_in_progress(&clone) || is_merge_in_progress(&clone) {
        let msg = "unfinished merge/rebase conflicts — resolve or abort before Push".to_string();
        settings.cloud_sync.last_status = msg.clone();
        settings.cloud_sync.last_branch = branch.clone();
        return SyncResult {
            ok: false,
            skipped: false,
            message: msg,
            branch,
        };
    }
    if let Err(e) = checkout_env_branch(&clone, &branch) {
        let msg = format!("cloud sync checkout: {e}");
        settings.cloud_sync.last_status = msg.clone();
        return SyncResult {
            ok: false,
            skipped: false,
            message: msg,
            branch,
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
            branch,
        };
    }
    let override_msg = commit_message
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let commit = match commit_local_changes(&clone, override_msg) {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("cloud sync git: {e}");
            settings.cloud_sync.last_status = msg.clone();
            return SyncResult {
                ok: false,
                skipped: false,
                message: msg,
                branch,
            };
        }
    };
    if !commit.ok {
        settings.cloud_sync.last_status = commit.message.clone();
        return commit;
    }

    // Rebase onto remote changes before pushing so diverged histories surface conflicts cleanly.
    let pull = match pull_rebase(&clone) {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("cloud sync pull: {e}");
            settings.cloud_sync.last_status = msg.clone();
            return SyncResult {
                ok: false,
                skipped: false,
                message: msg,
                branch,
            };
        }
    };
    if !pull.ok {
        settings.cloud_sync.last_status = pull.message.clone();
        settings.cloud_sync.last_branch = branch;
        return pull;
    }

    match push_only(&clone) {
        Ok(r) => {
            settings.cloud_sync.last_branch = branch.clone();
            settings.cloud_sync.last_status = r.message.clone();
            SyncResult {
                ok: r.ok,
                skipped: commit.skipped && !r.ok,
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

#[cfg(test)]
mod multi_user_tests {
    use super::*;
    use crate::core::collection::{CollectionItem, Folder};
    use uuid::Uuid;
    use crate::core::environment::EnvVar;
    use crate::core::request::{AuthConfig, AuthType, KeyValue, RequestDraft};
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir(PathBuf);
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn new_sandbox() -> TestDir {
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("blitz-e2e-{nanos}-{n}"));
        fs::create_dir_all(&base).unwrap();
        TestDir(base)
    }

    fn git(cwd: &Path, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_EDITOR", "true")
            .env("GIT_SEQUENCE_EDITOR", "true")
            .output()
            .expect("spawn git")
    }

    fn init_bare(path: &Path, branch: &str) {
        fs::create_dir_all(path).unwrap();
        let out = git(path, &["init", "--bare", "--template=", "-b", branch]);
        if !out.status.success() {
            // Older git without `-b`
            let _ = git(path, &["init", "--bare", "--template="]);
            let _ = git(
                path,
                &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")],
            );
        }
    }

    fn init_user_wt(base: &Path, name: &str, email: &str, remote: &Path, branch: &str) -> PathBuf {
        let wt = base.join(format!("{name}-wt"));
        let meta = base.join(format!("{name}-meta"));
        fs::create_dir_all(&wt).unwrap();
        let sep = format!("--separate-git-dir={}", meta.display());
        let out = git(&wt, &["init", "--template=", &sep]);
        assert!(out.status.success(), "{:?}", String::from_utf8_lossy(&out.stderr));
        let _ = git(&wt, &["config", "user.name", name]);
        let _ = git(&wt, &["config", "user.email", email]);
        let _ = git(&wt, &["checkout", "-B", branch]);
        let r_str = remote.to_str().unwrap();
        let _ = git(&wt, &["remote", "add", "origin", r_str]);
        wt
    }

    fn settings_for(remote: &Path, clone_wt: &Path) -> AppSettings {
        let mut s = AppSettings::default();
        s.cloud_sync.enabled = true;
        s.cloud_sync.remote_url = remote.to_str().unwrap().to_string();
        s.cloud_sync.clone_path = clone_wt.to_str().unwrap().to_string();
        s
    }

    #[test]
    fn empty_remote_first_sync_succeeds_and_pushes() {
        let sb = new_sandbox();
        let remote = sb.0.join("empty-remote");
        init_bare(&remote, "main");
        let alice = init_user_wt(&sb.0, "Alice", "alice@example.com", &remote, "main");

        let mut col = Collection::new("API");
        col.add_request(RequestDraft {
            name: "Health".into(),
            url: "https://example.com/health".into(),
            ..Default::default()
        });
        let env = Environment::new("Development");
        let mut s = settings_for(&remote, &alice);
        let vars = VariableContext::default();

        let synced =
            sync_environment(&sb.0, &mut s, &[col], &[env], "Development", &vars).unwrap();
        assert!(synced.result.ok, "{}", synced.result.message);
        assert_eq!(synced.collections.len(), 1);

        // Verify commit actually landed on bare remote's `main`
        let refs = git(&remote, &["show-ref"]);
        let ref_str = String::from_utf8_lossy(&refs.stdout);
        assert!(ref_str.contains("refs/heads/main"), "{ref_str}");
    }

    #[test]
    fn master_default_branch_repo_sync_and_pull_succeed() {
        let sb = new_sandbox();
        let remote = sb.0.join("master-remote");
        init_bare(&remote, "master");
        let seed = init_user_wt(&sb.0, "Seed", "seed@example.com", &remote, "master");
        fs::write(seed.join("README.md"), "hello").unwrap();
        let _ = git(&seed, &["add", "-A"]);
        let _ = git(&seed, &["commit", "-m", "init"]);
        let push = git(&seed, &["push", "-u", "origin", "master"]);
        assert!(push.status.success(), "{:?}", String::from_utf8_lossy(&push.stderr));

        let alice = init_user_wt(&sb.0, "Alice", "alice@example.com", &remote, "master");
        let mut s = settings_for(&remote, &alice);
        let mut col = Collection::new("MasterCol");
        col.add_request(RequestDraft {
            name: "Ping".into(),
            url: "https://example.com/ping".into(),
            ..Default::default()
        });
        let env = Environment::new("Staging");
        let synced = sync_environment(
            &sb.0,
            &mut s,
            &[col],
            &[env],
            "Staging",
            &VariableContext::default(),
        )
        .unwrap();
        assert!(synced.result.ok, "{}", synced.result.message);
        assert_eq!(synced.result.branch, "master");

        let bob = init_user_wt(&sb.0, "Bob", "bob@example.com", &remote, "master");
        let mut bs = settings_for(&remote, &bob);
        let pulled = pull_workspace(&sb.0, &mut bs, &[], &[], &HashMap::new()).unwrap();
        assert!(pulled.result.ok, "{}", pulled.result.message);
        assert_eq!(pulled.collections.len(), 1);
        assert_eq!(pulled.collections[0].name, "MasterCol");
    }

    #[test]
    fn two_users_concurrent_different_requests_resolve_without_orphans() {
        let sb = new_sandbox();
        let remote = sb.0.join("team-remote");
        init_bare(&remote, "main");
        let alice = init_user_wt(&sb.0, "Alice", "alice@example.com", &remote, "main");

        let mut base_col = Collection::new("Shop");
        let health = RequestDraft {
            name: "Health".into(),
            url: "https://example.com/health".into(),
            ..Default::default()
        };
        base_col.add_request(health.clone());
        let dev = Environment::new("Development");
        let prod = Environment::new("Production");

        let mut as_set = settings_for(&remote, &alice);
        let vars = VariableContext::default();
        let r0 = sync_environment(
            &sb.0,
            &mut as_set,
            &[base_col.clone()],
            &[dev.clone(), prod.clone()],
            "Development",
            &vars,
        )
        .unwrap();
        assert!(r0.result.ok, "{}", r0.result.message);

        // Bob pulls initial workspace
        let bob = init_user_wt(&sb.0, "Bob", "bob@example.com", &remote, "main");
        let mut bs_set = settings_for(&remote, &bob);
        let b0 = pull_workspace(&sb.0, &mut bs_set, &[], &[], &HashMap::new()).unwrap();
        assert!(b0.result.ok, "{}", b0.result.message);
        let mut bob_col = b0.collections[0].clone();

        // Alice adds Login and syncs first (on Development)
        let mut alice_col = r0.collections[0].clone();
        let login = RequestDraft {
            name: "Login".into(),
            url: "https://example.com/login".into(),
            ..Default::default()
        };
        let login_id = login.id;
        alice_col.add_request(login);
        let r1 = sync_environment(
            &sb.0,
            &mut as_set,
            &[alice_col],
            &r0.environments,
            "Development",
            &vars,
        )
        .unwrap();
        assert!(r1.result.ok, "{}", r1.result.message);

        // Concurrently, Bob adds Orders (on Production) -> pulls/syncs -> hits collection.json conflict ONLY (NOT project.json!)
        let orders = RequestDraft {
            name: "Orders".into(),
            url: "https://example.com/orders".into(),
            ..Default::default()
        };
        let orders_id = orders.id;
        bob_col.add_request(orders);
        let b1 = sync_environment(
            &sb.0,
            &mut bs_set,
            &[bob_col],
            &b0.environments,
            "Production",
            &vars,
        )
        .unwrap();
        assert!(!b1.result.ok, "expected conflict on collection.json");
        let conflicts = detect_git_conflicts(&bob).unwrap();
        assert!(
            !conflicts.iter().any(|p| p.contains("project.json")),
            "project.json must not conflict on env difference: {conflicts:?}"
        );
        assert!(
            conflicts.iter().any(|p| p.ends_with("collection.json")),
            "expected collection.json conflict: {conflicts:?}"
        );

        // Even keeping remote collection.json (which only lists Health + Login), Bob's Orders request on disk must be recovered without orphan loss!
        let rem = resolve_git_conflicts(&bob, &conflicts, false).unwrap();
        assert!(rem.is_empty(), "{rem:?}");
        let (reloaded, _, _) =
            load_synced_tree_with_secrets(&bob, &[], &[], &HashMap::new()).unwrap();
        assert!(reloaded[0].find_request(health.id).is_some());
        assert!(reloaded[0].find_request(login_id).is_some());
        assert!(reloaded[0].find_request(orders_id).is_some());
    }

    #[test]
    fn rebase_conflict_keep_local_and_keep_remote_not_inverted() {
        for keep_local in [true, false] {
            let sb = new_sandbox();
            let remote = sb.0.join("rebase-remote");
            init_bare(&remote, "main");
            let alice = init_user_wt(&sb.0, "Alice", "alice@example.com", &remote, "main");
            fs::write(alice.join("shared.json"), "base").unwrap();
            let _ = git(&alice, &["add", "-A"]);
            let _ = git(&alice, &["commit", "-m", "base"]);
            let _ = git(&alice, &["push", "-u", "origin", "main"]);

            let bob = init_user_wt(&sb.0, "Bob", "bob@example.com", &remote, "main");
            let _ = git(&bob, &["pull", "--rebase", "origin", "main"]);

            // Alice pushes remote update
            fs::write(alice.join("shared.json"), "REMOTE_ALICE").unwrap();
            let _ = git(&alice, &["add", "-A"]);
            let _ = git(&alice, &["commit", "-m", "alice"]);
            let _ = git(&alice, &["push", "origin", "main"]);

            // Bob commits conflicting local update then pulls (triggers rebase conflict)
            fs::write(bob.join("shared.json"), "LOCAL_BOB").unwrap();
            let _ = git(&bob, &["add", "-A"]);
            let _ = git(&bob, &["commit", "-m", "bob"]);
            let pull_res = pull_rebase(&bob).unwrap();
            assert!(!pull_res.ok);
            assert!(is_rebase_in_progress(&bob));

            // Preview must map (local_version, remote_version) accurately!
            let (preview_local, preview_remote) = get_conflict_file_versions(&bob, "shared.json");
            assert_eq!(preview_local.trim(), "LOCAL_BOB");
            assert_eq!(preview_remote.trim(), "REMOTE_ALICE");

            let rem = resolve_git_conflicts(&bob, &["shared.json".into()], keep_local).unwrap();
            assert!(rem.is_empty(), "{rem:?}");
            assert!(!is_rebase_in_progress(&bob));

            let final_text = fs::read_to_string(bob.join("shared.json")).unwrap();
            if keep_local {
                assert_eq!(final_text.trim(), "LOCAL_BOB");
            } else {
                assert_eq!(final_text.trim(), "REMOTE_ALICE");
            }
        }
    }

    #[test]
    fn modify_delete_conflict_du_and_ud_resolve_cleanly() {
        for keep_local in [true, false] {
            let sb = new_sandbox();
            let remote = sb.0.join("del-remote");
            init_bare(&remote, "main");
            let alice = init_user_wt(&sb.0, "Alice", "alice@example.com", &remote, "main");
            fs::write(alice.join("req.json"), "v0").unwrap();
            let _ = git(&alice, &["add", "-A"]);
            let _ = git(&alice, &["commit", "-m", "base"]);
            let _ = git(&alice, &["push", "-u", "origin", "main"]);

            let bob = init_user_wt(&sb.0, "Bob", "bob@example.com", &remote, "main");
            let _ = git(&bob, &["pull", "--rebase", "origin", "main"]);

            // Alice deletes req.json
            fs::remove_file(alice.join("req.json")).unwrap();
            let _ = git(&alice, &["add", "-A"]);
            let _ = git(&alice, &["commit", "-m", "alice delete"]);
            let _ = git(&alice, &["push", "origin", "main"]);

            // Bob edits req.json
            fs::write(bob.join("req.json"), "BOB_KEPT_EDIT").unwrap();
            let _ = git(&bob, &["add", "-A"]);
            let _ = git(&bob, &["commit", "-m", "bob edit"]);
            let pr = pull_rebase(&bob).unwrap();
            assert!(!pr.ok);

            let conflicts = detect_git_conflicts(&bob).unwrap();
            assert_eq!(conflicts, vec!["req.json".to_string()]);
            let rem = resolve_git_conflicts(&bob, &conflicts, keep_local).unwrap();
            assert!(rem.is_empty(), "{rem:?}");
            assert!(!is_rebase_in_progress(&bob));

            if keep_local {
                assert_eq!(fs::read_to_string(bob.join("req.json")).unwrap(), "BOB_KEPT_EDIT");
            } else {
                assert!(!bob.join("req.json").exists());
            }
        }
    }

    #[test]
    fn abandoned_rebase_detected_and_aborted_cleanly() {
        let sb = new_sandbox();
        let remote = sb.0.join("abort-remote");
        init_bare(&remote, "main");
        let alice = init_user_wt(&sb.0, "Alice", "alice@example.com", &remote, "main");
        fs::write(alice.join("f.json"), "v0").unwrap();
        let _ = git(&alice, &["add", "-A"]);
        let _ = git(&alice, &["commit", "-m", "base"]);
        let _ = git(&alice, &["push", "-u", "origin", "main"]);

        let bob = init_user_wt(&sb.0, "Bob", "bob@example.com", &remote, "main");
        let _ = git(&bob, &["pull", "--rebase", "origin", "main"]);

        fs::write(alice.join("f.json"), "alice").unwrap();
        let _ = git(&alice, &["add", "-A"]);
        let _ = git(&alice, &["commit", "-m", "alice"]);
        let _ = git(&alice, &["push", "origin", "main"]);

        fs::write(bob.join("f.json"), "bob").unwrap();
        let _ = git(&bob, &["add", "-A"]);
        let _ = git(&bob, &["commit", "-m", "bob"]);
        let _ = pull_rebase(&bob).unwrap();

        assert!(is_rebase_in_progress(&bob));
        assert!(!detect_git_conflicts(&bob).unwrap().is_empty());

        abort_git_rebase(&bob).unwrap();
        assert!(!is_rebase_in_progress(&bob));
        assert!(detect_git_conflicts(&bob).unwrap().is_empty());
        assert_eq!(fs::read_to_string(bob.join("f.json")).unwrap(), "bob");
    }

    #[test]
    fn switching_remote_url_rejects_unrelated_repo_safely() {
        let sb = new_sandbox();
        let repo_a = sb.0.join("repo-a");
        let repo_b = sb.0.join("repo-b");
        init_bare(&repo_a, "main");
        init_bare(&repo_b, "main");

        let wt = init_user_wt(&sb.0, "Alice", "alice@example.com", &repo_a, "main");
        assert!(ensure_clone(repo_a.to_str().unwrap(), &wt).is_ok());

        let err = ensure_clone(repo_b.to_str().unwrap(), &wt).unwrap_err();
        assert!(err.to_string().contains("different remote"), "{err}");
        // Origin must remain Repo A (never silently rewritten to Repo B)
        assert_eq!(read_origin_url(&wt).as_deref(), Some(repo_a.to_str().unwrap()));
    }

    #[test]
    fn link_folder_preserves_actual_origin_url() {
        let sb = new_sandbox();
        let origin = sb.0.join("team-repo");
        init_bare(&origin, "main");
        let wt = init_user_wt(&sb.0, "Dev", "dev@example.com", &origin, "main");
        fs::write(wt.join("README.md"), "hi").unwrap();
        let _ = git(&wt, &["add", "-A"]);
        let _ = git(&wt, &["commit", "-m", "init"]);

        let mut settings = AppSettings::default();
        let synced = link_folder(&sb.0, &mut settings, &wt).unwrap();
        assert!(synced.result.ok);
        assert!(settings.cloud_sync.enabled);
        assert_eq!(settings.cloud_sync.remote_url, origin.to_str().unwrap());

        // Repo with NO origin stays empty (never injects DEFAULT_CLOUD_REMOTE)
        let local_only = init_user_wt(&sb.0, "Solo", "solo@example.com", &origin, "main");
        let _ = git(&local_only, &["remote", "remove", "origin"]);
        let mut settings2 = AppSettings::default();
        let _ = link_folder(&sb.0, &mut settings2, &local_only).unwrap();
        assert!(settings2.cloud_sync.remote_url.is_empty());
    }

    #[test]
    fn secrets_redacted_in_git_and_preserved_locally_for_auth_vars_globals_and_overrides() {
        let sb = new_sandbox();
        let remote = sb.0.join("sec-remote");
        init_bare(&remote, "main");
        let alice = init_user_wt(&sb.0, "Alice", "alice@example.com", &remote, "main");

        let mut col = Collection::new("SecCol");
        col.auth = AuthConfig {
            auth_type: AuthType::Oauth2,
            oauth2: crate::core::request::OAuth2Config {
                grant: crate::core::request::OAuth2Grant::AuthorizationCode,
                auth_url: "https://auth.example.com/authorize".into(),
                token_url: "https://auth.example.com/token".into(),
                client_id: "client-123".into(),
                client_secret: "SUPER_OAUTH_SECRET".into(),
                scope: "read write".into(),
                redirect_uri: "http://127.0.0.1:8765/cb".into(),
                access_token: "LIVE_ACCESS_TOK".into(),
                refresh_token: "LIVE_REFRESH_TOK".into(),
            },
            ..Default::default()
        };
        col.variables.push(KeyValue::new("apiHost", "https://api.example.com"));
        col.variables.push(KeyValue::new("col_api_key", "COL_SECRET_KEY_XYZ"));
        let req = RequestDraft {
            name: "Private".into(),
            url: "https://api.example.com/me".into(),
            auth: AuthConfig {
                auth_type: AuthType::Bearer,
                token: "REQ_BEARER_SECRET".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let req_id = req.id;
        col.add_request(req);
        col.environment_overrides.variables.push(KeyValue::new("env_token", "OV_SECRET_999"));

        let mut dev_env = Environment::new("Development");
        dev_env.values.push(EnvVar::new("api_token", "DEV_ENV_SECRET"));
        let mut local_env = Environment::new("Local");
        local_env.values.push(EnvVar::new("myLocal", "ONLY_ON_MY_MACHINE"));

        let mut vars = VariableContext::default();
        vars.global.insert("publicVar".into(), "public-val".into());
        vars.global.insert("global_api_token".into(), "GLOBAL_SECRET_777".into());
        vars.local.insert("ephemeral".into(), "NEVER_EXPORT".into());

        let mut s = settings_for(&remote, &alice);
        let synced = sync_environment(
            &sb.0,
            &mut s,
            &[col.clone()],
            &[dev_env.clone(), local_env.clone()],
            "Development",
            &vars,
        )
        .unwrap();
        assert!(synced.result.ok, "{}", synced.result.message);

        // Ensure NO secret leaked anywhere into git working tree, and Local env was excluded!
        let mut blob = String::new();
        fn collect(dir: &Path, out: &mut String) {
            for e in fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.file_name().is_some_and(|n| n == ".git") {
                    continue;
                }
                if p.is_dir() {
                    collect(&p, out);
                } else if let Ok(t) = fs::read_to_string(&p) {
                    out.push_str(&t);
                }
            }
        }
        collect(&alice, &mut blob);
        for secret in [
            "SUPER_OAUTH_SECRET",
            "LIVE_ACCESS_TOK",
            "LIVE_REFRESH_TOK",
            "COL_SECRET_KEY_XYZ",
            "REQ_BEARER_SECRET",
            "OV_SECRET_999",
            "DEV_ENV_SECRET",
            "ONLY_ON_MY_MACHINE",
            "GLOBAL_SECRET_777",
            "NEVER_EXPORT",
        ] {
            assert!(!blob.contains(secret), "secret leaked to disk: {secret}");
        }
        assert!(!alice.join("environments/local.environment.json").exists());

        // Non-secret OAuth2 config survived in Git and local secrets were restored in `synced`!
        let sc = &synced.collections[0];
        assert_eq!(sc.auth.oauth2.auth_url, "https://auth.example.com/authorize");
        assert_eq!(sc.auth.oauth2.client_id, "client-123");
        assert_eq!(sc.auth.oauth2.client_secret, "SUPER_OAUTH_SECRET");
        assert_eq!(sc.find_request(req_id).unwrap().auth.token, "REQ_BEARER_SECRET");
        assert_eq!(
            sc.variables.iter().find(|v| v.key == "col_api_key").unwrap().value,
            "COL_SECRET_KEY_XYZ"
        );
        assert_eq!(
            sc.environment_overrides.variables[0].value,
            "OV_SECRET_999"
        );
        assert_eq!(
            synced.globals.get("global_api_token").map(String::as_str),
            Some("GLOBAL_SECRET_777")
        );
        assert_eq!(
            synced.globals.get("publicVar").map(String::as_str),
            Some("public-val")
        );
        // Machine-local "Local" env stays intact across sync!
        assert!(synced.environments.iter().any(|e| e.name == "Local"));
    }

    #[test]
    fn folder_auth_description_and_env_initial_value_survive_roundtrip() {
        let sb = new_sandbox();
        let wt = sb.0.join("rt");
        fs::create_dir_all(&wt).unwrap();

        let mut col = Collection::new("MetaCol");
        col.pinned_environment_id = Some(Uuid::new_v4());
        let mut f = Folder::new("Admin");
        f.description = "Admin endpoints".into();
        f.auth = AuthConfig {
            auth_type: AuthType::Basic,
            username: "admin".into(),
            password: "LOCAL_PASS".into(),
            ..Default::default()
        };
        let fid = f.id;
        col.add_folder(f);

        let mut env = Environment::new("Staging");
        env.values.push(EnvVar {
            key: "region".into(),
            initial_value: "us-east-1".into(),
            current_value: "eu-west-1".into(),
            enabled: true,
        });

        export_workspace_tree(
            &wt,
            "MetaCol",
            "",
            "Staging",
            &[col.clone()],
            &[env.clone()],
            &VariableContext::default(),
        )
        .unwrap();

        let (cols, envs, _) =
            load_synced_tree_with_secrets(&wt, &[col.clone()], &[env], &HashMap::new()).unwrap();
        let loaded_f = match &cols[0].items[0] {
            CollectionItem::Folder(folder) if folder.id == fid => folder,
            other => panic!("unexpected item: {other:?}"),
        };
        assert_eq!(loaded_f.description, "Admin endpoints");
        assert_eq!(loaded_f.auth.auth_type, AuthType::Basic);
        assert_eq!(loaded_f.auth.username, "admin");
        assert_eq!(loaded_f.auth.password, "LOCAL_PASS");
        assert_eq!(cols[0].pinned_environment_id, col.pinned_environment_id);

        assert_eq!(envs[0].values[0].initial_value, "us-east-1");
        assert_eq!(envs[0].values[0].current_value, "eu-west-1");
    }

    #[test]
    fn duplicate_slug_deletion_does_not_shift_sibling_filenames() {
        let sb = new_sandbox();
        let wt = sb.0.join("slug-wt");
        fs::create_dir_all(&wt).unwrap();

        let mut col = Collection::new("SlugTest");
        let r1 = RequestDraft { name: "Get User".into(), url: "https://x.com/1".into(), ..Default::default() };
        let r2 = RequestDraft { name: "Get-User".into(), url: "https://x.com/2".into(), ..Default::default() };
        let r3 = RequestDraft { name: "Get_User".into(), url: "https://x.com/3".into(), ..Default::default() };
        let r3_id8 = &r3.id.simple().to_string()[..8];
        col.add_request(r1);
        col.add_request(r2.clone());
        col.add_request(r3.clone());

        export_workspace_tree(
            &wt,
            "SlugTest",
            "",
            "Dev",
            &[col.clone()],
            &[],
            &VariableContext::default(),
        )
        .unwrap();

        let r3_expected = wt.join(format!("collections/slugtest/requests/get-user-{r3_id8}.request.json"));
        assert!(r3_expected.exists(), "missing {}", r3_expected.display());

        // Delete middle duplicate r2 -> r3 filename MUST NOT shift!
        col.items.retain(|i| !matches!(i, CollectionItem::Request(r) if r.id == r2.id));
        export_workspace_tree(
            &wt,
            "SlugTest",
            "",
            "Dev",
            &[col],
            &[],
            &VariableContext::default(),
        )
        .unwrap();

        assert!(r3_expected.exists(), "r3 filename shifted after deleting r2!");
    }

    #[test]
    fn team_activity_keeps_github_noreply_humans_filters_bots_and_handles_pipes() {
        let sb = new_sandbox();
        let remote = sb.0.join("act-remote");
        init_bare(&remote, "main");
        let alice = init_user_wt(
            &sb.0,
            "Alice | Core",
            "12345+alice@users.noreply.github.com",
            &remote,
            "main",
        );
        fs::write(alice.join("a.txt"), "one").unwrap();
        let _ = git(&alice, &["add", "-A"]);
        let _ = git(&alice, &["commit", "-m", "feat: add a | b endpoint"]);
        let _ = git(&alice, &["push", "-u", "origin", "main"]);

        // Add a bot commit
        fs::write(alice.join("bot.txt"), "bot").unwrap();
        let _ = git(&alice, &["add", "-A"]);
        let _ = git(
            &alice,
            &[
                "-c",
                "user.name=dependabot[bot]",
                "-c",
                "user.email=49699333+dependabot[bot]@users.noreply.github.com",
                "commit",
                "-m",
                "chore(deps): bump",
            ],
        );

        let act = get_team_activity_ex(&alice, 10, true).unwrap();
        assert!(
            act.members
                .iter()
                .any(|m| m.name == "Alice | Core" && m.email == "12345+alice@users.noreply.github.com"),
            "human GitHub privacy user missing: {:?}",
            act.members
        );
        assert!(
            !act.members.iter().any(|m| m.name.contains("dependabot")),
            "bot must be filtered: {:?}",
            act.members
        );
        assert!(
            act.recent_commits
                .iter()
                .any(|c| c.message == "feat: add a | b endpoint" && c.author_name == "Alice | Core"),
            "pipe characters in commit message/author must parse intact: {:?}",
            act.recent_commits
        );
    }

    #[test]
    fn switching_between_git_repos_loads_only_target_repo_files_and_tracks_recent_repos() {
        let sb = new_sandbox();
        let remote_a = sb.0.join("org/alpha-api");
        let remote_b = sb.0.join("org/beta-api");
        let remote_c = sb.0.join("org/empty-api");
        init_bare(&remote_a, "main");
        init_bare(&remote_b, "main");
        init_bare(&remote_c, "main");

        // Populate Repo A with AlphaCol, AlphaEnv, and alphaGlobal
        let wt_a = init_user_wt(&sb.0, "AliceA", "alice@example.com", &remote_a, "main");
        let mut col_a = Collection::new("AlphaCol");
        col_a.add_request(RequestDraft {
            name: "AlphaOnly".into(),
            url: "https://alpha.example.com/v1".into(),
            ..Default::default()
        });
        let env_a = Environment::new("AlphaEnv");
        let mut vars_a = VariableContext::default();
        vars_a.global.insert("alphaGlobal".into(), "A1".into());
        let mut settings = settings_for(&remote_a, &wt_a);
        let synced_a =
            sync_environment(&sb.0, &mut settings, &[col_a], &[env_a], "AlphaEnv", &vars_a)
                .unwrap();
        assert!(synced_a.result.ok);
        assert_eq!(synced_a.collections.len(), 1);
        assert_eq!(synced_a.collections[0].name, "AlphaCol");

        // Populate Repo B with BetaCol, BetaEnv, and betaGlobal
        let wt_b = init_user_wt(&sb.0, "AliceB", "alice@example.com", &remote_b, "main");
        let mut col_b = Collection::new("BetaCol");
        col_b.add_request(RequestDraft {
            name: "BetaOnly".into(),
            url: "https://beta.example.com/v2".into(),
            ..Default::default()
        });
        let env_b = Environment::new("BetaEnv");
        let mut vars_b = VariableContext::default();
        vars_b.global.insert("betaGlobal".into(), "B2".into());
        let mut settings_b_seed = settings_for(&remote_b, &wt_b);
        let synced_b_seed = sync_environment(
            &sb.0,
            &mut settings_b_seed,
            &[col_b],
            &[env_b],
            "BetaEnv",
            &vars_b,
        )
        .unwrap();
        assert!(synced_b_seed.result.ok);

        // Now user switches `settings` from Repo A -> Repo B: must load ONLY Repo B's files
        settings.cloud_sync.remote_url = remote_b.to_str().unwrap().to_string();
        settings.cloud_sync.clone_path = wt_b.to_str().unwrap().to_string();
        let switched_b = switch_git_workspace(&sb.0, &mut settings).unwrap();
        assert!(switched_b.result.ok, "{}", switched_b.result.message);
        assert_eq!(switched_b.collections.len(), 1);
        assert_eq!(switched_b.collections[0].name, "BetaCol");
        assert_eq!(switched_b.environments.len(), 1);
        assert_eq!(switched_b.environments[0].name, "BetaEnv");
        assert_eq!(switched_b.globals.get("betaGlobal").map(String::as_str), Some("B2"));
        assert!(!switched_b.globals.contains_key("alphaGlobal"));

        // Switch from Repo B -> empty Repo C: must return 0 collections, 0 envs, 0 globals (no Beta carryover!)
        let wt_c = init_user_wt(&sb.0, "AliceC", "alice@example.com", &remote_c, "main");
        settings.cloud_sync.remote_url = remote_c.to_str().unwrap().to_string();
        settings.cloud_sync.clone_path = wt_c.to_str().unwrap().to_string();
        let switched_c = switch_git_workspace(&sb.0, &mut settings).unwrap();
        assert!(switched_c.result.ok, "{}", switched_c.result.message);
        assert!(switched_c.collections.is_empty(), "empty repo must have 0 collections");
        assert!(switched_c.environments.is_empty(), "empty repo must have 0 environments");
        assert!(switched_c.globals.is_empty(), "empty repo must have 0 globals");

        // Recent repos must list Repo C, Repo B, Repo A in MRU order!
        assert_eq!(settings.cloud_sync.recent_repos.len(), 3);
        assert_eq!(
            settings.cloud_sync.recent_repos[0].remote_url,
            remote_c.to_str().unwrap()
        );
        assert_eq!(
            settings.cloud_sync.recent_repos[1].remote_url,
            remote_b.to_str().unwrap()
        );
        assert_eq!(
            settings.cloud_sync.recent_repos[2].remote_url,
            remote_a.to_str().unwrap()
        );
    }
}
