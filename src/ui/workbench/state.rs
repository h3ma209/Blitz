use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

use dioxus::prelude::*;

use crate::core::{
    collect_all_subtree_ids, compact_response_for_disk, ensure_default_environments,
    find_environment, find_request_across, remove_across, resolve_ancestor_auth, sync_on_save,
    trash_collection, upsert_collection, upsert_environments, AncestorAuthOrigin, AppSettings,
    AuthConfig, AuthType, CaptureProxyHandle, Collection, CollectionItem, ConsoleLine, CookieJar,
    Environment, HistoryEntry, ImportBundle, MockHandle, PerfReport, RequestDraft, ResponseView,
    TeamActivity, UpdateInfo, UpdateProgress, UpdateStage, VariableContext, VaultStore, WsSession,
};
use crate::core::collection::RequestEnvOverride;
use crate::runtime::RunnerReport;
use crate::storage::{AppSnapshot, JsonStore};
use crate::ui::{push_toast, ConflictState, Toast, ToastLevel};

struct SavePending {
    gen: u64,
    cloud: bool,
    collections_dirty: bool,
}

static SAVE_PENDING: Mutex<SavePending> = Mutex::new(SavePending {
    gen: 0,
    cloud: false,
    collections_dirty: false,
});

pub static MOCK_HANDLE: Mutex<Option<MockHandle>> = Mutex::new(None);
pub static WS_SESSION: Mutex<Option<WsSession>> = Mutex::new(None);
pub(crate) static WS_POLL_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static PROXY_HANDLE: Mutex<Option<CaptureProxyHandle>> = Mutex::new(None);
static GUI_COLLECTIONS_MTIME: Mutex<Option<(std::path::PathBuf, u128)>> = Mutex::new(None);

pub(crate) fn record_gui_collections_mtime(root: &Path) {
    let col_path = root.join("collections.json");
    if let Some(mtime) = JsonStore::file_mtime_nanos(&col_path) {
        if let Ok(mut guard) = GUI_COLLECTIONS_MTIME.lock() {
            *guard = Some((col_path, mtime));
        }
    }
}

pub(crate) fn external_collections_mtime_changed(root: &Path) -> bool {
    let col_path = root.join("collections.json");
    let Some(disk_mtime) = JsonStore::file_mtime_nanos(&col_path) else {
        return false;
    };
    let Ok(guard) = GUI_COLLECTIONS_MTIME.lock() else {
        return false;
    };
    match &*guard {
        Some((p, seen)) if p == &col_path => *seen != disk_mtime,
        Some(_) | None => true,
    }
}

/// Vertical pane drag state — handled on `.workbench`, never as bare `.app-shell` grid children.
#[derive(Clone, Copy, PartialEq)]
pub enum PaneDrag {
    Request { start_y: f64, start_h: u32 },
    Console { start_y: f64, start_h: u32 },
}

/// Delete confirmation dialog payload for sidebar tree items.
#[derive(Clone, PartialEq)]
pub struct PendingDelete {
    pub id: Uuid,
    pub ids: Vec<Uuid>,
    pub label: String,
}

impl PendingDelete {
    pub fn single(id: Uuid, label: String) -> Self {
        Self {
            id,
            ids: vec![id],
            label,
        }
    }

    pub fn batch(ids: Vec<Uuid>, label: String) -> Self {
        Self {
            id: ids.first().copied().unwrap_or_default(),
            ids,
            label,
        }
    }
}

/// Pending close confirmation for tabs with unsaved changes.
#[derive(Clone, PartialEq)]
pub struct PendingCloseTab {
    pub tab_id: Uuid,
    pub draft: RequestDraft,
}

/// In-app clipboard for collection tree cut/copy/paste (folder/request items).
#[derive(Clone)]
pub struct TreeClipboard {
    pub item: CollectionItem,
    pub cut_from: Option<Uuid>,
}

#[derive(Clone, PartialEq)]
pub enum ImpexModal {
    Import { text: String },
    Export { name: String, text: String },
}

#[derive(Clone, Copy, PartialEq)]
pub struct WorkbenchState {
    pub draft: Signal<RequestDraft>,
    pub response: Signal<Option<Arc<ResponseView>>>,
    pub responses: Signal<HashMap<Uuid, Arc<ResponseView>>>,
    pub prev_responses: Signal<HashMap<Uuid, Arc<ResponseView>>>,
    pub sending: Signal<bool>,
    pub sending_requests: Signal<HashMap<Uuid, Arc<AtomicBool>>>,
    pub send_gen: Signal<u64>,
    pub send_cancel: Signal<Option<Arc<AtomicBool>>>,
    pub collections: Signal<Vec<Collection>>,
    pub environments: Signal<Vec<Environment>>,
    pub active_env_id: Signal<Option<Uuid>>,
    pub active_collection_id: Signal<Option<Uuid>>,
    pub active_request_id: Signal<Option<Uuid>>,
    pub vars: Signal<VariableContext>,
    pub history: Signal<Vec<HistoryEntry>>,
    pub cookies: Signal<CookieJar>,
    pub console_lines: Signal<Vec<ConsoleLine>>,
    pub console_open: Signal<bool>,
    pub show_env_manager: Signal<bool>,
    pub editing_env_id: Signal<Option<Uuid>>,
    pub status_msg: Signal<String>,
    pub impex_modal: Signal<Option<ImpexModal>>,
    pub loaded: Signal<bool>,
    pub splash_visible: Signal<bool>,
    pub splash_leaving: Signal<bool>,
    pub env_switch_overlay: Signal<Option<String>>,
    pub fold_epoch: Signal<u64>,
    pub app_settings: Signal<AppSettings>,
    pub data_root: Signal<String>,
    pub show_settings: Signal<bool>,
    pub settings_tab_open: Signal<bool>,
    pub settings_tab: Signal<crate::ui::settings_panel::SettingsTab>,
    pub show_runner: Signal<bool>,
    pub runner_running: Signal<bool>,
    pub runner_report: Signal<Option<RunnerReport>>,
    pub trash: Signal<Vec<Collection>>,
    pub open_tabs: Signal<Vec<RequestDraft>>,
    pub show_vars: Signal<bool>,
    pub vars_active_tab: Signal<usize>,
    pub show_palette: Signal<bool>,
    pub show_trash: Signal<bool>,
    pub show_snippets: Signal<bool>,
    pub snippet_lang: Signal<String>,
    pub snippet_resolve_vars: Signal<bool>,
    pub mock_port: Signal<Option<u16>>,
    pub ws_connected: Signal<bool>,
    pub ws_timeline: Signal<Vec<crate::core::WsFrameItem>>,
    pub ws_auto_ping: Signal<bool>,
    pub protocol_log: Signal<Vec<String>>,
    pub protocol_logs: Signal<HashMap<Uuid, Vec<String>>>,
    pub proxy_port: Signal<Option<u16>>,
    pub show_lint: Signal<bool>,
    pub show_perf: Signal<bool>,
    pub perf_running: Signal<bool>,
    pub perf_report: Signal<Option<PerfReport>>,
    pub perf_vus: Signal<u32>,
    pub perf_secs: Signal<u64>,
    pub file_menu_open: Signal<bool>,
    pub tools_menu_open: Signal<bool>,
    pub view_menu_open: Signal<bool>,
    pub cloud_menu_open: Signal<bool>,
    pub show_cookies: Signal<bool>,
    pub show_team: Signal<bool>,
    pub team_activity: Signal<TeamActivity>,
    pub show_col_vars: Signal<bool>,
    pub col_vars_all: Signal<bool>,
    pub col_vars_open_id: Signal<Option<Uuid>>,
    pub editing_collection_id: Signal<Option<Uuid>>,
    pub conflict_state: Signal<Option<ConflictState>>,
    pub sidebar_drag: Signal<Option<(f64, f64)>>,
    pub sidebar_w: Signal<f64>,
    pub pane_drag: Signal<Option<PaneDrag>>,
    pub pane_h: Signal<f64>,
    pub console_h: Signal<f64>,
    pub is_loading: Signal<bool>,
    pub cloud_busy: Signal<bool>,
    pub team_syncing: Signal<bool>,
    pub pending_delete: Signal<Option<PendingDelete>>,
    pub pending_batch_move: Signal<Option<Vec<Uuid>>>,
    pub pending_close_tab: Signal<Option<PendingCloseTab>>,
    pub tree_clipboard: Signal<Option<TreeClipboard>>,
    pub available_update: Signal<Option<UpdateInfo>>,
    pub show_update_modal: Signal<bool>,
    pub update_progress: Signal<Option<UpdateProgress>>,
    pub update_error: Signal<Option<String>>,
    pub checking_update: Signal<bool>,
    pub installing_update: Signal<bool>,
    pub toasts: Signal<Vec<Toast>>,
    pub drag_tab_idx: Signal<Option<usize>>,
    pub show_tab_menu: Signal<bool>,
    pub tab_groups_enabled: Signal<bool>,
    pub collapsed_tab_groups: Signal<std::collections::HashSet<String>>,
    pub custom_tab_groups: Signal<HashMap<Uuid, String>>,
}

impl WorkbenchState {
    pub fn use_new() -> Self {
        let app_settings = use_signal(AppSettings::default);
        let sidebar_w = use_signal(|| app_settings().ui.sidebar_width_px as f64);
        let pane_h = use_signal(|| app_settings().ui.request_pane_height_px as f64);
        let console_h = use_signal(|| app_settings().ui.console_height_px as f64);
        Self {
            draft: use_signal(RequestDraft::default),
            response: use_signal(|| None::<Arc<ResponseView>>),
            responses: use_signal(HashMap::<Uuid, Arc<ResponseView>>::new),
            prev_responses: use_signal(HashMap::<Uuid, Arc<ResponseView>>::new),
            sending: use_signal(|| false),
            sending_requests: use_signal(HashMap::<Uuid, Arc<AtomicBool>>::new),
            send_gen: use_signal(|| 0u64),
            send_cancel: use_signal(|| None::<Arc<AtomicBool>>),
            collections: use_signal(Vec::<Collection>::new),
            environments: use_signal(Vec::<Environment>::new),
            active_env_id: use_signal(|| None::<Uuid>),
            active_collection_id: use_signal(|| None::<Uuid>),
            active_request_id: use_signal(|| None::<Uuid>),
            vars: use_signal(VariableContext::demo),
            history: use_signal(Vec::<HistoryEntry>::new),
            cookies: use_signal(CookieJar::default),
            console_lines: use_signal(Vec::<ConsoleLine>::new),
            console_open: use_signal(|| false),
            show_env_manager: use_signal(|| false),
            editing_env_id: use_signal(|| None::<Uuid>),
            status_msg: use_signal(String::new),
            impex_modal: use_signal(|| None::<ImpexModal>),
            loaded: use_signal(|| false),
            splash_visible: use_signal(|| true),
            splash_leaving: use_signal(|| false),
            env_switch_overlay: use_signal(|| None::<String>),
            fold_epoch: use_signal(|| 0u64),
            app_settings,
            data_root: use_signal(String::new),
            show_settings: use_signal(|| false),
            settings_tab_open: use_signal(|| false),
            settings_tab: use_signal(crate::ui::settings_panel::SettingsTab::default),
            show_runner: use_signal(|| false),
            runner_running: use_signal(|| false),
            runner_report: use_signal(|| None::<RunnerReport>),
            trash: use_signal(Vec::<Collection>::new),
            open_tabs: use_signal(Vec::<RequestDraft>::new),
            show_vars: use_signal(|| false),
            vars_active_tab: use_signal(|| 0usize),
            show_palette: use_signal(|| false),
            show_trash: use_signal(|| false),
            show_snippets: use_signal(|| false),
            snippet_lang: use_signal(|| "cURL".to_string()),
            snippet_resolve_vars: use_signal(|| true),
            mock_port: use_signal(|| None::<u16>),
            ws_connected: use_signal(|| false),
            ws_timeline: use_signal(Vec::<crate::core::WsFrameItem>::new),
            ws_auto_ping: use_signal(|| false),
            protocol_log: use_signal(Vec::<String>::new),
            protocol_logs: use_signal(HashMap::<Uuid, Vec<String>>::new),
            proxy_port: use_signal(|| None::<u16>),
            show_lint: use_signal(|| false),
            show_perf: use_signal(|| false),
            perf_running: use_signal(|| false),
            perf_report: use_signal(|| None::<PerfReport>),
            perf_vus: use_signal(|| 5u32),
            perf_secs: use_signal(|| 5u64),
            file_menu_open: use_signal(|| false),
            tools_menu_open: use_signal(|| false),
            view_menu_open: use_signal(|| false),
            cloud_menu_open: use_signal(|| false),
            show_cookies: use_signal(|| false),
            show_team: use_signal(|| false),
            team_activity: use_signal(TeamActivity::default),
            show_col_vars: use_signal(|| false),
            col_vars_all: use_signal(|| false),
            col_vars_open_id: use_signal(|| None::<Uuid>),
            editing_collection_id: use_signal(|| None::<Uuid>),
            conflict_state: use_signal(|| None::<ConflictState>),
            sidebar_drag: use_signal(|| None::<(f64, f64)>),
            sidebar_w,
            pane_drag: use_signal(|| None::<PaneDrag>),
            pane_h,
            console_h,
            is_loading: use_signal(|| false),
            cloud_busy: use_signal(|| false),
            team_syncing: use_signal(|| false),
            pending_delete: use_signal(|| None::<PendingDelete>),
            pending_batch_move: use_signal(|| None::<Vec<Uuid>>),
            pending_close_tab: use_signal(|| None::<PendingCloseTab>),
            tree_clipboard: use_signal(|| None::<TreeClipboard>),
            available_update: use_signal(|| None::<UpdateInfo>),
            show_update_modal: use_signal(|| false),
            update_progress: use_signal(|| None::<UpdateProgress>),
            update_error: use_signal(|| None::<String>),
            checking_update: use_signal(|| false),
            installing_update: use_signal(|| false),
            toasts: use_signal(Vec::<Toast>::new),
            drag_tab_idx: use_signal(|| None::<usize>),
            show_tab_menu: use_signal(|| false),
            tab_groups_enabled: use_signal(|| false),
            collapsed_tab_groups: use_signal(std::collections::HashSet::<String>::new),
            custom_tab_groups: use_signal(HashMap::<Uuid, String>::new),
        }
    }

    pub fn notify(&self, msg: impl Into<String>, level: ToastLevel) {
        push_toast(self.toasts, Toast::new(msg, level));
    }

    pub fn start_update_install(&self, info: UpdateInfo) {
        let mut st = *self;
        if (st.installing_update)() {
            return;
        }
        st.show_update_modal.set(true);
        st.installing_update.set(true);
        st.update_error.set(None);
        st.update_progress.set(Some(UpdateProgress {
            stage: UpdateStage::Downloading,
            downloaded_bytes: 0,
            total_bytes: None,
            message: format!("Starting download for Blitz v{}…", info.latest_version),
        }));
        st.status_msg
            .set(format!("downloading Blitz v{}…", info.latest_version));

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<UpdateProgress>();
        let mut progress_sig = st.update_progress;
        spawn(async move {
            while let Some(p) = rx.recv().await {
                progress_sig.set(Some(p));
            }
        });

        let info_bg = info.clone();
        spawn(async move {
            let res = tokio::task::spawn_blocking(move || {
                crate::core::download_and_verify_update_with_progress(&info_bg, move |p| {
                    let _ = tx.send(p);
                })
            })
            .await;
            match res {
                Ok(Ok(path)) => {
                    st.persist_now();
                    st.update_progress.set(Some(UpdateProgress {
                        stage: UpdateStage::Installing,
                        downloaded_bytes: 1,
                        total_bytes: Some(1),
                        message: "Installing update and restarting Blitz…".into(),
                    }));
                    st.status_msg
                        .set("installing update and restarting Blitz…".to_string());
                    let p_install = path.clone();
                    let install_res = tokio::task::spawn_blocking(move || {
                        crate::core::install_update_and_restart(&p_install)
                    })
                    .await;
                    st.installing_update.set(false);
                    match install_res {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => {
                            let msg = format!(
                                "Auto-update failed ({e}) — opened downloaded installer instead."
                            );
                            st.update_error.set(Some(msg.clone()));
                            st.status_msg.set(msg);
                            crate::core::reveal_or_open_installer(&path);
                        }
                        Err(e) => {
                            let msg = format!("Install task error: {e}");
                            st.update_error.set(Some(msg.clone()));
                            st.status_msg.set(msg);
                            crate::core::reveal_or_open_installer(&path);
                        }
                    }
                }
                Ok(Err(e)) => {
                    st.installing_update.set(false);
                    st.update_error.set(Some(format!("Download failed: {e}")));
                    st.status_msg
                        .set(format!("update download failed ({e})"));
                }
                Err(e) => {
                    st.installing_update.set(false);
                    st.update_error.set(Some(format!("Update task error: {e}")));
                    st.status_msg.set(format!("update join error: {e}"));
                }
            }
        });
    }

    pub fn log(&self, line: ConsoleLine) {
        let mut lines = (self.console_lines)();
        crate::core::push_console(&mut lines, line);
        let mut sig = self.console_lines;
        sig.set(lines);
    }

    #[allow(dead_code)]
    pub fn log_info(&self, msg: impl Into<String>) {
        self.log(ConsoleLine::info(msg));
    }

    pub fn log_warn(&self, msg: impl Into<String>) {
        self.log(ConsoleLine::warn(msg));
    }

    pub fn log_error(&self, msg: impl Into<String>) {
        self.log(ConsoleLine::error(msg));
    }

    pub fn log_network(&self, msg: impl Into<String>) {
        self.log(ConsoleLine::network(msg));
    }

    pub fn log_cloud(&self, msg: impl Into<String>) {
        self.log(ConsoleLine::cloud(msg));
    }

    pub fn build_snapshot(
        &self,
        settings: AppSettings,
        active_environment: String,
        include_collections: bool,
    ) -> AppSnapshot {
        let mut tabs = (self.open_tabs)();
        let cur = (self.draft)();
        if let Some(slot) = tabs.iter_mut().find(|t| t.id == cur.id) {
            *slot = cur;
        }
        AppSnapshot {
            collections: if include_collections {
                (self.collections)()
            } else {
                Vec::new()
            },
            variables: (self.vars)(),
            active_environment,
            environments: (self.environments)(),
            active_environment_id: (self.active_env_id)(),
            history: (self.history)(),
            cookies: (self.cookies)(),
            active_collection_id: (self.active_collection_id)(),
            active_request_id: (self.active_request_id)(),
            settings,
            trash: (self.trash)(),
            open_tabs: tabs,
            responses: (self.responses)()
                .iter()
                .map(|(id, view)| (*id, compact_response_for_disk(view)))
                .collect(),
            show_vars: false,
            show_settings: (self.show_settings)(),
            show_runner: false,
            show_perf: false,
            show_env_manager: false,
            show_col_vars: false,
        }
    }

    /// Coalesce bursts of saves. Latest state wins. Quit inside the window can drop the last edit.
    pub fn persist_local(&self) {
        self.schedule_save(false, false);
    }

    pub fn persist(&mut self) {
        self.schedule_save(true, true);
    }

    /// Apply a categorized [`ImportBundle`] (collections, environments, globals, vault secrets)
    /// into workbench state, persist, and return a human-readable summary.
    pub fn apply_import_bundle(&mut self, bundle: ImportBundle) -> String {
        let summary = bundle.summary();
        let mut v = (self.vars)();

        if !bundle.collections.is_empty() {
            let mut cols = (self.collections)();
            let mut last_id = None;
            for col in bundle.collections {
                let stats = upsert_collection(&mut cols, col);
                last_id = Some(stats.collection_id);
            }
            self.collections.set(cols.clone());
            self.open_tabs.with_mut(|tabs| {
                for tab in tabs.iter_mut() {
                    if let Some(updated) = cols.iter().find_map(|c| c.find_request(tab.id)) {
                        *tab = updated.clone();
                    }
                }
            });
            if let Some(active_id) = (self.active_request_id)() {
                if let Some(updated) = cols.iter().find_map(|c| c.find_request(active_id)) {
                    self.draft.set(updated.clone());
                }
            }
            if let Some(id) = last_id {
                self.active_collection_id.set(Some(id));
                if let Some(col) = cols.iter().find(|c| c.id == id) {
                    v.collection = col.variables_map();
                }
            }
            let ep = (self.fold_epoch)();
            self.fold_epoch.set(ep + 1);
        }

        if !bundle.environments.is_empty() {
            let mut envs = (self.environments)();
            let (ids, _, _) = upsert_environments(&mut envs, bundle.environments);
            if let Some(&last_env_id) = ids.last() {
                self.active_env_id.set(Some(last_env_id));
                sync_env_vars(&envs, Some(last_env_id), &mut v);
            }
            self.environments.set(envs);
        }

        for (k, val) in bundle.globals {
            if !k.is_empty() && (!v.global.contains_key(&k) || !val.is_empty()) {
                v.global.insert(k, val);
            }
        }
        for (k, val) in bundle.vault_secrets {
            if !k.is_empty() && (v.vault.get(&k).is_none() || !val.is_empty()) {
                v.vault.set(k, val);
            }
        }

        self.vars.set(v);
        self.persist();
        summary
    }

    /// Immediately flush collections + view state to disk, cancelling any pending debounce timer.
    /// Used for explicit Save (Cmd+S / Save button), Save & Close, and tab close so rapid quit/close never drops edits.
    pub fn persist_now(&mut self) {
        {
            let mut pending = SAVE_PENDING.lock().unwrap_or_else(|e| e.into_inner());
            pending.gen = pending.gen.wrapping_add(1);
            pending.cloud = false;
            pending.collections_dirty = false;
        }
        self.write_snapshot(true, true);
    }

    fn schedule_save(&self, with_cloud: bool, collections_dirty: bool) {
        let gen = {
            let mut pending = SAVE_PENDING.lock().unwrap_or_else(|e| e.into_inner());
            pending.gen = pending.gen.wrapping_add(1);
            pending.cloud |= with_cloud;
            pending.collections_dirty |= collections_dirty;
            pending.gen
        };
        let mut st = *self;
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(400)).await;
            let (do_cloud, include_collections) = {
                let mut pending = SAVE_PENDING.lock().unwrap_or_else(|e| e.into_inner());
                if pending.gen != gen {
                    return;
                }
                let cloud = pending.cloud;
                let dirty_cols = pending.collections_dirty;
                pending.cloud = false;
                pending.collections_dirty = false;
                (cloud, dirty_cols)
            };
            st.write_snapshot(do_cloud, include_collections);
        });
    }

    fn write_snapshot(&mut self, with_cloud: bool, include_collections: bool) {
        let root = (self.data_root)();
        let store = if !root.trim().is_empty() {
            JsonStore::open_path(&root)
        } else {
            JsonStore::open_for_settings(&(self.app_settings)())
        };
        if let Ok(store) = store {
            let active_environment = find_environment(&(self.environments)(), (self.active_env_id)())
                .map(|e| e.name.clone())
                .unwrap_or_else(|| "No Environment".into());
            let v = (self.vars)();
            let _ = v.vault.save(store.root());
            let mut settings = (self.app_settings)();
            if settings.storage_root.trim().is_empty() {
                settings.storage_root = store.root().display().to_string();
            }
            let write_collections = include_collections || !store.collections_path().exists();
            let mut snap = self.build_snapshot(
                settings.clone(),
                active_environment.clone(),
                write_collections,
            );
            let save_res = if write_collections {
                store.save(&snap)
            } else {
                store.save_view_state(&snap)
            };
            match save_res {
                Ok(()) => {
                    if write_collections {
                        record_gui_collections_mtime(store.root());
                    }
                    let base = format!("saved · {}", store.root().display());
                    self.status_msg.set(base.clone());
                    if with_cloud && settings.cloud_sync.enabled && !(self.cloud_busy)() {
                        let mut st = *self;
                        let root = store.root().to_path_buf();
                        let mut settings_bg = settings;
                        let cols = if write_collections {
                            std::mem::take(&mut snap.collections)
                        } else {
                            (self.collections)()
                        };
                        let envs = (self.environments)();
                        st.cloud_busy.set(true);
                        spawn(async move {
                            let res = tokio::task::spawn_blocking(move || {
                                let r = sync_on_save(
                                    &root,
                                    &mut settings_bg,
                                    &cols,
                                    &envs,
                                    &active_environment,
                                    &v,
                                );
                                (r, settings_bg)
                            })
                            .await;
                            st.cloud_busy.set(false);
                            if let Ok((result, updated_settings)) = res {
                                st.app_settings.set(updated_settings);
                                st.status_msg.set(format!("{base} · {}", result.message));
                                st.write_snapshot(false, false);
                            }
                        });
                    }
                }
                Err(e) => self.status_msg.set(format!("save error: {e}")),
            }
        }
    }

    pub fn is_request_sending(&self, req_id: Uuid) -> bool {
        self.sending_requests.read().contains_key(&req_id)
    }

    pub fn start_request_send(&mut self, req_id: Uuid) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        let mut map = self.sending_requests.peek().clone();
        if let Some(prev) = map.insert(req_id, flag.clone()) {
            prev.store(true, Ordering::SeqCst);
        }
        self.sending_requests.set(map);
        self.sending.set(true);
        flag
    }

    pub fn finish_request_send(&mut self, req_id: Uuid, flag: &Arc<AtomicBool>) {
        let mut map = self.sending_requests.peek().clone();
        if map.get(&req_id).is_some_and(|cur| Arc::ptr_eq(cur, flag)) {
            map.remove(&req_id);
            let any = !map.is_empty();
            self.sending_requests.set(map);
            self.sending.set(any);
        }
    }

    pub fn cancel_request_send(&mut self, req_id: Uuid) -> bool {
        let mut map = self.sending_requests.peek().clone();
        if let Some(flag) = map.remove(&req_id) {
            flag.store(true, Ordering::SeqCst);
            let any = !map.is_empty();
            self.sending_requests.set(map);
            self.sending.set(any);
            true
        } else {
            false
        }
    }

    pub fn set_request_protocol_log(&mut self, req_id: Uuid, lines: Vec<String>) {
        let mut logs = self.protocol_logs.peek().clone();
        logs.insert(req_id, lines.clone());
        self.protocol_logs.set(logs);
        if self.draft.peek().id == req_id {
            self.protocol_log.set(lines);
        }
    }

    pub fn get_request_protocol_log(&self, req_id: Uuid) -> Vec<String> {
        self.protocol_logs
            .read()
            .get(&req_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn compute_request_env_cache(&self, req_id: Uuid) -> CachedRequestEnv {
        let mut ctx = (self.vars)();
        let envs = self.environments.read();
        let active_env = *self.active_env_id.read();
        sync_env_vars(&envs, active_env, &mut ctx);

        let cols = self.collections.read();
        let active_col_id = *self.active_collection_id.read();
        let owner_col = owning_collection(&cols, active_col_id, req_id);

        let mut col_vars = HashMap::new();
        let mut env_override = None;
        if let Some(col) = owner_col {
            col_vars = col.variables_map();
            if active_env.is_some() {
                for kv in &col.environment_overrides.variables {
                    if kv.enabled && !kv.key.is_empty() {
                        col_vars.insert(kv.key.clone(), kv.value.clone());
                    }
                }
                if let Some(req_ov) = col.environment_overrides.requests.get(&req_id) {
                    env_override = Some(req_ov.clone());
                    for kv in &req_ov.variables {
                        if kv.enabled && !kv.key.is_empty() {
                            col_vars.insert(kv.key.clone(), kv.value.clone());
                        }
                    }
                }
            }
        }
        ctx.collection = col_vars;

        let ancestor_resolved = resolve_ancestor_auth(&cols, req_id);
        let ancestor_origin = ancestor_resolved.as_ref().map(|(_, o)| o.clone());
        let mut inherited_auth = ancestor_resolved.and_then(|(a, _)| {
            if a.auth_type != AuthType::Inherit && a.auth_type != AuthType::NoAuth {
                Some(a)
            } else {
                None
            }
        });
        if inherited_auth.is_none() {
            if let Some(col) = owner_col {
                let col_auth = if active_env.is_some() {
                    col.environment_overrides
                        .auth
                        .as_ref()
                        .unwrap_or(&col.auth)
                } else {
                    &col.auth
                };
                if col_auth.auth_type != AuthType::Inherit {
                    inherited_auth = Some(col_auth.clone());
                }
            }
        }

        CachedRequestEnv {
            ancestor_origin,
            base_vars: ctx,
            env_override,
            inherited_auth,
        }
    }

    pub fn effective_vars_for_request(&self, req: &RequestDraft) -> VariableContext {
        self.compute_request_env_cache(req.id).apply(req).0
    }

    pub fn effective_draft_for_codegen(&self, req: &RequestDraft) -> RequestDraft {
        self.compute_request_env_cache(req.id).apply(req).1
    }

    pub fn cleanup_closed_tabs(&mut self, open_tabs: &[RequestDraft], active_id: Option<Uuid>) {
        let mut pinned: std::collections::HashSet<Uuid> =
            open_tabs.iter().map(|t| t.id).collect();
        if let Some(id) = active_id {
            pinned.insert(id);
        }

        let cancel_ids: Vec<Uuid> = self
            .sending_requests
            .peek()
            .keys()
            .copied()
            .filter(|id| !pinned.contains(id))
            .collect();
        for id in cancel_ids {
            self.cancel_request_send(id);
        }

        let mut logs = self.protocol_logs.peek().clone();
        let before_logs = logs.len();
        logs.retain(|id, _| pinned.contains(id));
        if logs.len() != before_logs {
            self.protocol_logs.set(logs);
        }

        let mut resp_map = self.responses.peek().clone();
        let mut prev_map = self.prev_responses.peek().clone();
        prune_response_cache(&mut resp_map, &mut prev_map, open_tabs, active_id);
        self.responses.set(resp_map);
        self.prev_responses.set(prev_map);

        let has_ws_tab = open_tabs
            .iter()
            .any(|t| t.protocol == crate::core::RequestProtocol::WebSocket);
        if !has_ws_tab {
            WS_POLL_EPOCH.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let old_ws = WS_SESSION
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            if let Some(old) = old_ws {
                old.close();
            }
            if *self.ws_connected.peek() {
                self.ws_connected.set(false);
            }
            if !self.ws_timeline.peek().is_empty() {
                self.ws_timeline.set(Vec::new());
            }
        }
    }

    pub fn apply_snapshot(&mut self, snap: AppSnapshot, root: &Path) {
        self.collections.set(snap.collections.clone());
        self.environments.set(snap.environments.clone());
        let mut active_col_id = snap
            .active_collection_id
            .or_else(|| snap.collections.first().map(|c| c.id));
        if let Some(req_id) = snap.active_request_id {
            if let Some((col_id, req)) = find_request_across(&snap.collections, req_id) {
                self.draft.set(req.clone());
                active_col_id = Some(col_id);
            }
        }
        let mut v = snap.variables;
        if let Ok(vault) = VaultStore::load(root) {
            v.vault = vault;
        }
        sync_env_vars(&snap.environments, snap.active_environment_id, &mut v);
        if let Some(cid) = active_col_id {
            if let Some(col) = snap.collections.iter().find(|c| c.id == cid) {
                v.collection = col.variables_map();
            }
        }
        self.vars.set(v);
        self.active_env_id.set(snap.active_environment_id);
        self.history.set(snap.history);
        self.cookies.set(snap.cookies);
        let mut settings = snap.settings;
        settings.ui = settings.ui.clamped();
        self.app_settings.set(settings);
        self.trash.set(snap.trash);
        self.open_tabs.set(snap.open_tabs);
        self.responses.set(
            snap.responses
                .into_iter()
                .map(|(id, view)| (id, Arc::new(compact_response_for_disk(&view))))
                .collect(),
        );
        self.show_vars.set(false);
        self.show_settings.set(snap.show_settings);
        self.settings_tab_open.set(snap.show_settings);
        self.show_runner.set(false);
        self.show_perf.set(false);
        self.show_env_manager.set(false);
        self.show_col_vars.set(false);
        self.active_collection_id.set(active_col_id);
        self.active_request_id.set(snap.active_request_id);

        self.data_root.set(root.display().to_string());
        record_gui_collections_mtime(root);
        self.status_msg.set(format!("storage: {}", root.display()));
    }

    pub fn reload_storage_root(&mut self, root: &str) {
        let Ok(store) = JsonStore::open_path(root) else {
            self.status_msg.set(format!("invalid storage path: {root}"));
            return;
        };
        let keep_settings = (self.show_settings)();
        let keep_settings_tab = (self.settings_tab_open)();
        let prev_bookmarks = (self.app_settings)().storage_roots;
        match store.load() {
            Ok(snap) => {
                self.apply_snapshot(snap, store.root());
                let mut merged = (self.app_settings)();
                merged.storage_root = store.root().display().to_string();
                for path in prev_bookmarks {
                    if !merged.storage_roots.iter().any(|p| p == &path) {
                        merged.storage_roots.push(path);
                    }
                }
                self.app_settings.set(merged);
                if keep_settings {
                    self.show_settings.set(true);
                }
                if keep_settings_tab {
                    self.settings_tab_open.set(true);
                }
                self.fold_epoch.set((self.fold_epoch)() + 1);
                self.status_msg.set(format!("switched workspace: {}", store.root().display()));
            }
            Err(e) => self.status_msg.set(format!("load error: {e}")),
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct CachedRequestEnv {
    pub ancestor_origin: Option<AncestorAuthOrigin>,
    pub base_vars: VariableContext,
    pub env_override: Option<RequestEnvOverride>,
    pub inherited_auth: Option<AuthConfig>,
}

impl CachedRequestEnv {
    pub fn apply(&self, draft: &RequestDraft) -> (VariableContext, RequestDraft) {
        let mut vars = self.base_vars.clone();
        for p in &draft.path_params {
            if p.enabled && !p.key.trim().is_empty() {
                vars.path.insert(p.key.trim().to_string(), p.value.clone());
            }
        }

        let mut codegen_draft = draft.clone();
        if let Some(ref ov) = self.env_override {
            codegen_draft = codegen_draft.apply_env_override(ov);
        }
        if codegen_draft.auth.auth_type == AuthType::Inherit {
            if let Some(ref auth) = self.inherited_auth {
                codegen_draft.auth = auth.clone();
            }
        }
        (vars, codegen_draft)
    }
}

fn owning_collection(
    cols: &[Collection],
    active_col_id: Option<Uuid>,
    req_id: Uuid,
) -> Option<&Collection> {
    if let Some(cid) = active_col_id {
        if let Some(col) = cols.iter().find(|c| c.id == cid) {
            if col.find_request(req_id).is_some() {
                return Some(col);
            }
        }
    }
    cols.iter()
        .find(|c| c.find_request(req_id).is_some())
        .or_else(|| active_col_id.and_then(|cid| cols.iter().find(|c| c.id == cid)))
}

pub fn apply_collection_env_pin(
    collections: &[Collection],
    col_id: Option<Uuid>,
    environments: &[Environment],
    active_env_id: &mut Signal<Option<Uuid>>,
    vars: &mut Signal<VariableContext>,
) {
    let Some(cid) = col_id else {
        return;
    };
    let Some(col) = collections.iter().find(|c| c.id == cid) else {
        return;
    };
    let Some(env_id) = col.pinned_environment_id else {
        return;
    };
    active_env_id.set(Some(env_id));
    let mut ctx = vars();
    sync_env_vars(environments, Some(env_id), &mut ctx);
    vars.set(ctx);
}

pub fn sync_env_vars(envs: &[Environment], id: Option<Uuid>, v: &mut VariableContext) {
    if let Some(env) = find_environment(envs, id) {
        v.environment = env.to_map();
    } else {
        v.environment.clear();
    }
}

/// Remove a collection (to trash) or folder/request from the tree, closing associated tabs and subtrees.
pub fn execute_tree_delete(
    id: Uuid,
    collections: &mut Vec<Collection>,
    trash: &mut Vec<Collection>,
    active_collection_id: &mut Option<Uuid>,
    active_request_id: &mut Option<Uuid>,
    draft: &mut RequestDraft,
    open_tabs: &mut Vec<RequestDraft>,
) -> Option<String> {
    let is_col = collections.iter().any(|c| c.id == id);
    let count = execute_batch_delete(
        &[id],
        collections,
        trash,
        active_collection_id,
        active_request_id,
        draft,
        open_tabs,
    );
    if count > 0 && is_col {
        Some("moved collection to Trash".into())
    } else {
        None
    }
}

/// Remove multiple items from the tree in batch, closing associated tabs and handling trash.
pub fn execute_batch_delete(
    ids: &[Uuid],
    collections: &mut Vec<Collection>,
    trash: &mut Vec<Collection>,
    active_collection_id: &mut Option<Uuid>,
    active_request_id: &mut Option<Uuid>,
    draft: &mut RequestDraft,
    open_tabs: &mut Vec<RequestDraft>,
) -> usize {
    let mut total_deleted = 0;
    let mut all_removed_ids = std::collections::HashSet::new();

    for &id in ids {
        let sub = collect_all_subtree_ids(collections, id);
        all_removed_ids.extend(sub);
        if trash_collection(collections, trash, id) || remove_across(collections, id) {
            total_deleted += 1;
        }
    }

    if let Some(col_id) = *active_collection_id {
        if all_removed_ids.contains(&col_id) {
            *active_collection_id = collections.first().map(|c| c.id);
        }
    }

    if let Some(req_id) = *active_request_id {
        if all_removed_ids.contains(&req_id) {
            *active_request_id = None;
            *draft = RequestDraft::default();
        }
    }

    open_tabs.retain(|t| !all_removed_ids.contains(&t.id));

    if active_request_id.is_none() && !open_tabs.is_empty() {
        if let Some(last) = open_tabs.last() {
            *active_request_id = Some(last.id);
            *draft = last.clone();
        }
    }

    total_deleted
}

pub fn ensure_cloud_defaults(mut st: WorkbenchState) {
    let mut envs = (st.environments)();
    if ensure_default_environments(&mut envs) {
        st.environments.set(envs);
    }
}

const MAX_CACHED_RESPONSES: usize = 32;
const MAX_CACHED_PREV_RESPONSES: usize = 12;
const IN_MEM_COMPACT_THRESHOLD: usize = 256 * 1024;

/// Bound in-memory response caches so closed tabs and large payloads do not pin RAM.
pub fn prune_response_cache(
    responses: &mut std::collections::HashMap<Uuid, Arc<ResponseView>>,
    prev_responses: &mut std::collections::HashMap<Uuid, Arc<ResponseView>>,
    open_tabs: &[RequestDraft],
    active_id: Option<Uuid>,
) {
    let mut pinned: std::collections::HashSet<Uuid> =
        open_tabs.iter().map(|t| t.id).collect();
    if let Some(id) = active_id {
        pinned.insert(id);
    }

    // Diff snapshots are only meaningful for open/active requests.
    prev_responses.retain(|id, _| pinned.contains(id));
    if prev_responses.len() > MAX_CACHED_PREV_RESPONSES {
        let drop_ids: Vec<Uuid> = prev_responses
            .keys()
            .copied()
            .filter(|id| active_id != Some(*id))
            .take(prev_responses.len() - MAX_CACHED_PREV_RESPONSES)
            .collect();
        for id in drop_ids {
            prev_responses.remove(&id);
        }
    }

    // Compact oversized payloads on any non-active entries so dozens of open tabs never pin multi-MB bodies in RAM.
    for (id, view) in responses.iter_mut() {
        if active_id != Some(*id)
            && (view.body.len() > IN_MEM_COMPACT_THRESHOLD
                || view.visualizer_html.len() > IN_MEM_COMPACT_THRESHOLD)
        {
            *view = Arc::new(compact_response_for_disk(view));
        }
    }
    for (id, view) in prev_responses.iter_mut() {
        if active_id != Some(*id)
            && (view.body.len() > IN_MEM_COMPACT_THRESHOLD
                || view.visualizer_html.len() > IN_MEM_COMPACT_THRESHOLD)
        {
            *view = Arc::new(compact_response_for_disk(view));
        }
    }

    if responses.len() > MAX_CACHED_RESPONSES {
        let mut drop_ids: Vec<Uuid> = responses
            .keys()
            .copied()
            .filter(|id| !pinned.contains(id))
            .take(responses.len() - MAX_CACHED_RESPONSES)
            .collect();
        for id in drop_ids.drain(..) {
            responses.remove(&id);
        }
        if responses.len() > MAX_CACHED_RESPONSES {
            let extra: Vec<Uuid> = responses
                .keys()
                .copied()
                .filter(|id| active_id != Some(*id))
                .take(responses.len() - MAX_CACHED_RESPONSES)
                .collect();
            for id in extra {
                responses.remove(&id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::prune_response_cache;
    use crate::core::{RequestDraft, ResponseView};
    use std::collections::HashMap;
    use std::sync::Arc;
    use uuid::Uuid;

    #[test]
    fn prune_response_cache_compacts_closed_large_bodies_and_drops_closed_diffs() {
        let open_id = Uuid::new_v4();
        let closed_id = Uuid::new_v4();
        let open_tab = RequestDraft {
            id: open_id,
            ..RequestDraft::default()
        };

        let mut large = ResponseView::error("ok", vec!["log".into()]);
        large.status = 200;
        large.body = "x".repeat(512 * 1024);

        let mut responses = HashMap::from([
            (open_id, Arc::new(large.clone())),
            (closed_id, Arc::new(large.clone())),
        ]);
        let mut prev_responses = HashMap::from([
            (open_id, Arc::new(large.clone())),
            (closed_id, Arc::new(large)),
        ]);

        prune_response_cache(&mut responses, &mut prev_responses, &[open_tab], Some(open_id));

        assert_eq!(responses[&open_id].body.len(), 512 * 1024);
        assert!(responses[&closed_id].body.len() < 512 * 1024);
        assert!(prev_responses.contains_key(&open_id));
        assert!(!prev_responses.contains_key(&closed_id));
    }

    #[test]
    fn prune_response_cache_caps_closed_entries() {
        let active_id = Uuid::new_v4();
        let mut responses = HashMap::new();
        let mut prev_responses = HashMap::new();
        for _ in 0..45 {
            responses.insert(Uuid::new_v4(), Arc::new(ResponseView::error("e", vec![])));
        }
        responses.insert(active_id, Arc::new(ResponseView::error("a", vec![])));

        prune_response_cache(&mut responses, &mut prev_responses, &[], Some(active_id));

        assert!(responses.len() <= 32);
        assert!(responses.contains_key(&active_id));
    }
}
