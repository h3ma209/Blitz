use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use dioxus::prelude::*;

use crate::core::{
    collect_all_subtree_ids, ensure_default_environments, find_environment, find_request_across,
    remove_across, sync_on_save, trash_collection, AppSettings, CaptureProxyHandle, Collection,
    CollectionItem, ConsoleLine, CookieJar, Environment, HistoryEntry, MockHandle, PerfReport,
    RequestDraft, ResponseView, TeamActivity, UpdateInfo, VariableContext, VaultStore, WsSession,
};
use crate::runtime::RunnerReport;
use crate::storage::{AppSnapshot, JsonStore};
use crate::ui::{push_toast, ConflictState, Toast, ToastLevel};

pub static MOCK_HANDLE: Mutex<Option<MockHandle>> = Mutex::new(None);
pub static WS_SESSION: Mutex<Option<WsSession>> = Mutex::new(None);
pub static PROXY_HANDLE: Mutex<Option<CaptureProxyHandle>> = Mutex::new(None);

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
    pub response: Signal<Option<ResponseView>>,
    pub responses: Signal<HashMap<Uuid, ResponseView>>,
    pub prev_responses: Signal<HashMap<Uuid, ResponseView>>,
    pub sending: Signal<bool>,
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
    pub protocol_log: Signal<Vec<String>>,
    pub proxy_port: Signal<Option<u16>>,
    pub show_perf: Signal<bool>,
    pub perf_running: Signal<bool>,
    pub perf_report: Signal<Option<PerfReport>>,
    pub perf_vus: Signal<u32>,
    pub perf_secs: Signal<u64>,
    pub file_menu_open: Signal<bool>,
    pub tools_menu_open: Signal<bool>,
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
    pub checking_update: Signal<bool>,
    pub installing_update: Signal<bool>,
    pub toasts: Signal<Vec<Toast>>,
    pub drag_tab_idx: Signal<Option<usize>>,
    pub show_tab_menu: Signal<bool>,
}

impl WorkbenchState {
    pub fn use_new() -> Self {
        let app_settings = use_signal(AppSettings::default);
        let sidebar_w = use_signal(|| app_settings().ui.sidebar_width_px as f64);
        let pane_h = use_signal(|| app_settings().ui.request_pane_height_px as f64);
        let console_h = use_signal(|| app_settings().ui.console_height_px as f64);
        Self {
            draft: use_signal(RequestDraft::default),
            response: use_signal(|| None::<ResponseView>),
            responses: use_signal(HashMap::<Uuid, ResponseView>::new),
            prev_responses: use_signal(HashMap::<Uuid, ResponseView>::new),
            sending: use_signal(|| false),
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
            protocol_log: use_signal(Vec::<String>::new),
            proxy_port: use_signal(|| None::<u16>),
            show_perf: use_signal(|| false),
            perf_running: use_signal(|| false),
            perf_report: use_signal(|| None::<PerfReport>),
            perf_vus: use_signal(|| 5u32),
            perf_secs: use_signal(|| 5u64),
            file_menu_open: use_signal(|| false),
            tools_menu_open: use_signal(|| false),
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
            checking_update: use_signal(|| false),
            installing_update: use_signal(|| false),
            toasts: use_signal(Vec::<Toast>::new),
            drag_tab_idx: use_signal(|| None::<usize>),
            show_tab_menu: use_signal(|| false),
        }
    }

    pub fn notify(&self, msg: impl Into<String>, level: ToastLevel) {
        push_toast(self.toasts, Toast::new(msg, level));
    }

    pub fn build_snapshot(&self, settings: AppSettings, active_environment: String) -> AppSnapshot {
        let mut tabs = (self.open_tabs)();
        let cur = (self.draft)();
        if let Some(slot) = tabs.iter_mut().find(|t| t.id == cur.id) {
            *slot = cur;
        }
        AppSnapshot {
            collections: (self.collections)(),
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
            responses: (self.responses)(),
            show_vars: (self.show_vars)(),
            show_settings: (self.show_settings)(),
            show_runner: (self.show_runner)(),
            show_perf: (self.show_perf)(),
            show_env_manager: (self.show_env_manager)(),
            show_col_vars: (self.show_col_vars)(),
        }
    }

    /// Write snapshot (+ vault) without git push — for Cloud menu / after explicit sync.
    pub fn persist_local(&self) {
        if let Ok(store) = JsonStore::open_default() {
            let active_environment = find_environment(&(self.environments)(), (self.active_env_id)())
                .map(|e| e.name.clone())
                .unwrap_or_else(|| "No Environment".into());
            let v = (self.vars)();
            let _ = v.vault.save(store.root());
            let settings = (self.app_settings)();
            let snap = self.build_snapshot(settings, active_environment);
            let _ = store.save(&snap);
        }
    }

    pub fn persist(&mut self) {
        if let Ok(store) = JsonStore::open_default() {
            let active_environment = find_environment(&(self.environments)(), (self.active_env_id)())
                .map(|e| e.name.clone())
                .unwrap_or_else(|| "No Environment".into());
            let v = (self.vars)();
            let _ = v.vault.save(store.root());
            let settings = (self.app_settings)();
            let snap = self.build_snapshot(settings.clone(), active_environment.clone());
            match store.save(&snap) {
                Ok(()) => {
                    let base = format!("saved · {}", store.root().display());
                    self.status_msg.set(base.clone());
                    if settings.cloud_sync.enabled && !(self.cloud_busy)() {
                        let mut st = *self;
                        let root = store.root().to_path_buf();
                        let mut settings_bg = settings;
                        let cols = (self.collections)();
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
                                st.persist_local();
                            }
                        });
                    }
                }
                Err(e) => self.status_msg.set(format!("save error: {e}")),
            }
        }
    }

    pub fn apply_snapshot(&mut self, snap: AppSnapshot, root: &Path) {
        self.collections.set(snap.collections.clone());
        self.environments.set(snap.environments.clone());
        let mut v = snap.variables;
        if let Ok(vault) = VaultStore::load(root) {
            v.vault = vault;
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
        self.responses.set(snap.responses);
        self.show_vars.set(snap.show_vars);
        self.show_settings.set(snap.show_settings);
        self.show_runner.set(snap.show_runner);
        self.show_perf.set(snap.show_perf);
        self.show_env_manager.set(snap.show_env_manager);
        self.show_col_vars.set(snap.show_col_vars);
        self.active_collection_id.set(
            snap.active_collection_id
                .or_else(|| snap.collections.first().map(|c| c.id)),
        );
        self.active_request_id.set(snap.active_request_id);

        if let Some(req_id) = snap.active_request_id {
            if let Some((col_id, req)) = find_request_across(&snap.collections, req_id) {
                self.draft.set(req.clone());
                self.active_collection_id.set(Some(col_id));
            }
        }

        self.status_msg.set(format!("storage: {}", root.display()));
    }
}

pub fn sync_env_vars(envs: &[Environment], id: Option<Uuid>, v: &mut VariableContext) {
    if let Some(env) = find_environment(envs, id) {
        v.environment = env.to_map();
    } else {
        v.environment.clear();
    }
}

/// Remove a collection (to trash) or folder/request from the tree.
pub fn execute_tree_delete(
    id: Uuid,
    collections: &mut Vec<Collection>,
    trash: &mut Vec<Collection>,
    active_collection_id: &mut Option<Uuid>,
    active_request_id: &mut Option<Uuid>,
    draft: &mut RequestDraft,
) -> Option<String> {
    if trash_collection(collections, trash, id) {
        if *active_collection_id == Some(id) {
            *active_collection_id = collections.first().map(|c| c.id);
        }
        Some("moved collection to Trash".into())
    } else {
        remove_across(collections, id);
        if *active_request_id == Some(id) {
            *active_request_id = None;
            *draft = RequestDraft::default();
        }
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
