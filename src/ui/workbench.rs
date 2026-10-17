use dioxus::prelude::*;
use uuid::Uuid;

use crate::core::{
    add_folder_under, add_request_under, bundled_postman_collections, duplicate_across,
    export_collection_v21, find_environment, find_request_across, generate_snippet, grpc_unary,
    import_collection_v21, import_curl, import_openapi3, limitations_blurb, mcp_call_tool,
    mcp_list_tools, mqtt_publish, mqtt_subscribe_once, move_item_to, purge_from_trash, push_console,
    push_history, remove_across, rename_across, restore_from_trash, routes_from_collections,
    run_performance, save_example_to_request, save_request_to_collections, snippet_languages,
    sort_collections, split_mcp_args, start_capture_proxy, start_mock, trash_collection,
    ws_connect_session, AppSettings, CaptureProxyHandle, Collection, ConsoleLine, CookieJar,
    EnvVar, Environment, Folder, HistoryEntry, MockHandle, PerfReport, RequestDraft,
    RequestProtocol, ResponseView, SavedExample, VariableContext, VaultStore, WsSession,
    HISTORY_MAX,
};
use crate::runtime::{
    run_collection_with_config, send_request_with_options, RunnerConfig, RunnerReport, SendOptions,
};
use std::sync::Mutex;
use std::time::Duration;

static MOCK_HANDLE: Mutex<Option<MockHandle>> = Mutex::new(None);
static WS_SESSION: Mutex<Option<WsSession>> = Mutex::new(None);
static PROXY_HANDLE: Mutex<Option<CaptureProxyHandle>> = Mutex::new(None);
use crate::storage::{AppSnapshot, JsonStore};
use crate::ui::console_panel::ConsolePanel;
use crate::ui::env_manager::{EnvManager, EnvManagerEvent};
use crate::ui::env_selector::EnvSelector;
use crate::ui::request_builder::RequestBuilder;
use crate::ui::response_viewer::ResponseViewer;
use crate::ui::runner_panel::{RunnerEvent, RunnerPanel};
use crate::ui::settings_panel::{SettingsEvent, SettingsPanel};
use crate::ui::sidebar::{Sidebar, SidebarAction, SidebarEvent};

#[component]
pub fn Workbench() -> Element {
    let mut draft = use_signal(RequestDraft::default);
    let mut response = use_signal(|| None::<ResponseView>);
    let mut sending = use_signal(|| false);
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
    let mut app_settings = use_signal(AppSettings::default);
    let mut data_root = use_signal(|| String::new());
    let mut show_settings = use_signal(|| false);
    let mut show_runner = use_signal(|| false);
    let mut runner_running = use_signal(|| false);
    let mut runner_report = use_signal(|| None::<RunnerReport>);
    let mut trash = use_signal(Vec::<Collection>::new);
    let mut open_tabs = use_signal(|| vec![RequestDraft::default()]);
    let mut show_eye = use_signal(|| false);
    let mut show_palette = use_signal(|| false);
    let mut show_trash = use_signal(|| false);
    let mut show_snippets = use_signal(|| false);
    let mut snippet_lang = use_signal(|| "cURL".to_string());
    let mut mock_port = use_signal(|| None::<u16>);
    let mut mock_status = use_signal(|| String::new());
    let mut ws_connected = use_signal(|| false);
    let mut protocol_log = use_signal(Vec::<String>::new);
    let mut proxy_port = use_signal(|| None::<u16>);
    let mut show_perf = use_signal(|| false);
    let mut perf_running = use_signal(|| false);
    let mut perf_report = use_signal(|| None::<PerfReport>);
    let mut perf_vus = use_signal(|| 5u32);
    let mut perf_secs = use_signal(|| 5u64);
    let mut file_menu_open = use_signal(|| false);

    use_effect(move || {
        if loaded() {
            return;
        }
        loaded.set(true);
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
                        &mut status_msg,
                        store.root(),
                    );
                }
                Err(e) => status_msg.set(format!("load error: {e}")),
            }
        }
    });

    let mut persist = move || {
        if let Ok(store) = JsonStore::open_default() {
            let active_environment = find_environment(&environments(), active_env_id())
                .map(|e| e.name.clone())
                .unwrap_or_else(|| "No Environment".into());
            // Persist vault file separately (not inside snapshot vars).
            let v = vars();
            let _ = v.vault.save(store.root());
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
                settings: app_settings(),
                trash: trash(),
            };
            match store.save(&snap) {
                Ok(()) => status_msg.set(format!("saved · {}", store.root().display())),
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

    let do_send = use_callback(move |_| {
        if sending() {
            return;
        }
        let req = draft();

        // gRPC unary via shared Send button / Invoke
        if req.protocol == RequestProtocol::Grpc {
            sending.set(true);
            response.set(None);
            let endpoint = req.url.clone();
            let method = req.grpc_method.clone();
            let body = req.body.clone();
            let resolved = format!("{endpoint}/{method}");
            spawn(async move {
                let ep = endpoint.clone();
                let meth = method.clone();
                let result = tokio::task::spawn_blocking(move || grpc_unary(&ep, &meth, &body)).await;
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
                        }
                    }
                    Err(e) => ResponseView::error(format!("join: {e}"), vec![]),
                };
                let mut lines = console_lines();
                for msg in &view.console_messages {
                    push_console(&mut lines, ConsoleLine::info(msg.clone()));
                }
                console_lines.set(lines);
                protocol_log.set(view.pipeline_log.clone());
                response.set(Some(view));
                sending.set(false);
            });
            return;
        }

        if req.protocol != RequestProtocol::Http {
            status_msg.set("use protocol buttons for WS/MQTT/MCP".into());
            return;
        }

        sending.set(true);
        response.set(None);

        let mut ctx = vars();
        if let Some(env) = find_environment(&environments(), active_env_id()) {
            ctx.environment = env.to_map();
            ctx.local
                .insert("environment".into(), env.name.clone());
        }
        let col_auth = active_collection_id().and_then(|id| {
            collections()
                .into_iter()
                .find(|c| c.id == id)
                .map(|c| c.auth)
        });
        let mut jar = cookies();
        let settings = app_settings();

        spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                send_request_with_options(
                    &req,
                    &ctx,
                    SendOptions {
                        secrets_store: None,
                        collection_auth: col_auth.as_ref(),
                        cookie_jar: Some(&mut jar),
                        app_settings: Some(&settings),
                        ..Default::default()
                    },
                )
                .map(|view| (view, jar, ctx))
            })
            .await;

            let (view, new_jar, new_ctx) = match result {
                Ok(Ok((view, jar, ctx))) => (view, jar, Some(ctx)),
                Ok(Err(e)) => (
                    ResponseView::error(e.to_string(), vec!["pipeline failed".into()]),
                    cookies(),
                    None,
                ),
                Err(e) => (
                    ResponseView::error(format!("join: {e}"), vec![]),
                    cookies(),
                    None,
                ),
            };

            cookies.set(new_jar);

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

            let mut lines = console_lines();
            for msg in &view.console_messages {
                push_console(&mut lines, ConsoleLine::info(msg.clone()));
            }
            if let Some(err) = &view.error {
                push_console(&mut lines, ConsoleLine::error(err.clone()));
            }
            console_lines.set(lines);
            console_open.set(true);

            if let Some(mut v) = new_ctx {
                // Write env mutations from scripts back into active Environment.
                if let Some(id) = active_env_id() {
                    let mut envs = environments();
                    if let Some(env) = envs.iter_mut().find(|e| e.id == id) {
                        for (k, val) in v.environment.iter() {
                            if let Some(ev) = env.values.iter_mut().find(|ev| ev.key == *k) {
                                ev.current_value = val.clone();
                            } else {
                                env.values.push(EnvVar {
                                    key: k.clone(),
                                    initial_value: val.clone(),
                                    current_value: val.clone(),
                                    enabled: true,
                                });
                            }
                        }
                    }
                    environments.set(envs);
                }
                let vault = vars().vault.clone();
                v.vault = vault;
                vars.set(v);
            }

            response.set(Some(view));
            sending.set(false);
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
                        response.set(None);
                        persist();
                    }
                }
            }
            SidebarAction::SelectCollection => {
                let col_id = evt.collection_id.or(evt.id);
                active_collection_id.set(col_id);
                active_request_id.set(None);
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
                    let mut cols = collections();
                    let mut tr = trash();
                    if trash_collection(&mut cols, &mut tr, id) {
                        trash.set(tr);
                        if active_collection_id() == Some(id) {
                            active_collection_id.set(cols.first().map(|c| c.id));
                        }
                        status_msg.set("moved collection to Trash".into());
                    } else {
                        remove_across(&mut cols, id);
                        if active_request_id() == Some(id) {
                            active_request_id.set(None);
                            draft.set(RequestDraft::default());
                        }
                    }
                    collections.set(cols);
                    persist();
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
                    if move_item_to(&mut cols, item_id, target_id) {
                        collections.set(cols);
                        status_msg.set("moved item".into());
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
        }
    };

    let on_env_manager = move |evt: EnvManagerEvent| {
        match evt {
            EnvManagerEvent::Close => show_env_manager.set(false),
            EnvManagerEvent::Select(id) => {
                active_env_id.set(id);
                editing_env_id.set(id);
                let mut v = vars();
                sync_env_vars(&environments(), id, &mut v);
                vars.set(v);
                persist();
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

    rsx! {
        div {
            class: "app-shell",
            tabindex: "0",
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
                        // Close: clear to untitled
                        draft.set(RequestDraft::default());
                        active_request_id.set(None);
                        response.set(None);
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

            header { class: "top-bar",
                span { class: "brand",
                    span { class: "brand-mark" }
                    span { class: "brand-not", "not" }
                    span { class: "brand-name", "Postman" }
                }
                span { class: "shortcut-strip",
                    kbd { "⌘↵" } " Send · "
                    kbd { "⌘S" } " Save · "
                    kbd { "⌘T" } " New · "
                    kbd { "⌘⇧P" } " Palette"
                }
                span { class: "spacer" }
                div { class: "top-actions",
                div { class: "menu-file",
                    button {
                        class: "btn btn-ghost",
                        title: "File",
                        onclick: move |_| file_menu_open.set(!file_menu_open()),
                        "File ▾"
                    }
                    if file_menu_open() {
                        div { class: "menu-dropdown",
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    file_menu_open.set(false);
                                    let picked = rfd::FileDialog::new()
                                        .set_title("Import from Postman export")
                                        .add_filter("Postman Collection", &["json"])
                                        .pick_files();
                                    let Some(paths) = picked else {
                                        status_msg.set("import cancelled".into());
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
                                        collections.set(cols);
                                        if let Some(id) = last_id {
                                            active_collection_id.set(Some(id));
                                        }
                                        persist();
                                    }
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
                                    collections.set(cols);
                                    if let Some(id) = last_id {
                                        active_collection_id.set(Some(id));
                                    }
                                    persist();
                                    status_msg.set(format!(
                                        "loaded {n} bundled Postman export(s)"
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
                        }
                    }
                }
                button {
                    class: "btn btn-ghost",
                    title: "Variables quick-look",
                    onclick: move |_| show_eye.set(!show_eye()),
                    "Vars"
                }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| show_snippets.set(!show_snippets()),
                    "Code"
                }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| {
                        if mock_port().is_some() {
                            if let Ok(mut g) = MOCK_HANDLE.lock() {
                                if let Some(h) = g.take() {
                                    h.stop();
                                }
                            }
                            mock_port.set(None);
                            mock_status.set("mock stopped".into());
                        } else {
                            let routes = routes_from_collections(&collections());
                            match start_mock(routes, 0) {
                                Ok(h) => {
                                    let p = h.port;
                                    if let Ok(mut g) = MOCK_HANDLE.lock() {
                                        *g = Some(h);
                                    }
                                    mock_port.set(Some(p));
                                    mock_status.set(format!("mock http://127.0.0.1:{p}"));
                                }
                                Err(e) => mock_status.set(format!("mock error: {e}")),
                            }
                        }
                    },
                    if mock_port().is_some() { "Stop mock" } else { "Mock" }
                }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| show_trash.set(!show_trash()),
                    "Trash"
                }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| show_runner.set(true),
                    "Runner"
                }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| show_perf.set(true),
                    "Perf"
                }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| {
                        if proxy_port().is_some() {
                            if let Ok(mut g) = PROXY_HANDLE.lock() {
                                if let Some(h) = g.take() {
                                    // Flush captures into history
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
                                        "capture proxy http://127.0.0.1:{p} (HTTP absolute-form; no CONNECT MITM)"
                                    ));
                                }
                                Err(e) => status_msg.set(format!("proxy error: {e}")),
                            }
                        }
                    },
                    if proxy_port().is_some() { "Stop proxy" } else { "Proxy" }
                }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| show_settings.set(true),
                    "Settings"
                }
                EnvSelector {
                    environments: environments(),
                    active_id: active_env_id(),
                    on_change: move |id| {
                        active_env_id.set(id);
                        editing_env_id.set(id);
                        let mut v = vars();
                        sync_env_vars(&environments(), id, &mut v);
                        vars.set(v);
                        persist();
                    },
                    on_manage: move |_| {
                        show_env_manager.set(true);
                        editing_env_id.set(active_env_id());
                    },
                }
                }
            }

            if show_eye() {
                div { class: "eye-pane",
                    h4 { "Variables in scope" }
                    for (scope, map) in [
                        ("local", vars().local.clone()),
                        ("environment", vars().environment.clone()),
                        ("collection", vars().collection.clone()),
                        ("global", vars().global.clone()),
                    ] {
                        p { class: "hint", "{scope}" }
                        for (k, v) in map.iter() {
                            div { class: "eye-row", code { "{k}" } " = " code { "{v}" } }
                        }
                    }
                    p { class: "hint", "vault keys: {vars().vault.entries.len()}" }
                }
            }

            if show_palette() {
                div { class: "modal-backdrop",
                    div { class: "modal palette-modal",
                        h3 { "Command palette" }
                        button { class: "btn btn-ghost", onclick: move |_| { do_send(()); show_palette.set(false); }, "Send request" }
                        button { class: "btn btn-ghost", onclick: move |_| { do_save(()); show_palette.set(false); }, "Save" }
                        button { class: "btn btn-ghost", onclick: move |_| { do_new_tab(()); show_palette.set(false); }, "New request" }
                        button { class: "btn btn-ghost", onclick: move |_| { show_runner.set(true); show_palette.set(false); }, "Runner" }
                        button { class: "btn btn-ghost", onclick: move |_| { console_open.set(true); show_palette.set(false); }, "Console" }
                        button { class: "btn btn-ghost", onclick: move |_| { show_settings.set(true); show_palette.set(false); }, "Settings" }
                        button { class: "btn btn-ghost", onclick: move |_| show_palette.set(false), "Close" }
                    }
                }
            }

            if show_trash() {
                div { class: "modal-backdrop",
                    div { class: "modal",
                        h3 { "Trash" }
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
                        button { class: "btn btn-ghost", onclick: move |_| show_trash.set(false), "Close" }
                    }
                }
            }

            if show_snippets() {
                div { class: "modal-backdrop",
                    div { class: "modal",
                        h3 { "Code snippet" }
                        select {
                            value: "{snippet_lang()}",
                            onchange: move |evt| snippet_lang.set(evt.value()),
                            for lang in snippet_languages() {
                                option { value: "{lang}", "{lang}" }
                            }
                        }
                        pre { class: "response-body", "{generate_snippet(&snippet_lang(), &draft())}" }
                        button { class: "btn btn-ghost", onclick: move |_| show_snippets.set(false), "Close" }
                    }
                }
            }

            if !mock_status().is_empty() {
                p { class: "hint", style: "padding: 4px 12px;", "{mock_status}" }
            }

            if show_settings() {
                SettingsPanel {
                    settings: app_settings(),
                    data_root: data_root(),
                    on_event: move |evt| match evt {
                        SettingsEvent::Close => show_settings.set(false),
                        SettingsEvent::Save(s) => {
                            app_settings.set(s);
                            show_settings.set(false);
                            persist();
                            status_msg.set("settings saved".into());
                        }
                    },
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
                            if let Some(env) = find_environment(&environments(), active_env_id()) {
                                ctx.environment = env.to_map();
                            }
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
                                        &ctx,
                                        &mut jar,
                                        Some(&settings),
                                        None,
                                        &cfg,
                                    );
                                    (rep, jar)
                                })
                                .await;
                                match report {
                                    Ok((rep, jar)) => {
                                        cookies.set(jar);
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
                div { class: "modal-backdrop",
                    div { class: "modal runner-modal",
                        h3 { "Performance (minimal VUs)" }
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
                                if perf_running() { "Running…" } else { "Start" }
                            }
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| show_perf.set(false),
                                "Close"
                            }
                        }
                    }
                }
            }

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
                Sidebar {
                    collections: collections(),
                    active_collection_id: active_collection_id(),
                    active_request_id: active_request_id(),
                    history: history(),
                    on_event: on_sidebar,
                }
            }

            main { class: "workbench",
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
                            let tname = t.name.clone();
                            let active = draft().id == tid;
                            rsx! {
                                button {
                                    class: if active { "tab active" } else { "tab" },
                                    onclick: move |_| {
                                        let mut tabs = open_tabs();
                                        let cur = draft();
                                        if let Some(slot) = tabs.iter_mut().find(|x| x.id == cur.id) {
                                            *slot = cur;
                                        }
                                        if let Some(found) = tabs.iter().find(|x| x.id == tid).cloned() {
                                            draft.set(found);
                                            active_request_id.set(Some(tid));
                                            response.set(None);
                                        }
                                        open_tabs.set(tabs);
                                    },
                                    "{tname}"
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

                RequestBuilder {
                    draft: draft(),
                    on_change: move |d| draft.set(d),
                    on_send: move |_| do_send(()),
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
                            _ => {}
                        }
                    },
                    sending: sending(),
                    ws_connected: ws_connected(),
                    protocol_log: protocol_log(),
                }

                ResponseViewer {
                    response: response(),
                    sending: sending(),
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

                ConsolePanel {
                    open: console_open(),
                    lines: console_lines(),
                    on_toggle: move |_| console_open.set(!console_open()),
                    on_clear: move |_| console_lines.set(vec![]),
                }

                if !status_msg().is_empty() {
                    p { class: "status-line", "{status_msg}" }
                }
            }

            if let Some(modal) = impex_modal() {
                ImpexDialog {
                    modal,
                    on_close: move |_| impex_modal.set(None),
                    on_import: move |text: String| {
                        let text = text.trim().to_string();
                        let imported = if text.trim_start().starts_with('{')
                            && text.contains("\"openapi\"")
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
                                let mut cols = collections();
                                cols.push(col);
                                collections.set(cols);
                                active_collection_id.set(Some(id));
                                impex_modal.set(None);
                                persist();
                                status_msg.set("imported collection".into());
                            }
                            Err(e) => status_msg.set(format!("import error: {e}")),
                        }
                    },
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
        div { class: "modal-backdrop",
            div { class: "modal",
                h3 { "{title}" }
                p { class: "hint", "{limitations_blurb()}" }
                textarea {
                    class: "impex-editor",
                    value: "{text}",
                    oninput: move |evt| text.set(evt.value()),
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
    app_settings.set(snap.settings);
    trash.set(snap.trash);
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
