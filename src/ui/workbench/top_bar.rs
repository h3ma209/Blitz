use dioxus::prelude::*;

use crate::core::{
    bundled_postman_collections, check_for_updates, export_collection_v21,
    export_environment_json, generate_static_doc_html, generate_swagger_ui_html,
    import_auto_bundle, import_bruno_dir, import_environment_texts,
    interceptor_limits_blurb, open_browser_url, push_history, routes_from_collections,
    start_capture_proxy, start_mock, upsert_environments, HistoryEntry,
    HttpMethod, ImportBundle, RequestDraft, UiSettings, UiTheme, default_zoom_percent,
    HISTORY_MAX,
};
use crate::ui::env_selector::EnvSelector;
use crate::ui::theme_selector::ThemeSelector;
use crate::ui::workbench::cloud::{
    cloud_clone_or_link, cloud_link_folder, cloud_open_folder, cloud_pull, cloud_push,
    cloud_sync_status, open_team_activity, request_git_switch, switch_to_env, sync_selected_env,
};
use crate::ui::workbench::state::{ImpexModal, WorkbenchState, MOCK_HANDLE, PROXY_HANDLE};

#[component]
pub fn TopBar(st: WorkbenchState) -> Element {
    let mut file_menu_open = st.file_menu_open;
    let mut tools_menu_open = st.tools_menu_open;
    let mut view_menu_open = st.view_menu_open;
    let mut cloud_menu_open = st.cloud_menu_open;
    let mut is_loading = st.is_loading;
    let mut collections = st.collections;
    let mut environments = st.environments;
    let mut active_collection_id = st.active_collection_id;
    let mut active_env_id = st.active_env_id;
    let mut active_request_id = st.active_request_id;
    let mut editing_collection_id = st.editing_collection_id;
    let mut editing_env_id = st.editing_env_id;
    let mut vars = st.vars;
    let mut fold_epoch = st.fold_epoch;
    let mut status_msg = st.status_msg;
    let mut impex_modal = st.impex_modal;
    let mut show_runner = st.show_runner;
    let mut show_lint = st.show_lint;
    let mut show_perf = st.show_perf;
    let mut mock_port = st.mock_port;
    let mut proxy_port = st.proxy_port;
    let mut history = st.history;
    let mut show_cookies = st.show_cookies;
    let mut col_vars_all = st.col_vars_all;
    let mut col_vars_open_id = st.col_vars_open_id;
    let mut show_col_vars = st.show_col_vars;
    let mut show_trash = st.show_trash;
    let mut checking_update = st.checking_update;
    let mut available_update = st.available_update;
    let mut show_settings = st.show_settings;
    let mut app_settings = st.app_settings;
    let mut show_vars = st.show_vars;
    let mut vars_active_tab = st.vars_active_tab;
    let show_team = st.show_team;
    let conflict_state = st.conflict_state;
    let cloud_busy = st.cloud_busy;

    let top_nav_menus_open =
        file_menu_open() || tools_menu_open() || view_menu_open() || cloud_menu_open();
    let active_session_label = crate::storage::JsonStore::active_session_name();
    let top_bar_class = if top_nav_menus_open {
        "top-bar top-bar-elevated"
    } else {
        "top-bar"
    };

    rsx! {
        header { class: "{top_bar_class}",
            span {
                class: "brand",
                title: "Blitz · ⌘↵ Send/Cancel · ⌘S Save · ⌘T New · ⌘K Palette · ⌘+/- Zoom",
                span { class: "brand-name", "Blitz" }
            }

            {if is_loading() || cloud_busy() {
                rsx! {
                    div { class: "loading-indicator",
                        span { class: "loading-spinner" }
                        span { if cloud_busy() { "Syncing…" } else { "Loading..." } }
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
                            view_menu_open.set(false);
                            cloud_menu_open.set(false);
                            file_menu_open.set(!file_menu_open());
                        },
                        "File"
                        span { class: "top-caret", "▾" }
                    }
                    if file_menu_open() {
                        div { class: "menu-dropdown menu-dropdown-wide",
                            onmousedown: move |evt| evt.stop_propagation(),
                            div { class: "menu-category-header", "Window & Session" }
                            button {
                                class: "menu-item",
                                title: "Open a new Blitz window with isolated auth tokens, cookies, and WebSocket sessions",
                                onclick: move |_| {
                                    file_menu_open.set(false);
                                    match spawn_new_blitz_window(true) {
                                        Ok(label) => status_msg.set(format!("opened isolated window: {label}")),
                                        Err(e) => status_msg.set(format!("new window error: {e}")),
                                    }
                                },
                                "New Session Window (Isolated Auth)"
                            }
                            button {
                                class: "menu-item",
                                title: "Open another Blitz window sharing the same workspace",
                                onclick: move |_| {
                                    file_menu_open.set(false);
                                    match spawn_new_blitz_window(false) {
                                        Ok(_) => status_msg.set("opened new Blitz window".into()),
                                        Err(e) => status_msg.set(format!("new window error: {e}")),
                                    }
                                },
                                "New Window (Shared Workspace)"
                            }
                            div { class: "menu-category-header", "Import" }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    file_menu_open.set(false);
                                    is_loading.set(true);
                                    let mut st = st;
                                    spawn(async move {
                                        let picked = rfd::FileDialog::new()
                                            .set_title("Import Postman / Bruno / Yaak / OpenAPI")
                                            .add_filter(
                                                "API Exports",
                                                &["json", "yaml", "yml", "bru", "har"],
                                            )
                                            .pick_files();
                                        let Some(paths) = picked else {
                                            is_loading.set(false);
                                            return;
                                        };
                                        let mut merged = ImportBundle::default();
                                        let mut errs = Vec::new();
                                        for path in paths {
                                            match std::fs::read_to_string(&path) {
                                                Ok(text) => match import_auto_bundle(&text) {
                                                    Ok(bundle) => merged.merge(bundle),
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
                                        let summary = if !merged.is_empty() {
                                            st.apply_import_bundle(merged)
                                        } else {
                                            "nothing imported".to_string()
                                        };
                                        is_loading.set(false);
                                        if errs.is_empty() {
                                            status_msg.set(summary);
                                        } else {
                                            status_msg.set(format!(
                                                "{summary}; errors: {}",
                                                errs.join("; ")
                                            ));
                                        }
                                    });
                                },
                                "Import Postman / Bruno / Yaak…"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    file_menu_open.set(false);
                                    is_loading.set(true);
                                    let mut st = st;
                                    spawn(async move {
                                        let picked = rfd::FileDialog::new()
                                            .set_title("Select Bruno Collection Folder")
                                            .pick_folder();
                                        let Some(dir) = picked else {
                                            is_loading.set(false);
                                            return;
                                        };
                                        match import_bruno_dir(&dir) {
                                            Ok(bundle) => {
                                                let summary = st.apply_import_bundle(bundle);
                                                status_msg.set(summary);
                                            }
                                            Err(e) => status_msg.set(format!("bruno import: {e}")),
                                        }
                                        is_loading.set(false);
                                    });
                                },
                                "Import Bruno Folder…"
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
                                    let mut st = st;
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
                                    st.persist();
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
                                    is_loading.set(true);
                                    let mut st = st;
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
                                                let mut store = environments();
                                                let (ids, created_n, updated_n) =
                                                    upsert_environments(&mut store, envs);
                                                if let Some(&last_id) = ids.last() {
                                                    active_env_id.set(Some(last_id));
                                                }
                                                environments.set(store);
                                                st.persist();
                                                status_msg.set(format!(
                                                    "env import: {created_n} added, {updated_n} updated"
                                                ));
                                            }
                                            Err(e) => status_msg.set(format!("env import: {e}")),
                                        }
                                        is_loading.set(false);
                                    });
                                },
                                "Import Environment(s)…"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    file_menu_open.set(false);
                                    is_loading.set(true);
                                    let mut st = st;
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
                                            Ok(text) => match import_auto_bundle(&text) {
                                                Ok(bundle) => {
                                                    let summary = st.apply_import_bundle(bundle);
                                                    status_msg.set(summary);
                                                }
                                                Err(e) => status_msg.set(format!("import: {e}")),
                                            },
                                            Err(e) => status_msg.set(format!("read: {e}")),
                                        }
                                        is_loading.set(false);
                                    });
                                },
                                "Import HAR / OpenAPI…"
                            }
                            div { class: "menu-category-header", "Export & Docs" }
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
                                "Export Active Collection (JSON)…"
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
                                "Export Active Environment…"
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
                                        Some(c) => {
                                            let html = generate_static_doc_html(c);
                                            impex_modal.set(Some(ImpexModal::Export {
                                                name: format!("{}_docs.html", c.name.to_lowercase().replace(' ', "_")),
                                                text: html,
                                            }));
                                        }
                                        None => status_msg.set("no collection to export docs".into()),
                                    }
                                },
                                "Export Static HTML Docs…"
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
                                        Some(c) => match generate_swagger_ui_html(c) {
                                            Ok(html) => {
                                                impex_modal.set(Some(ImpexModal::Export {
                                                    name: format!("{}_swagger.html", c.name.to_lowercase().replace(' ', "_")),
                                                    text: html,
                                                }));
                                            }
                                            Err(err) => status_msg.set(format!("failed to export swagger: {}", err)),
                                        },
                                        None => status_msg.set("no collection to export swagger".into()),
                                    }
                                },
                                "Export Swagger UI Docs…"
                            }
                        }
                    }
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
                            view_menu_open.set(false);
                            cloud_menu_open.set(false);
                            tools_menu_open.set(!tools_menu_open());
                        },
                        "Tools"
                        span { class: "top-caret", "▾" }
                    }
                    if tools_menu_open() {
                        div { class: "menu-dropdown menu-dropdown-wide",
                            onmousedown: move |evt| evt.stop_propagation(),
                            div { class: "menu-category-header", "Run & Quality" }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    tools_menu_open.set(false);
                                    let mut st = st;
                                    st.hide_stage_pages();
                                    st.runner_tab_open.set(true);
                                    show_runner.set(true);
                                },
                                "Collection Runner…"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    tools_menu_open.set(false);
                                    let mut st = st;
                                    st.hide_stage_pages();
                                    st.lint_tab_open.set(true);
                                    show_lint.set(true);
                                },
                                "Collection lint…"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    tools_menu_open.set(false);
                                    let mut st = st;
                                    st.hide_stage_pages();
                                    st.perf_tab_open.set(true);
                                    show_perf.set(true);
                                },
                                "Performance…"
                            }
                            div { class: "menu-category-header", "Local Servers" }
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
                                    let mut st = st;
                                    if proxy_port().is_some() {
                                        if let Ok(mut g) = PROXY_HANDLE.lock() {
                                            if let Some(h) = g.take() {
                                                for cap in h.snapshot() {
                                                    let mut hist = history();
                                                    let d = RequestDraft {
                                                        method: match cap.method.as_str() {
                                                            "POST" => HttpMethod::Post,
                                                            "PUT" => HttpMethod::Put,
                                                            "PATCH" => HttpMethod::Patch,
                                                            "DELETE" => HttpMethod::Delete,
                                                            _ => HttpMethod::Get,
                                                        },
                                                        url: cap.url.clone(),
                                                        ..Default::default()
                                                    };
                                                    let entry = HistoryEntry::from_send(
                                                        &d,
                                                        &cap.url,
                                                        cap.status,
                                                        cap.elapsed_ms,
                                                        None,
                                                    );
                                                    push_history(&mut hist, entry, HISTORY_MAX);
                                                    history.set(hist);
                                                }
                                                h.stop();
                                            }
                                        }
                                        proxy_port.set(None);
                                        status_msg.set("capture proxy stopped".into());
                                        st.persist();
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
                            div { class: "menu-category-header", "Workspace & Data" }
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
                        class: if view_menu_open() { "top-link top-link-active" } else { "top-link" },
                        title: "View, Windows & Help",
                        onmousedown: move |evt| evt.stop_propagation(),
                        onclick: move |_| {
                            file_menu_open.set(false);
                            tools_menu_open.set(false);
                            cloud_menu_open.set(false);
                            view_menu_open.set(!view_menu_open());
                        },
                        "View"
                        span { class: "top-caret", "▾" }
                    }
                    if view_menu_open() {
                        div { class: "menu-dropdown menu-dropdown-wide",
                            onmousedown: move |evt| evt.stop_propagation(),
                            div { class: "menu-category-header", "Zoom & Display" }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    view_menu_open.set(false);
                                    let mut s = app_settings();
                                    let old_z = s.ui.zoom_percent;
                                    let new_z = UiSettings::clamp_zoom_percent(old_z.saturating_add(UiSettings::ZOOM_STEP_PERCENT));
                                    if new_z != old_z {
                                        s.ui.zoom_percent = new_z;
                                        app_settings.set(s);
                                        st.persist_local();
                                    }
                                    status_msg.set(format!("Zoom: {new_z}%"));
                                },
                                "Zoom In (⌘+)"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    view_menu_open.set(false);
                                    let mut s = app_settings();
                                    let old_z = s.ui.zoom_percent;
                                    let new_z = UiSettings::clamp_zoom_percent(old_z.saturating_sub(UiSettings::ZOOM_STEP_PERCENT));
                                    if new_z != old_z {
                                        s.ui.zoom_percent = new_z;
                                        app_settings.set(s);
                                        st.persist_local();
                                    }
                                    status_msg.set(format!("Zoom: {new_z}%"));
                                },
                                "Zoom Out (⌘-)"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    view_menu_open.set(false);
                                    let mut s = app_settings();
                                    let new_z = default_zoom_percent();
                                    if s.ui.zoom_percent != new_z {
                                        s.ui.zoom_percent = new_z;
                                        app_settings.set(s);
                                        st.persist_local();
                                    }
                                    status_msg.set(format!("Zoom reset: {new_z}%"));
                                },
                                "Reset Zoom (⌘0 · {app_settings().ui.zoom_percent}%)"
                            }
                            div { class: "menu-category-header", "Windows" }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    view_menu_open.set(false);
                                    match spawn_new_blitz_window(true) {
                                        Ok(label) => status_msg.set(format!("opened isolated window: {label}")),
                                        Err(e) => status_msg.set(format!("new window error: {e}")),
                                    }
                                },
                                "New Isolated Session Window"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    view_menu_open.set(false);
                                    match spawn_new_blitz_window(false) {
                                        Ok(_) => status_msg.set("opened new Blitz window".into()),
                                        Err(e) => status_msg.set(format!("new window error: {e}")),
                                    }
                                },
                                "New Shared Window"
                            }
                            div { class: "menu-category-header", "Help & Updates" }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    view_menu_open.set(false);
                                    checking_update.set(true);
                                    status_msg.set("checking for updates…".into());
                                    spawn(async move {
                                        let res = tokio::task::spawn_blocking(check_for_updates).await.ok().flatten();
                                        checking_update.set(false);
                                        match res {
                                            Some(info) if info.has_update => {
                                                status_msg.set(format!("New version v{} available!", info.latest_version));
                                                available_update.set(Some(info));
                                                let mut show_upd = st.show_update_modal;
                                                show_upd.set(true);
                                            }
                                            Some(info) => {
                                                status_msg.set(format!("Blitz v{} is up to date", info.current_version));
                                            }
                                            None => {
                                                status_msg.set("could not check for updates (offline or rate-limited)".into());
                                            }
                                        }
                                    });
                                },
                                "Check for updates…"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    view_menu_open.set(false);
                                    open_browser_url("https://h3ma209.github.io/Blitz/docs.html");
                                },
                                "Documentation & Reference…"
                            }
                        }
                    }
                }

                div { class: "menu-file",
                    button {
                        class: if cloud_menu_open() {
                            "top-link top-link-active"
                        } else {
                            "top-link"
                        },
                        title: "Git cloud — Pull / Push / status (blitz-cloud)",
                        onmousedown: move |evt| evt.stop_propagation(),
                        onclick: move |_| {
                            file_menu_open.set(false);
                            tools_menu_open.set(false);
                            view_menu_open.set(false);
                            cloud_menu_open.set(!cloud_menu_open());
                        },
                        "Cloud"
                        span { class: "top-caret", "▾" }
                    }
                    if cloud_menu_open() {
                        div { class: "menu-dropdown menu-dropdown-wide",
                            onmousedown: move |evt| evt.stop_propagation(),
                            div { class: "menu-category-header", "Git Repository" }
                            button {
                                class: "menu-item",
                                disabled: cloud_busy(),
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    cloud_clone_or_link(st);
                                },
                                "Clone / Link Remote…"
                            }
                            button {
                                class: "menu-item",
                                disabled: cloud_busy(),
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    cloud_link_folder(st);
                                },
                                "Link Local Folder…"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    cloud_open_folder(st);
                                },
                                "Open Cloud Folder"
                            }
                            {
                                let cs = app_settings().cloud_sync;
                                let recents = cs.recent_repos.clone();
                                let cur_rem = cs.remote_url.clone();
                                let cur_cl = cs.clone_path.clone();
                                rsx! {
                                    if !recents.is_empty() {
                                        div { class: "menu-category-header", "Recent Git Repositories" }
                                        for (ri, entry) in recents.iter().enumerate() {
                                            {
                                                let short = entry.short_name();
                                                let target = entry.display_target();
                                                let is_active = entry.matches(&cur_rem, &cur_cl);
                                                let rem_sw = entry.remote_url.clone();
                                                let cl_sw = entry.clone_path.clone();
                                                rsx! {
                                                    button {
                                                        key: "top-recent-git-{ri}-{target}",
                                                        class: "menu-item",
                                                        disabled: cloud_busy() || is_active,
                                                        title: if is_active { format!("Active: {target}") } else { format!("Switch to {target}") },
                                                        onclick: move |_| {
                                                            cloud_menu_open.set(false);
                                                            let mut s = app_settings();
                                                            s.cloud_sync.enabled = true;
                                                            s.cloud_sync.remote_url = rem_sw.clone();
                                                            s.cloud_sync.clone_path = cl_sw.clone();
                                                            s.cloud_sync.last_status.clear();
                                                            s.cloud_sync.last_branch.clear();
                                                            s.cloud_sync.record_recent_repo();
                                                            request_git_switch(st, s);
                                                        },
                                                        if is_active {
                                                            "✓ {short}"
                                                        } else {
                                                            "{short}"
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            div { class: "menu-category-header", "Sync & Changes" }
                            button {
                                class: "menu-item",
                                disabled: cloud_busy(),
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    cloud_sync_status(st);
                                },
                                "Sync Status"
                            }
                            button {
                                class: "menu-item",
                                disabled: cloud_busy(),
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    cloud_pull(st);
                                },
                                "Pull from Remote"
                            }
                            button {
                                class: "menu-item",
                                disabled: cloud_busy(),
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    cloud_push(st);
                                },
                                "Push to Remote"
                            }
                            div { class: "menu-category-header", "Team & Configuration" }
                            button {
                                class: "menu-item",
                                disabled: cloud_busy(),
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    open_team_activity(st, false);
                                },
                                "Team Activity…"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    let mut settings_tab = st.settings_tab;
                                    settings_tab.set(crate::ui::settings_panel::SettingsTab::CloudSync);
                                    st.settings_tab_open.set(true);
                                    show_vars.set(false);
                                    show_settings.set(true);
                                },
                                "Cloud Settings…"
                            }
                        }
                    }
                }
            } // top-nav

            span { class: "spacer" }

            div { class: "top-end",
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
                if let Some(conf) = conflict_state() {
                    button {
                        class: "status-chip status-err",
                        style: "cursor: pointer; margin-right: 4px; font-weight: 700;",
                        title: "Click to resolve git merge conflicts in main section",
                        onclick: move |_| {
                            active_request_id.set(None);
                            editing_collection_id.set(None);
                            show_settings.set(false);
                        },
                        crate::ui::icons::ActionIcon { name: "git-merge", size: 15, style: "margin-right: 4px;".to_string() }
                        "{conf.files.len()} Conflicts"
                    }
                }
                if let Some(ref sess_name) = active_session_label {
                    span {
                        class: "live-chip live-mock",
                        title: "Isolated window session (separate auth tokens, cookies, and WS connections)",
                        "{sess_name}"
                    }
                }
                {
                    let cs = app_settings().cloud_sync;
                    if cs.enabled && (!cs.remote_url.trim().is_empty() || !cs.clone_path.trim().is_empty()) {
                        let repo_entry = crate::core::RecentGitRepo {
                            remote_url: cs.remote_url.clone(),
                            clone_path: cs.clone_path.clone(),
                            last_branch: cs.last_branch.clone(),
                        };
                        let repo_name = repo_entry.short_name();
                        let target_desc = repo_entry.display_target();
                        let branch = if cs.last_branch.trim().is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", cs.last_branch.trim())
                        };
                        rsx! {
                            button {
                                class: "cloud-branch-chip",
                                style: "cursor: pointer; border: 1px solid rgba(56, 189, 248, 0.35);",
                                title: "Active Git repo: {target_desc}{branch} — click for Team Activity",
                                onclick: move |_| open_team_activity(st, false),
                                crate::ui::icons::ActionIcon { name: "branch", size: 13 }
                                span { "{repo_name}{branch}" }
                            }
                        }
                    } else {
                        rsx! {}
                    }
                }
                div { class: "top-nav-cluster",
                    button {
                        class: "top-icon top-icon-only",
                        title: "New isolated session window (separate auth tokens, cookies & WS for 2nd user)",
                        aria_label: "New Session Window",
                        onclick: move |_| {
                            match spawn_new_blitz_window(true) {
                                Ok(label) => status_msg.set(format!("opened isolated window: {label}")),
                                Err(e) => status_msg.set(format!("new window error: {e}")),
                            }
                        },
                        crate::ui::icons::ActionIcon { name: "users", size: 17 }
                    }
                    button {
                        class: if show_team() { "top-icon top-icon-only top-icon-on" } else { "top-icon top-icon-only" },
                        title: "Team collaboration · Members, recent git commits & edited files",
                        aria_label: "Team collaboration",
                        disabled: cloud_busy(),
                        onclick: move |_| {
                            open_team_activity(st, true);
                        },
                        crate::ui::icons::ActionIcon { name: "branch", size: 17 }
                    }
                    button {
                        class: if show_vars() { "top-icon top-icon-on" } else { "top-icon" },
                        title: "Variables (environment, collection, global, vault; local stays on this machine)",
                        onclick: move |_| {
                            if show_vars() && vars_active_tab() == 1 {
                                show_vars.set(false);
                            } else {
                                vars_active_tab.set(1);
                                st.vars_tab_open.set(true);
                                show_settings.set(false);
                                show_vars.set(true);
                            }
                        },
                        crate::ui::icons::ActionIcon { name: "braces", size: 17 }
                        span { "Vars" }
                    }
                }
                span { class: "top-nav-divider", aria_hidden: "true" }
                div { class: "top-nav-cluster top-env-territory",
                    EnvSelector {
                        environments: environments(),
                        active_id: active_env_id(),
                        vars: Some((st.vars)()),
                        on_change: move |id| {
                            switch_to_env(st, id);
                        },
                        on_sync: move |id| {
                            sync_selected_env(st, id);
                        },
                        on_manage: move |_| {
                            vars_active_tab.set(0);
                            editing_env_id.set(active_env_id());
                            st.vars_tab_open.set(true);
                            show_settings.set(false);
                            show_vars.set(true);
                        },
                        on_compare: move |_| {
                            vars_active_tab.set(2);
                            st.vars_tab_open.set(true);
                            show_settings.set(false);
                            show_vars.set(true);
                        },
                    }
                }
                span { class: "top-nav-divider", aria_hidden: "true" }
                div { class: "top-nav-cluster",
                    ThemeSelector {
                        theme: app_settings().ui.theme,
                        on_change: move |t: UiTheme| {
                            let mut s = app_settings();
                            s.ui.theme = t;
                            s.ui = s.ui.clamped();
                            app_settings.set(s);
                            st.persist_view_only();
                        },
                    }
                if let Some(upd) = available_update() {
                    div { class: "update-pill-group",
                        button {
                            class: "top-link update-pill-pulse",
                            title: format!("View update & install Blitz v{}", upd.latest_version),
                            onclick: move |_| {
                                let mut show_upd = st.show_update_modal;
                                show_upd.set(true);
                            },
                            span { class: "update-badge-dot" }
                            span {
                                if (st.installing_update)() {
                                    "Updating & Restarting…"
                                } else {
                                    "Update to v{upd.latest_version}"
                                }
                            }
                        }
                    }
                }
                button {
                    class: if show_settings() {
                        "top-icon top-icon-only top-icon-on"
                    } else {
                        "top-icon top-icon-only"
                    },
                    title: "Settings & preferences",
                    aria_label: "Settings",
                    onclick: move |_| {
                        if show_settings() {
                            show_settings.set(false);
                        } else {
                            st.settings_tab_open.set(true);
                            show_vars.set(false);
                            show_settings.set(true);
                        }
                    },
                    crate::ui::icons::ActionIcon { name: "sliders", size: 17 }
                }
                }
            }
        }
    }
}

fn spawn_new_blitz_window(isolated_session: bool) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(exe);
    if isolated_session {
        let base_sessions = crate::storage::JsonStore::base_data_root().join("sessions");
        let mut n = 2usize;
        while base_sessions.join(format!("Session-{n}")).exists() && n < 99 {
            n += 1;
        }
        let label = format!("Session {n}");
        cmd.arg("--session").arg(&label);
        cmd.spawn().map_err(|e| e.to_string())?;
        Ok(label)
    } else {
        cmd.arg("--new-window");
        cmd.spawn().map_err(|e| e.to_string())?;
        Ok("Shared Window".to_string())
    }
}
