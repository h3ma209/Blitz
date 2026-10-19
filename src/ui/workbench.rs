use dioxus::prelude::*;
use uuid::Uuid;

use crate::core::{
    add_folder_under, add_request_under, apply_collection_map, apply_collection_var_edit,
    apply_environment_map, apply_env_var_edit, branch_name_for_env, bundled_postman_collections,
    clone_or_ensure, delete_needs_confirmation, duplicate_across, ensure_default_environments,
    export_collection_v21, flatten_collections, insert_item_under,
    export_environment_json, find_environment, find_request_across, generate_snippet,
    graphql_ws_subscribe, grpc_reflection_list, grpc_unary, import_collection_v21, import_curl,
    import_environment_texts, import_har, import_openapi3, interceptor_limits_blurb,
    limitations_blurb, link_folder, mcp_call_tool, mcp_list_tools, mqtt_publish, mqtt_subscribe_once,
    move_item_to, open_in_file_manager, pull_workspace, purge_from_trash, push_console, push_history,
    push_workspace, refresh_status, remove_across, rename_across, reorder_before, restore_from_trash,
    routes_from_collections, run_performance, save_example_to_request, save_request_to_collections,
    snippet_languages, socketio_session, sort_collections, split_mcp_args, sse_listen,
    start_capture_proxy, start_mock, switch_env_branch, sync_environment, sync_on_save,
    remint_pasted_item, request_tab_display_name, snapshot_tree_item, trash_collection,
    ws_connect_session, AppSettings,
    CaptureProxyHandle, Collection, CollectionItem, ConsoleLine,
    Cookie, CookieJar, EnvVar, Environment, Folder, HistoryEntry, KeyValue, MockHandle, PerfReport,
    RequestDraft, RequestProtocol, ResponseView, SavedExample, UiSettings, UiTheme, VarResolveScope,
    VariableContext, VaultStore, WsSession, DEFAULT_CLOUD_REMOTE, HISTORY_MAX,
};
use crate::runtime::{
    run_collection_with_config, send_request_with_options, RunnerConfig, RunnerReport, SendError,
    SendOptions,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static MOCK_HANDLE: Mutex<Option<MockHandle>> = Mutex::new(None);
static WS_SESSION: Mutex<Option<WsSession>> = Mutex::new(None);
static PROXY_HANDLE: Mutex<Option<CaptureProxyHandle>> = Mutex::new(None);
use crate::storage::{AppSnapshot, JsonStore};
use crate::ui::console_panel::ConsolePanel;
use crate::ui::env_manager::{EnvManager, EnvManagerEvent};
use crate::ui::env_selector::EnvSelector;
use crate::ui::request_builder::{RequestBuilder, RequestVarEditEvent};
use crate::ui::response_viewer::ResponseViewer;
use crate::ui::runner_panel::{RunnerEvent, RunnerPanel};
use crate::ui::settings_panel::{SettingsEvent, SettingsPanel};
use crate::ui::theme_selector::ThemeSelector;
use crate::ui::sidebar::{Sidebar, SidebarAction, SidebarEvent};
use crate::ui::CommentableTextarea;
use crate::ui::{DismissBackdrop, ModalShell};
use crate::ui::loading::Spinner;

/// Vertical pane drag state — handled on `.workbench`, never as bare `.app-shell` grid children.
#[derive(Clone, Copy, PartialEq)]
enum PaneDrag {
    Request { start_y: f64, start_h: u32 },
    Console { start_y: f64, start_h: u32 },
}

/// Delete confirmation dialog payload for sidebar tree items.
#[derive(Clone, PartialEq)]
struct PendingDelete {
    id: Uuid,
    label: String,
}

/// In-app clipboard for collection tree cut/copy/paste (folder/request items).
#[derive(Clone)]
struct TreeClipboard {
    item: CollectionItem,
    cut_from: Option<Uuid>,
}

/// Remove a collection (to trash) or folder/request from the tree.
fn execute_tree_delete(
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

fn tree_item_label(collections: &[Collection], id: Uuid) -> String {
    flatten_collections(collections)
        .into_iter()
        .find(|r| r.id == id)
        .map(|r| r.name)
        .unwrap_or_else(|| "item".into())
}

#[component]
pub fn Workbench() -> Element {
    let mut draft = use_signal(RequestDraft::default);
    let mut response = use_signal(|| None::<ResponseView>);
    // Persist responses across tab/navigation switches - keyed by request id
    let mut responses = use_signal(|| std::collections::HashMap::<uuid::Uuid, ResponseView>::new());
    let mut sending = use_signal(|| false);
    // Bumped on each Send / Cancel so late worker results are ignored.
    let mut send_gen = use_signal(|| 0u64);
    let mut send_cancel = use_signal(|| None::<Arc<AtomicBool>>);
    let mut collections = use_signal(Vec::<Collection>::new);
    let mut environments = use_signal(Vec::<Environment>::new);
    let mut active_env_id = use_signal(|| None::<Uuid>);
    let mut active_collection_id = use_signal(|| None::<Uuid>);
    let mut active_request_id = use_signal(|| None::<Uuid>);
    let mut vars = use_signal(VariableContext::demo);
    let mut history = use_signal(Vec::<HistoryEntry>::new);
    let mut cookies = use_signal(CookieJar::default);
    let mut console_lines = use_signal(Vec::<ConsoleLine>::new);
    let mut console_open = use_signal(|| false);
    let mut show_env_manager = use_signal(|| false);
    let mut editing_env_id = use_signal(|| None::<Uuid>);
    let mut status_msg = use_signal(|| String::new());
    let mut impex_modal = use_signal(|| None::<ImpexModal>);
    let mut loaded = use_signal(|| false);
    let mut splash_visible = use_signal(|| true);
    let mut splash_leaving = use_signal(|| false);
    // Compact overlay while cloud checkout / tree reload runs (not a bare grid child).
    let mut env_switch_overlay = use_signal(|| None::<String>);
    // Remount sidebar (via `key`) so tree starts fully folded after load/import/sync.
    let mut fold_epoch = use_signal(|| 0u64);
    let mut app_settings = use_signal(AppSettings::default);
    let mut data_root = use_signal(|| String::new());
    let mut show_settings = use_signal(|| false);
    let mut show_runner = use_signal(|| false);
    let mut runner_running = use_signal(|| false);
    let mut runner_report = use_signal(|| None::<RunnerReport>);
    let mut trash = use_signal(Vec::<Collection>::new);
    let mut open_tabs = use_signal(|| vec![RequestDraft::default()]);
    let mut show_vars = use_signal(|| false);
    let mut show_palette = use_signal(|| false);
    let mut show_trash = use_signal(|| false);
    let mut show_snippets = use_signal(|| false);
    let mut snippet_lang = use_signal(|| "cURL".to_string());
    let mut mock_port = use_signal(|| None::<u16>);
    let mut ws_connected = use_signal(|| false);
    let mut protocol_log = use_signal(Vec::<String>::new);
    let mut proxy_port = use_signal(|| None::<u16>);
    let mut show_perf = use_signal(|| false);
    let mut perf_running = use_signal(|| false);
    let mut perf_report = use_signal(|| None::<PerfReport>);
    let mut perf_vus = use_signal(|| 5u32);
    let mut perf_secs = use_signal(|| 5u64);
    let mut file_menu_open = use_signal(|| false);
    let mut tools_menu_open = use_signal(|| false);
    let mut cloud_menu_open = use_signal(|| false);
    let mut show_cookies = use_signal(|| false);
    let mut show_col_vars = use_signal(|| false);
    // false = active collection, true = browse all collections.
    let mut col_vars_all = use_signal(|| false);
    // Accordion expansion in "all" mode.
    let mut col_vars_open_id = use_signal(|| None::<Uuid>);
    // Sidebar resize drag: (start client X, start width px).
    let mut sidebar_drag = use_signal(|| None::<(f64, f64)>);
    // Live sidebar width during drag — kept out of `app_settings` so the
    // whole tree isn't re-rendered on every mouse-move (that caused the
    // stutter). Committed to `app_settings` only on mouseup.
    let mut sidebar_w = use_signal(|| app_settings().ui.sidebar_width_px as f64);
    // Workbench pane drag — handled on `.workbench`, not as bare `.app-shell` grid children.
    let mut pane_drag = use_signal(|| None::<PaneDrag>);
    // Live heights during a pane drag — kept out of `app_settings` until
    // mouseup, for the same reason as the sidebar resize.
    let mut pane_h = use_signal(|| app_settings().ui.request_pane_height_px as f64);
    let mut console_h = use_signal(|| app_settings().ui.console_height_px as f64);
    // Loading state for async operations (import, etc.)
    let mut is_loading = use_signal(|| false);
    let mut pending_delete = use_signal(|| None::<PendingDelete>);
    let mut tree_clipboard = use_signal(|| None::<TreeClipboard>);

    use_effect(move || {
        if loaded() {
            return;
        }
        if let Ok(store) = JsonStore::open_default() {
            match store.load() {
                Ok(snap) => {
                    data_root.set(store.root().display().to_string());
                    apply_snapshot(
                        snap,
                        &mut collections,
                        &mut environments,
                        &mut vars,
                        &mut active_env_id,
                        &mut active_collection_id,
                        &mut active_request_id,
                        &mut history,
                        &mut cookies,
                        &mut draft,
                        &mut app_settings,
                        &mut trash,
                        &mut open_tabs,
                        &mut responses,
                        &mut show_vars,
                        &mut show_settings,
                        &mut show_runner,
                        &mut show_perf,
                        &mut show_env_manager,
                        &mut show_col_vars,
                        &mut status_msg,
                        store.root(),
                    );
                }
                Err(e) => status_msg.set(format!("load error: {e}")),
            }
        }
        fold_epoch.set(fold_epoch() + 1);
        loaded.set(true);
        // Sync the live drag signals from the restored settings so a later
        // resize starts from the persisted width/height.
        if sidebar_drag().is_none() {
            sidebar_w.set(app_settings().ui.sidebar_width_px as f64);
        }
        if pane_drag().is_none() {
            pane_h.set(app_settings().ui.request_pane_height_px as f64);
            console_h.set(app_settings().ui.console_height_px as f64);
        }
        // Paint opaque splash first, then fade out (not a bare grid child).
        let splash_hold = app_settings().ui.clamped().splash_min_ms as u64;
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(splash_hold)).await;
            splash_leaving.set(true);
            tokio::time::sleep(Duration::from_millis(320)).await;
            splash_visible.set(false);
        });
    });

    // Global click-outside handler to close dropdown menus
    // Keep the live drag signals in sync with `app_settings` whenever it
    // changes outside of an active drag (e.g. settings reset, restore).
    use_effect(move || {
        let _ = app_settings();
        if sidebar_drag().is_none() {
            sidebar_w.set(app_settings().ui.sidebar_width_px as f64);
        }
        if pane_drag().is_none() {
            pane_h.set(app_settings().ui.request_pane_height_px as f64);
            console_h.set(app_settings().ui.console_height_px as f64);
        }
    });
    // Write snapshot (+ vault) without git push — for Cloud menu / after explicit sync.
    let persist_local = move || {
        if let Ok(store) = JsonStore::open_default() {
            let active_environment = find_environment(&environments(), active_env_id())
                .map(|e| e.name.clone())
                .unwrap_or_else(|| "No Environment".into());
            let v = vars();
            let _ = v.vault.save(store.root());
            let settings = app_settings();
            let snap = AppSnapshot {
                collections: collections(),
                variables: v,
                active_environment,
                environments: environments(),
                active_environment_id: active_env_id(),
                history: history(),
                cookies: cookies(),
                active_collection_id: active_collection_id(),
                active_request_id: active_request_id(),
                settings,
                trash: trash(),
                open_tabs: open_tabs(),
                responses: responses(),
                show_vars: show_vars(),
                show_settings: show_settings(),
                show_runner: show_runner(),
                show_perf: show_perf(),
                show_env_manager: show_env_manager(),
                show_col_vars: show_col_vars(),
            };
            let _ = store.save(&snap);
        }
    };

    let mut persist = move || {
        if let Ok(store) = JsonStore::open_default() {
            let active_environment = find_environment(&environments(), active_env_id())
                .map(|e| e.name.clone())
                .unwrap_or_else(|| "No Environment".into());
            // Persist vault file separately (not inside snapshot vars).
            let v = vars();
            let _ = v.vault.save(store.root());
            let mut settings = app_settings();
            let cloud_msg = if settings.cloud_sync.enabled {
                let result = sync_on_save(
                    store.root(),
                    &mut settings,
                    &collections(),
                    &environments(),
                    &active_environment,
                    &v,
                );
                app_settings.set(settings.clone());
                Some(result.message)
            } else {
                None
            };
            let snap = AppSnapshot {
                collections: collections(),
                variables: v,
                active_environment,
                environments: environments(),
                active_environment_id: active_env_id(),
                history: history(),
                cookies: cookies(),
                active_collection_id: active_collection_id(),
                active_request_id: active_request_id(),
                settings,
                trash: trash(),
                open_tabs: open_tabs(),
                responses: responses(),
                show_vars: show_vars(),
                show_settings: show_settings(),
                show_runner: show_runner(),
                show_perf: show_perf(),
                show_env_manager: show_env_manager(),
                show_col_vars: show_col_vars(),
            };
            match store.save(&snap) {
                Ok(()) => {
                    let base = format!("saved · {}", store.root().display());
                    status_msg.set(match cloud_msg {
                        Some(m) => format!("{base} · {m}"),
                        None => base,
                    });
                }
                Err(e) => status_msg.set(format!("save error: {e}")),
            }
        }
    };

    let sync_env_vars = move |envs: &[Environment], id: Option<Uuid>, v: &mut VariableContext| {
        if let Some(env) = find_environment(envs, id) {
            v.environment = env.to_map();
        } else {
            v.environment.clear();
        }
    };

    // When cloud sync on: commit current branch, checkout env/<name>, reload tree.
    // Returns the env id to select (may remap after import).
    let mut apply_env_cloud_switch = move |new_id: Option<Uuid>| -> Option<Uuid> {
        let settings = app_settings();
        if !settings.cloud_sync.enabled {
            return new_id;
        }
        let envs = environments();
        let from_name = find_environment(&envs, active_env_id())
            .map(|e| e.name.clone())
            .unwrap_or_else(|| "No Environment".into());
        let to_name = find_environment(&envs, new_id)
            .map(|e| e.name.clone())
            .unwrap_or_else(|| "No Environment".into());
        if from_name == to_name {
            return new_id;
        }
        let Ok(store) = JsonStore::open_default() else {
            return new_id;
        };
        let mut settings = app_settings();
        match switch_env_branch(
            store.root(),
            &mut settings,
            &collections(),
            &envs,
            &from_name,
            &to_name,
            &vars(),
        ) {
            Ok((cols, loaded_envs)) => {
                app_settings.set(settings);
                if !cols.is_empty() {
                    collections.set(cols);
                    fold_epoch.set(fold_epoch() + 1);
                }
                let pick = if loaded_envs.is_empty() {
                    new_id
                } else {
                    let pick = loaded_envs
                        .iter()
                        .find(|e| e.name == to_name)
                        .map(|e| e.id)
                        .or(new_id);
                    environments.set(loaded_envs);
                    pick
                };
                status_msg.set(format!(
                    "cloud branch {}",
                    branch_name_for_env(&to_name)
                ));
                pick
            }
            Err(e) => {
                app_settings.set(settings);
                status_msg.set(format!("cloud env switch: {e}"));
                new_id
            }
        }
    };

    // Env dropdown / manager: paint compact overlay, then checkout+reload (hide on success or error).
    let mut switch_to_env = move |new_id: Option<Uuid>| {
        let cur = active_env_id();
        let envs = environments();
        let to_name = find_environment(&envs, new_id)
            .map(|e| e.name.clone())
            .unwrap_or_else(|| "No Environment".into());
        // No overlay for No Environment or re-selecting the same env.
        let skip_overlay = new_id.is_none() || cur == new_id;

        let mut finish = move |id: Option<Uuid>| {
            active_env_id.set(id);
            editing_env_id.set(id);
            let mut v = vars();
            sync_env_vars(&environments(), id, &mut v);
            vars.set(v);
            persist();
            env_switch_overlay.set(None);
        };

        if skip_overlay {
            let id = apply_env_cloud_switch(new_id);
            finish(id);
            return;
        }

        env_switch_overlay.set(Some(to_name));
        spawn(async move {
            // Yield so the overlay paints before sync git/checkout work.
            tokio::time::sleep(Duration::from_millis(40)).await;
            let id = apply_env_cloud_switch(new_id);
            finish(id);
        });
    };

    let do_save = use_callback(move |_| {
        let mut cols = collections();
        let owner = save_request_to_collections(&mut cols, active_collection_id(), draft());
        let req_id = draft().id;
        collections.set(cols);
        active_collection_id.set(Some(owner));
        active_request_id.set(Some(req_id));
        persist();
    });

    let do_new_tab = use_callback(move |_| {
        let mut req = RequestDraft::default();
        req.name = "Untitled Request".into();
        let mut tabs = open_tabs();
        // snapshot current draft into tabs if not already
        let cur = draft();
        if let Some(t) = tabs.iter_mut().find(|t| t.id == cur.id) {
            *t = cur.clone();
        } else {
            tabs.push(cur);
        }
        tabs.push(req.clone());
        open_tabs.set(tabs);
        draft.set(req);
        active_request_id.set(None);
        response.set(None);
        status_msg.set("new tab (unsaved)".into());
    });

    let do_close_tab = use_callback(move |tab_id: Uuid| {
        let mut tabs = open_tabs();
        let cur = draft();
        if let Some(slot) = tabs.iter_mut().find(|t| t.id == cur.id) {
            *slot = cur.clone();
        } else {
            tabs.push(cur.clone());
        }
        let Some(idx) = tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };
        let closing_active = cur.id == tab_id;
        tabs.remove(idx);

        if tabs.is_empty() {
            let empty = RequestDraft::default();
            draft.set(empty.clone());
            tabs.push(empty);
            active_request_id.set(None);
            response.set(None);
            open_tabs.set(tabs);
            status_msg.set("tab closed".into());
            return;
        }

        if closing_active {
            let next_idx = if idx < tabs.len() { idx } else { tabs.len() - 1 };
            let next = tabs[next_idx].clone();
            draft.set(next.clone());
            active_request_id.set(Some(next.id));
            response.set(None);
        }
        open_tabs.set(tabs);
        status_msg.set("tab closed".into());
    });

    let do_send = use_callback(move |_| {
        // While sending: Cancel aborts in-flight work and returns to idle.
        if sending() {
            if let Some(flag) = send_cancel() {
                flag.store(true, Ordering::SeqCst);
            }
            send_gen.set(send_gen() + 1);
            send_cancel.set(None);
            sending.set(false);
            status_msg.set("cancelled".into());
            response.set(Some(ResponseView {
                status: 0,
                status_text: "CANCELLED".into(),
                headers: vec![],
                body: String::new(),
                elapsed_ms: 0,
                error: Some("cancelled".into()),
                pipeline_log: vec!["cancelled".into()],
                console_messages: vec!["cancelled".into()],
                test_results: vec![],
                resolved_url: String::new(),
                timing: Default::default(),
                visualizer_html: String::new(),
            }));
            return;
        }

        let mut req = draft();

        // Apply environment-specific overrides from active collection
        let mut override_vars = Vec::new();
        if let Some(col_id) = active_collection_id() {
            if let Some(col) = collections().iter().find(|c| c.id == col_id) {
                if let Some(_env) = find_environment(&environments(), active_env_id()) {
                    if let Some(override_) = col.environment_overrides.requests.get(&req.id) {
                        req = req.apply_env_override(override_);
                        // Collect override variables to merge into collection vars
                        override_vars = override_.variables.clone();
                    }
                    // Collection-level variable overrides
                    for kv in &col.environment_overrides.variables {
                        if kv.enabled && !kv.key.is_empty() {
                            override_vars.push(kv.clone());
                        }
                    }
                }
            }
        }

        // gRPC unary via shared Send button / Invoke
        if req.protocol == RequestProtocol::Grpc {
            let my_gen = {
                let g = send_gen() + 1;
                send_gen.set(g);
                g
            };
            let cancel_flag = Arc::new(AtomicBool::new(false));
            send_cancel.set(Some(cancel_flag.clone()));
            sending.set(true);
            response.set(None);
            let endpoint = req.url.clone();
            let method = req.grpc_method.clone();
            let body = req.body.clone();
            let resolved = format!("{endpoint}/{method}");
            let cancel_grpc = cancel_flag.clone();
            spawn(async move {
                let ep = endpoint.clone();
                let meth = method.clone();
                let result = tokio::task::spawn_blocking(move || grpc_unary(&ep, &meth, &body)).await;
                if send_gen() != my_gen || cancel_grpc.load(Ordering::SeqCst) {
                    return;
                }
                let view = match result {
                    Ok(r) => {
                        let mut log = r.log;
                        if !r.grpc_message.is_empty() {
                            log.push(format!("grpc-message: {}", r.grpc_message));
                        }
                        ResponseView {
                            status: if r.grpc_status == 0 { 200 } else { 0 },
                            status_text: r.status,
                            headers: vec![("grpc-status".into(), r.grpc_status.to_string())],
                            body: r.body_json,
                            elapsed_ms: r.elapsed_ms,
                            error: r.error,
                            pipeline_log: log.clone(),
                            console_messages: log,
                            test_results: vec![],
                            resolved_url: resolved,
                            timing: crate::core::TimingBreakdown {
                                total_ms: r.elapsed_ms,
                                ttfb_ms: r.elapsed_ms,
                                ..Default::default()
                            },
                            visualizer_html: String::new(),
                        }
                    }
                    Err(e) => ResponseView::error(format!("join: {e}"), vec![]),
                };
                if send_gen() != my_gen {
                    return;
                }
                let mut lines = console_lines();
                for msg in &view.console_messages {
                    push_console(&mut lines, ConsoleLine::info(msg.clone()));
                }
                console_lines.set(lines);
                protocol_log.set(view.pipeline_log.clone());
                response.set(Some(view));
                sending.set(false);
                send_cancel.set(None);
            });
            return;
        }

        if req.protocol != RequestProtocol::Http {
            status_msg.set("use protocol buttons for WS/MQTT/MCP".into());
            return;
        }

        let my_gen = {
            let g = send_gen() + 1;
            send_gen.set(g);
            g
        };
        let cancel_flag = Arc::new(AtomicBool::new(false));
        send_cancel.set(Some(cancel_flag.clone()));
        sending.set(true);
        response.set(None);
        status_msg.set(String::new());

        let mut ctx = vars();
        // Always sync collection vars from active collection (needed for
        // epm.collectionVariables.get in Initiate Envs / SNB scripts).
        if let Some(col_id) = active_collection_id() {
            if let Some(col) = collections().iter().find(|c| c.id == col_id) {
                ctx.collection = col.variables_map();
        // Apply collection-level environment overrides
                if let Some(_env) = find_environment(&environments(), active_env_id()) {
                    for kv in &col.environment_overrides.variables {
                        if kv.enabled && !kv.key.is_empty() {
                            ctx.collection.insert(kv.key.clone(), kv.value.clone());
                        }
                    }
                }
            }
        } else {
            ctx.collection.clear();
        }
        // Merge request-level environment override variables
        for kv in override_vars {
            if kv.enabled && !kv.key.is_empty() {
                ctx.collection.insert(kv.key, kv.value);
            }
        }
        if let Some(env) = find_environment(&environments(), active_env_id()) {
            ctx.environment = env.to_map();
            ctx.local
                .insert("environment".into(), env.name.clone());
        } else {
            ctx.environment.clear();
            ctx.local.remove("environment");
        }
        let col_auth = active_collection_id().and_then(|id| {
            collections()
                .into_iter()
                .find(|c| c.id == id)
                .map(|c| {
                    // Check for collection-level auth override for active environment
                    if let Some(_env) = find_environment(&environments(), active_env_id()) {
                        if let Some(override_) = c.environment_overrides.auth.as_ref() {
                            return override_.clone();
                        }
                    }
                    c.auth
                })
        });
        let mut jar = cookies();
        let settings = app_settings();

        let had_active_env = active_env_id().is_some();
        let active_col_id = active_collection_id();

        spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                let send = send_request_with_options(
                    &req,
                    &mut ctx,
                    SendOptions {
                        secrets_store: None,
                        collection_auth: col_auth.as_ref(),
                        cookie_jar: Some(&mut jar),
                        app_settings: Some(&settings),
                        cancel: Some(cancel_flag),
                        ..Default::default()
                    },
                );
                // Always return mutated ctx so prerequest/test side-effects persist
                // even when HTTP fails (Postman semantics).
                (send, jar, ctx)
            })
            .await;

            // Cancel raced: ignore late result (UI already idle).
            if send_gen() != my_gen {
                return;
            }

            let (view, new_jar, new_ctx, cancelled) = match result {
                Ok((Ok(view), jar, ctx)) => (view, jar, ctx, false),
                Ok((Err(SendError::Cancelled), jar, ctx)) => {
                    // Still apply script vars from prerequest that already ran.
                    (ResponseView {
                        status: 0,
                        status_text: "CANCELLED".into(),
                        headers: vec![],
                        body: String::new(),
                        elapsed_ms: 0,
                        error: Some("cancelled".into()),
                        pipeline_log: vec!["cancelled".into()],
                        console_messages: vec!["cancelled".into()],
                        test_results: vec![],
                        resolved_url: String::new(),
                        timing: Default::default(),
                        visualizer_html: String::new(),
                    }, jar, ctx, true)
                }
                Ok((Err(e), jar, ctx)) => (
                    ResponseView::error(e.to_string(), vec!["pipeline failed".into()]),
                    jar,
                    ctx,
                    false,
                ),
                Err(e) => (
                    ResponseView::error(format!("join: {e}"), vec![]),
                    cookies(),
                    vars(),
                    false,
                ),
            };

            if send_gen() != my_gen {
                return;
            }

            cookies.set(new_jar);

            if !cancelled {
                let mut hist = history();
                let draft_snap = draft();
                let hist_url = if view.resolved_url.is_empty() {
                    draft_snap.url.clone()
                } else {
                    view.resolved_url.clone()
                };
                let entry = HistoryEntry::from_send(
                    &draft_snap,
                    &hist_url,
                    if view.error.is_some() {
                        None
                    } else {
                        Some(view.status)
                    },
                    if view.error.is_some() {
                        None
                    } else {
                        Some(view.elapsed_ms)
                    },
                );
                push_history(&mut hist, entry, HISTORY_MAX);
                history.set(hist);
            }

            let mut lines = console_lines();
            for msg in &view.console_messages {
                push_console(&mut lines, ConsoleLine::info(msg.clone()));
            }
            if let Some(err) = &view.error {
                push_console(&mut lines, ConsoleLine::error(err.clone()));
            }
            console_lines.set(lines);
            console_open.set(true);

            // Persist script scope mutations → Environment / Collection + VariableContext.
            let env_keys = new_ctx.environment.len();
            let mut v = new_ctx;
            let vault = vars().vault.clone();
            v.vault = vault;

            if let Some(id) = active_env_id() {
                let mut envs = environments();
                if let Some(env) = envs.iter_mut().find(|e| e.id == id) {
                    apply_environment_map(env, &v.environment);
                }
                environments.set(envs);
            } else if env_keys > 0 {
                // Prefer Postman semantics: require an active environment.
                // Do not silently globalize; clear ephemeral env map so Vars UI stays honest.
                let mut lines = console_lines();
                push_console(
                    &mut lines,
                    ConsoleLine::error(
                        "no active environment — epm.environment.set ignored (select Local/Development/Production first)",
                    ),
                );
                console_lines.set(lines);
                status_msg.set(
                    "no active environment — epm.environment.set ignored".into(),
                );
                v.environment.clear();
            }

            if let Some(col_id) = active_col_id {
                let mut cols = collections();
                if let Some(col) = cols.iter_mut().find(|c| c.id == col_id) {
                    apply_collection_map(col, &v.collection);
                }
                collections.set(cols);
            }

            vars.set(v);

            if cancelled {
                status_msg.set("cancelled".into());
            } else if had_active_env && env_keys > 0 && status_msg().is_empty() {
                status_msg.set(format!("scripts updated {env_keys} env var(s)"));
            }

            // Save response to history and map for preservation across tab switches
            let draft_id = draft().id;
            response.set(Some(view.clone()));
            let mut resp_map = responses();
            resp_map.insert(draft_id, view.clone());
            responses.set(resp_map);
            sending.set(false);
            send_cancel.set(None);
            persist();
        });
    });

    let on_sidebar = move |evt: SidebarEvent| {
        match evt.action {
            SidebarAction::SelectRequest => {
                if let Some(id) = evt.id {
                    if let Some((col_id, req)) = find_request_across(&collections(), id) {
                        let mut tabs = open_tabs();
                        let cur = draft();
                        if let Some(slot) = tabs.iter_mut().find(|t| t.id == cur.id) {
                            *slot = cur;
                        } else {
                            tabs.push(cur);
                        }
                        if let Some(slot) = tabs.iter_mut().find(|t| t.id == req.id) {
                            *slot = req.clone();
                        } else {
                            tabs.push(req.clone());
                        }
                        open_tabs.set(tabs);
                        draft.set(req.clone());
                        active_request_id.set(Some(id));
                        active_collection_id.set(Some(col_id));
                        if let Some(col) = collections().iter().find(|c| c.id == col_id) {
                            let mut v = vars();
                            v.collection = col.variables_map();
                            vars.set(v);
                        }
                        // Restore response from cache if it exists
                        let resp_map = responses();
                        if let Some(cached) = resp_map.get(&req.id).cloned() {
                            response.set(Some(cached));
                        } else {
                            response.set(None);
                        }
                        persist();
                    }
                }
            }
            SidebarAction::SelectCollection => {
                let col_id = evt.collection_id.or(evt.id);
                active_collection_id.set(col_id);
                active_request_id.set(None);
                if let Some(id) = col_id {
                    let cols = collections();
                    if let Some(col) = cols.iter().find(|c| c.id == id) {
                        let mut v = vars();
                        v.collection = col.variables_map();
                        vars.set(v);
                    }
                } else {
                    let mut v = vars();
                    v.collection.clear();
                    vars.set(v);
                }
                persist();
            }
            SidebarAction::ToggleExpand => {
                // Expand state lives in Sidebar; no persistence needed.
            }
            SidebarAction::NewCollection => {
                let mut cols = collections();
                let n = cols.len() + 1;
                let col = Collection::new(format!("Collection {n}"));
                let id = col.id;
                cols.push(col);
                collections.set(cols);
                active_collection_id.set(Some(id));
                active_request_id.set(None);
                persist();
            }
            SidebarAction::NewFolder => {
                // `id` = parent (collection or folder); `collection_id` = owning collection
                if let Some(parent_id) = evt.id.or(evt.collection_id) {
                    let mut cols = collections();
                    let n = count_folders_under(&cols, parent_id) + 1;
                    let folder = Folder::new(format!("Folder {n}"));
                    if add_folder_under(&mut cols, parent_id, folder) {
                        collections.set(cols);
                        if let Some(col_id) = evt.collection_id {
                            active_collection_id.set(Some(col_id));
                        }
                        persist();
                    }
                }
            }
            SidebarAction::NewRequest => {
                if let Some(parent_id) = evt.id.or(evt.collection_id) {
                    let mut cols = collections();
                    let mut req = RequestDraft::default();
                    let req_id = req.id;
                    let n = count_requests_under(&cols, parent_id) + 1;
                    req.name = format!("Request {n}");
                    if add_request_under(&mut cols, parent_id, req.clone()) {
                        let col_id = evt.collection_id.unwrap_or(parent_id);
                        collections.set(cols);
                        let mut tabs = open_tabs();
                        let cur = draft();
                        if let Some(slot) = tabs.iter_mut().find(|t| t.id == cur.id) {
                            *slot = cur;
                        }
                        tabs.push(req.clone());
                        open_tabs.set(tabs);
                        draft.set(req);
                        active_collection_id.set(Some(col_id));
                        active_request_id.set(Some(req_id));
                        response.set(None);
                        persist();
                    }
                }
            }
            SidebarAction::Rename => {
                if let (Some(id), Some(raw)) = (evt.id, evt.name.clone()) {
                    let next = sanitize_rename_name(&raw);
                    if !next.is_empty() {
                        let mut cols = collections();
                        if rename_across(&mut cols, id, next.clone()) {
                            if active_request_id() == Some(id) {
                                let mut d = draft();
                                d.name = next.clone();
                                draft.set(d);
                            }
                            let mut tabs = open_tabs();
                            if let Some(t) = tabs.iter_mut().find(|t| t.id == id) {
                                t.name = next.clone();
                            }
                            open_tabs.set(tabs);
                            collections.set(cols);
                            status_msg.set(format!("renamed → {next}"));
                            persist();
                        }
                    }
                }
            }
            SidebarAction::Delete => {
                if let Some(id) = evt.id {
                    let cols = collections();
                    if delete_needs_confirmation(&cols, id) {
                        pending_delete.set(Some(PendingDelete {
                            id,
                            label: tree_item_label(&cols, id),
                        }));
                    } else {
                        let mut cols = cols;
                        let mut tr = trash();
                        let mut col_id = active_collection_id();
                        let mut req_id = active_request_id();
                        let mut d = draft();
                        if let Some(msg) = execute_tree_delete(
                            id,
                            &mut cols,
                            &mut tr,
                            &mut col_id,
                            &mut req_id,
                            &mut d,
                        ) {
                            status_msg.set(msg);
                        }
                        trash.set(tr);
                        collections.set(cols);
                        active_collection_id.set(col_id);
                        active_request_id.set(req_id);
                        draft.set(d);
                        persist();
                    }
                }
            }
            SidebarAction::Copy => {
                if let Some(id) = evt.id {
                    if let Some(item) = snapshot_tree_item(&collections(), id) {
                        tree_clipboard.set(Some(TreeClipboard {
                            item,
                            cut_from: None,
                        }));
                        status_msg.set("copied to collection clipboard".into());
                    } else {
                        status_msg.set("copy: select a folder or request".into());
                    }
                }
            }
            SidebarAction::Cut => {
                if let Some(id) = evt.id {
                    if let Some(item) = snapshot_tree_item(&collections(), id) {
                        tree_clipboard.set(Some(TreeClipboard {
                            item,
                            cut_from: Some(id),
                        }));
                        status_msg.set("cut — paste to move".into());
                    } else {
                        status_msg.set("cut: select a folder or request".into());
                    }
                }
            }
            SidebarAction::Paste => {
                if let Some(parent_id) = evt.id {
                    if let Some(clip) = tree_clipboard() {
                        let mut cols = collections();
                        if let Some(cut_id) = clip.cut_from {
                            remove_across(&mut cols, cut_id);
                            if active_request_id() == Some(cut_id) {
                                active_request_id.set(None);
                                draft.set(RequestDraft::default());
                            }
                        }
                        let mut item = clip.item.clone();
                        remint_pasted_item(&mut item);
                        if insert_item_under(&mut cols, parent_id, item) {
                            collections.set(cols);
                            if clip.cut_from.is_some() {
                                tree_clipboard.set(None);
                            }
                            status_msg.set("pasted".into());
                            persist();
                        } else {
                            status_msg.set("paste failed — invalid target".into());
                        }
                    } else {
                        status_msg.set("nothing to paste".into());
                    }
                }
            }
            SidebarAction::Duplicate => {
                if let Some(id) = evt.id {
                    let mut cols = collections();
                    if let Some(new_id) = duplicate_across(&mut cols, id) {
                        sort_collections(&mut cols);
                        collections.set(cols);
                        status_msg.set(format!("duplicated → {new_id}"));
                        persist();
                    }
                }
            }
            SidebarAction::Favorite => {
                if let Some(id) = evt.id {
                    let mut cols = collections();
                    if let Some(col) = cols.iter_mut().find(|c| c.id == id) {
                        col.favorite = !col.favorite;
                    }
                    sort_collections(&mut cols);
                    collections.set(cols);
                    persist();
                }
            }
            SidebarAction::Sort => {
                let mut cols = collections();
                sort_collections(&mut cols);
                collections.set(cols);
                persist();
            }
            SidebarAction::MoveItem => {
                if let (Some(item_id), Some(target_id)) = (evt.id, evt.target_id) {
                    let mut cols = collections();
                    let ok = move_item_to(&mut cols, item_id, target_id)
                        || reorder_before(&mut cols, item_id, Some(target_id));
                    if ok {
                        collections.set(cols);
                        status_msg.set("moved / reordered".into());
                        persist();
                    } else {
                        status_msg.set("cannot move there".into());
                    }
                }
            }
            SidebarAction::CopyId => {
                if let Some(id) = evt.id {
                    status_msg.set(format!("id: {id}"));
                }
            }
            SidebarAction::OpenHistory => {
                if let Some(id) = evt.id {
                    if let Some(entry) = history().into_iter().find(|h| h.id == id) {
                        draft.set(entry.draft);
                        active_request_id.set(None);
                        response.set(None);
                        status_msg.set("reopened from history".into());
                    }
                }
            }
            SidebarAction::SaveHistoryToCollection => {
                if let Some(id) = evt.id {
                    if let Some(entry) = history().into_iter().find(|h| h.id == id) {
                        let mut cols = collections();
                        let mut d = entry.draft;
                        d.id = Uuid::new_v4();
                        d.name = format!("{} (from history)", d.name);
                        let owner =
                            save_request_to_collections(&mut cols, evt.collection_id, d.clone());
                        collections.set(cols);
                        draft.set(d.clone());
                        active_collection_id.set(Some(owner));
                        active_request_id.set(Some(d.id));
                        persist();
                        status_msg.set("saved history item to collection".into());
                    }
                }
            }
            SidebarAction::ImportCollection => {
                impex_modal.set(Some(ImpexModal::Import {
                    text: String::new(),
                }));
            }
            SidebarAction::ExportCollection => {
                let cols = collections();
                let col = evt
                    .collection_id
                    .and_then(|id| cols.iter().find(|c| c.id == id))
                    .or_else(|| cols.first());
                match col {
                    Some(c) => match export_collection_v21(c) {
                        Ok(text) => {
                            impex_modal.set(Some(ImpexModal::Export {
                                name: c.name.clone(),
                                text,
                            }));
                        }
                        Err(e) => status_msg.set(format!("export error: {e}")),
                    },
                    None => status_msg.set("no collection to export".into()),
                }
            }
            SidebarAction::ToggleHistoryCollapse => {
                let mut s = app_settings();
                s.ui.history_collapsed = !s.ui.history_collapsed;
                app_settings.set(s);
                persist();
            }
        }
    };

    let on_env_manager = move |evt: EnvManagerEvent| {
        match evt {
            EnvManagerEvent::Close => show_env_manager.set(false),
            EnvManagerEvent::Select(id) => {
                switch_to_env(id);
            }
            EnvManagerEvent::Create => {
                let mut envs = environments();
                let n = envs.len() + 1;
                let mut env = Environment::new(format!("Environment {n}"));
                env.values.push(EnvVar {
                    key: String::new(),
                    initial_value: String::new(),
                    current_value: String::new(),
                    enabled: true,
                });
                let id = env.id;
                envs.push(env);
                environments.set(envs);
                active_env_id.set(Some(id));
                editing_env_id.set(Some(id));
                persist();
            }
            EnvManagerEvent::Rename { id, name } => {
                let mut envs = environments();
                if let Some(e) = envs.iter_mut().find(|e| e.id == id) {
                    e.name = name;
                }
                environments.set(envs);
                persist();
            }
            EnvManagerEvent::Delete(id) => {
                let mut envs = environments();
                envs.retain(|e| e.id != id);
                if active_env_id() == Some(id) {
                    active_env_id.set(envs.first().map(|e| e.id));
                }
                environments.set(envs.clone());
                let mut v = vars();
                sync_env_vars(&envs, active_env_id(), &mut v);
                vars.set(v);
                persist();
            }
            EnvManagerEvent::SetVars { id, values } => {
                let mut envs = environments();
                if let Some(e) = envs.iter_mut().find(|e| e.id == id) {
                    e.values = values;
                }
                environments.set(envs.clone());
                if active_env_id() == Some(id) {
                    let mut v = vars();
                    sync_env_vars(&envs, Some(id), &mut v);
                    vars.set(v);
                }
                persist();
            }
        }
    };

    let ui = app_settings().ui.clone().clamped();
    // During a sidebar drag, drive the width off the live signal so the
    // resize is smooth without re-rendering the whole tree on every move.
    let live_sidebar_w = if sidebar_drag().is_some() {
        sidebar_w()
    } else {
        ui.sidebar_width_px as f64
    };
    let shell_style = format!(
        "--fs-base: {}px; --fs-body: {}px; --fs-editor: {}px; --sidebar-w: {}px; --editor-tab-size: {};",
        ui.ui_font_size_px,
        ui.ui_font_size_px,
        ui.editor_font_size_px,
        live_sidebar_w,
        ui.tab_size
    );
    let live_request_h = if matches!(pane_drag(), Some(PaneDrag::Request { .. })) {
        pane_h()
    } else {
        ui.request_pane_height_px as f64
    };
    let live_console_h = if matches!(pane_drag(), Some(PaneDrag::Console { .. })) {
        console_h()
    } else {
        ui.console_height_px as f64
    };
    let workbench_style = format!(
        "--request-h: {}px; --console-h: {}px;",
        live_request_h, live_console_h
    );
    let appearance_tokens = ui.shell_class_tokens();
    let shell_class = {
        let base = if sidebar_drag().is_some() {
            format!("app-shell sidebar-resizing {appearance_tokens}")
        } else {
            format!("app-shell {appearance_tokens}")
        };
        if show_settings() {
            format!("{base} settings-open")
        } else {
            base
        }
    };
    let workbench_class = match pane_drag() {
        Some(PaneDrag::Request { .. } | PaneDrag::Console { .. }) => "workbench pane-resizing-v",
        None => "workbench",
    };
    let top_nav_menus_open =
        file_menu_open() || tools_menu_open() || cloud_menu_open();
    let top_bar_class = if top_nav_menus_open {
        "top-bar top-bar-elevated"
    } else {
        "top-bar"
    };

    let on_settings_event = move |evt: SettingsEvent| match evt {
        SettingsEvent::Close => show_settings.set(false),
        SettingsEvent::Save(s) => {
            let enabling_cloud = s.cloud_sync.enabled;
            app_settings.set(s);
            if enabling_cloud {
                let mut envs = environments();
                if ensure_default_environments(&mut envs) {
                    environments.set(envs);
                }
            }
            show_settings.set(false);
            persist();
            status_msg.set("settings saved".into());
        }
    };

    rsx! {
        div {
            class: "{shell_class}",
            style: "{shell_style}",
            tabindex: "0",
            onmousedown: move |_| {
                file_menu_open.set(false);
                tools_menu_open.set(false);
                cloud_menu_open.set(false);
            },
            // Global drag handlers live on `.app-shell` so they keep firing
            // while the cursor is off the 12px handle. They only touch the
            // cheap live signals during the drag — `app_settings` is written
            // once, on mouseup — which is what makes resizing stutter-free.
            onmousemove: move |evt| {
                let Some((start_x, start_w)) = sidebar_drag() else {
                    return;
                };
                let delta = (evt.client_coordinates().x - start_x).round() as i32;
                let new_w = UiSettings::clamp_sidebar_width(
                    (start_w as i32 + delta).max(0) as u32,
                );
                sidebar_w.set(new_w as f64);
            },
            onmouseup: move |_| {
                if sidebar_drag().is_some() {
                    sidebar_drag.set(None);
                    let mut s = app_settings();
                    s.ui.sidebar_width_px = sidebar_w() as u32;
                    app_settings.set(s);
                    persist();
                }
            },
            onmouseleave: move |_| {
                if sidebar_drag().is_some() {
                    sidebar_drag.set(None);
                    let mut s = app_settings();
                    s.ui.sidebar_width_px = sidebar_w() as u32;
                    app_settings.set(s);
                    persist();
                }
            },
            onkeydown: move |evt| {
                let meta = evt.modifiers().meta() || evt.modifiers().ctrl();
                let shift = evt.modifiers().shift();
                let alt = evt.modifiers().alt();
                match evt.key() {
                    Key::Enter if meta => {
                        evt.prevent_default();
                        do_send(());
                    }
                    Key::Character(c) if meta && !shift && c.eq_ignore_ascii_case("s") => {
                        evt.prevent_default();
                        do_save(());
                    }
                    Key::Character(c) if meta && !shift && c.eq_ignore_ascii_case("t") => {
                        evt.prevent_default();
                        do_new_tab(());
                    }
                    Key::Character(c) if meta && !shift && c.eq_ignore_ascii_case("w") => {
                        evt.prevent_default();
                        do_close_tab(draft().id);
                    }
                    Key::Character(c) if meta && shift && c.eq_ignore_ascii_case("p") => {
                        evt.prevent_default();
                        show_palette.set(!show_palette());
                    }
                    Key::Character(c) if meta && alt && c.eq_ignore_ascii_case("c") => {
                        evt.prevent_default();
                        console_open.set(!console_open());
                    }
                    _ => {}
                }
            },

            if top_nav_menus_open {
                DismissBackdrop {
                    z_index: 35,
                    on_dismiss: move |_| {
                        file_menu_open.set(false);
                        tools_menu_open.set(false);
                        cloud_menu_open.set(false);
                    },
                }
            }

            header { class: "{top_bar_class}",
                span {
                    class: "brand",
                    title: "⚡ Blitz · ⌘↵ Send/Cancel · ⌘S Save · ⌘T New · ⌘⇧P Palette",
                    img {
                        class: "brand-logo",
                        src: "{crate::brand::LOGO_SRC.as_str()}",
                        alt: "Blitz",
                    }
                    span { class: "brand-name", "Blitz" }
                }

                {if is_loading() {
                    rsx! {
                        div { class: "loading-indicator",
                            span { class: "loading-spinner" }
                            span { "Loading..." }
                        }
                    }
                } else {
                    rsx! {}
                }}

                nav { class: "top-nav",
                div { class: "menu-file",
                    button {
                        class: "top-link",
                        title: "File",
                        onmousedown: move |evt| evt.stop_propagation(),
                        onclick: move |_| {
                            tools_menu_open.set(false);
                            cloud_menu_open.set(false);
                            file_menu_open.set(!file_menu_open());
                        },
                        "File"
                        span { class: "top-caret", "▾" }
                    }
                    if file_menu_open() {
                        div { class: "menu-dropdown",
                            onmousedown: move |evt| evt.stop_propagation(),
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    file_menu_open.set(false);
                                    is_loading.set(true);
                                    spawn(async move {
                                        let picked = rfd::FileDialog::new()
                                            .set_title("Import from Postman export")
                                            .add_filter("Postman Collection", &["json"])
                                            .pick_files();
                                        let Some(paths) = picked else {
                                            is_loading.set(false);
                                            return;
                                        };
                                        let mut cols = collections();
                                        let mut imported_n = 0usize;
                                        let mut last_id = None;
                                        let mut errs = Vec::new();
                                        for path in paths {
                                            match std::fs::read_to_string(&path) {
                                                Ok(text) => match import_collection_v21(&text) {
                                                    Ok(col) => {
                                                        last_id = Some(col.id);
                                                        cols.push(col);
                                                        imported_n += 1;
                                                    }
                                                    Err(e) => errs.push(format!(
                                                        "{}: {e}",
                                                        path.file_name()
                                                            .and_then(|s| s.to_str())
                                                            .unwrap_or("file")
                                                    )),
                                                },
                                                Err(e) => errs.push(format!(
                                                    "{}: {e}",
                                                    path.display()
                                                )),
                                            }
                                        }
                                        if imported_n > 0 {
                                            collections.set(cols.clone());
                                            if let Some(id) = last_id {
                                                active_collection_id.set(Some(id));
                                                if let Some(col) = cols.iter().find(|c| c.id == id) {
                                                    let mut v = vars();
                                                    v.collection = col.variables_map();
                                                    vars.set(v);
                                                }
                                            }
                                            fold_epoch.set(fold_epoch() + 1);
                                            persist();
                                        }
                                        is_loading.set(false);
                                        if errs.is_empty() {
                                            status_msg.set(format!(
                                                "imported {imported_n} Postman collection(s)"
                                            ));
                                        } else {
                                            status_msg.set(format!(
                                                "imported {imported_n}; errors: {}",
                                                errs.join("; ")
                                            ));
                                        }
                                    });
                                },
                                "Import from Postman export…"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    file_menu_open.set(false);
                                    impex_modal.set(Some(ImpexModal::Import {
                                        text: String::new(),
                                    }));
                                },
                                "Paste Postman / cURL / OpenAPI…"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    file_menu_open.set(false);
                                    let mut cols = collections();
                                    let bundled = bundled_postman_collections();
                                    let n = bundled.len();
                                    let last_id = bundled.last().map(|c| c.id);
                                    cols.extend(bundled);
                                    collections.set(cols.clone());
                                    if let Some(id) = last_id {
                                        active_collection_id.set(Some(id));
                                        if let Some(col) = cols.iter().find(|c| c.id == id) {
                                            let mut v = vars();
                                            v.collection = col.variables_map();
                                            vars.set(v);
                                        }
                                    }
                                    fold_epoch.set(fold_epoch() + 1);
                                    persist();
                                    status_msg.set(format!(
                                        "loaded {n} bundled Postman export(s) — open Initiate Envs → Local/SNB and Send (with an active environment) to seed endpoint_* vars"
                                    ));
                                },
                                "Load bundled sample exports"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    file_menu_open.set(false);
                                    let cols = collections();
                                    let col = active_collection_id()
                                        .and_then(|id| cols.iter().find(|c| c.id == id))
                                        .or_else(|| cols.first());
                                    match col {
                                        Some(c) => match export_collection_v21(c) {
                                            Ok(text) => impex_modal.set(Some(ImpexModal::Export {
                                                name: c.name.clone(),
                                                text,
                                            })),
                                            Err(e) => {
                                                status_msg.set(format!("export error: {e}"))
                                            }
                                        },
                                        None => status_msg.set("no collection to export".into()),
                                    }
                                },
                                "Export active collection…"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    file_menu_open.set(false);
                                    is_loading.set(true);
                                    spawn(async move {
                                        let picked = rfd::FileDialog::new()
                                            .set_title("Import Postman environment(s)")
                                            .add_filter("Environment JSON", &["json"])
                                            .pick_files();
                                        let Some(paths) = picked else {
                                            is_loading.set(false);
                                            return;
                                        };
                                        if paths.is_empty() {
                                            is_loading.set(false);
                                            return;
                                        }
                                        let texts: Vec<String> = paths
                                            .iter()
                                            .filter_map(|p| std::fs::read_to_string(p).ok())
                                            .collect();
                                        match import_environment_texts(&texts) {
                                            Ok(envs) => {
                                                let count = envs.len();
                                                let mut store = environments();
                                                for env in envs {
                                                    let id = env.id;
                                                    store.push(env);
                                                    active_env_id.set(Some(id));
                                                }
                                                environments.set(store);
                                                persist();
                                                status_msg.set(format!(
                                                    "imported {count} environment(s)"
                                                ));
                                            }
                                            Err(e) => status_msg.set(format!("env import: {e}")),
                                        }
                                        is_loading.set(false);
                                    });
                                },
                                "Import environment(s)…"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    file_menu_open.set(false);
                                    is_loading.set(true);
                                    spawn(async move {
                                        let envs = environments();
                                        let env = active_env_id()
                                            .and_then(|id| envs.iter().find(|e| e.id == id))
                                            .or_else(|| envs.first());
                                        match env {
                                            Some(e) => match export_environment_json(e) {
                                                Ok(text) => {
                                                    if let Some(path) = rfd::FileDialog::new()
                                                        .set_title("Export environment")
                                                        .set_file_name(format!("{}.postman_environment.json", e.name))
                                                        .save_file()
                                                    {
                                                        match std::fs::write(&path, text) {
                                                            Ok(()) => status_msg.set(format!(
                                                                "exported env to {}",
                                                                path.display()
                                                            )),
                                                            Err(err) => {
                                                                status_msg.set(format!("write: {err}"))
                                                            }
                                                        }
                                                    }
                                                }
                                                Err(e) => status_msg.set(format!("env export: {e}")),
                                            },
                                            None => status_msg.set("no environment to export".into()),
                                        }
                                        is_loading.set(false);
                                    });
                                },
                                "Export active environment…"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    file_menu_open.set(false);
                                    is_loading.set(true);
                                    spawn(async move {
                                        let picked = rfd::FileDialog::new()
                                            .set_title("Import HAR / OpenAPI")
                                            .add_filter("HAR / OpenAPI", &["json", "yaml", "yml", "har"])
                                            .pick_file();
                                        let Some(path) = picked else {
                                            is_loading.set(false);
                                            return;
                                        };
                                        match std::fs::read_to_string(&path) {
                                            Ok(text) => {
                                                let col = if path
                                                    .extension()
                                                    .and_then(|e| e.to_str())
                                                    .map(|e| {
                                                        e.eq_ignore_ascii_case("yaml")
                                                            || e.eq_ignore_ascii_case("yml")
                                                    })
                                                    .unwrap_or(false)
                                                {
                                                    import_openapi3(&text)
                                                } else if text.contains("\"log\"")
                                                    && text.contains("\"entries\"")
                                                {
                                                    import_har(&text).map_err(|e| e)
                                                } else if text.contains("openapi")
                                                    || text.contains("\"paths\"")
                                                {
                                                    import_openapi3(&text)
                                                } else {
                                                    import_collection_v21(&text)
                                                };
                                                match col {
                                                    Ok(c) => {
                                                        let id = c.id;
                                                        let mut v = vars();
                                                        v.collection = c.variables_map();
                                                        vars.set(v);
                                                        let mut cols = collections();
                                                        cols.push(c);
                                                        collections.set(cols);
                                                        active_collection_id.set(Some(id));
                                                        fold_epoch.set(fold_epoch() + 1);
                                                        persist();
                                                        status_msg.set("imported HAR/OpenAPI/collection".into());
                                                    }
                                                    Err(e) => {
                                                        status_msg.set(format!("import: {e}"))
                                                    }
                                                }
                                            }
                                            Err(e) => status_msg.set(format!("read: {e}")),
                                        }
                                        is_loading.set(false);
                                    });
                                },
                                "Import HAR / OpenAPI…"
                            }
                        }
                    }
                }

                button {
                    class: "top-link",
                    onclick: move |_| {
                        file_menu_open.set(false);
                        tools_menu_open.set(false);
                        cloud_menu_open.set(false);
                        show_runner.set(true);
                    },
                    "Runner"
                }

                div { class: "menu-file",
                    button {
                        class: if tools_menu_open() || mock_port().is_some() || proxy_port().is_some() {
                            "top-link top-link-active"
                        } else {
                            "top-link"
                        },
                        title: "Tools",
                        onmousedown: move |evt| evt.stop_propagation(),
                        onclick: move |_| {
                            file_menu_open.set(false);
                            cloud_menu_open.set(false);
                            tools_menu_open.set(!tools_menu_open());
                        },
                        "Tools"
                        span { class: "top-caret", "▾" }
                    }
                    if tools_menu_open() {
                        div { class: "menu-dropdown menu-dropdown-wide",
                            onmousedown: move |evt| evt.stop_propagation(),
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    tools_menu_open.set(false);
                                    show_perf.set(true);
                                },
                                "Performance…"
                            }
                            button {
                                class: "menu-item",
                                title: if let Some(p) = mock_port() {
                                    format!("Local mock on http://127.0.0.1:{p}")
                                } else {
                                    "Start local mock from saved examples".into()
                                },
                                onclick: move |_| {
                                    tools_menu_open.set(false);
                                    if mock_port().is_some() {
                                        if let Ok(mut g) = MOCK_HANDLE.lock() {
                                            if let Some(h) = g.take() {
                                                h.stop();
                                            }
                                        }
                                        mock_port.set(None);
                                        status_msg.set("mock stopped".into());
                                    } else {
                                        let routes = routes_from_collections(&collections());
                                        let n_routes = routes.len();
                                        match start_mock(routes, 0) {
                                            Ok(h) => {
                                                let p = h.port;
                                                if let Ok(mut g) = MOCK_HANDLE.lock() {
                                                    *g = Some(h);
                                                }
                                                mock_port.set(Some(p));
                                                status_msg.set(format!(
                                                    "mock http://127.0.0.1:{p} · {n_routes} route(s)"
                                                ));
                                            }
                                            Err(e) => status_msg.set(format!("mock error: {e}")),
                                        }
                                    }
                                },
                                if let Some(p) = mock_port() {
                                    {format!("Stop mock :{p}")}
                                } else {
                                    "Start mock"
                                }
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    tools_menu_open.set(false);
                                    if proxy_port().is_some() {
                                        if let Ok(mut g) = PROXY_HANDLE.lock() {
                                            if let Some(h) = g.take() {
                                                for cap in h.snapshot() {
                                                    let mut hist = history();
                                                    let mut d = RequestDraft::default();
                                                    d.method = match cap.method.as_str() {
                                                        "POST" => crate::core::HttpMethod::Post,
                                                        "PUT" => crate::core::HttpMethod::Put,
                                                        "PATCH" => crate::core::HttpMethod::Patch,
                                                        "DELETE" => crate::core::HttpMethod::Delete,
                                                        _ => crate::core::HttpMethod::Get,
                                                    };
                                                    d.url = cap.url.clone();
                                                    let entry = HistoryEntry::from_send(
                                                        &d,
                                                        &cap.url,
                                                        cap.status,
                                                        cap.elapsed_ms,
                                                    );
                                                    push_history(&mut hist, entry, HISTORY_MAX);
                                                    history.set(hist);
                                                }
                                                h.stop();
                                            }
                                        }
                                        proxy_port.set(None);
                                        status_msg.set("capture proxy stopped".into());
                                        persist();
                                    } else {
                                        match start_capture_proxy(0) {
                                            Ok(h) => {
                                                let p = h.port;
                                                if let Ok(mut g) = PROXY_HANDLE.lock() {
                                                    *g = Some(h);
                                                }
                                                proxy_port.set(Some(p));
                                                status_msg.set(format!(
                                                    "capture proxy http://127.0.0.1:{p} — {}",
                                                    interceptor_limits_blurb()
                                                ));
                                            }
                                            Err(e) => status_msg.set(format!("proxy error: {e}")),
                                        }
                                    }
                                },
                                if proxy_port().is_some() { "Stop capture proxy" } else { "Start capture proxy" }
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    tools_menu_open.set(false);
                                    show_cookies.set(true);
                                },
                                "Cookies…"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    tools_menu_open.set(false);
                                    col_vars_all.set(false);
                                    col_vars_open_id.set(active_collection_id());
                                    show_col_vars.set(true);
                                },
                                "Collection variables…"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    tools_menu_open.set(false);
                                    show_trash.set(true);
                                },
                                "Trash…"
                            }
                        }
                    }
                }

                div { class: "menu-file",
                    button {
                        class: if cloud_menu_open() || app_settings().cloud_sync.enabled {
                            "top-link top-link-active"
                        } else {
                            "top-link"
                        },
                        title: "Git cloud — Pull / Push / status (blitz-cloud)",
                        onmousedown: move |evt| evt.stop_propagation(),
                        onclick: move |_| {
                            file_menu_open.set(false);
                            tools_menu_open.set(false);
                            cloud_menu_open.set(!cloud_menu_open());
                        },
                        "Cloud"
                        span { class: "top-caret", "▾" }
                    }
                    if cloud_menu_open() {
                        div { class: "menu-dropdown menu-dropdown-wide",
                            onmousedown: move |evt| evt.stop_propagation(),
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    let Ok(store) = JsonStore::open_default() else {
                                        status_msg.set("cloud: cannot open store".into());
                                        return;
                                    };
                                    let mut settings = app_settings();
                                    if settings.cloud_sync.remote_url.trim().is_empty() {
                                        settings.cloud_sync.remote_url =
                                            DEFAULT_CLOUD_REMOTE.to_string();
                                    }
                                    match clone_or_ensure(store.root(), &mut settings) {
                                        Ok(r) => {
                                            app_settings.set(settings);
                                            status_msg.set(r.message);
                                            persist_local();
                                        }
                                        Err(e) => status_msg.set(format!("clone: {e}")),
                                    }
                                },
                                "Clone / link remote…"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    is_loading.set(true);
                                    spawn(async move {
                                        let picked = rfd::FileDialog::new()
                                            .set_title("Link existing git workspace folder")
                                            .pick_folder();
                                        let Some(folder) = picked else {
                                            is_loading.set(false);
                                            status_msg.set("link cancelled".into());
                                            return;
                                        };
                                        let Ok(store) = JsonStore::open_default() else {
                                            is_loading.set(false);
                                            status_msg.set("cloud: cannot open store".into());
                                            return;
                                        };
                                        let mut settings = app_settings();
                                        match link_folder(store.root(), &mut settings, &folder) {
                                            Ok((cols, envs, r)) => {
                                                app_settings.set(settings);
                                                if !cols.is_empty() {
                                                    collections.set(cols);
                                                    fold_epoch.set(fold_epoch() + 1);
                                                }
                                                if !envs.is_empty() {
                                                    environments.set(envs);
                                                }
                                                status_msg.set(r.message);
                                                persist_local();
                                            }
                                            Err(e) => status_msg.set(format!("link: {e}")),
                                        }
                                        is_loading.set(false);
                                    });
                                },
                                "Link folder…"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    let Ok(store) = JsonStore::open_default() else {
                                        status_msg.set("cloud: cannot open store".into());
                                        return;
                                    };
                                    let mut settings = app_settings();
                                    settings.cloud_sync.enabled = true;
                                    let r = refresh_status(store.root(), &mut settings);
                                    app_settings.set(settings);
                                    status_msg.set(r.message);
                                    persist_local();
                                },
                                "Sync status"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    let Ok(store) = JsonStore::open_default() else {
                                        status_msg.set("cloud: cannot open store".into());
                                        return;
                                    };
                                    let mut settings = app_settings();
                                    settings.cloud_sync.enabled = true;
                                    match pull_workspace(
                                        store.root(),
                                        &mut settings,
                                        &environments(),
                                    ) {
                                        Ok((cols, envs, r)) => {
                                            app_settings.set(settings);
                                            if !cols.is_empty() {
                                                collections.set(cols);
                                                fold_epoch.set(fold_epoch() + 1);
                                            }
                                            if !envs.is_empty() {
                                                let mut v = vars();
                                                sync_env_vars(&envs, active_env_id(), &mut v);
                                                vars.set(v);
                                                environments.set(envs);
                                            }
                                            status_msg.set(r.message);
                                            persist_local();
                                        }
                                        Err(e) => status_msg.set(format!("pull: {e}")),
                                    }
                                },
                                "Pull"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    let Ok(store) = JsonStore::open_default() else {
                                        status_msg.set("cloud: cannot open store".into());
                                        return;
                                    };
                                    let mut settings = app_settings();
                                    settings.cloud_sync.enabled = true;
                                    if settings.cloud_sync.remote_url.trim().is_empty() {
                                        settings.cloud_sync.remote_url =
                                            DEFAULT_CLOUD_REMOTE.to_string();
                                    }
                                    let active_environment =
                                        find_environment(&environments(), active_env_id())
                                            .map(|e| e.name.clone())
                                            .unwrap_or_else(|| "No Environment".into());
                                    let commit = settings.cloud_sync.commit_message.clone();
                                    let r = push_workspace(
                                        store.root(),
                                        &mut settings,
                                        &collections(),
                                        &environments(),
                                        &active_environment,
                                        Some(commit.as_str()),
                                        &vars(),
                                    );
                                    app_settings.set(settings);
                                    status_msg.set(r.message);
                                    persist_local();
                                },
                                "Push"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    let Ok(store) = JsonStore::open_default() else {
                                        status_msg.set("cloud: cannot open store".into());
                                        return;
                                    };
                                    let settings = app_settings();
                                    let path = settings.cloud_sync.resolved_clone_path(store.root());
                                    match open_in_file_manager(&path) {
                                        Ok(()) => status_msg.set(format!("opened {}", path.display())),
                                        Err(e) => status_msg.set(format!("open folder: {e}")),
                                    }
                                },
                                "Open folder"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    show_env_manager.set(false);
                                    show_settings.set(true);
                                },
                                "Cloud settings…"
                            }
                        }
                    }
                }
                } // top-nav

                span { class: "spacer" }

                div { class: "top-end",
                    if app_settings().cloud_sync.enabled {
                        span {
                            class: "live-chip live-cloud",
                            title: "{app_settings().cloud_sync.last_status}",
                            onclick: move |_| cloud_menu_open.set(!cloud_menu_open()),
                            {
                                let cs = app_settings().cloud_sync;
                                let branch = if cs.last_branch.is_empty() {
                                    "cloud".to_string()
                                } else {
                                    cs.last_branch.clone()
                                };
                                let st = if cs.last_status.is_empty() {
                                    "on".to_string()
                                } else if cs.last_status.to_lowercase().contains("fail")
                                    || cs.last_status.to_lowercase().contains("error")
                                    || cs.last_status.to_lowercase().contains("conflict")
                                    || cs.last_status.to_lowercase().contains("auth")
                                {
                                    "err".to_string()
                                } else if cs.last_status.to_lowercase().contains("push")
                                    || cs.last_status.to_lowercase().contains("synced")
                                    || cs.last_status.to_lowercase().contains("pulled")
                                {
                                    "ok".to_string()
                                } else {
                                    "…".to_string()
                                };
                                format!("{branch} · {st}")
                            }
                        }
                    }
                    if mock_port().is_some() || proxy_port().is_some() {
                        span { class: "top-live",
                            if let Some(p) = mock_port() {
                                span { class: "live-chip live-mock", title: "Mock running", "Mock :{p}" }
                            }
                            if let Some(p) = proxy_port() {
                                span { class: "live-chip live-proxy", title: "Capture proxy running", "Proxy :{p}" }
                            }
                        }
                    }
                    button {
                        class: if show_vars() { "top-icon top-icon-on" } else { "top-icon" },
                        title: "Variables (environment, collection, global, vault; local stays on this machine)",
                        onclick: move |_| show_vars.set(!show_vars()),
                        "Vars"
                    }
                    button {
                        class: if show_snippets() { "top-icon top-icon-on" } else { "top-icon" },
                        title: "Code snippets",
                        onclick: move |_| show_snippets.set(!show_snippets()),
                        "</>"
                    }
                    EnvSelector {
                        environments: environments(),
                        active_id: active_env_id(),
                        on_change: move |id| {
                            switch_to_env(id);
                        },
                        on_sync: move |id| {
                            let envs = environments();
                            let Some(env) = envs.iter().find(|e| e.id == id) else {
                                status_msg.set("sync: env not found".into());
                                return;
                            };
                            let env_name = env.name.clone();
                            if env_name.trim().eq_ignore_ascii_case("Local") {
                                status_msg.set(
                                    "Local environment is not synced — pick Development/Production (or another team env)"
                                        .into(),
                                );
                                return;
                            }
                            // Activate this env first (UI + var map).
                            active_env_id.set(Some(id));
                            editing_env_id.set(Some(id));
                            let mut v = vars();
                            sync_env_vars(&envs, Some(id), &mut v);
                            vars.set(v);

                            let Ok(store) = JsonStore::open_default() else {
                                status_msg.set("sync: cannot open store".into());
                                return;
                            };
                            let mut settings = app_settings();
                            // First Sync turns cloud on with canonical remote (no Settings hunt).
                            if !settings.cloud_sync.enabled {
                                settings.cloud_sync.enabled = true;
                                if settings.cloud_sync.remote_url.trim().is_empty() {
                                    settings.cloud_sync.remote_url =
                                        DEFAULT_CLOUD_REMOTE.to_string();
                                }
                                status_msg.set(format!(
                                    "cloud sync enabled → syncing {env_name}…"
                                ));
                            }
                            // Ensure default trio present before sync.
                            let mut envs_mut = envs;
                            let _ = ensure_default_environments(&mut envs_mut);
                            environments.set(envs_mut.clone());

                            match sync_environment(
                                store.root(),
                                &mut settings,
                                &collections(),
                                &envs_mut,
                                &env_name,
                                &vars(),
                            ) {
                                Ok((cols, loaded_envs, result)) => {
                                    app_settings.set(settings.clone());
                                    if !cols.is_empty() {
                                        collections.set(cols);
                                        fold_epoch.set(fold_epoch() + 1);
                                    }
                                    let final_envs = if loaded_envs.is_empty() {
                                        envs_mut
                                    } else {
                                        // Remap active id if import assigned new UUIDs.
                                        let pick = loaded_envs
                                            .iter()
                                            .find(|e| e.name == env_name)
                                            .map(|e| e.id)
                                            .unwrap_or(id);
                                        active_env_id.set(Some(pick));
                                        editing_env_id.set(Some(pick));
                                        let mut v = vars();
                                        sync_env_vars(&loaded_envs, Some(pick), &mut v);
                                        vars.set(v);
                                        environments.set(loaded_envs.clone());
                                        loaded_envs
                                    };
                                    status_msg.set(result.message.clone());

                                    // Persist local snapshot + cloud status (Save already committed; no push).
                                    let active_environment = env_name.clone();
                                    let vault = vars();
                                    let _ = vault.vault.save(store.root());
                                    let snap = AppSnapshot {
                                        collections: collections(),
                                        variables: vault,
                                        active_environment,
                                        environments: final_envs,
                                        active_environment_id: active_env_id(),
                                        history: history(),
                                        cookies: cookies(),
                                        active_collection_id: active_collection_id(),
                                        active_request_id: active_request_id(),
                                        settings,
                                        trash: trash(),
                                        open_tabs: open_tabs(),
                                        responses: responses(),
                                        show_vars: show_vars(),
                                        show_settings: show_settings(),
                                        show_runner: show_runner(),
                                        show_perf: show_perf(),
                                        show_env_manager: show_env_manager(),
                                        show_col_vars: show_col_vars(),
                                    };
                                    let _ = store.save(&snap);
                                }
                                Err(e) => {
                                    app_settings.set(settings);
                                    status_msg.set(format!("sync: {e}"));
                                }
                            }
                        },
                        on_manage: move |_| {
                            show_env_manager.set(true);
                            editing_env_id.set(active_env_id());
                        },
                    }
                    ThemeSelector {
                        theme: app_settings().ui.theme,
                        on_change: move |t: UiTheme| {
                            let mut s = app_settings();
                            s.ui.theme = t;
                            s.ui = s.ui.clamped();
                            app_settings.set(s);
                            persist();
                        },
                    }
                    button {
                        class: if show_settings() {
                            "top-icon top-icon-on"
                        } else {
                            "top-icon"
                        },
                        title: "Settings & preferences",
                        aria_label: "Settings",
                        onclick: move |_| {
                            show_env_manager.set(false);
                            show_settings.set(!show_settings());
                        },
                        span { class: "action-icon action-icon-sliders", aria_hidden: "true" }
                    }
                }
            }

            if show_vars() {
                div { class: "vars-page",
                    div { class: "vars-page-head",
                        h2 { "Variables" }
                        button {
                            class: "btn btn-ghost",
                            onclick: move |_| {
                                show_vars.set(false);
                                persist();
                            },
                            "Done"
                        }
                    }
                    div { class: "vars-page-inner",
                        p { class: "hint",
                            "Resolve order: local → environment → collection → global. Vault via {{{{vault:key}}}}."
                        }

                        // —— Environment ——
                        div { class: "vars-section",
                            div { class: "vars-section-head",
                                h4 { "Environment" }
                                button {
                                    class: "btn btn-ghost",
                                    onclick: move |_| {
                                        show_vars.set(false);
                                        show_env_manager.set(true);
                                        editing_env_id.set(active_env_id());
                                    },
                                    "Manage…"
                                }
                            }
                            {
                                let envs = environments();
                                let eid = active_env_id();
                                match eid.and_then(|id| envs.iter().find(|e| e.id == id).cloned()) {
                                    Some(env) => {
                                        let env_id = env.id;
                                        let env_name = env.name.clone();
                                        let values_len = env.values.len();
                                        rsx! {
                                            p { class: "hint", "Active: {env_name}" }
                                            table { class: "kv-table",
                                                thead {
                                                    tr {
                                                        th { "Variable" }
                                                        th { "Initial" }
                                                        th { "Current" }
                                                    }
                                                }
                                                tbody {
                                                    // Render only rows that exist - avoid re-cloning via values cloning per-keystroke
                                                    for i in 0..values_len {
                                                        tr {
                                                            td {
                                                                input {
                                                                    value: "{env.values.get(i).map(|r| r.key.clone()).unwrap_or_default()}",
                                                                    oninput: {
                                                                        let env_id = env_id;
                                                                        let idx = i;
                                                                        move |evt: Event<FormData>| {
                                                                            let mut envs = environments();
                                                                            if let Some(e) = envs.iter_mut().find(|e| e.id == env_id) {
                                                                                if let Some(r) = e.values.get_mut(idx) {
                                                                                    r.key = evt.value();
                                                                                }
                                                                            }
                                                                            environments.set(envs);
                                                                            // Sync vars lazily - don't sync on every keystroke
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                            td {
                                                                input {
                                                                    value: "{env.values.get(i).map(|r| r.initial_value.clone()).unwrap_or_default()}",
                                                                    oninput: {
                                                                        let env_id = env_id;
                                                                        let idx = i;
                                                                        move |evt: Event<FormData>| {
                                                                            let mut envs = environments();
                                                                            if let Some(e) = envs.iter_mut().find(|e| e.id == env_id) {
                                                                                if let Some(r) = e.values.get_mut(idx) {
                                                                                    r.initial_value = evt.value();
                                                                                }
                                                                            }
                                                                            environments.set(envs);
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                            td {
                                                                input {
                                                                    value: "{env.values.get(i).map(|r| r.current_value.clone()).unwrap_or_default()}",
                                                                    oninput: {
                                                                        let env_id = env_id;
                                                                        let idx = i;
                                                                        move |evt: Event<FormData>| {
                                                                            let mut envs = environments();
                                                                            if let Some(e) = envs.iter_mut().find(|e| e.id == env_id) {
                                                                                if let Some(r) = e.values.get_mut(idx) {
                                                                                    r.current_value = evt.value();
                                                                                }
                                                                            }
                                                                            environments.set(envs);
                                                                            if active_env_id() == Some(env_id) {
                                                                                let mut v = vars();
                                                                                sync_env_vars(&environments(), Some(env_id), &mut v);
                                                                                vars.set(v);
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    None => rsx! {
                                        p { class: "hint",
                                            "No environment selected — pick one in the top bar or Manage…"
                                        }
                                    },
                                }
                            }
                        }

                        // —— Collection ——
                        div { class: "vars-section",
                            div { class: "vars-section-head",
                                h4 { "Collection" }
                                button {
                                    class: "btn btn-ghost",
                                    onclick: move |_| {
                                        show_vars.set(false);
                                        col_vars_all.set(true);
                                        col_vars_open_id.set(active_collection_id());
                                        show_col_vars.set(true);
                                    },
                                    "All collections…"
                                }
                            }
                            {
                                let cols = collections();
                                let active_id = active_collection_id();
                                let col = active_id.and_then(|id| cols.iter().find(|c| c.id == id));
                                match col {
                                    Some(c) => {
                                        let cid = c.id;
                                        let rows = c.variables.clone();
                                        let name = c.name.clone();
                                        rsx! {
                                            p { class: "hint", "Active: {name}" }
                                            ColVarEditor {
                                                collection_id: cid,
                                                rows,
                                                sync_active: true,
                                                collections,
                                                vars,
                                            }
                                        }
                                    }
                                    None => rsx! {
                                        p { class: "hint", "Select a collection in the sidebar first." }
                                    },
                                }
                            }
                        }

                        // —— Global ——
                        div { class: "vars-section",
                            h4 { "Global" }
                            MapVarEditor {
                                rows: sorted_map_rows(&vars().global),
                                on_change: move |rows: Vec<(String, String)>| {
                                    let mut v = vars();
                                    v.global = rows_to_map(rows);
                                    vars.set(v);
                                },
                            }
                        }

                        // —— Vault ——
                        div { class: "vars-section",
                            h4 { "Vault" }
                            p { class: "hint", "Stored in vault.json — use {{{{vault:key}}}}." }
                            MapVarEditor {
                                rows: sorted_map_rows(&vars().vault.entries),
                                on_change: move |rows: Vec<(String, String)>| {
                                    let mut v = vars();
                                    v.vault.entries = rows_to_map(rows);
                                    vars.set(v);
                                },
                            }
                        }

                        // —— Local (read-only) ——
                        div { class: "vars-section",
                            h4 { "Local" }
                            p { class: "hint", "Script / request-local — machine only; never git synced." }
                            {
                                let local = sorted_map_rows(&vars().local);
                                if local.is_empty() {
                                    rsx! { p { class: "hint", "(empty)" } }
                                } else {
                                    rsx! {
                                        for (k, val) in local.iter() {
                                            div { class: "eye-row", code { "{k}" } " = " code { "{val}" } }
                                        }
                                    }
                                }
                            }
                        }

                        div { class: "kv-actions",
                            button {
                                class: "btn btn-send",
                                onclick: move |_| {
                                    show_vars.set(false);
                                    persist();
                                },
                                "Done"
                            }
                        }
                    }
                }
            }

            if show_palette() {
                ModalShell {
                    title: "Command palette".to_string(),
                    class: "palette-modal".to_string(),
                    on_close: move |_| show_palette.set(false),
                    div { class: "modal-body",
                        button {
                            class: if sending() { "btn btn-ghost btn-cancel" } else { "btn btn-ghost" },
                            onclick: move |_| { do_send(()); show_palette.set(false); },
                            if sending() { "Cancel request" } else { "Send request" }
                        }
                        button { class: "btn btn-ghost", onclick: move |_| { do_save(()); show_palette.set(false); }, "Save" }
                        button { class: "btn btn-ghost", onclick: move |_| { do_new_tab(()); show_palette.set(false); }, "New request" }
                        button { class: "btn btn-ghost", onclick: move |_| { show_runner.set(true); show_palette.set(false); }, "Runner" }
                        button { class: "btn btn-ghost", onclick: move |_| { console_open.set(true); show_palette.set(false); }, "Console" }
                        button {
                            class: "btn btn-ghost",
                            onclick: move |_| {
                                show_env_manager.set(false);
                                show_settings.set(true);
                                show_palette.set(false);
                            },
                            "Settings"
                        }
                    }
                }
            }

            if let Some(del) = pending_delete() {
                {
                    let del_id = del.id;
                    let del_label = del.label.clone();
                    rsx! {
                        ModalShell {
                            title: "Delete item?".to_string(),
                            on_close: move |_| pending_delete.set(None),
                            div { class: "modal-body",
                                p { "Delete “{del_label}”? This cannot be undone." }
                            }
                            div { class: "modal-actions",
                                button {
                                    class: "btn btn-ghost",
                                    onclick: move |_| pending_delete.set(None),
                                    "Cancel"
                                }
                                button {
                                    class: "btn btn-ghost danger",
                                    onclick: move |_| {
                                        pending_delete.set(None);
                                        let mut cols = collections();
                                        let mut tr = trash();
                                        let mut col_id = active_collection_id();
                                        let mut req_id = active_request_id();
                                        let mut d = draft();
                                        if let Some(msg) = execute_tree_delete(
                                            del_id,
                                            &mut cols,
                                            &mut tr,
                                            &mut col_id,
                                            &mut req_id,
                                            &mut d,
                                        ) {
                                            status_msg.set(msg);
                                        }
                                        trash.set(tr);
                                        collections.set(cols);
                                        active_collection_id.set(col_id);
                                        active_request_id.set(req_id);
                                        draft.set(d);
                                        persist();
                                    },
                                    "Delete"
                                }
                            }
                        }
                    }
                }
            }

            if show_trash() {
                ModalShell {
                    title: "Trash".to_string(),
                    on_close: move |_| show_trash.set(false),
                    div { class: "modal-body",
                        if trash().is_empty() {
                            p { class: "hint", "Empty." }
                        } else {
                            for c in trash() {
                                div { class: "trash-row",
                                    span { "{c.name}" }
                                    button {
                                        class: "btn btn-ghost",
                                        onclick: move |_| {
                                            let mut cols = collections();
                                            let mut tr = trash();
                                            if restore_from_trash(&mut cols, &mut tr, c.id) {
                                                collections.set(cols);
                                                trash.set(tr);
                                                persist();
                                            }
                                        },
                                        "Restore"
                                    }
                                    button {
                                        class: "btn btn-ghost",
                                        onclick: move |_| {
                                            let mut tr = trash();
                                            if purge_from_trash(&mut tr, c.id) {
                                                trash.set(tr);
                                                persist();
                                                status_msg.set("permanently deleted".into());
                                            }
                                        },
                                        "Delete forever"
                                    }
                                }
                            }
                        }
                    }
                    div { class: "modal-actions",
                        button { class: "btn btn-ghost", onclick: move |_| show_trash.set(false), "Close" }
                    }
                }
            }

            if show_snippets() {
                ModalShell {
                    title: "Code snippet".to_string(),
                    on_close: move |_| show_snippets.set(false),
                    div { class: "modal-body",
                        select {
                            value: "{snippet_lang()}",
                            onchange: move |evt| snippet_lang.set(evt.value()),
                            for lang in snippet_languages() {
                                option { value: "{lang}", "{lang}" }
                            }
                        }
                        pre { class: "response-body", "{generate_snippet(&snippet_lang(), &draft())}" }
                    }
                    div { class: "modal-actions",
                        button { class: "btn btn-ghost", onclick: move |_| show_snippets.set(false), "Close" }
                    }
                }
            }

            if show_runner() {
                RunnerPanel {
                    collections: collections(),
                    selected_id: active_collection_id(),
                    running: runner_running(),
                    report: runner_report(),
                    on_event: move |evt| match evt {
                        RunnerEvent::Close => show_runner.set(false),
                        RunnerEvent::Run {
                            collection_id,
                            iterations,
                            delay_ms,
                            data_file,
                        } => {
                            if runner_running() {
                                return;
                            }
                            runner_running.set(true);
                            runner_report.set(None);
                            let cols = collections();
                            let Some(col) = cols.into_iter().find(|c| c.id == collection_id) else {
                                runner_running.set(false);
                                status_msg.set("collection not found".into());
                                return;
                            };
                            let mut ctx = vars();
                            ctx.collection = col.variables_map();
                            if let Some(env) = find_environment(&environments(), active_env_id()) {
                                ctx.environment = env.to_map();
                            } else {
                                ctx.environment.clear();
                            }
                            let env_id = active_env_id();
                            let col_id = col.id;
                            let mut jar = cookies();
                            let settings = app_settings();
                            let cfg = RunnerConfig {
                                iterations,
                                delay_ms,
                                data_file,
                            };
                            spawn(async move {
                                let report = tokio::task::spawn_blocking(move || {
                                    let rep = run_collection_with_config(
                                        &col,
                                        &mut ctx,
                                        &mut jar,
                                        Some(&settings),
                                        None,
                                        &cfg,
                                    );
                                    (rep, jar, ctx)
                                })
                                .await;
                                match report {
                                    Ok((rep, jar, mut ctx)) => {
                                        cookies.set(jar);
                                        if let Some(id) = env_id {
                                            let mut envs = environments();
                                            if let Some(env) =
                                                envs.iter_mut().find(|e| e.id == id)
                                            {
                                                apply_environment_map(env, &ctx.environment);
                                            }
                                            environments.set(envs);
                                        } else if !ctx.environment.is_empty() {
                                            status_msg.set(
                                                "no active environment — epm.environment.set ignored"
                                                    .into(),
                                            );
                                            ctx.environment.clear();
                                        }
                                        let mut cols = collections();
                                        if let Some(c) = cols.iter_mut().find(|c| c.id == col_id) {
                                            apply_collection_map(c, &ctx.collection);
                                        }
                                        collections.set(cols);
                                        let vault = vars().vault.clone();
                                        ctx.vault = vault;
                                        vars.set(ctx);
                                        let mut lines = console_lines();
                                        for m in &rep.console {
                                            push_console(&mut lines, ConsoleLine::info(m.clone()));
                                        }
                                        console_lines.set(lines);
                                        console_open.set(true);
                                        status_msg.set(format!(
                                            "runner: pass={} fail={}",
                                            rep.passed_count(),
                                            rep.failed_count()
                                        ));
                                        runner_report.set(Some(rep));
                                    }
                                    Err(e) => {
                                        status_msg.set(format!("runner join: {e}"));
                                    }
                                }
                                runner_running.set(false);
                                persist();
                            });
                        }
                    },
                }
            }

            if show_perf() {
                ModalShell {
                    title: "Performance (minimal VUs)".to_string(),
                    class: "runner-modal".to_string(),
                    on_close: move |_| show_perf.set(false),
                    div { class: "modal-body",
                        p { class: "hint", "Concurrent VUs hit active collection for a duration. Reuses HTTP send pipeline." }
                        label { class: "hint", "VUs" }
                        input {
                            r#type: "number",
                            value: "{perf_vus()}",
                            oninput: move |evt| {
                                if let Ok(n) = evt.value().parse::<u32>() {
                                    perf_vus.set(n.max(1).min(50));
                                }
                            }
                        }
                        label { class: "hint", "Duration (seconds)" }
                        input {
                            r#type: "number",
                            value: "{perf_secs()}",
                            oninput: move |evt| {
                                if let Ok(n) = evt.value().parse::<u64>() {
                                    perf_secs.set(n.max(1).min(120));
                                }
                            }
                        }
                        if let Some(r) = perf_report() {
                            pre { class: "console-line",
                                "requests={r.total_requests} errors={r.errors}\navg_ms={r.avg_latency_ms():.1} min={r.latency_min_ms} max={r.latency_max_ms}\nvus={r.vus} duration_s={r.duration_secs}"
                            }
                        }
                        div { class: "kv-actions",
                            button {
                                class: "btn btn-send",
                                disabled: perf_running(),
                                onclick: move |_| {
                                    let Some(id) = active_collection_id() else {
                                        status_msg.set("select a collection".into());
                                        return;
                                    };
                                    let cols = collections();
                                    let Some(col) = cols.into_iter().find(|c| c.id == id) else {
                                        status_msg.set("collection not found".into());
                                        return;
                                    };
                                    let mut ctx = vars();
                                    if let Some(env) = find_environment(&environments(), active_env_id()) {
                                        ctx.environment = env.to_map();
                                    }
                                    let vus = perf_vus();
                                    let secs = perf_secs();
                                    perf_running.set(true);
                                    perf_report.set(None);
                                    spawn(async move {
                                        let report = tokio::task::spawn_blocking(move || {
                                            run_performance(
                                                &col,
                                                &ctx,
                                                vus,
                                                Duration::from_secs(secs),
                                            )
                                        })
                                        .await;
                                        match report {
                                            Ok(r) => {
                                                status_msg.set(format!(
                                                    "perf: {} req, {} err, avg {:.0}ms",
                                                    r.total_requests,
                                                    r.errors,
                                                    r.avg_latency_ms()
                                                ));
                                                perf_report.set(Some(r));
                                            }
                                            Err(e) => status_msg.set(format!("perf join: {e}")),
                                        }
                                        perf_running.set(false);
                                    });
                                },
                                if perf_running() {
                                    div { class: "loading-row",
                                        Spinner { }
                                        span { class: "loading-text", "Running…" }
                                    }
                                } else {
                                    "Start"
                                }
                            }
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| show_perf.set(false),
                                "Close"
                            }
                        }
                        if perf_running() && perf_report().is_none() {
                            div { class: "loading-row", Spinner { } span { class: "loading-text", "Executing load test…" } }
                        }
                    }
                }
            }

            if !show_settings() {
            if show_env_manager() {
                div { class: "env-manager-overlay",
                    EnvManager {
                        environments: environments(),
                        active_id: active_env_id(),
                        editing_id: editing_env_id(),
                        on_event: on_env_manager,
                    }
                }
            } else {
                div { class: "sidebar-slot",
                    Sidebar {
                        key: "{fold_epoch()}",
                        collections: collections(),
                        active_collection_id: active_collection_id(),
                        active_request_id: active_request_id(),
                        history: history(),
                        history_collapsed: app_settings().ui.history_collapsed,
                        on_event: on_sidebar,
                    }
                    div {
                        class: if sidebar_drag().is_some() {
                            "sidebar-resize active"
                        } else {
                            "sidebar-resize"
                        },
                        title: "Drag to resize sidebar",
                        onmousedown: move |evt| {
                            evt.prevent_default();
                            evt.stop_propagation();
                            sidebar_drag.set(Some((
                                evt.client_coordinates().x,
                                app_settings().ui.sidebar_width_px as f64,
                            )));
                        },
                    }
                }
            }
            }

            main {
                class: if show_settings() {
                    "workbench workbench-settings"
                } else {
                    workbench_class
                },
                style: "{workbench_style}",
                onmousemove: move |evt| {
                    if show_settings() {
                        return;
                    }
                    let Some(drag) = pane_drag() else {
                        return;
                    };
                    let y = evt.client_coordinates().y;
                    match drag {
                        PaneDrag::Request { start_y, start_h } => {
                            let delta = (y - start_y).round() as i32;
                            let new_h = UiSettings::clamp_request_pane_height(
                                (start_h as i32 + delta).max(0) as u32,
                            );
                            pane_h.set(new_h as f64);
                        }
                        PaneDrag::Console { start_y, start_h } => {
                            // Dragging the top edge up grows the console.
                            let delta = (start_y - y).round() as i32;
                            let new_h = UiSettings::clamp_console_height(
                                (start_h as i32 + delta).max(0) as u32,
                            );
                            console_h.set(new_h as f64);
                        }
                    }
                },
                onmouseup: move |_| {
                    if show_settings() {
                        return;
                    }
                    if pane_drag().is_some() {
                        pane_drag.set(None);
                        let mut s = app_settings();
                        s.ui.request_pane_height_px = pane_h() as u32;
                        s.ui.console_height_px = console_h() as u32;
                        app_settings.set(s);
                        persist();
                    }
                },
                onmouseleave: move |_| {
                    if show_settings() {
                        return;
                    }
                    if pane_drag().is_some() {
                        pane_drag.set(None);
                        persist();
                    }
                },
                if show_settings() {
                    SettingsPanel {
                        settings: app_settings(),
                        data_root: data_root(),
                        on_event: on_settings_event,
                    }
                } else {
                div { class: "tab-strip",
                    input {
                        class: "tab-name",
                        value: "{draft().name}",
                        oninput: move |evt| {
                            let mut d = draft();
                            d.name = evt.value();
                            draft.set(d);
                        },
                    }
                    button {
                        class: "btn btn-ghost",
                        title: "Save (⌘S)",
                        onclick: move |_| do_save(()),
                        "Save"
                    }
                    for t in open_tabs() {
                        {
                            let tid = t.id;
                            let tname = request_tab_display_name(&collections(), tid);
                            let active = draft().id == tid;
                            rsx! {
                                div {
                                    class: if active { "tab active" } else { "tab" },
                                    role: "button",
                                    tabindex: "0",
                                    onclick: move |_| {
                                        let mut tabs = open_tabs();
                                        let cur = draft();
                                        if let Some(slot) = tabs.iter_mut().find(|x| x.id == cur.id) {
                                            *slot = cur;
                                        }
                                        if let Some(found) = tabs.iter().find(|x| x.id == tid).cloned() {
                                            draft.set(found.clone());
                                            active_request_id.set(Some(tid));
                                            if let Some((col_id, _)) = find_request_across(&collections(), tid) {
                                                active_collection_id.set(Some(col_id));
                                                if let Some(col) = collections().iter().find(|c| c.id == col_id) {
                                                    let mut v = vars();
                                                    v.collection = col.variables_map();
                                                    vars.set(v);
                                                }
                                            }
                                            // Restore response from cache
                                            let resp_map = responses();
                                            if let Some(cached) = resp_map.get(&tid).cloned() {
                                                response.set(Some(cached));
                                            } else {
                                                response.set(None);
                                            }
                                        }
                                        open_tabs.set(tabs);
                                    },
                                    span { class: "tab-label", "{tname}" }
                                    button {
                                        class: "tab-close",
                                        r#type: "button",
                                        title: "Close tab",
                                        onclick: move |evt| {
                                            evt.stop_propagation();
                                            do_close_tab(tid);
                                        },
                                        "×"
                                    }
                                }
                            }
                        }
                    }
                    button {
                        class: "tab",
                        title: "New request tab (⌘T)",
                        onclick: move |_| do_new_tab(()),
                        "+"
                    }
                }

                div { class: "workbench-split",
                RequestBuilder {
                    draft: draft(),
                    vars: vars(),
                    on_change: move |d| draft.set(d),
                    on_send: move |_| do_send(()),
                    on_var_edit: move |evt: RequestVarEditEvent| {
                        match evt {
                            RequestVarEditEvent::Save {
                                name,
                                scope,
                                value,
                                ..
                            } => {
                                match scope {
                                    VarResolveScope::Environment => {
                                        let mut envs = environments();
                                        if let Some(id) = active_env_id() {
                                            if let Some(env) =
                                                envs.iter_mut().find(|e| e.id == id)
                                            {
                                                apply_env_var_edit(env, &name, &value);
                                            }
                                        }
                                        environments.set(envs.clone());
                                        let mut v = vars();
                                        sync_env_vars(&envs, active_env_id(), &mut v);
                                        vars.set(v);
                                    }
                                    VarResolveScope::Collection => {
                                        let mut cols = collections();
                                        if let Some(id) = active_collection_id() {
                                            if let Some(col) =
                                                cols.iter_mut().find(|c| c.id == id)
                                            {
                                                apply_collection_var_edit(col, &name, &value);
                                                let mut v = vars();
                                                v.collection = col.variables_map();
                                                vars.set(v);
                                            }
                                        }
                                        collections.set(cols);
                                    }
                                    VarResolveScope::Global
                                    | VarResolveScope::Local
                                    | VarResolveScope::Vault => {
                                        let mut v = vars();
                                        v.apply_edit(scope, &name, &value);
                                        vars.set(v);
                                    }
                                    VarResolveScope::Unresolved => {}
                                }
                                persist();
                                status_msg.set(format!("updated {name}"));
                            }
                            RequestVarEditEvent::AddToEnvironment { name, value } => {
                                let mut envs = environments();
                                let Some(id) = active_env_id() else {
                                    status_msg.set("no active environment".into());
                                    return;
                                };
                                if let Some(env) = envs.iter_mut().find(|e| e.id == id) {
                                    apply_env_var_edit(env, &name, &value);
                                } else {
                                    status_msg.set("no active environment".into());
                                    return;
                                }
                                environments.set(envs.clone());
                                let mut v = vars();
                                sync_env_vars(&envs, Some(id), &mut v);
                                vars.set(v);
                                persist();
                                status_msg.set(format!("added {name} to environment"));
                            }
                        }
                    },
                    on_protocol_action: move |action: String| {
                        let req = draft();
                        match action.as_str() {
                            "ws_connect" => {
                                if let Ok(mut g) = WS_SESSION.lock() {
                                    if let Some(old) = g.take() {
                                        old.close();
                                    }
                                }
                                protocol_log.set(vec![format!("ws → {}", req.url)]);
                                match ws_connect_session(&req.url) {
                                    Ok(sess) => {
                                        if let Ok(mut g) = WS_SESSION.lock() {
                                            *g = Some(sess);
                                        }
                                        ws_connected.set(true);
                                        status_msg.set("websocket connecting…".into());
                                        // Poll events briefly into log
                                        spawn(async move {
                                            for _ in 0..40 {
                                                tokio::time::sleep(Duration::from_millis(250)).await;
                                                let snap = WS_SESSION
                                                    .lock()
                                                    .ok()
                                                    .and_then(|g| g.as_ref().map(|s| s.events_snapshot()))
                                                    .unwrap_or_default();
                                                let lines: Vec<String> = snap
                                                    .iter()
                                                    .map(|e| format!("{e:?}"))
                                                    .collect();
                                                protocol_log.set(lines);
                                                let still = WS_SESSION.lock().ok().map(|g| g.is_some()).unwrap_or(false);
                                                if !still {
                                                    ws_connected.set(false);
                                                    break;
                                                }
                                            }
                                        });
                                    }
                                    Err(e) => {
                                        ws_connected.set(false);
                                        protocol_log.set(vec![e]);
                                    }
                                }
                            }
                            "ws_send" => {
                                let msg = req.ws_message.clone();
                                if let Ok(g) = WS_SESSION.lock() {
                                    if let Some(s) = g.as_ref() {
                                        if let Err(e) = s.send_text(msg) {
                                            protocol_log.set(vec![e]);
                                        } else {
                                            let mut lines = protocol_log();
                                            lines.push("→ sent".into());
                                            protocol_log.set(lines);
                                        }
                                    }
                                }
                            }
                            "ws_disconnect" => {
                                if let Ok(mut g) = WS_SESSION.lock() {
                                    if let Some(s) = g.take() {
                                        s.close();
                                    }
                                }
                                ws_connected.set(false);
                                let mut lines = protocol_log();
                                lines.push("disconnected".into());
                                protocol_log.set(lines);
                            }
                            "mqtt_publish" => {
                                sending.set(true);
                                let url = req.url.clone();
                                let cid = req.mqtt_client_id.clone();
                                let topic = req.mqtt_topic.clone();
                                let body = req.body.clone();
                                spawn(async move {
                                    let r = tokio::task::spawn_blocking(move || {
                                        mqtt_publish(&url, &cid, &topic, &body)
                                    })
                                    .await
                                    .unwrap_or_else(|e| crate::core::MqttActionResult {
                                        ok: false,
                                        messages: vec![],
                                        error: Some(e.to_string()),
                                        elapsed_ms: 0,
                                    });
                                    let mut lines = r.messages;
                                    if let Some(e) = r.error {
                                        lines.push(e);
                                    }
                                    lines.push(format!("{} ms", r.elapsed_ms));
                                    protocol_log.set(lines);
                                    sending.set(false);
                                });
                            }
                            "mqtt_subscribe" => {
                                sending.set(true);
                                let url = req.url.clone();
                                let cid = req.mqtt_client_id.clone();
                                let topic = req.mqtt_topic.clone();
                                spawn(async move {
                                    let r = tokio::task::spawn_blocking(move || {
                                        mqtt_subscribe_once(
                                            &url,
                                            &cid,
                                            &topic,
                                            Duration::from_secs(8),
                                        )
                                    })
                                    .await
                                    .unwrap_or_else(|e| crate::core::MqttActionResult {
                                        ok: false,
                                        messages: vec![],
                                        error: Some(e.to_string()),
                                        elapsed_ms: 0,
                                    });
                                    let mut lines = r.messages;
                                    if let Some(e) = r.error {
                                        lines.push(e);
                                    }
                                    protocol_log.set(lines);
                                    sending.set(false);
                                });
                            }
                            "mcp_list" => {
                                sending.set(true);
                                let cmd = req.mcp_command.clone();
                                let args = split_mcp_args(&req.mcp_args);
                                spawn(async move {
                                    let result = tokio::task::spawn_blocking(move || {
                                        mcp_list_tools(&cmd, &args)
                                    })
                                    .await;
                                    match result {
                                        Ok(Ok(tools)) => {
                                            let lines: Vec<String> = tools
                                                .into_iter()
                                                .map(|t| format!("{} — {}", t.name, t.description))
                                                .collect();
                                            protocol_log.set(if lines.is_empty() {
                                                vec!["(no tools)".into()]
                                            } else {
                                                lines
                                            });
                                        }
                                        Ok(Err(e)) => protocol_log.set(vec![format!("mcp: {e}")]),
                                        Err(e) => protocol_log.set(vec![format!("mcp join: {e}")]),
                                    }
                                    sending.set(false);
                                });
                            }
                            "mcp_call" => {
                                sending.set(true);
                                let cmd = req.mcp_command.clone();
                                let args = split_mcp_args(&req.mcp_args);
                                let tool = req.mcp_tool_name.clone();
                                let args_json = req.mcp_tool_args_json.clone();
                                spawn(async move {
                                    let result = tokio::task::spawn_blocking(move || {
                                        let v: serde_json::Value = serde_json::from_str(&args_json)
                                            .unwrap_or_else(|_| serde_json::json!({}));
                                        mcp_call_tool(&cmd, &args, &tool, v)
                                    })
                                    .await;
                                    match result {
                                        Ok(Ok(body)) => protocol_log.set(vec![body]),
                                        Ok(Err(e)) => protocol_log.set(vec![e]),
                                        Err(e) => protocol_log.set(vec![format!("join: {e}")]),
                                    }
                                    sending.set(false);
                                });
                            }
                            "socketio_session" => {
                                sending.set(true);
                                let url = req.url.clone();
                                let ev = req.socketio_event.clone();
                                let body = req.body.clone();
                                spawn(async move {
                                    let result = tokio::task::spawn_blocking(move || {
                                        socketio_session(&url, &ev, &body, &ev, 3_000)
                                    })
                                    .await;
                                    match result {
                                        Ok(Ok(lines)) => protocol_log.set(lines),
                                        Ok(Err(e)) => protocol_log.set(vec![e]),
                                        Err(e) => protocol_log.set(vec![format!("join: {e}")]),
                                    }
                                    sending.set(false);
                                });
                            }
                            "sse_listen" => {
                                sending.set(true);
                                let url = req.url.clone();
                                let headers: Vec<(String, String)> = req
                                    .headers
                                    .iter()
                                    .filter(|h| h.enabled && !h.key.is_empty())
                                    .map(|h| (h.key.clone(), h.value.clone()))
                                    .collect();
                                spawn(async move {
                                    let result = tokio::task::spawn_blocking(move || {
                                        sse_listen(&url, &headers, 20, 15_000)
                                    })
                                    .await;
                                    match result {
                                        Ok(Ok((lines, _))) => protocol_log.set(lines),
                                        Ok(Err(e)) => protocol_log.set(vec![e]),
                                        Err(e) => protocol_log.set(vec![format!("join: {e}")]),
                                    }
                                    sending.set(false);
                                });
                            }
                            "graphql_ws_subscribe" => {
                                sending.set(true);
                                let url = req.url.clone();
                                let q = if req.graphql_ws_query.is_empty() {
                                    req.graphql_query.clone()
                                } else {
                                    req.graphql_ws_query.clone()
                                };
                                let vars = req.graphql_variables.clone();
                                spawn(async move {
                                    let result = tokio::task::spawn_blocking(move || {
                                        graphql_ws_subscribe(&url, &q, &vars, 10, 15_000)
                                    })
                                    .await;
                                    match result {
                                        Ok(Ok(lines)) => protocol_log.set(lines),
                                        Ok(Err(e)) => protocol_log.set(vec![e]),
                                        Err(e) => protocol_log.set(vec![format!("join: {e}")]),
                                    }
                                    sending.set(false);
                                });
                            }
                            "grpc_reflect" => {
                                let url = req.url.clone();
                                protocol_log.set(grpc_reflection_list(&url));
                            }
                            _ => {}
                        }
                    },
                    sending: sending(),
                    ws_connected: ws_connected(),
                    protocol_log: protocol_log(),
                }

                div {
                    class: if matches!(pane_drag(), Some(PaneDrag::Request { .. })) {
                        "pane-resize-h active"
                    } else {
                        "pane-resize-h"
                    },
                    title: "Drag to resize request / response",
                    onmousedown: move |evt| {
                        evt.prevent_default();
                        evt.stop_propagation();
                        pane_drag.set(Some(PaneDrag::Request {
                            start_y: evt.client_coordinates().y,
                            start_h: app_settings().ui.request_pane_height_px,
                        }));
                    },
                }

                ResponseViewer {
                    response: response(),
                    sending: sending(),
                    default_tab: app_settings().ui.default_response_tab,
                    on_save_example: move |_| {
                        if let Some(view) = response() {
                            let mut d = draft();
                            let name = format!("Example {}", d.examples.len() + 1);
                            let ex = SavedExample::from_response(name, &view);
                            d.examples.push(ex.clone());
                            let req_id = d.id;
                            draft.set(d);
                            let mut cols = collections();
                            let _ = save_example_to_request(&mut cols, req_id, ex);
                            // also update if request exists in tree
                            for col in cols.iter_mut() {
                                let _ = col.update_request(draft());
                            }
                            collections.set(cols);
                            persist();
                            status_msg.set("saved response as example".into());
                        }
                    },
                }
                }

                ConsolePanel {
                    open: console_open(),
                    lines: console_lines(),
                    resizing: matches!(pane_drag(), Some(PaneDrag::Console { .. })),
                    on_toggle: move |_| console_open.set(!console_open()),
                    on_clear: move |_| console_lines.set(vec![]),
                    on_resize_start: move |start_y: f64| {
                        if !console_open() {
                            console_open.set(true);
                        }
                        pane_drag.set(Some(PaneDrag::Console {
                            start_y,
                            start_h: app_settings().ui.console_height_px,
                        }));
                    },
                }

                if app_settings().ui.show_status_bar
                    && (mock_port().is_some() || !status_msg().is_empty())
                {
                    p { class: "status-line",
                        if let Some(p) = mock_port() {
                            span { class: "mock-status-pill", "mock :{p}" }
                        }
                        if mock_port().is_some() && !status_msg().is_empty() {
                            span { " · " }
                        }
                        if !status_msg().is_empty() {
                            "{status_msg}"
                        }
                    }
                }
                }
            }

            if show_cookies() {
                ModalShell {
                    title: "Cookie manager".to_string(),
                    on_close: move |_| {
                        show_cookies.set(false);
                        persist();
                    },
                    div { class: "modal-body",
                        p { class: "hint", "Jar used on send (domain / path match)." }
                        table { class: "kv-table",
                            thead {
                                tr {
                                    th { "Domain" }
                                    th { "Name" }
                                    th { "Value" }
                                    th { "Path" }
                                    th { "" }
                                }
                            }
                            tbody {
                                for (i, c) in cookies().cookies.iter().enumerate() {
                                    tr {
                                        td {
                                            input {
                                                value: "{c.domain}",
                                                oninput: move |evt: Event<FormData>| {
                                                    let mut jar = cookies();
                                                    if let Some(slot) = jar.cookies.get_mut(i) {
                                                        slot.domain = evt.value();
                                                    }
                                                    cookies.set(jar);
                                                }
                                            }
                                        }
                                        td {
                                            input {
                                                value: "{c.name}",
                                                oninput: move |evt: Event<FormData>| {
                                                    let mut jar = cookies();
                                                    if let Some(slot) = jar.cookies.get_mut(i) {
                                                        slot.name = evt.value();
                                                    }
                                                    cookies.set(jar);
                                                }
                                            }
                                        }
                                        td {
                                            input {
                                                value: "{c.value}",
                                                oninput: move |evt: Event<FormData>| {
                                                    let mut jar = cookies();
                                                    if let Some(slot) = jar.cookies.get_mut(i) {
                                                        slot.value = evt.value();
                                                    }
                                                    cookies.set(jar);
                                                }
                                            }
                                        }
                                        td {
                                            input {
                                                value: "{c.path}",
                                                oninput: move |evt: Event<FormData>| {
                                                    let mut jar = cookies();
                                                    if let Some(slot) = jar.cookies.get_mut(i) {
                                                        slot.path = evt.value();
                                                    }
                                                    cookies.set(jar);
                                                }
                                            }
                                        }
                                        td {
                                            button {
                                                class: "btn btn-ghost",
                                                onclick: move |_| {
                                                    let mut jar = cookies();
                                                    jar.remove_at(i);
                                                    cookies.set(jar);
                                                    persist();
                                                },
                                                "×"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "kv-actions",
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| {
                                    let mut jar = cookies();
                                    jar.upsert(Cookie {
                                        domain: "example.com".into(),
                                        name: "session".into(),
                                        value: String::new(),
                                        path: "/".into(),
                                    });
                                    cookies.set(jar);
                                },
                                "Add"
                            }
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| {
                                    let mut jar = cookies();
                                    jar.clear();
                                    cookies.set(jar);
                                    persist();
                                },
                                "Clear all"
                            }
                            button {
                                class: "btn btn-send",
                                onclick: move |_| {
                                    show_cookies.set(false);
                                    persist();
                                },
                                "Done"
                            }
                        }
                    }
                }
            }

            if show_col_vars() {
                ModalShell {
                    title: "Collection variables".to_string(),
                    class: "col-vars-modal".to_string(),
                    on_close: move |_| {
                        show_col_vars.set(false);
                        persist();
                    },
                    div { class: "modal-body",
                        div { class: "modal-tabs",
                            button {
                                class: if !col_vars_all() { "builder-tab active" } else { "builder-tab" },
                                onclick: move |_| col_vars_all.set(false),
                                "Active"
                            }
                            button {
                                class: if col_vars_all() { "builder-tab active" } else { "builder-tab" },
                                onclick: move |_| {
                                    col_vars_all.set(true);
                                    if col_vars_open_id().is_none() {
                                        col_vars_open_id.set(active_collection_id());
                                    }
                                },
                                "All collections"
                            }
                        }
                        {
                            let cols = collections();
                            let active_id = active_collection_id();
                            if !col_vars_all() {
                                let col = active_id.and_then(|id| cols.iter().find(|c| c.id == id));
                                match col {
                                    Some(c) => {
                                        let cid = c.id;
                                        let rows = c.variables.clone();
                                        let name = c.name.clone();
                                        rsx! {
                                            p { class: "hint",
                                                "Editing “{name}” — synced to VariableContext.collection"
                                            }
                                            ColVarEditor {
                                                collection_id: cid,
                                                rows,
                                                sync_active: true,
                                                collections,
                                                vars,
                                            }
                                        }
                                    }
                                    None => rsx! {
                                        p { class: "hint", "Select a collection in the sidebar first." }
                                    },
                                }
                            } else {
                                rsx! {
                                    p { class: "hint",
                                        "Browse and edit variables for every collection. Active collection still drives VariableContext.collection."
                                    }
                                    div { class: "col-vars-accordion",
                                        for c in cols.iter() {
                                            {
                                                let cid = c.id;
                                                let name = c.name.clone();
                                                let rows = c.variables.clone();
                                                let n = rows.len();
                                                let open = col_vars_open_id() == Some(cid);
                                                let is_active = active_id == Some(cid);
                                                rsx! {
                                                    div { class: "col-vars-section",
                                                        button {
                                                            class: "col-vars-section-head",
                                                            onclick: move |_| {
                                                                if col_vars_open_id() == Some(cid) {
                                                                    col_vars_open_id.set(None);
                                                                } else {
                                                                    col_vars_open_id.set(Some(cid));
                                                                }
                                                            },
                                                            span {
                                                                if open { "▾ " } else { "▸ " }
                                                                "{name}"
                                                                if is_active { " · active" }
                                                            }
                                                            span { class: "hint", "{n} vars" }
                                                        }
                                                        if open {
                                                            ColVarEditor {
                                                                collection_id: cid,
                                                                rows,
                                                                sync_active: is_active,
                                                                collections,
                                                                vars,
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "kv-actions",
                            button {
                                class: "btn btn-send",
                                onclick: move |_| {
                                    show_col_vars.set(false);
                                    persist();
                                },
                                "Done"
                            }
                        }
                    }
                }
            }

            if let Some(modal) = impex_modal() {
                ImpexDialog {
                    modal,
                    on_close: move |_| impex_modal.set(None),
                    on_import: move |text: String| {
                        let text = text.trim().to_string();
                        let imported = if text.contains("\"log\"") && text.contains("\"entries\"") {
                            import_har(&text).map_err(|e| e)
                        } else if text.trim_start().starts_with('{')
                            && (text.contains("\"openapi\"") || text.contains("\"paths\""))
                        {
                            import_openapi3(&text)
                        } else if !text.trim_start().starts_with('{')
                            && (text.contains("openapi:") || text.contains("paths:"))
                        {
                            import_openapi3(&text)
                        } else if text.to_ascii_lowercase().contains("curl ")
                            || text.to_ascii_lowercase().starts_with("curl")
                        {
                            import_curl(&text).map(|d| {
                                let mut c = Collection::new("cURL import");
                                c.add_request(d);
                                c
                            })
                        } else {
                            import_collection_v21(&text)
                        };
                        match imported {
                            Ok(col) => {
                                let id = col.id;
                                let mut v = vars();
                                v.collection = col.variables_map();
                                vars.set(v);
                                let mut cols = collections();
                                cols.push(col);
                                collections.set(cols);
                                active_collection_id.set(Some(id));
                                fold_epoch.set(fold_epoch() + 1);
                                impex_modal.set(None);
                                persist();
                                status_msg.set("imported collection".into());
                            }
                            Err(e) => status_msg.set(format!("import error: {e}")),
                        }
                    },
                }
            }

            // Overlay inside `.app-shell` (fixed) — never a bare grid child.
            if splash_visible() {
                div {
                    class: if splash_leaving() {
                        "startup-splash leaving"
                    } else {
                        "startup-splash"
                    },
                    div { class: "startup-splash-inner",
                        img {
                            class: "brand-logo brand-logo-splash",
                            src: "{crate::brand::LOGO_SRC.as_str()}",
                            alt: "Blitz",
                        }
                        div { class: "startup-splash-wordmark",
                            span { class: "brand-name", "Blitz" }
                        }
                        div { class: "startup-splash-dots", aria_hidden: "true",
                            span {}
                            span {}
                            span {}
                        }
                    }
                }
            }

            // Compact env-switch loader (modal-backdrop style) — not a grid child.
            if let Some(name) = env_switch_overlay() {
                div { class: "env-switch-overlay",
                    div { class: "env-switch-card",
                        img {
                            class: "brand-logo brand-logo-env",
                            src: "{crate::brand::LOGO_SRC.as_str()}",
                            alt: "Blitz",
                        }
                        div { class: "loading-row",
                            Spinner { }
                            span { class: "loading-text", "Switching environment…" }
                        }
                        div { class: "env-switch-name", "{name}" }
                        div { class: "env-switch-dots", aria_hidden: "true",
                            span {}
                            span {}
                            span {}
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone, PartialEq)]
enum ImpexModal {
    Import { text: String },
    Export { name: String, text: String },
}

fn sorted_map_rows(map: &std::collections::HashMap<String, String>) -> Vec<(String, String)> {
    let mut rows: Vec<_> = map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

fn rows_to_map(rows: Vec<(String, String)>) -> std::collections::HashMap<String, String> {
    rows.into_iter()
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

/// Editable flat key/value map (global / vault).
#[component]
fn MapVarEditor(rows: Vec<(String, String)>, on_change: EventHandler<Vec<(String, String)>>) -> Element {
    rsx! {
        table { class: "kv-table",
            thead { tr { th { "Key" } th { "Value" } th { "" } } }
            tbody {
                for (i, (key, val)) in rows.iter().enumerate() {
                    tr {
                        td {
                            input {
                                value: "{key}",
                                oninput: {
                                    let rows = rows.clone();
                                    move |evt: Event<FormData>| {
                                        let mut next = rows.clone();
                                        if let Some(r) = next.get_mut(i) {
                                            r.0 = evt.value();
                                        }
                                        on_change.call(next);
                                    }
                                }
                            }
                        }
                        td {
                            input {
                                value: "{val}",
                                oninput: {
                                    let rows = rows.clone();
                                    move |evt: Event<FormData>| {
                                        let mut next = rows.clone();
                                        if let Some(r) = next.get_mut(i) {
                                            r.1 = evt.value();
                                        }
                                        on_change.call(next);
                                    }
                                }
                            }
                        }
                        td {
                            button {
                                class: "btn btn-ghost",
                                onclick: {
                                    let rows = rows.clone();
                                    move |_| {
                                        let mut next = rows.clone();
                                        if i < next.len() {
                                            next.remove(i);
                                        }
                                        on_change.call(next);
                                    }
                                },
                                "×"
                            }
                        }
                    }
                }
            }
        }
        div { class: "kv-actions",
            button {
                class: "btn btn-ghost",
                onclick: {
                    let rows = rows.clone();
                    move |_| {
                        let mut next = rows.clone();
                        next.push((String::new(), String::new()));
                        on_change.call(next);
                    }
                },
                "Add row"
            }
        }
    }
}

/// Editable key/value rows for one collection's variables.
#[component]
fn ColVarEditor(
    collection_id: Uuid,
    rows: Vec<KeyValue>,
    sync_active: bool,
    mut collections: Signal<Vec<Collection>>,
    mut vars: Signal<VariableContext>,
) -> Element {
    let cid = collection_id;
    rsx! {
        table { class: "kv-table",
            thead { tr { th { "Key" } th { "Value" } th { "" } } }
            tbody {
                for (i, row) in rows.iter().enumerate() {
                    tr {
                        td {
                            input {
                                value: "{row.key}",
                                oninput: move |evt: Event<FormData>| {
                                    let mut cols = collections();
                                    if let Some(col) = cols.iter_mut().find(|c| c.id == cid) {
                                        if let Some(r) = col.variables.get_mut(i) {
                                            r.key = evt.value();
                                        }
                                        if sync_active {
                                            let mut v = vars();
                                            v.collection = col.variables_map();
                                            vars.set(v);
                                        }
                                    }
                                    collections.set(cols);
                                }
                            }
                        }
                        td {
                            input {
                                value: "{row.value}",
                                oninput: move |evt: Event<FormData>| {
                                    let mut cols = collections();
                                    if let Some(col) = cols.iter_mut().find(|c| c.id == cid) {
                                        if let Some(r) = col.variables.get_mut(i) {
                                            r.value = evt.value();
                                        }
                                        if sync_active {
                                            let mut v = vars();
                                            v.collection = col.variables_map();
                                            vars.set(v);
                                        }
                                    }
                                    collections.set(cols);
                                }
                            }
                        }
                        td {
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| {
                                    let mut cols = collections();
                                    if let Some(col) = cols.iter_mut().find(|c| c.id == cid) {
                                        if i < col.variables.len() {
                                            col.variables.remove(i);
                                        }
                                        if sync_active {
                                            let mut v = vars();
                                            v.collection = col.variables_map();
                                            vars.set(v);
                                        }
                                    }
                                    collections.set(cols);
                                },
                                "×"
                            }
                        }
                    }
                }
            }
        }
        div { class: "kv-actions",
            button {
                class: "btn btn-ghost",
                onclick: move |_| {
                    let mut cols = collections();
                    if let Some(col) = cols.iter_mut().find(|c| c.id == cid) {
                        col.variables.push(KeyValue::new("", ""));
                    }
                    collections.set(cols);
                },
                "Add row"
            }
        }
    }
}

#[component]
fn ImpexDialog(
    modal: ImpexModal,
    on_close: EventHandler<()>,
    on_import: EventHandler<String>,
) -> Element {
    let mut text = use_signal(|| match &modal {
        ImpexModal::Import { text } | ImpexModal::Export { text, .. } => text.clone(),
    });
    let is_import = matches!(modal, ImpexModal::Import { .. });
    let title = match &modal {
        ImpexModal::Import { .. } => "Import from Postman export (v2.1 JSON / cURL / OpenAPI)".to_string(),
        ImpexModal::Export { name, .. } => format!("Export: {name}"),
    };

    rsx! {
        ModalShell {
            title,
            on_close: move |_| on_close.call(()),
            div { class: "modal-body",
                p { class: "hint", "{limitations_blurb()}" }
                CommentableTextarea {
                    class: "impex-editor".to_string(),
                    value: text(),
                    on_change: move |v: String| text.set(v),
                }
            }
            div { class: "modal-actions",
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| on_close.call(()),
                    "Close"
                }
                if is_import {
                    button {
                        class: "btn btn-send",
                        onclick: move |_| on_import.call(text()),
                        "Import"
                    }
                }
            }
        }
    }
}

fn apply_snapshot(
    snap: AppSnapshot,
    collections: &mut Signal<Vec<Collection>>,
    environments: &mut Signal<Vec<Environment>>,
    vars: &mut Signal<VariableContext>,
    active_env_id: &mut Signal<Option<Uuid>>,
    active_collection_id: &mut Signal<Option<Uuid>>,
    active_request_id: &mut Signal<Option<Uuid>>,
    history: &mut Signal<Vec<HistoryEntry>>,
    cookies: &mut Signal<CookieJar>,
    draft: &mut Signal<RequestDraft>,
    app_settings: &mut Signal<AppSettings>,
    trash: &mut Signal<Vec<Collection>>,
    open_tabs: &mut Signal<Vec<RequestDraft>>,
    responses: &mut Signal<std::collections::HashMap<Uuid, ResponseView>>,
    show_vars: &mut Signal<bool>,
    show_settings: &mut Signal<bool>,
    show_runner: &mut Signal<bool>,
    show_perf: &mut Signal<bool>,
    show_env_manager: &mut Signal<bool>,
    show_col_vars: &mut Signal<bool>,
    status_msg: &mut Signal<String>,
    root: &std::path::Path,
) {
    collections.set(snap.collections.clone());
    environments.set(snap.environments.clone());
    let mut v = snap.variables;
    if let Ok(vault) = VaultStore::load(root) {
        v.vault = vault;
    }
    vars.set(v);
    active_env_id.set(snap.active_environment_id);
    history.set(snap.history);
    cookies.set(snap.cookies);
    let mut settings = snap.settings;
    settings.ui = settings.ui.clamped();
    app_settings.set(settings);
    trash.set(snap.trash);
    open_tabs.set(snap.open_tabs);
    responses.set(snap.responses);
    show_vars.set(snap.show_vars);
    show_settings.set(snap.show_settings);
    show_runner.set(snap.show_runner);
    show_perf.set(snap.show_perf);
    show_env_manager.set(snap.show_env_manager);
    show_col_vars.set(snap.show_col_vars);
    active_collection_id.set(
        snap.active_collection_id
            .or_else(|| snap.collections.first().map(|c| c.id)),
    );
    active_request_id.set(snap.active_request_id);

    if let Some(req_id) = snap.active_request_id {
        if let Some((col_id, req)) = find_request_across(&snap.collections, req_id) {
            draft.set(req.clone());
            active_collection_id.set(Some(col_id));
        }
    } else if let Some(col) = snap.collections.first() {
        if let Some(req) = col.items.iter().find_map(|i| match i {
            crate::core::CollectionItem::Request(r) => Some(r),
            _ => None,
        }) {
            draft.set(req.clone());
            active_request_id.set(Some(req.id));
            active_collection_id.set(Some(col.id));
        }
    }

    status_msg.set(format!("storage: {}", root.display()));
}

/// Strip display-only favorite markers before persisting a rename.
fn sanitize_rename_name(name: &str) -> String {
    name.trim()
        .trim_start_matches('★')
        .trim()
        .trim_end_matches(" *")
        .trim()
        .to_string()
}

fn count_requests_under(cols: &[Collection], parent_id: Uuid) -> usize {
    for col in cols {
        if col.id == parent_id {
            return col
                .items
                .iter()
                .filter(|i| matches!(i, crate::core::CollectionItem::Request(_)))
                .count();
        }
        if let Some(n) = count_direct_requests_in(&col.items, parent_id) {
            return n;
        }
    }
    0
}

fn count_direct_requests_in(items: &[crate::core::CollectionItem], parent_id: Uuid) -> Option<usize> {
    for item in items {
        if let crate::core::CollectionItem::Folder(f) = item {
            if f.id == parent_id {
                return Some(
                    f.items
                        .iter()
                        .filter(|i| matches!(i, crate::core::CollectionItem::Request(_)))
                        .count(),
                );
            }
            if let Some(n) = count_direct_requests_in(&f.items, parent_id) {
                return Some(n);
            }
        }
    }
    None
}

fn count_folders_under(cols: &[Collection], parent_id: Uuid) -> usize {
    for col in cols {
        if col.id == parent_id {
            return col
                .items
                .iter()
                .filter(|i| matches!(i, crate::core::CollectionItem::Folder(_)))
                .count();
        }
        if let Some(n) = count_direct_folders_in(&col.items, parent_id) {
            return n;
        }
    }
    0
}

fn count_direct_folders_in(items: &[crate::core::CollectionItem], parent_id: Uuid) -> Option<usize> {
    for item in items {
        if let crate::core::CollectionItem::Folder(f) = item {
            if f.id == parent_id {
                return Some(
                    f.items
                        .iter()
                        .filter(|i| matches!(i, crate::core::CollectionItem::Folder(_)))
                        .count(),
                );
            }
            if let Some(n) = count_direct_folders_in(&f.items, parent_id) {
                return Some(n);
            }
        }
    }
    None
}
