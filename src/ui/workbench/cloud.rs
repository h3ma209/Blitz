use std::path::{Path, PathBuf};
use uuid::Uuid;

use dioxus::prelude::*;

use crate::core::{
    clone_or_ensure, detect_git_conflicts, find_environment, find_request_across,
    get_team_activity_ex, is_git_repo, is_rebase_in_progress, link_folder, open_in_file_manager,
    pull_workspace, push_workspace, refresh_status, switch_git_workspace, sync_environment,
    AppSettings,
};
use crate::storage::JsonStore;
use crate::ui::workbench::state::{sync_env_vars, WorkbenchState};
use crate::ui::{ConflictState, ToastLevel};

pub fn open_conflicts_if_any(
    mut st: WorkbenchState,
    clone_path: PathBuf,
    conflicts: Vec<String>,
    rebasing: bool,
) -> bool {
    if conflicts.is_empty() && !rebasing {
        return false;
    }
    let count = conflicts.len();
    st.conflict_state.set(Some(ConflictState {
        clone_path,
        files: conflicts,
        selected_idx: 0,
    }));
    st.active_request_id.set(None);
    st.editing_collection_id.set(None);
    st.show_settings.set(false);
    st.show_team.set(false);
    let msg = if count > 0 {
        format!("⚔️ Merge conflicts detected in {count} file(s)!")
    } else {
        "⚔️ Unfinished rebase in progress — resolve or abort rebase".into()
    };
    st.status_msg.set(msg.clone());
    st.log_warn(format!("☁ [Cloud] {msg}"));
    true
}

fn check_clone_conflicts(clone_path: &Path) -> (Vec<String>, bool) {
    let conflicts = detect_git_conflicts(clone_path).unwrap_or_default();
    let rebasing = is_rebase_in_progress(clone_path);
    (conflicts, rebasing)
}

pub fn switch_to_env(mut st: WorkbenchState, new_id: Option<Uuid>) {
    let envs = (st.environments)();
    st.active_env_id.set(new_id);
    st.editing_env_id.set(new_id);
    let mut v = (st.vars)();
    sync_env_vars(&envs, new_id, &mut v);
    st.vars.set(v);
    st.persist();
    st.env_switch_overlay.set(None);
}

pub fn sync_selected_env(mut st: WorkbenchState, id: Uuid) {
    if (st.cloud_busy)() {
        return;
    }
    let envs = (st.environments)();
    let Some(env) = envs.iter().find(|e| e.id == id) else {
        st.status_msg.set("sync: env not found".into());
        return;
    };
    let env_name = env.name.clone();
    if env_name.trim().eq_ignore_ascii_case("Local") {
        st.status_msg.set(
            "Local environment stays local-only — switch to a shared environment to Sync".into(),
        );
        st.log_warn("☁ [Cloud] Local environment is local-only; skipping remote sync");
        return;
    }
    let mut settings = (st.app_settings)();
    if settings.cloud_sync.remote_url.trim().is_empty() {
        let msg = "Configure Remote Git Repository URL in Cloud Settings first".to_string();
        st.status_msg.set(msg.clone());
        st.notify(msg, ToastLevel::Warn);
        return;
    }
    settings.cloud_sync.enabled = true;

    // Activate this env first (UI + var map).
    st.active_env_id.set(Some(id));
    st.editing_env_id.set(Some(id));
    let mut v = (st.vars)();
    sync_env_vars(&envs, Some(id), &mut v);
    st.vars.set(v.clone());

    let Ok(store) = JsonStore::open_default() else {
        st.status_msg.set("sync: cannot open store".into());
        return;
    };
    st.status_msg.set(format!("syncing {env_name}…"));
    st.log_cloud(format!("☁ [Cloud] Syncing environment '{env_name}' with remote…"));
    let envs_mut = envs;

    let root = store.root().to_path_buf();
    let cols = (st.collections)();
    st.cloud_busy.set(true);

    spawn(async move {
        let env_name_bg = env_name.clone();
        let clone_path = settings.cloud_sync.resolved_clone_path(&root);
        let res = tokio::task::spawn_blocking(move || {
            let r = sync_environment(&root, &mut settings, &cols, &envs_mut, &env_name_bg, &v);
            let need_conflict_check =
                r.is_err() || matches!(&r, Ok(sw) if !sw.result.ok);
            let (conflicts, rebasing) = if need_conflict_check {
                check_clone_conflicts(&clone_path)
            } else {
                (Vec::new(), false)
            };
            (r, settings, clone_path, conflicts, rebasing)
        })
        .await;

        st.cloud_busy.set(false);
        match res {
            Ok((Ok(synced), updated_settings, clone_path, conflicts, rebasing)) => {
                if !synced.result.ok {
                    st.patch_cloud_meta(
                        &updated_settings.cloud_sync.last_status,
                        &updated_settings.cloud_sync.last_branch,
                        Some(updated_settings.cloud_sync.enabled),
                        None,
                        None,
                    );
                    if !open_conflicts_if_any(st, clone_path, conflicts, rebasing) {
                        st.status_msg.set(synced.result.message.clone());
                        st.log_error(format!("☁ [Cloud] Sync failed: {}", synced.result.message));
                    }
                    return;
                }
                st.apply_synced_workspace(
                    synced.collections,
                    synced.environments,
                    synced.globals,
                    Some(&env_name),
                    &synced.result.message,
                    &synced.result.branch,
                );
                st.patch_cloud_meta(
                    &synced.result.message,
                    &synced.result.branch,
                    Some(updated_settings.cloud_sync.enabled),
                    None,
                    None,
                );
                st.status_msg.set(synced.result.message.clone());
                st.log_cloud(format!("☁ [Cloud] Sync complete: {}", synced.result.message));
            }
            Ok((Err(e), updated_settings, clone_path, conflicts, rebasing)) => {
                st.patch_cloud_meta(
                    &updated_settings.cloud_sync.last_status,
                    &updated_settings.cloud_sync.last_branch,
                    None,
                    None,
                    None,
                );
                if !open_conflicts_if_any(st, clone_path, conflicts, rebasing) {
                    st.status_msg.set(format!("sync: {e}"));
                    st.log_error(format!("☁ [Cloud] Sync failed: {e}"));
                }
            }
            Err(e) => {
                st.status_msg.set(format!("sync join: {e}"));
                st.log_error(format!("☁ [Cloud] Sync join error: {e}"));
            }
        }
    });
}

/// Guarded Git repository switch: warns if dirty open tabs or uncommitted clone changes exist
/// before wiping state and switching to `next_settings`.
pub fn request_git_switch(mut st: WorkbenchState, next_settings: AppSettings) {
    let cols = (st.collections)();
    let dirty_tabs: Vec<String> = (st.open_tabs)()
        .iter()
        .filter(|tab| {
            find_request_across(&cols, tab.id).is_some_and(|(_, saved)| saved != *tab)
        })
        .map(|tab| tab.name.clone())
        .collect();

    let mut reasons = Vec::new();
    if !dirty_tabs.is_empty() {
        reasons.push(format!("{} unsaved tab(s)", dirty_tabs.len()));
    }

    let cur_settings = (st.app_settings)();
    if let Ok(store) = JsonStore::open_default() {
        let cur_clone = cur_settings.cloud_sync.resolved_clone_path(store.root());
        if is_git_repo(&cur_clone) {
            if let Ok(detail) = crate::core::cloud_sync::status_detail(&cur_clone) {
                if detail.dirty > 0 {
                    reasons.push(format!("{} uncommitted file(s) in current Git clone", detail.dirty));
                } else if detail.ahead > 0 {
                    reasons.push(format!("{} unpushed commit(s) in current Git clone", detail.ahead));
                }
            }
        }
    }

    if !reasons.is_empty() {
        st.pending_git_switch
            .set(Some((next_settings, reasons.join(" and "))));
        return;
    }

    st.app_settings.set(next_settings);
    st.persist_local();
    cloud_switch_repo(st);
}

/// Switch to the currently configured Git repository: immediately cleans all UI state
/// from any previous Git repo and loads ONLY the files from the target repository.
pub fn cloud_switch_repo(mut st: WorkbenchState) {
    if (st.cloud_busy)() {
        return;
    }
    let Ok(store) = JsonStore::open_default() else {
        st.status_msg.set("cloud: cannot open store".into());
        return;
    };
    let mut settings = (st.app_settings)();
    let has_remote = !settings.cloud_sync.remote_url.trim().is_empty();
    let has_clone = !settings.cloud_sync.clone_path.trim().is_empty();

    // Clean UI immediately so nothing from the previous Git repository lingers.
    st.reset_for_git_switch();

    if !has_remote && !has_clone {
        st.status_msg.set("cleared git workspace".into());
        st.persist_local();
        return;
    }

    let root = store.root().to_path_buf();
    let clone_path = settings.cloud_sync.resolved_clone_path(&root);
    st.cloud_busy.set(true);
    st.is_loading.set(true);
    st.status_msg.set("switching git repository…".into());
    st.log_cloud(format!(
        "☁ [Cloud] Switching Git repository ({})…",
        if has_remote {
            crate::core::cloud_sync::mask_remote_url(&settings.cloud_sync.remote_url)
        } else {
            settings.cloud_sync.clone_path.clone()
        }
    ));
    spawn(async move {
        let clone_for_check = clone_path.clone();
        let res = tokio::task::spawn_blocking(move || {
            let r = switch_git_workspace(&root, &mut settings);
            let need_conflict_check =
                r.is_err() || matches!(&r, Ok(sw) if !sw.result.ok);
            let (conflicts, rebasing) = if need_conflict_check {
                check_clone_conflicts(&clone_for_check)
            } else {
                (Vec::new(), false)
            };
            (r, settings, conflicts, rebasing)
        })
        .await;
        st.cloud_busy.set(false);
        st.is_loading.set(false);
        match res {
            Ok((Ok(synced), updated_settings, conflicts, rebasing)) => {
                if !synced.result.ok {
                    st.patch_cloud_meta(
                        &updated_settings.cloud_sync.last_status,
                        &updated_settings.cloud_sync.last_branch,
                        Some(updated_settings.cloud_sync.enabled),
                        Some(&updated_settings.cloud_sync.remote_url),
                        Some(&updated_settings.cloud_sync.clone_path),
                    );
                    if !open_conflicts_if_any(st, clone_path, conflicts, rebasing) {
                        st.status_msg.set(synced.result.message.clone());
                        st.log_error(format!(
                            "☁ [Cloud] Switch git failed: {}",
                            synced.result.message
                        ));
                    }
                    return;
                }
                st.apply_switched_git_workspace(
                    synced.collections,
                    synced.environments,
                    synced.globals,
                    &synced.result.message,
                    &synced.result.branch,
                );
                st.patch_cloud_meta(
                    &updated_settings.cloud_sync.last_status,
                    &updated_settings.cloud_sync.last_branch,
                    Some(true),
                    Some(&updated_settings.cloud_sync.remote_url),
                    Some(&updated_settings.cloud_sync.clone_path),
                );
                st.status_msg.set(synced.result.message.clone());
                st.log_cloud(format!("☁ [Cloud] {}", synced.result.message));
                st.persist_local();
            }
            Ok((Err(e), updated_settings, conflicts, rebasing)) => {
                st.patch_cloud_meta(
                    &updated_settings.cloud_sync.last_status,
                    &updated_settings.cloud_sync.last_branch,
                    None,
                    None,
                    None,
                );
                if !open_conflicts_if_any(st, clone_path, conflicts, rebasing) {
                    st.status_msg.set(format!("git switch: {e}"));
                    st.log_error(format!("☁ [Cloud] Switch git failed: {e}"));
                }
            }
            Err(e) => {
                st.status_msg.set(format!("git switch join: {e}"));
                st.log_error(format!("☁ [Cloud] Switch git join error: {e}"));
            }
        }
    });
}

pub fn cloud_clone_or_link(mut st: WorkbenchState) {
    if (st.cloud_busy)() {
        return;
    }
    let settings = (st.app_settings)();
    if settings.cloud_sync.remote_url.trim().is_empty() {
        let msg = "Configure Remote Git Repository URL in Cloud Settings first".to_string();
        st.status_msg.set(msg.clone());
        st.notify(msg, ToastLevel::Warn);
        return;
    }
    cloud_switch_repo(st);
}

pub fn cloud_link_folder(mut st: WorkbenchState) {
    if (st.cloud_busy)() {
        return;
    }
    st.is_loading.set(true);
    spawn(async move {
        let picked = rfd::FileDialog::new()
            .set_title("Link existing git workspace folder")
            .pick_folder();
        let Some(folder) = picked else {
            st.is_loading.set(false);
            st.status_msg.set("link cancelled".into());
            return;
        };
        let Ok(store) = JsonStore::open_default() else {
            st.is_loading.set(false);
            st.status_msg.set("cloud: cannot open store".into());
            return;
        };
        let root = store.root().to_path_buf();
        let mut settings = (st.app_settings)();
        // Clean UI immediately before loading linked folder so previous git state is wiped.
        st.reset_for_git_switch();
        st.cloud_busy.set(true);
        st.log_cloud(format!("☁ [Cloud] Linking local folder '{}'", folder.display()));
        let res = tokio::task::spawn_blocking(move || {
            let r = link_folder(&root, &mut settings, &folder);
            (r, settings)
        })
        .await;
        st.cloud_busy.set(false);
        st.is_loading.set(false);
        match res {
            Ok((Ok(synced), updated_settings)) => {
                let (cols, envs, r) = (synced.collections, synced.environments, synced.result);
                st.apply_switched_git_workspace(
                    cols,
                    envs,
                    synced.globals,
                    &r.message,
                    &r.branch,
                );
                st.patch_cloud_meta(
                    &updated_settings.cloud_sync.last_status,
                    &updated_settings.cloud_sync.last_branch,
                    Some(true),
                    Some(&updated_settings.cloud_sync.remote_url),
                    Some(&updated_settings.cloud_sync.clone_path),
                );
                st.status_msg.set(r.message.clone());
                st.log_cloud(format!("☁ [Cloud] Linked folder success: {}", r.message));
                st.persist_local();
            }
            Ok((Err(e), _)) => {
                st.status_msg.set(format!("link: {e}"));
                st.log_error(format!("☁ [Cloud] Link folder failed: {e}"));
            }
            Err(e) => {
                st.status_msg.set(format!("link join: {e}"));
                st.log_error(format!("☁ [Cloud] Link folder join error: {e}"));
            }
        }
    });
}

pub fn cloud_sync_status(mut st: WorkbenchState) {
    if (st.cloud_busy)() {
        return;
    }
    let Ok(store) = JsonStore::open_default() else {
        st.status_msg.set("cloud: cannot open store".into());
        return;
    };
    let root = store.root().to_path_buf();
    let mut settings = (st.app_settings)();
    st.cloud_busy.set(true);
    spawn(async move {
        let res = tokio::task::spawn_blocking(move || {
            let r = refresh_status(&root, &mut settings);
            (r, settings)
        })
        .await;
        st.cloud_busy.set(false);
        if let Ok((r, updated_settings)) = res {
            st.patch_cloud_meta(
                &updated_settings.cloud_sync.last_status,
                &updated_settings.cloud_sync.last_branch,
                None,
                None,
                None,
            );
            st.status_msg.set(r.message.clone());
            st.log_cloud(format!("☁ [Cloud] Status: {}", r.message));
            st.persist_local();
        }
    });
}

pub fn cloud_pull(mut st: WorkbenchState) {
    if (st.cloud_busy)() {
        return;
    }
    let Ok(store) = JsonStore::open_default() else {
        st.status_msg.set("cloud: cannot open store".into());
        return;
    };
    let mut settings = (st.app_settings)();
    if settings.cloud_sync.remote_url.trim().is_empty() {
        let msg = "Configure Remote Git Repository URL in Cloud Settings first".to_string();
        st.status_msg.set(msg.clone());
        st.notify(msg, ToastLevel::Warn);
        return;
    }
    settings.cloud_sync.enabled = true;
    let root = store.root().to_path_buf();
    let clone_path = settings.cloud_sync.resolved_clone_path(&root);
    let envs = (st.environments)();
    let cols = (st.collections)();
    let globals = (st.vars)().global;
    st.is_loading.set(true);
    st.cloud_busy.set(true);
    st.log_cloud("☁ [Cloud] Pulling latest changes from remote…");
    spawn(async move {
        let clone_for_check = clone_path.clone();
        let res = tokio::task::spawn_blocking(move || {
            let r = pull_workspace(&root, &mut settings, &cols, &envs, &globals);
            let need_conflict_check =
                r.is_err() || matches!(&r, Ok(sw) if !sw.result.ok);
            let (conflicts, rebasing) = if need_conflict_check {
                check_clone_conflicts(&clone_for_check)
            } else {
                (Vec::new(), false)
            };
            (r, settings, conflicts, rebasing)
        })
        .await;
        st.cloud_busy.set(false);
        st.is_loading.set(false);
        match res {
            Ok((Ok(synced), updated_settings, conflicts, rebasing)) => {
                if !synced.result.ok {
                    st.patch_cloud_meta(
                        &updated_settings.cloud_sync.last_status,
                        &updated_settings.cloud_sync.last_branch,
                        Some(true),
                        None,
                        None,
                    );
                    if !open_conflicts_if_any(st, clone_path, conflicts, rebasing) {
                        st.status_msg.set(format!("pull: {}", synced.result.message));
                        st.log_error(format!("☁ [Cloud] Pull failed: {}", synced.result.message));
                    }
                    return;
                }
                st.apply_synced_workspace(
                    synced.collections,
                    synced.environments,
                    synced.globals,
                    None,
                    &synced.result.message,
                    &synced.result.branch,
                );
                st.patch_cloud_meta(
                    &synced.result.message,
                    &synced.result.branch,
                    Some(true),
                    None,
                    None,
                );
                st.status_msg.set(synced.result.message.clone());
                st.log_cloud(format!("☁ [Cloud] Pull complete: {}", synced.result.message));
            }
            Ok((Err(e), _, conflicts, rebasing)) => {
                if !open_conflicts_if_any(st, clone_path, conflicts, rebasing) {
                    st.status_msg.set(format!("pull: {e}"));
                    st.log_error(format!("☁ [Cloud] Pull failed: {e}"));
                }
            }
            Err(e) => {
                st.status_msg.set(format!("pull join: {e}"));
                st.log_error(format!("☁ [Cloud] Pull join error: {e}"));
            }
        }
    });
}

pub fn cloud_push(mut st: WorkbenchState) {
    if (st.cloud_busy)() {
        return;
    }
    let Ok(store) = JsonStore::open_default() else {
        st.status_msg.set("cloud: cannot open store".into());
        return;
    };
    let mut settings = (st.app_settings)();
    if settings.cloud_sync.remote_url.trim().is_empty() {
        let msg = "Configure Remote Git Repository URL in Cloud Settings first".to_string();
        st.status_msg.set(msg.clone());
        st.notify(msg, ToastLevel::Warn);
        return;
    }
    settings.cloud_sync.enabled = true;
    let root = store.root().to_path_buf();
    let clone_path = settings.cloud_sync.resolved_clone_path(&root);
    let envs = (st.environments)();
    let cols = (st.collections)();
    let vars = (st.vars)();
    let active_environment = find_environment(&envs, (st.active_env_id)())
        .map(|e| e.name.clone())
        .unwrap_or_else(|| "No Environment".into());
    let commit = settings.cloud_sync.commit_message.clone();
    st.is_loading.set(true);
    st.cloud_busy.set(true);
    st.log_cloud(format!("☁ [Cloud] Pushing workspace (commit: \"{commit}\")…"));
    spawn(async move {
        let clone_for_check = clone_path.clone();
        let res = tokio::task::spawn_blocking(move || {
            let r = push_workspace(
                &root,
                &mut settings,
                &cols,
                &envs,
                &active_environment,
                Some(commit.as_str()),
                &vars,
            );
            let (conflicts, rebasing) = if !r.ok {
                check_clone_conflicts(&clone_for_check)
            } else {
                (Vec::new(), false)
            };
            (r, settings, conflicts, rebasing)
        })
        .await;
        st.cloud_busy.set(false);
        st.is_loading.set(false);
        match res {
            Ok((r, updated_settings, conflicts, rebasing)) => {
                st.patch_cloud_meta(
                    &updated_settings.cloud_sync.last_status,
                    &updated_settings.cloud_sync.last_branch,
                    Some(true),
                    None,
                    None,
                );
                if r.ok {
                    st.status_msg.set(r.message.clone());
                    st.log_cloud(format!("☁ [Cloud] Push complete: {}", r.message));
                    st.persist_local();
                } else if !open_conflicts_if_any(st, clone_path, conflicts, rebasing) {
                    st.status_msg.set(r.message.clone());
                    st.log_warn(format!("☁ [Cloud] Push: {}", r.message));
                }
            }
            Err(e) => {
                st.status_msg.set(format!("push join: {e}"));
                st.log_error(format!("☁ [Cloud] Push join error: {e}"));
            }
        }
    });
}

pub fn cloud_open_folder(mut st: WorkbenchState) {
    let Ok(store) = JsonStore::open_default() else {
        st.status_msg.set("cloud: cannot open store".into());
        return;
    };
    let settings = (st.app_settings)();
    let path = settings.cloud_sync.resolved_clone_path(store.root());
    match open_in_file_manager(&path) {
        Ok(()) => st.status_msg.set(format!("opened {}", path.display())),
        Err(e) => st.status_msg.set(format!("open folder: {e}")),
    }
}

pub fn open_team_activity(mut st: WorkbenchState, toggle: bool) {
    if toggle && (st.show_team)() {
        st.show_team.set(false);
        return;
    }
    if (st.cloud_busy)() {
        return;
    }
    let Ok(store) = JsonStore::open_default() else {
        st.status_msg.set("team: cannot open store".into());
        return;
    };
    let root = store.root().to_path_buf();
    let settings = (st.app_settings)();
    let clone_path = settings.cloud_sync.resolved_clone_path(&root);
    let has_repo = is_git_repo(&clone_path);

    if !has_repo && (!settings.cloud_sync.enabled || settings.cloud_sync.remote_url.trim().is_empty()) {
        st.hide_stage_pages();
        st.team_tab_open.set(true);
        st.show_team.set(true);
        st.status_msg.set(
            "Configure Remote Git URL and enable Cloud Sync in Settings to view team activity".into(),
        );
        return;
    }

    st.hide_stage_pages();
    st.team_tab_open.set(true);
    st.show_team.set(true);
    st.team_syncing.set(true);
    st.cloud_busy.set(true);
    spawn(async move {
        let mut settings_bg = settings;
        let res = tokio::task::spawn_blocking(move || {
            if !is_git_repo(&clone_path) && !settings_bg.cloud_sync.remote_url.trim().is_empty() {
                clone_or_ensure(&root, &mut settings_bg)?;
            }
            get_team_activity_ex(&clone_path, 30, true).map_err(|e| e.to_string())
        })
        .await;
        st.team_syncing.set(false);
        st.cloud_busy.set(false);
        match res {
            Ok(Ok(act)) => st.team_activity.set(act),
            Ok(Err(e)) => {
                st.status_msg.set(format!("team activity: {e}"));
                st.log_warn(format!("☁ [Cloud] Team activity: {e}"));
            }
            Err(e) => st.status_msg.set(format!("team join: {e}")),
        }
    });
}
