use std::time::Duration;
use uuid::Uuid;

use dioxus::prelude::*;

use crate::core::{
    branch_name_for_env, clone_or_ensure, detect_git_conflicts, ensure_default_environments,
    find_environment, get_team_activity, link_folder, open_in_file_manager, pull_workspace,
    push_workspace, refresh_status, switch_env_branch, sync_environment, DEFAULT_CLOUD_REMOTE,
};
use crate::storage::JsonStore;
use crate::ui::workbench::state::{sync_env_vars, WorkbenchState};
use crate::ui::ConflictState;

pub fn switch_to_env(mut st: WorkbenchState, new_id: Option<Uuid>) {
    let cur = (st.active_env_id)();
    let envs = (st.environments)();
    let from_name = find_environment(&envs, cur)
        .map(|e| e.name.clone())
        .unwrap_or_else(|| "No Environment".into());
    let to_name = find_environment(&envs, new_id)
        .map(|e| e.name.clone())
        .unwrap_or_else(|| "No Environment".into());
    let skip_overlay = new_id.is_none() || cur == new_id;
    let settings = (st.app_settings)();

    let finish = move |mut st: WorkbenchState, id: Option<Uuid>| {
        st.active_env_id.set(id);
        st.editing_env_id.set(id);
        let mut v = (st.vars)();
        sync_env_vars(&(st.environments)(), id, &mut v);
        st.vars.set(v);
        st.persist();
        st.env_switch_overlay.set(None);
    };

    if !settings.cloud_sync.enabled || from_name == to_name {
        finish(st, new_id);
        return;
    }

    let Ok(store) = JsonStore::open_default() else {
        finish(st, new_id);
        return;
    };

    if !skip_overlay {
        st.env_switch_overlay.set(Some(to_name.clone()));
    }
    st.cloud_busy.set(true);

    let root = store.root().to_path_buf();
    let mut settings_bg = settings;
    let cols = (st.collections)();
    let envs_bg = envs;
    let vars_bg = (st.vars)();

    spawn(async move {
        if !skip_overlay {
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        let res = tokio::task::spawn_blocking(move || {
            let r = switch_env_branch(
                &root,
                &mut settings_bg,
                &cols,
                &envs_bg,
                &from_name,
                &to_name,
                &vars_bg,
            );
            (r, settings_bg, to_name)
        })
        .await;

        st.cloud_busy.set(false);
        let picked_id = match res {
            Ok((Ok((cols, loaded_envs)), updated_settings, to_name)) => {
                st.app_settings.set(updated_settings);
                if !cols.is_empty() {
                    st.collections.set(cols);
                    st.fold_epoch.set((st.fold_epoch)() + 1);
                }
                let pick = if loaded_envs.is_empty() {
                    new_id
                } else {
                    let pick = loaded_envs
                        .iter()
                        .find(|e| e.name == to_name)
                        .map(|e| e.id)
                        .or(new_id);
                    st.environments.set(loaded_envs);
                    pick
                };
                st.status_msg
                    .set(format!("cloud branch {}", branch_name_for_env(&to_name)));
                pick
            }
            Ok((Err(e), updated_settings, _)) => {
                st.app_settings.set(updated_settings);
                st.status_msg.set(format!("cloud env switch: {e}"));
                new_id
            }
            Err(e) => {
                st.status_msg.set(format!("cloud env switch join: {e}"));
                new_id
            }
        };
        finish(st, picked_id);
    });
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
            "Local environment is not synced — pick Development/Production (or another team env)"
                .into(),
        );
        return;
    }
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
    let mut settings = (st.app_settings)();
    if !settings.cloud_sync.enabled {
        settings.cloud_sync.enabled = true;
        if settings.cloud_sync.remote_url.trim().is_empty() {
            settings.cloud_sync.remote_url = DEFAULT_CLOUD_REMOTE.to_string();
        }
        st.status_msg
            .set(format!("cloud sync enabled → syncing {env_name}…"));
    } else {
        st.status_msg.set(format!("syncing {env_name}…"));
    }
    let mut envs_mut = envs;
    let _ = ensure_default_environments(&mut envs_mut);
    st.environments.set(envs_mut.clone());

    let root = store.root().to_path_buf();
    let cols = (st.collections)();
    st.cloud_busy.set(true);

    spawn(async move {
        let env_name_bg = env_name.clone();
        let res = tokio::task::spawn_blocking(move || {
            let r = sync_environment(&root, &mut settings, &cols, &envs_mut, &env_name_bg, &v);
            (r, settings)
        })
        .await;

        st.cloud_busy.set(false);
        match res {
            Ok((Ok((cols, loaded_envs, result)), updated_settings)) => {
                st.app_settings.set(updated_settings.clone());
                if !cols.is_empty() {
                    st.collections.set(cols);
                    st.fold_epoch.set((st.fold_epoch)() + 1);
                }
                if !loaded_envs.is_empty() {
                    let pick = loaded_envs
                        .iter()
                        .find(|e| e.name == env_name)
                        .map(|e| e.id)
                        .unwrap_or(id);
                    st.active_env_id.set(Some(pick));
                    st.editing_env_id.set(Some(pick));
                    let mut v = (st.vars)();
                    sync_env_vars(&loaded_envs, Some(pick), &mut v);
                    st.vars.set(v);
                    st.environments.set(loaded_envs);
                }
                st.status_msg.set(result.message);
                st.persist_local();
            }
            Ok((Err(e), updated_settings)) => {
                st.app_settings.set(updated_settings);
                st.status_msg.set(format!("sync: {e}"));
            }
            Err(e) => {
                st.status_msg.set(format!("sync join: {e}"));
            }
        }
    });
}

pub fn cloud_clone_or_link(mut st: WorkbenchState) {
    if (st.cloud_busy)() {
        return;
    }
    let Ok(store) = JsonStore::open_default() else {
        st.status_msg.set("cloud: cannot open store".into());
        return;
    };
    let mut settings = (st.app_settings)();
    if settings.cloud_sync.remote_url.trim().is_empty() {
        settings.cloud_sync.remote_url = DEFAULT_CLOUD_REMOTE.to_string();
    }
    let root = store.root().to_path_buf();
    st.cloud_busy.set(true);
    st.status_msg.set("cloning / linking remote…".into());
    spawn(async move {
        let res = tokio::task::spawn_blocking(move || {
            let r = clone_or_ensure(&root, &mut settings);
            (r, settings)
        })
        .await;
        st.cloud_busy.set(false);
        match res {
            Ok((Ok(r), updated_settings)) => {
                st.app_settings.set(updated_settings);
                st.status_msg.set(r.message);
                st.persist_local();
            }
            Ok((Err(e), _)) => st.status_msg.set(format!("clone: {e}")),
            Err(e) => st.status_msg.set(format!("clone join: {e}")),
        }
    });
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
        st.cloud_busy.set(true);
        let res = tokio::task::spawn_blocking(move || {
            let r = link_folder(&root, &mut settings, &folder);
            (r, settings)
        })
        .await;
        st.cloud_busy.set(false);
        st.is_loading.set(false);
        match res {
            Ok((Ok((cols, envs, r)), updated_settings)) => {
                st.app_settings.set(updated_settings);
                if !cols.is_empty() {
                    st.collections.set(cols);
                    st.fold_epoch.set((st.fold_epoch)() + 1);
                }
                if !envs.is_empty() {
                    st.environments.set(envs);
                }
                st.status_msg.set(r.message);
                st.persist_local();
            }
            Ok((Err(e), _)) => st.status_msg.set(format!("link: {e}")),
            Err(e) => st.status_msg.set(format!("link join: {e}")),
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
    settings.cloud_sync.enabled = true;
    st.cloud_busy.set(true);
    spawn(async move {
        let res = tokio::task::spawn_blocking(move || {
            let r = refresh_status(&root, &mut settings);
            (r, settings)
        })
        .await;
        st.cloud_busy.set(false);
        if let Ok((r, updated_settings)) = res {
            st.app_settings.set(updated_settings);
            st.status_msg.set(r.message);
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
    let root = store.root().to_path_buf();
    let mut settings = (st.app_settings)();
    settings.cloud_sync.enabled = true;
    let envs = (st.environments)();
    st.is_loading.set(true);
    st.cloud_busy.set(true);
    spawn(async move {
        let root_for_conflicts = root.clone();
        let res = tokio::task::spawn_blocking(move || {
            let r = pull_workspace(&root, &mut settings, &envs);
            let conflicts = if r.is_err() {
                detect_git_conflicts(&root).unwrap_or_default()
            } else {
                vec![]
            };
            (r, settings, conflicts)
        })
        .await;
        st.cloud_busy.set(false);
        st.is_loading.set(false);
        match res {
            Ok((Ok((cols, envs, r)), updated_settings, _)) => {
                st.app_settings.set(updated_settings);
                if !cols.is_empty() {
                    st.collections.set(cols);
                    st.fold_epoch.set((st.fold_epoch)() + 1);
                }
                if !envs.is_empty() {
                    let mut v = (st.vars)();
                    sync_env_vars(&envs, (st.active_env_id)(), &mut v);
                    st.vars.set(v);
                    st.environments.set(envs);
                }
                st.status_msg.set(r.message);
                st.persist_local();
            }
            Ok((Err(e), _, conflicts)) => {
                let _ = root_for_conflicts;
                if !conflicts.is_empty() {
                    st.conflict_state.set(Some(ConflictState {
                        files: conflicts,
                        selected_idx: 0,
                    }));
                    st.active_request_id.set(None);
                    st.editing_collection_id.set(None);
                    st.status_msg
                        .set("⚔️ Merge conflicts detected — opened in main section".into());
                } else {
                    st.status_msg.set(format!("pull: {e}"));
                }
            }
            Err(e) => st.status_msg.set(format!("pull join: {e}")),
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
    let root = store.root().to_path_buf();
    let mut settings = (st.app_settings)();
    settings.cloud_sync.enabled = true;
    if settings.cloud_sync.remote_url.trim().is_empty() {
        settings.cloud_sync.remote_url = DEFAULT_CLOUD_REMOTE.to_string();
    }
    let envs = (st.environments)();
    let cols = (st.collections)();
    let vars = (st.vars)();
    let active_environment = find_environment(&envs, (st.active_env_id)())
        .map(|e| e.name.clone())
        .unwrap_or_else(|| "No Environment".into());
    let commit = settings.cloud_sync.commit_message.clone();
    st.is_loading.set(true);
    st.cloud_busy.set(true);
    spawn(async move {
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
            (r, settings)
        })
        .await;
        st.cloud_busy.set(false);
        st.is_loading.set(false);
        match res {
            Ok((r, updated_settings)) => {
                st.app_settings.set(updated_settings);
                st.status_msg.set(r.message);
                st.persist_local();
            }
            Err(e) => st.status_msg.set(format!("push join: {e}")),
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
    let Ok(store) = JsonStore::open_default() else {
        st.status_msg.set("team: cannot open store".into());
        return;
    };
    let root = store.root().to_path_buf();
    let mut settings = (st.app_settings)();
    if settings.cloud_sync.remote_url.trim().is_empty() {
        settings.cloud_sync.remote_url = DEFAULT_CLOUD_REMOTE.to_string();
    }
    st.show_team.set(true);
    st.team_syncing.set(true);
    st.cloud_busy.set(true);
    spawn(async move {
        let res = tokio::task::spawn_blocking(move || {
            let _ = clone_or_ensure(&root, &mut settings);
            let clone_path = settings.cloud_sync.resolved_clone_path(&root);
            get_team_activity(&clone_path, 30)
        })
        .await;
        st.team_syncing.set(false);
        st.cloud_busy.set(false);
        if let Ok(Ok(act)) = res {
            st.team_activity.set(act);
        }
    });
}
