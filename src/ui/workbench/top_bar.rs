use dioxus::prelude::*;

use crate::core::{
    bundled_postman_collections, check_for_updates, export_collection_v21,
    export_environment_json, generate_static_doc_html, generate_swagger_ui_html,
    import_collection_v21, import_environment_texts, import_har, import_openapi3,
    interceptor_limits_blurb, open_browser_url, push_history, routes_from_collections,
    start_capture_proxy, start_mock, HistoryEntry, HttpMethod, RequestDraft, UiTheme,
    WorkbenchLayout, HISTORY_MAX,
};
use crate::ui::env_selector::EnvSelector;
use crate::ui::theme_selector::ThemeSelector;
use crate::ui::workbench::cloud::{
    cloud_clone_or_link, cloud_link_folder, cloud_open_folder, cloud_pull, cloud_push,
    cloud_sync_status, open_team_activity, switch_to_env, sync_selected_env,
};
use crate::ui::workbench::state::{ImpexModal, WorkbenchState, MOCK_HANDLE, PROXY_HANDLE};

#[component]
pub fn TopBar(st: WorkbenchState) -> Element {
    let mut file_menu_open = st.file_menu_open;
    let mut tools_menu_open = st.tools_menu_open;
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
    let mut show_env_manager = st.show_env_manager;
    let mut show_settings = st.show_settings;
    let mut app_settings = st.app_settings;
    let mut show_vars = st.show_vars;
    let mut vars_active_tab = st.vars_active_tab;
    let mut show_snippets = st.show_snippets;
    let show_team = st.show_team;
    let conflict_state = st.conflict_state;
    let cloud_busy = st.cloud_busy;

    let top_nav_menus_open = file_menu_open() || tools_menu_open() || cloud_menu_open();
    let top_bar_class = if top_nav_menus_open {
        "top-bar top-bar-elevated"
    } else {
        "top-bar"
    };

    rsx! {
        header { class: "{top_bar_class}",
            span {
                class: "brand",
                title: "⚡ Blitz · ⌘↵ Send/Cancel · ⌘S Save · ⌘T New · ⌘K Palette · ⌘+/- Zoom",
                img {
                    class: "brand-logo",
                    src: "{crate::brand::LOGO_SRC.as_str()}",
                    alt: "Blitz",
                }
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
                                    let mut st = st;
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
                                            st.persist();
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
                                                let count = envs.len();
                                                let mut store = environments();
                                                for env in envs {
                                                    let id = env.id;
                                                    store.push(env);
                                                    active_env_id.set(Some(id));
                                                }
                                                environments.set(store);
                                                st.persist();
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
                                                    import_har(&text)
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
                                                        st.persist();
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
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    tools_menu_open.set(false);
                                    checking_update.set(true);
                                    status_msg.set("checking for updates…".into());
                                    spawn(async move {
                                        let res = tokio::task::spawn_blocking(check_for_updates).await.ok().flatten();
                                        checking_update.set(false);
                                        match res {
                                            Some(info) if info.has_update => {
                                                status_msg.set(format!("New version v{} available!", info.latest_version));
                                                available_update.set(Some(info));
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
                                disabled: cloud_busy(),
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    cloud_clone_or_link(st);
                                },
                                "Clone / link remote…"
                            }
                            button {
                                class: "menu-item",
                                disabled: cloud_busy(),
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    cloud_link_folder(st);
                                },
                                "Link folder…"
                            }
                            button {
                                class: "menu-item",
                                disabled: cloud_busy(),
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    cloud_sync_status(st);
                                },
                                "Sync status"
                            }
                            button {
                                class: "menu-item",
                                disabled: cloud_busy(),
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    cloud_pull(st);
                                },
                                "Pull"
                            }
                            button {
                                class: "menu-item",
                                disabled: cloud_busy(),
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    cloud_push(st);
                                },
                                "Push"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    cloud_open_folder(st);
                                },
                                "Open folder"
                            }
                            button {
                                class: "menu-item",
                                disabled: cloud_busy(),
                                onclick: move |_| {
                                    cloud_menu_open.set(false);
                                    open_team_activity(st, false);
                                },
                                "Team activity…"
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
                            let st_str = if cloud_busy() {
                                "…".to_string()
                            } else if cs.last_status.is_empty() {
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
                            format!("{branch} · {st_str}")
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
                if let Some(conf) = conflict_state() {
                    button {
                        class: "status-chip status-err",
                        style: "cursor: pointer; margin-right: 4px; font-weight: 700;",
                        title: "Click to resolve git merge conflicts in main section",
                        onclick: move |_| {
                            active_request_id.set(None);
                            editing_collection_id.set(None);
                        },
                        "⚔️ {conf.files.len()} Conflicts"
                    }
                }
                button {
                    class: if show_team() { "top-icon top-icon-on" } else { "top-icon" },
                    title: "Team collaboration · Members, recent git commits & edited files",
                    disabled: cloud_busy(),
                    onclick: move |_| {
                        open_team_activity(st, true);
                    },
                    "Team"
                }
                button {
                    class: if show_vars() { "top-icon top-icon-on" } else { "top-icon" },
                    title: "Variables (environment, collection, global, vault; local stays on this machine)",
                    onclick: move |_| {
                        if show_vars() && vars_active_tab() == 1 {
                            show_vars.set(false);
                        } else {
                            vars_active_tab.set(1);
                            show_vars.set(true);
                        }
                    },
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
                        switch_to_env(st, id);
                    },
                    on_sync: move |id| {
                        sync_selected_env(st, id);
                    },
                    on_manage: move |_| {
                        vars_active_tab.set(0);
                        editing_env_id.set(active_env_id());
                        show_vars.set(true);
                    },
                }
                ThemeSelector {
                    theme: app_settings().ui.theme,
                    on_change: move |t: UiTheme| {
                        let mut st = st;
                        let mut s = app_settings();
                        s.ui.theme = t;
                        s.ui = s.ui.clamped();
                        app_settings.set(s);
                        st.persist();
                    },
                }
                if let Some(upd) = available_update() {
                    div { class: "update-pill-group",
                        button {
                            class: "top-link update-pill-pulse",
                            disabled: (st.installing_update)(),
                            title: format!("Download, verify, and open Blitz v{} installer", upd.latest_version),
                            onclick: {
                                let info = upd.clone();
                                move |_| {
                                    let mut st = st;
                                    if (st.installing_update)() {
                                        return;
                                    }
                                    st.installing_update.set(true);
                                    st.status_msg.set(format!("downloading Blitz v{}…", info.latest_version));
                                    let info_bg = info.clone();
                                    let rel_url = info.release_url.clone();
                                    spawn(async move {
                                        let res = tokio::task::spawn_blocking(move || {
                                            crate::core::download_and_verify_update(&info_bg)
                                        }).await;
                                        st.installing_update.set(false);
                                        match res {
                                            Ok(Ok(path)) => {
                                                st.status_msg.set(format!("verified installer → {}", path.display()));
                                                crate::core::reveal_or_open_installer(&path);
                                            }
                                            Ok(Err(e)) => {
                                                st.status_msg.set(format!("update failed ({e}) — opening release page"));
                                                open_browser_url(&rel_url);
                                            }
                                            Err(e) => {
                                                st.status_msg.set(format!("update join error: {e}"));
                                            }
                                        }
                                    });
                                }
                            },
                            span { class: "update-badge-dot" }
                            span {
                                if (st.installing_update)() {
                                    "Installing v{upd.latest_version}…"
                                } else {
                                    "Update v{upd.latest_version}"
                                }
                            }
                        }
                    }
                }
                button {
                    class: "top-icon",
                    title: match app_settings().ui.layout {
                        WorkbenchLayout::Column => "Switch to side-by-side layout (Row)",
                        WorkbenchLayout::Row => "Switch to stacked layout (Column)",
                    },
                    aria_label: "Toggle workbench layout",
                    onclick: move |_| {
                        let mut st = st;
                        let mut s = app_settings();
                        s.ui.layout = s.ui.layout.toggle();
                        app_settings.set(s);
                        st.persist();
                    },
                    span {
                        class: match app_settings().ui.layout {
                            WorkbenchLayout::Column => "action-icon action-icon-split-row",
                            WorkbenchLayout::Row => "action-icon action-icon-split-col",
                        },
                        aria_hidden: "true",
                    }
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
    }
}
