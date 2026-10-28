pub mod cloud;
pub mod modals;
pub mod protocol_actions;
pub mod state;
pub mod top_bar;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use dioxus::prelude::*;

use crate::core::{
    add_folder_under, add_request_under, apply_collection_map, apply_collection_var_edit,
    apply_env_var_edit, apply_environment_map, batch_duplicate_across, batch_move_items,
    check_for_updates, delete_needs_confirmation, detect_git_conflicts, duplicate_across,
    export_collection_v21, find_environment, find_request_across, flatten_collections,
    generate_static_doc_html, generate_swagger_ui_html, grpc_unary, insert_item_under,
    is_draft_dirty, move_item_to, push_console, push_history, remint_pasted_item, remove_across,
    rename_across, reorder_before, request_tab_display_name, resolve_ancestor_auth,
    save_example_to_request, save_request_to_collections, snapshot_tree_item, sort_collections,
    Collection, CollectionItem, ConsoleLine, Folder, HistoryEntry, HttpMethod, RequestDraft,
    RequestProtocol, ResponseView, SavedExample, UiSettings, VarResolveScope, HISTORY_MAX,
    default_zoom_percent,
};
use crate::runtime::{send_request_with_options, SendError, SendOptions};
use crate::storage::JsonStore;
use crate::ui::console_panel::ConsolePanel;
use crate::ui::loading::Spinner;
use crate::ui::request_builder::{RequestBuilder, RequestVarEditEvent};
use crate::ui::response_viewer::ResponseViewer;
use crate::ui::settings_panel::{SettingsEvent, SettingsPanel};
use crate::ui::sidebar::{Sidebar, SidebarAction, SidebarEvent};
use crate::ui::workbench::modals::{VarsPage, WorkbenchModals};
use crate::ui::workbench::protocol_actions::run_protocol_action;
use crate::ui::workbench::state::{
    ensure_cloud_defaults, execute_tree_delete, sync_env_vars, ImpexModal, PaneDrag,
    PendingCloseTab, PendingDelete, TreeClipboard, WorkbenchState,
};
use crate::ui::workbench::top_bar::TopBar;
use crate::ui::{CollectionEditor, ConflictResolver, ConflictState, DismissBackdrop, ToastContainer, ToastLevel};

fn tree_item_label(collections: &[Collection], id: Uuid) -> String {
    flatten_collections(collections)
        .into_iter()
        .find(|r| r.id == id)
        .map(|r| r.name)
        .unwrap_or_else(|| "item".into())
}

#[component]
pub fn Workbench() -> Element {
    let mut st = WorkbenchState::use_new();
    let mut draft = st.draft;
    let mut response = st.response;
    let mut responses = st.responses;
    let mut prev_responses = st.prev_responses;
    let mut sending = st.sending;
    let mut send_gen = st.send_gen;
    let mut send_cancel = st.send_cancel;
    let mut collections = st.collections;
    let mut environments = st.environments;
    let active_env_id = st.active_env_id;
    let mut active_collection_id = st.active_collection_id;
    let mut active_request_id = st.active_request_id;
    let mut vars = st.vars;
    let mut history = st.history;
    let mut cookies = st.cookies;
    let mut console_lines = st.console_lines;
    let mut console_open = st.console_open;
    let mut status_msg = st.status_msg;
    let mut impex_modal = st.impex_modal;
    let mut loaded = st.loaded;
    let mut splash_visible = st.splash_visible;
    let mut splash_leaving = st.splash_leaving;
    let env_switch_overlay = st.env_switch_overlay;
    let mut fold_epoch = st.fold_epoch;
    let mut app_settings = st.app_settings;
    let mut data_root = st.data_root;
    let mut show_settings = st.show_settings;
    let mut trash = st.trash;
    let mut open_tabs = st.open_tabs;
    let mut show_palette = st.show_palette;
    let mut show_snippets = st.show_snippets;
    let mock_port = st.mock_port;
    let ws_connected = st.ws_connected;
    let mut protocol_log = st.protocol_log;
    let mut file_menu_open = st.file_menu_open;
    let mut tools_menu_open = st.tools_menu_open;
    let mut cloud_menu_open = st.cloud_menu_open;
    let mut show_col_vars = st.show_col_vars;
    let mut col_vars_all = st.col_vars_all;
    let mut col_vars_open_id = st.col_vars_open_id;
    let mut editing_collection_id = st.editing_collection_id;
    let mut conflict_state = st.conflict_state;
    let mut sidebar_drag = st.sidebar_drag;
    let mut sidebar_w = st.sidebar_w;
    let mut pane_drag = st.pane_drag;
    let mut pane_h = st.pane_h;
    let mut console_h = st.console_h;
    let mut pending_delete = st.pending_delete;
    let mut pending_batch_move = st.pending_batch_move;
    let mut pending_close_tab = st.pending_close_tab;
    let mut tree_clipboard = st.tree_clipboard;
    let mut available_update = st.available_update;
    let mut show_tab_menu = st.show_tab_menu;
    let mut drag_tab_idx = st.drag_tab_idx;
    let mut renaming_tab_id = use_signal(|| None::<Uuid>);
    let mut rename_tab_draft = use_signal(String::new);
    let mut zoom_overlay = use_signal(|| None::<(u32, u64)>);

    use_effect(move || {
        if loaded() {
            return;
        }
        let mut st = st;
        if let Ok(store) = JsonStore::open_default() {
            match store.load() {
                Ok(snap) => {
                    data_root.set(store.root().display().to_string());
                    st.apply_snapshot(snap, store.root());
                }
                Err(e) => status_msg.set(format!("load error: {e}")),
            }
        }
        fold_epoch.set(fold_epoch() + 1);
        loaded.set(true);
        if sidebar_drag().is_none() {
            sidebar_w.set(app_settings().ui.sidebar_width_px as f64);
        }
        if pane_drag().is_none() {
            pane_h.set(app_settings().ui.request_pane_height_px as f64);
            console_h.set(app_settings().ui.console_height_px as f64);
        }
        let splash_hold = app_settings().ui.clamped().splash_min_ms as u64;
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(splash_hold)).await;
            splash_leaving.set(true);
            tokio::time::sleep(Duration::from_millis(320)).await;
            splash_visible.set(false);

            let info = tokio::task::spawn_blocking(check_for_updates)
                .await
                .ok()
                .flatten();
            if let Some(info) = info {
                if info.has_update {
                    available_update.set(Some(info));
                }
            }
        });
    });

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

    use_effect(move || {
        let zoom_pct = app_settings().ui.zoom_percent;
        let factor = (zoom_pct as f64) / 100.0;
        let script = format!(
            r#"
            (function() {{
                const factor = {factor:.2};
                if (document.documentElement) {{
                    document.documentElement.style.zoom = factor;
                }}
                if (!window.__blitz_zoom_listener_installed) {{
                    window.__blitz_zoom_listener_installed = true;
                    window.addEventListener('keydown', function(e) {{
                        const isMeta = e.metaKey || e.ctrlKey;
                        if (!isMeta) return;

                        if (e.code === 'Equal' || e.code === 'NumpadAdd' || e.key === '=' || e.key === '+') {{
                            e.preventDefault();
                            e.stopPropagation();
                            const btn = document.getElementById('blitz-btn-zoom-in');
                            if (btn) btn.click();
                            return;
                        }}

                        if (e.code === 'Minus' || e.code === 'NumpadSubtract' || e.key === '-' || e.key === '_') {{
                            e.preventDefault();
                            e.stopPropagation();
                            const btn = document.getElementById('blitz-btn-zoom-out');
                            if (btn) btn.click();
                            return;
                        }}

                        if (e.code === 'Digit0' || e.code === 'Numpad0' || e.key === '0') {{
                            e.preventDefault();
                            e.stopPropagation();
                            const btn = document.getElementById('blitz-btn-zoom-reset');
                            if (btn) btn.click();
                            return;
                        }}
                    }}, {{ capture: true }});
                }}
            }})();
            "#
        );
        spawn(async move {
            let _ = document::eval(&script).await;
        });
    });

    let do_zoom_in = use_callback(move |_| {
        let st = st;
        let mut s = app_settings();
        let old_z = s.ui.zoom_percent;
        let new_z = UiSettings::clamp_zoom_percent(old_z.saturating_add(UiSettings::ZOOM_STEP_PERCENT));
        if new_z != old_z {
            s.ui.zoom_percent = new_z;
            app_settings.set(s);
            st.persist_local();
        }
        status_msg.set(format!("Zoom: {new_z}%"));
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        zoom_overlay.set(Some((new_z, epoch)));
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            if let Some((_, ep)) = zoom_overlay() {
                if ep == epoch {
                    zoom_overlay.set(None);
                }
            }
        });
    });

    let do_zoom_out = use_callback(move |_| {
        let st = st;
        let mut s = app_settings();
        let old_z = s.ui.zoom_percent;
        let new_z = UiSettings::clamp_zoom_percent(old_z.saturating_sub(UiSettings::ZOOM_STEP_PERCENT));
        if new_z != old_z {
            s.ui.zoom_percent = new_z;
            app_settings.set(s);
            st.persist_local();
        }
        status_msg.set(format!("Zoom: {new_z}%"));
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        zoom_overlay.set(Some((new_z, epoch)));
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            if let Some((_, ep)) = zoom_overlay() {
                if ep == epoch {
                    zoom_overlay.set(None);
                }
            }
        });
    });

    let do_zoom_reset = use_callback(move |_| {
        let st = st;
        let mut s = app_settings();
        let new_z = default_zoom_percent();
        if s.ui.zoom_percent != new_z {
            s.ui.zoom_percent = new_z;
            app_settings.set(s);
            st.persist_local();
        }
        status_msg.set(format!("Zoom reset: {new_z}%"));
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        zoom_overlay.set(Some((new_z, epoch)));
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            if let Some((_, ep)) = zoom_overlay() {
                if ep == epoch {
                    zoom_overlay.set(None);
                }
            }
        });
    });

    let do_save = use_callback(move |_| {
        let mut st = st;
        let mut cols = collections();
        let cur = draft();
        let owner = save_request_to_collections(&mut cols, active_collection_id(), cur.clone());
        let req_id = cur.id;
        collections.set(cols);
        active_collection_id.set(Some(owner));
        active_request_id.set(Some(req_id));
        let mut tabs = open_tabs();
        if let Some(slot) = tabs.iter_mut().find(|t| t.id == req_id) {
            *slot = cur.clone();
            open_tabs.set(tabs);
        }
        st.persist();
        status_msg.set(format!("saved '{}'", cur.name));
        st.notify(format!("Saved '{}'", cur.name), ToastLevel::Success);
    });

    let do_new_tab = use_callback(move |_| {
        let req = RequestDraft {
            name: "Untitled Request".into(),
            ..Default::default()
        };
        let mut tabs = open_tabs();
        let cur = draft();
        if tabs.iter().any(|t| t.id == cur.id) {
            if let Some(t) = tabs.iter_mut().find(|t| t.id == cur.id) {
                *t = cur.clone();
            }
        }
        tabs.push(req.clone());
        open_tabs.set(tabs);
        draft.set(req);
        active_request_id.set(None);
        response.set(None);
        status_msg.set("new tab (unsaved)".into());
    });

    let do_force_close_tab = use_callback(move |tab_id: Uuid| {
        let mut tabs = open_tabs();
        let cur = draft();
        let Some(idx) = tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };
        let closing_active = cur.id == tab_id;
        tabs.remove(idx);

        if tabs.is_empty() {
            active_request_id.set(None);
            response.set(None);
            draft.set(RequestDraft::default());
            open_tabs.set(tabs);
            status_msg.set("tab closed".into());
            return;
        }

        if closing_active {
            let next_idx = if idx < tabs.len() {
                idx
            } else {
                tabs.len() - 1
            };
            let next = tabs[next_idx].clone();
            draft.set(next.clone());
            active_request_id.set(Some(next.id));
            let resp_map = responses();
            if let Some(cached) = resp_map.get(&next.id).cloned() {
                response.set(Some(cached));
            } else {
                response.set(None);
            }
        }
        open_tabs.set(tabs);
        status_msg.set("tab closed".into());
    });

    let do_close_tab = use_callback(move |tab_id: Uuid| {
        let tabs = open_tabs();
        let cur = draft();
        let target_draft = if cur.id == tab_id {
            cur
        } else if let Some(t) = tabs.iter().find(|t| t.id == tab_id) {
            t.clone()
        } else {
            return;
        };

        if is_draft_dirty(&target_draft, &collections()) {
            pending_close_tab.set(Some(PendingCloseTab {
                tab_id,
                draft: target_draft,
            }));
        } else {
            do_force_close_tab(tab_id);
        }
    });

    let do_switch_tab = use_callback(move |tid: Uuid| {
        let mut tabs = open_tabs();
        let cur = draft();
        if let Some(slot) = tabs.iter_mut().find(|x| x.id == cur.id) {
            *slot = cur;
        }
        if let Some(found) = tabs.iter().find(|x| x.id == tid).cloned() {
            draft.set(found.clone());
            active_request_id.set(Some(tid));
            editing_collection_id.set(None);
            if let Some((col_id, _)) = find_request_across(&collections(), tid) {
                active_collection_id.set(Some(col_id));
                if let Some(col) = collections().iter().find(|c| c.id == col_id) {
                    let mut v = vars();
                    v.collection = col.variables_map();
                    vars.set(v);
                }
            }
            let resp_map = responses();
            if let Some(cached) = resp_map.get(&tid).cloned() {
                response.set(Some(cached));
            } else {
                response.set(None);
            }
        }
        open_tabs.set(tabs);
    });

    let do_reorder_tab = use_callback(move |(from_idx, to_idx): (usize, usize)| {
        if from_idx == to_idx {
            return;
        }
        let mut tabs = open_tabs();
        if from_idx < tabs.len() && to_idx < tabs.len() {
            let item = tabs.remove(from_idx);
            tabs.insert(to_idx, item);
            open_tabs.set(tabs);
            st.persist();
        }
    });

    let do_send = use_callback(move |_| {
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

        // Snapshot previous response for diffing before sending new one
        let cur_resp = response();
        if let Some(prev) = responses().get(&req.id).cloned().or(cur_resp) {
            if prev.status != 0 {
                let mut prev_map = prev_responses();
                prev_map.insert(req.id, prev);
                prev_responses.set(prev_map);
            }
        }

        let mut override_vars = Vec::new();
        if let Some(col_id) = active_collection_id() {
            if let Some(col) = collections().iter().find(|c| c.id == col_id) {
                if let Some(_env) = find_environment(&environments(), active_env_id()) {
                    if let Some(override_) = col.environment_overrides.requests.get(&req.id) {
                        req = req.apply_env_override(override_);
                        override_vars = override_.variables.clone();
                    }
                    for kv in &col.environment_overrides.variables {
                        if kv.enabled && !kv.key.is_empty() {
                            override_vars.push(kv.clone());
                        }
                    }
                }
            }
        }

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
                let result =
                    tokio::task::spawn_blocking(move || grpc_unary(&ep, &meth, &body)).await;
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
        if let Some(col_id) = active_collection_id() {
            if let Some(col) = collections().iter().find(|c| c.id == col_id) {
                ctx.collection = col.variables_map();
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
        for kv in override_vars {
            if kv.enabled && !kv.key.is_empty() {
                ctx.collection.insert(kv.key, kv.value);
            }
        }
        if let Some(env) = find_environment(&environments(), active_env_id()) {
            ctx.environment = env.to_map();
            ctx.local.insert("environment".into(), env.name.clone());
        } else {
            ctx.environment.clear();
            ctx.local.remove("environment");
        }
        let cols_snapshot = collections();
        let ancestor_resolved = resolve_ancestor_auth(&cols_snapshot, req.id);
        let col_auth = active_collection_id().and_then(|id| {
            cols_snapshot.iter().find(|c| c.id == id).map(|c| {
                if let Some(_env) = find_environment(&environments(), active_env_id()) {
                    if let Some(override_) = c.environment_overrides.auth.as_ref() {
                        return override_.clone();
                    }
                }
                c.auth.clone()
            })
        });
        let mut jar = cookies();
        let settings = app_settings();

        let had_active_env = active_env_id().is_some();
        let active_col_id = active_collection_id();
        let data_root_clone = data_root();
        let mut st = st;

        spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                let ancestor_ref = ancestor_resolved.as_ref().map(|(auth, origin)| {
                    let label = if origin.is_collection {
                        format!("collection '{}'", origin.name)
                    } else {
                        format!("folder '{}'", origin.name)
                    };
                    (auth, label)
                });
                let ws_path = if !data_root_clone.is_empty() {
                    Some(std::path::Path::new(&data_root_clone))
                } else {
                    None
                };
                let mut opts = SendOptions {
                    secrets_store: None,
                    collection_auth: col_auth.as_ref(),
                    cookie_jar: Some(&mut jar),
                    app_settings: Some(&settings),
                    cancel: Some(cancel_flag),
                    workspace_root: ws_path,
                    ..Default::default()
                };
                if let Some((auth, ref label)) = ancestor_ref {
                    opts.ancestor_auth = Some((auth, label.as_str()));
                }
                let send = send_request_with_options(&req, &mut ctx, opts);
                (send, jar, ctx)
            })
            .await;

            if send_gen() != my_gen {
                return;
            }

            let (view, new_jar, new_ctx, cancelled) = match result {
                Ok((Ok(view), jar, ctx)) => (view, jar, ctx, false),
                Ok((Err(SendError::Cancelled), jar, ctx)) => (
                    ResponseView {
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
                    },
                    jar,
                    ctx,
                    true,
                ),
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

            let env_keys = new_ctx.environment.len();
            let mut v = new_ctx;
            if let Ok(store) = JsonStore::open_default() {
                let _ = v.vault.save(store.root());
            }

            if let Some(id) = active_env_id() {
                let mut envs = environments();
                if let Some(env) = envs.iter_mut().find(|e| e.id == id) {
                    apply_environment_map(env, &v.environment);
                }
                environments.set(envs);
            } else if env_keys > 0 {
                let mut lines = console_lines();
                push_console(
                    &mut lines,
                    ConsoleLine::error(
                        "no active environment — blitz.environment.set ignored (select Local/Development/Production first)",
                    ),
                );
                console_lines.set(lines);
                status_msg.set("no active environment — blitz.environment.set ignored".into());
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

            let draft_id = draft().id;
            response.set(Some(view.clone()));
            let mut resp_map = responses();
            resp_map.insert(draft_id, view.clone());
            responses.set(resp_map);
            sending.set(false);
            send_cancel.set(None);
            st.persist();
        });
    });

    let on_sidebar = move |evt: SidebarEvent| {
        let mut st = st;
        match evt.action {
            SidebarAction::SelectRequest => {
                if let Some(id) = evt.id {
                    if let Some((col_id, req)) = find_request_across(&collections(), id) {
                        let mut tabs = open_tabs();
                        let cur = draft();
                        if tabs.iter().any(|t| t.id == cur.id) {
                            if let Some(slot) = tabs.iter_mut().find(|t| t.id == cur.id) {
                                *slot = cur;
                            }
                        }
                        let target_draft = if let Some(existing) = tabs.iter().find(|t| t.id == req.id) {
                            existing.clone()
                        } else {
                            tabs.push(req.clone());
                            req.clone()
                        };
                        open_tabs.set(tabs);
                        draft.set(target_draft);
                        active_request_id.set(Some(id));
                        active_collection_id.set(Some(col_id));
                        editing_collection_id.set(None);
                        if let Some(col) = collections().iter().find(|c| c.id == col_id) {
                            let mut v = vars();
                            v.collection = col.variables_map();
                            vars.set(v);
                        }
                        let resp_map = responses();
                        if let Some(cached) = resp_map.get(&req.id).cloned() {
                            response.set(Some(cached));
                        } else {
                            response.set(None);
                        }
                        st.persist_local();
                    }
                }
            }
            SidebarAction::SelectCollection => {
                let col_id = evt.collection_id.or(evt.id);
                active_collection_id.set(col_id);
                editing_collection_id.set(col_id);
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
                st.persist();
            }
            SidebarAction::ToggleExpand => {}
            SidebarAction::NewCollection => {
                let mut cols = collections();
                let n = cols.len() + 1;
                let col = Collection::new(format!("Collection {n}"));
                let id = col.id;
                cols.push(col);
                collections.set(cols);
                active_collection_id.set(Some(id));
                editing_collection_id.set(Some(id));
                active_request_id.set(None);
                st.persist();
            }
            SidebarAction::NewFolder => {
                if let Some(parent_id) = evt.id.or(evt.collection_id) {
                    let mut cols = collections();
                    let n = count_folders_under(&cols, parent_id) + 1;
                    let folder = Folder::new(format!("Folder {n}"));
                    if add_folder_under(&mut cols, parent_id, folder) {
                        collections.set(cols);
                        if let Some(col_id) = evt.collection_id {
                            active_collection_id.set(Some(col_id));
                        }
                        st.persist();
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
                        st.persist();
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
                            st.persist();
                        }
                    }
                }
            }
            SidebarAction::Delete => {
                if let Some(id) = evt.id {
                    let cols = collections();
                    if delete_needs_confirmation(&cols, id) {
                        pending_delete.set(Some(PendingDelete::single(
                            id,
                            tree_item_label(&cols, id),
                        )));
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
                        st.persist();
                    }
                }
            }
            SidebarAction::BatchDelete => {
                let ids = if !evt.ids.is_empty() {
                    evt.ids
                } else if let Some(id) = evt.id {
                    vec![id]
                } else {
                    Vec::new()
                };
                if ids.len() == 1 {
                    let id = ids[0];
                    let cols = collections();
                    if delete_needs_confirmation(&cols, id) {
                        pending_delete.set(Some(PendingDelete::single(
                            id,
                            tree_item_label(&cols, id),
                        )));
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
                        st.persist();
                    }
                } else if ids.len() > 1 {
                    let count = ids.len();
                    pending_delete.set(Some(PendingDelete::batch(
                        ids,
                        format!("{count} items"),
                    )));
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
                            st.persist();
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
                        st.persist();
                    }
                }
            }
            SidebarAction::BatchDuplicate => {
                let mut cols = collections();
                let new_ids = batch_duplicate_across(&mut cols, &evt.ids);
                if !new_ids.is_empty() {
                    sort_collections(&mut cols);
                    collections.set(cols);
                    status_msg.set(format!("duplicated {} item{}", new_ids.len(), if new_ids.len() == 1 { "" } else { "s" }));
                    st.persist();
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
                    st.persist();
                }
            }
            SidebarAction::Sort => {
                let mut cols = collections();
                sort_collections(&mut cols);
                collections.set(cols);
                st.persist();
            }
            SidebarAction::MoveItem => {
                if let (Some(item_id), Some(target_id)) = (evt.id, evt.target_id) {
                    let mut cols = collections();
                    let ok = move_item_to(&mut cols, item_id, target_id)
                        || reorder_before(&mut cols, item_id, Some(target_id));
                    if ok {
                        collections.set(cols);
                        status_msg.set("moved / reordered".into());
                        st.persist();
                    } else {
                        status_msg.set("cannot move there".into());
                    }
                }
            }
            SidebarAction::BatchMove => {
                if let Some(target_id) = evt.target_id {
                    let mut cols = collections();
                    let count = batch_move_items(&mut cols, &evt.ids, target_id);
                    if count > 0 {
                        collections.set(cols);
                        status_msg.set(format!("moved {} item{}", count, if count == 1 { "" } else { "s" }));
                        st.persist();
                    } else {
                        status_msg.set("cannot move there".into());
                    }
                } else if !evt.ids.is_empty() {
                    pending_batch_move.set(Some(evt.ids.clone()));
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
                        st.persist();
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
            SidebarAction::ExportStaticDoc => {
                let cols = collections();
                let col = evt
                    .collection_id
                    .and_then(|id| cols.iter().find(|c| c.id == id))
                    .or_else(|| cols.first());
                if let Some(c) = col {
                    let html = generate_static_doc_html(c);
                    let safe_name = c.name.to_lowercase().replace(' ', "-");
                    let script = format!(
                        r#"
                        const blob = new Blob([{}], {{ type: 'text/html' }});
                        const url = URL.createObjectURL(blob);
                        const a = document.createElement('a');
                        a.href = url;
                        a.download = '{}-docs.html';
                        document.body.appendChild(a);
                        a.click();
                        document.body.removeChild(a);
                        URL.revokeObjectURL(url);
                        "#,
                        serde_json::to_string(&html).unwrap_or_else(|_| "\"\"".into()),
                        safe_name
                    );
                    spawn(async move {
                        let _ = document::eval(&script).await;
                    });
                    status_msg.set(format!("exported static docs for '{}'", c.name));
                }
            }
            SidebarAction::ExportSwaggerDoc => {
                let cols = collections();
                let col = evt
                    .collection_id
                    .and_then(|id| cols.iter().find(|c| c.id == id))
                    .or_else(|| cols.first());
                if let Some(c) = col {
                    match generate_swagger_ui_html(c) {
                        Ok(html) => {
                            let safe_name = c.name.to_lowercase().replace(' ', "-");
                            let script = format!(
                                r#"
                                const blob = new Blob([{}], {{ type: 'text/html' }});
                                const url = URL.createObjectURL(blob);
                                const a = document.createElement('a');
                                a.href = url;
                                a.download = '{}-swagger.html';
                                document.body.appendChild(a);
                                a.click();
                                document.body.removeChild(a);
                                URL.revokeObjectURL(url);
                                "#,
                                serde_json::to_string(&html).unwrap_or_else(|_| "\"\"".into()),
                                safe_name
                            );
                            spawn(async move {
                                let _ = document::eval(&script).await;
                            });
                            status_msg.set(format!("exported swagger ui docs for '{}'", c.name));
                        }
                        Err(err) => {
                            status_msg.set(format!("failed to export swagger docs: {}", err));
                        }
                    }
                }
            }
            SidebarAction::ToggleHistoryCollapse => {
                let mut s = app_settings();
                s.ui.history_collapsed = !s.ui.history_collapsed;
                app_settings.set(s);
                st.persist();
            }
        }
    };

    let ui = app_settings().ui.clone().clamped();
    let live_sidebar_w = if sidebar_drag().is_some() {
        sidebar_w()
    } else {
        ui.sidebar_width_px as f64
    };
    let shell_style = format!(
        "--fs-base: {}px; --fs-body: {}px; --fs-editor: {}px; --sidebar-w: {}px; --editor-tab-size: {}; --bar-h: {}px;",
        ui.ui_font_size_px,
        ui.ui_font_size_px,
        ui.editor_font_size_px,
        live_sidebar_w,
        ui.tab_size,
        ui.toolbar_height_px
    );
    let live_request_h = pane_h();
    let live_console_h = console_h();
    let workbench_style = format!(
        "--request-h: {}px; --console-h: {}px;",
        live_request_h, live_console_h
    );
    let appearance_tokens = ui.shell_class_tokens();
    let is_resizing_sidebar = sidebar_drag().is_some();
    let is_resizing_pane = pane_drag().is_some();
    let shell_class = {
        let mut classes = vec!["app-shell", &appearance_tokens];
        if is_resizing_sidebar {
            classes.push("sidebar-resizing");
        }
        if is_resizing_pane {
            classes.push("pane-resizing-v");
        }
        if show_settings() {
            classes.push("settings-open");
        }
        classes.join(" ")
    };
    let workbench_class = match pane_drag() {
        Some(PaneDrag::Request { .. } | PaneDrag::Console { .. }) => "workbench pane-resizing-v",
        None => "workbench",
    };
    let top_nav_menus_open = file_menu_open() || tools_menu_open() || cloud_menu_open();

    let on_settings_event = move |evt: SettingsEvent| {
        let mut st = st;
        match evt {
            SettingsEvent::Close => show_settings.set(false),
            SettingsEvent::Save(s) => {
                let enabling_cloud = s.cloud_sync.enabled;
                app_settings.set(s);
                if enabling_cloud {
                    ensure_cloud_defaults(st);
                }
                show_settings.set(false);
                st.persist();
                status_msg.set("settings saved".into());
            }
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
            onmousemove: move |evt| {
                if let Some((start_x, start_w)) = sidebar_drag() {
                    let delta = (evt.client_coordinates().x - start_x).round() as i32;
                    let new_w = UiSettings::clamp_sidebar_width(
                        (start_w as i32 + delta).max(0) as u32,
                    );
                    sidebar_w.set(new_w as f64);
                } else if let Some(drag) = pane_drag() {
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
                            let delta = (start_y - y).round() as i32;
                            let new_h = UiSettings::clamp_console_height(
                                (start_h as i32 + delta).max(0) as u32,
                            );
                            console_h.set(new_h as f64);
                        }
                    }
                }
            },
            onmouseup: move |_| {
                let mut changed = false;
                if sidebar_drag().is_some() {
                    sidebar_drag.set(None);
                    changed = true;
                }
                if pane_drag().is_some() {
                    pane_drag.set(None);
                    changed = true;
                }
                if changed {
                    let mut st = st;
                    let mut s = app_settings();
                    s.ui.sidebar_width_px = sidebar_w() as u32;
                    s.ui.request_pane_height_px = pane_h() as u32;
                    s.ui.console_height_px = console_h() as u32;
                    app_settings.set(s);
                    st.persist();
                }
            },
            onmouseleave: move |_| {
                let mut changed = false;
                if sidebar_drag().is_some() {
                    sidebar_drag.set(None);
                    changed = true;
                }
                if pane_drag().is_some() {
                    pane_drag.set(None);
                    changed = true;
                }
                if changed {
                    let mut st = st;
                    let mut s = app_settings();
                    s.ui.sidebar_width_px = sidebar_w() as u32;
                    s.ui.request_pane_height_px = pane_h() as u32;
                    s.ui.console_height_px = console_h() as u32;
                    app_settings.set(s);
                    st.persist();
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
                    Key::Character(c)
                        if meta
                            && !shift
                            && !alt
                            && c.len() == 1
                            && c.chars().next().is_some_and(|ch| ch.is_ascii_digit()) =>
                    {
                        if let Some(digit) = c.chars().next().and_then(|ch| ch.to_digit(10)) {
                            if (1..=9).contains(&digit) {
                                evt.prevent_default();
                                let target_idx = (digit - 1) as usize;
                                let tabs = open_tabs();
                                if let Some(target) = tabs.get(target_idx) {
                                    do_switch_tab(target.id);
                                }
                            }
                        }
                    }
                    Key::Character(c)
                        if meta
                            && ((shift && c.eq_ignore_ascii_case("p"))
                                || (!shift && c.eq_ignore_ascii_case("k"))) =>
                    {
                        evt.prevent_default();
                        show_palette.set(!show_palette());
                    }
                    Key::Character(c) if meta && alt && c.eq_ignore_ascii_case("c") => {
                        evt.prevent_default();
                        console_open.set(!console_open());
                    }
                    _ if meta
                        && (evt.code() == Code::Equal
                            || evt.code() == Code::NumpadAdd
                            || matches!(evt.key(), Key::ZoomIn)
                            || matches!(evt.key(), Key::Character(ref c) if c == "+" || c == "=")) =>
                    {
                        evt.prevent_default();
                        do_zoom_in(());
                    }
                    _ if meta
                        && (evt.code() == Code::Minus
                            || evt.code() == Code::NumpadSubtract
                            || matches!(evt.key(), Key::ZoomOut)
                            || matches!(evt.key(), Key::Character(ref c) if c == "-" || c == "_")) =>
                    {
                        evt.prevent_default();
                        do_zoom_out(());
                    }
                    _ if meta
                        && (evt.code() == Code::Digit0
                            || evt.code() == Code::Numpad0
                            || matches!(evt.key(), Key::Character(ref c) if c == "0")) =>
                    {
                        evt.prevent_default();
                        do_zoom_reset(());
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

            button {
                id: "blitz-btn-zoom-in",
                style: "display:none;",
                tabindex: "-1",
                aria_hidden: "true",
                onclick: move |_| do_zoom_in(()),
            }
            button {
                id: "blitz-btn-zoom-out",
                style: "display:none;",
                tabindex: "-1",
                aria_hidden: "true",
                onclick: move |_| do_zoom_out(()),
            }
            button {
                id: "blitz-btn-zoom-reset",
                style: "display:none;",
                tabindex: "-1",
                aria_hidden: "true",
                onclick: move |_| do_zoom_reset(()),
            }

            if let Some((pct, _)) = zoom_overlay() {
                div {
                    class: "zoom-overlay-indicator",
                    "Zoom: {pct}%"
                }
            }

            TopBar { st }
            VarsPage { st }

            if !show_settings() {
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
                                sidebar_w(),
                            )));
                        },
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
                if show_settings() {
                    SettingsPanel {
                        settings: app_settings(),
                        data_root: data_root(),
                        on_event: on_settings_event,
                    }
                } else {
                    div { class: "tab-strip",
                        div { class: "tab-strip-fixed",
                            button {
                                class: "btn btn-save",
                                title: "Save request (⌘S)",
                                onclick: move |_| do_save(()),
                                span { class: "action-icon action-icon-save", "aria-hidden": "true" }
                                span { "Save" }
                            }
                        }
                        div { class: "tab-strip-scrollable",
                            for (idx, t) in open_tabs().into_iter().enumerate() {
                                {
                                    let tid = t.id;
                                    let tname = request_tab_display_name(&collections(), tid);
                                    let active = draft().id == tid;
                                    let is_tab_renaming = renaming_tab_id() == Some(tid);
                                    let raw_cur_name = if active {
                                        draft().name.clone()
                                    } else {
                                        t.name.clone()
                                    };
                                    let tab_draft = if active {
                                        draft()
                                    } else {
                                        t.clone()
                                    };
                                    let is_dirty = is_draft_dirty(&tab_draft, &collections());
                                    rsx! {
                                        div {
                                            class: if active { "tab active" } else { "tab" },
                                            role: "button",
                                            tabindex: "0",
                                            draggable: !is_tab_renaming,
                                            ondragstart: move |_| {
                                                drag_tab_idx.set(Some(idx));
                                            },
                                            ondragover: move |evt| {
                                                evt.prevent_default();
                                            },
                                            ondrop: move |evt| {
                                                evt.prevent_default();
                                                if let Some(from) = drag_tab_idx() {
                                                    do_reorder_tab((from, idx));
                                                    drag_tab_idx.set(None);
                                                }
                                            },
                                            oncontextmenu: move |evt| {
                                                evt.prevent_default();
                                                evt.stop_propagation();
                                                renaming_tab_id.set(Some(tid));
                                                rename_tab_draft.set(raw_cur_name.clone());
                                            },
                                            onclick: move |_| {
                                                if is_tab_renaming {
                                                    return;
                                                }
                                                do_switch_tab(tid);
                                            },
                                            span {
                                                class: "tab-method method-{method_class(t.method)}",
                                                "{t.method.as_str()}"
                                            }
                                            if is_tab_renaming {
                                                input {
                                                    class: "tab-rename-input",
                                                    value: "{rename_tab_draft()}",
                                                    autofocus: true,
                                                    onclick: move |evt| evt.stop_propagation(),
                                                    onmousedown: move |evt| evt.stop_propagation(),
                                                    oninput: move |evt| {
                                                        rename_tab_draft.set(evt.value());
                                                    },
                                                    onblur: move |_| {
                                                        let raw = rename_tab_draft();
                                                        let next = sanitize_rename_name(&raw);
                                                        renaming_tab_id.set(None);
                                                        rename_tab_draft.set(String::new());
                                                        if !next.is_empty() {
                                                            let mut cols = collections();
                                                            if rename_across(&mut cols, tid, next.clone()) {
                                                                collections.set(cols);
                                                                st.persist();
                                                            }
                                                            if draft().id == tid {
                                                                let mut d = draft();
                                                                d.name = next.clone();
                                                                draft.set(d);
                                                            }
                                                            let mut tabs = open_tabs();
                                                            if let Some(tab_slot) = tabs.iter_mut().find(|x| x.id == tid) {
                                                                tab_slot.name = next.clone();
                                                            }
                                                            open_tabs.set(tabs);
                                                        }
                                                    },
                                                    onkeydown: move |evt| {
                                                        evt.stop_propagation();
                                                        match evt.key() {
                                                            Key::Enter => {
                                                                let raw = rename_tab_draft();
                                                                let next = sanitize_rename_name(&raw);
                                                                renaming_tab_id.set(None);
                                                                rename_tab_draft.set(String::new());
                                                                if !next.is_empty() {
                                                                    let mut cols = collections();
                                                                    if rename_across(&mut cols, tid, next.clone()) {
                                                                        collections.set(cols);
                                                                        st.persist();
                                                                    }
                                                                    if draft().id == tid {
                                                                        let mut d = draft();
                                                                        d.name = next.clone();
                                                                        draft.set(d);
                                                                    }
                                                                    let mut tabs = open_tabs();
                                                                    if let Some(tab_slot) = tabs.iter_mut().find(|x| x.id == tid) {
                                                                        tab_slot.name = next.clone();
                                                                    }
                                                                    open_tabs.set(tabs);
                                                                }
                                                            }
                                                            Key::Escape => {
                                                                renaming_tab_id.set(None);
                                                                rename_tab_draft.set(String::new());
                                                            }
                                                            _ => {}
                                                        }
                                                    },
                                                }
                                            } else {
                                                span { class: "tab-label", "{tname}" }
                                            }
                                            if is_dirty {
                                                span {
                                                    class: "tab-dirty-dot",
                                                    title: "Unsaved changes",
                                                    "•"
                                                }
                                            }
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
                                class: "tab tab-add",
                                title: "New request tab (⌘T)",
                                onclick: move |_| do_new_tab(()),
                                "+"
                            }
                            if !open_tabs().is_empty() {
                                button {
                                    class: if show_tab_menu() { "tab tab-more active" } else { "tab tab-more" },
                                    title: "All open tabs",
                                    onclick: move |_| show_tab_menu.set(!show_tab_menu()),
                                    "▼"
                                }
                            }
                        }
                        if show_tab_menu() {
                            div {
                                class: "tab-dropdown-menu",
                                for (i, t) in open_tabs().into_iter().enumerate() {
                                    {
                                        let tid = t.id;
                                        let tname = request_tab_display_name(&collections(), tid);
                                        let active = draft().id == tid;
                                        let tab_draft = if active { draft() } else { t.clone() };
                                        let is_dirty = is_draft_dirty(&tab_draft, &collections());
                                        let idx_label = if i < 9 { format!("⌘{}", i + 1) } else { String::new() };
                                        rsx! {
                                            div {
                                                key: "{tid}",
                                                class: if active { "tab-dropdown-item active" } else { "tab-dropdown-item" },
                                                onclick: move |_| {
                                                    show_tab_menu.set(false);
                                                    do_switch_tab(tid);
                                                },
                                                span {
                                                    class: "tab-method method-{method_class(t.method)}",
                                                    "{t.method.as_str()}"
                                                }
                                                span { class: "tab-dropdown-name", "{tname}" }
                                                if is_dirty {
                                                    span { class: "tab-dirty-dot", title: "Unsaved changes", "•" }
                                                }
                                                if !idx_label.is_empty() {
                                                    span { class: "tab-dropdown-idx", "{idx_label}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if let Some(conflict) = conflict_state() {
                        ConflictResolver {
                            root: std::path::PathBuf::from(data_root()),
                            conflict,
                            on_resolved: move |_| {
                                let root = std::path::PathBuf::from(data_root());
                                let remaining = detect_git_conflicts(&root).unwrap_or_default();
                                if remaining.is_empty() {
                                    conflict_state.set(None);
                                    status_msg.set("Git conflicts resolved successfully".into());
                                    if let Ok(store) = JsonStore::open_default() {
                                        if let Ok(snap) = store.load() {
                                            collections.set(snap.collections);
                                            environments.set(snap.environments);
                                        }
                                    }
                                } else {
                                    conflict_state.set(Some(ConflictState { files: remaining, selected_idx: 0 }));
                                }
                            },
                            on_close: move |_| conflict_state.set(None),
                        }
                    } else if let Some(col) = editing_collection_id().and_then(|cid| collections().into_iter().find(|c| c.id == cid)) {
                        CollectionEditor {
                            collection: col.clone(),
                            vars: vars(),
                            active_env_id: active_env_id(),
                            on_change: move |updated_col: Collection| {
                                let mut cols = collections();
                                if let Some(slot) = cols.iter_mut().find(|c| c.id == updated_col.id) {
                                    *slot = updated_col.clone();
                                }
                                collections.set(cols);
                                if active_collection_id() == Some(updated_col.id) {
                                    let mut v = vars();
                                    v.collection = updated_col.variables_map();
                                    vars.set(v);
                                }
                            },
                            on_close: move |_| {
                                let mut st = st;
                                st.persist();
                                editing_collection_id.set(None);
                            },
                        }
                    } else if open_tabs().is_empty() {
                        div {
                            class: "workbench-empty-page",
                            div {
                                class: "workbench-empty-content",
                                div { class: "workbench-empty-icon", "⚡" }
                                h2 { class: "workbench-empty-title", "No Open Requests" }
                                p { class: "workbench-empty-subtitle", "Select a request from the sidebar or click below to create a new tab." }
                                div {
                                    class: "workbench-empty-actions",
                                    button {
                                        class: "btn btn-primary",
                                        onclick: move |_| do_new_tab(()),
                                        "+ New Request (⌘T)"
                                    }
                                }
                            }
                        }
                    } else {
                        div {
                            class: format!("workbench-split {}", app_settings().ui.layout.class_token()),
                            div {
                                class: "workbench-request-side",
                                {
                                    let cols_snap = collections();
                                    let cur_draft = draft();
                                    let ancestor_origin = resolve_ancestor_auth(&cols_snap, cur_draft.id).map(|(_, o)| o);
                                    let mut effective_vars = vars();
                                    for p in &cur_draft.path_params {
                                        if p.enabled {
                                            effective_vars.path.insert(p.key.clone(), p.value.clone());
                                        }
                                    }
                                    rsx! {
                                        RequestBuilder {
                                            draft: cur_draft,
                                            vars: effective_vars,
                                            data_root: data_root(),
                                            ancestor_origin,
                                            on_edit_parent_auth: move |origin_id: Uuid| {
                                                col_vars_open_id.set(Some(origin_id));
                                                col_vars_all.set(true);
                                                show_col_vars.set(true);
                                            },
                                            on_change: move |d: RequestDraft| {
                                                let mut tabs = open_tabs();
                                                if let Some(slot) = tabs.iter_mut().find(|t| t.id == d.id) {
                                                    *slot = d.clone();
                                                    open_tabs.set(tabs);
                                                }
                                                draft.set(d);
                                            },
                                            on_send: move |_| do_send(()),
                                            on_open_code: move |_| show_snippets.set(true),
                                            on_var_edit: move |evt: RequestVarEditEvent| {
                                                let mut st = st;
                                                match evt {
                                                    RequestVarEditEvent::Save {
                                                        name,
                                                        scope,
                                                        value,
                                                        ..
                                                    } => {
                                                        match scope {
                                                            VarResolveScope::Path => {
                                                                let mut d = draft();
                                                                if let Some(p) = d.path_params.iter_mut().find(|p| p.key == name) {
                                                                    p.value = value.clone();
                                                                } else {
                                                                    d.path_params.push(crate::core::KeyValue::new(&name, &value));
                                                                }
                                                                let mut tabs = open_tabs();
                                                                if let Some(slot) = tabs.iter_mut().find(|t| t.id == d.id) {
                                                                    *slot = d.clone();
                                                                    open_tabs.set(tabs);
                                                                }
                                                                draft.set(d);
                                                                let mut v = vars();
                                                                v.path.insert(name.clone(), value);
                                                                vars.set(v);
                                                            }
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
                                                            VarResolveScope::Unresolved | VarResolveScope::Dynamic => {}
                                                        }
                                                        st.persist();
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
                                                        st.persist();
                                                        status_msg.set(format!("added {name} to environment"));
                                                    }
                                                }
                                            },
                                            on_protocol_action: move |action: String| {
                                                run_protocol_action(st, action);
                                            },
                                            sending: sending(),
                                            ws_connected: ws_connected(),
                                            protocol_log: protocol_log(),
                                        }
                                    }
                                }
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
                                        start_h: pane_h() as u32,
                                    }));
                                },
                            }

                            div {
                                class: "workbench-response-side",
                                ResponseViewer {
                                    response: response(),
                                    prev_response: prev_responses().get(&draft().id).cloned(),
                                    examples: draft().examples,
                                    sending: sending(),
                                    default_tab: app_settings().ui.default_response_tab,
                                    on_save_example: move |_| {
                                        let mut st = st;
                                        if let Some(view) = response() {
                                            let mut d = draft();
                                            let name = format!("Example {}", d.examples.len() + 1);
                                            let ex = SavedExample::from_response(name, &view);
                                            d.examples.push(ex.clone());
                                            let req_id = d.id;
                                            draft.set(d);
                                            let mut cols = collections();
                                            let _ = save_example_to_request(&mut cols, req_id, ex);
                                            for col in cols.iter_mut() {
                                                let _ = col.update_request(draft());
                                            }
                                            collections.set(cols);
                                            st.persist();
                                            status_msg.set("saved response as example".into());
                                        }
                                    },
                                    on_auto_assert: move |_| {
                                        let Some(view) = response() else { return; };
                                        if view.error.is_some() || view.status == 0 {
                                            status_msg.set("auto-assert: no successful response to learn from".into());
                                            return;
                                        }
                                        let suggested = crate::core::suggest_from_response(
                                            view.status,
                                            &view.headers,
                                            &view.body,
                                            view.elapsed_ms,
                                        );
                                        let mut d = draft();
                                        let added = crate::core::merge_assertions(&mut d.assertions, suggested);
                                        draft.set(d);
                                        status_msg.set(if added == 0 {
                                            "auto-assert: all assertions already present".into()
                                        } else {
                                            format!("auto-assert: added {added} assertion(s) — review in Tests tab, ⌘S to save")
                                        });
                                    },
                                }
                            }
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
                                start_h: console_h() as u32,
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

            WorkbenchModals {
                st,
                on_send: move |_| do_send(()),
                on_save: move |_| do_save(()),
                on_new_tab: move |_| do_new_tab(()),
            }

            ToastContainer { toasts: st.toasts }

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
                .filter(|i| matches!(i, CollectionItem::Request(_)))
                .count();
        }
        if let Some(n) = count_direct_requests_in(&col.items, parent_id) {
            return n;
        }
    }
    0
}

fn count_direct_requests_in(items: &[CollectionItem], parent_id: Uuid) -> Option<usize> {
    for item in items {
        if let CollectionItem::Folder(f) = item {
            if f.id == parent_id {
                return Some(
                    f.items
                        .iter()
                        .filter(|i| matches!(i, CollectionItem::Request(_)))
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
                .filter(|i| matches!(i, CollectionItem::Folder(_)))
                .count();
        }
        if let Some(n) = count_direct_folders_in(&col.items, parent_id) {
            return n;
        }
    }
    0
}

fn count_direct_folders_in(items: &[CollectionItem], parent_id: Uuid) -> Option<usize> {
    for item in items {
        if let CollectionItem::Folder(f) = item {
            if f.id == parent_id {
                return Some(
                    f.items
                        .iter()
                        .filter(|i| matches!(i, CollectionItem::Folder(_)))
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

pub fn method_class(m: HttpMethod) -> &'static str {
    match m {
        HttpMethod::Get => "get",
        HttpMethod::Post => "post",
        HttpMethod::Put => "put",
        HttpMethod::Patch => "patch",
        HttpMethod::Delete => "delete",
        HttpMethod::Head => "head",
        HttpMethod::Options => "options",
    }
}
