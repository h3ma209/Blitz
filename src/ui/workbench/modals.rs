use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use uuid::Uuid;

use dioxus::prelude::*;

use crate::core::{
    apply_collection_map, apply_environment_map, batch_move_items, detect_git_conflicts,
    find_environment, find_request_across, generate_snippet_resolved, get_team_activity_ex,
    import_auto_bundle, is_rebase_in_progress, limitations_blurb, pull_workspace, purge_from_trash,
    push_console, push_workspace, reorder_environments, restore_from_trash,
    run_performance_with_cancel, save_request_to_collections, snippet_languages,
    valid_move_destinations, Collection, ConsoleLine, Cookie, EnvVar, Environment, KeyValue,
    RequestDraft, VariableContext,
};
use crate::runtime::{
    run_collection_with_config, RunnerConfig, RunnerProgress, RunnerProgressSnapshot,
};
use crate::storage::JsonStore;
use crate::ui::runner_panel::{RunnerEvent, RunnerPanel};
use crate::ui::var_hover::VarHoverInput;
use crate::ui::workbench::cloud::switch_to_env;
use crate::ui::workbench::state::{
    execute_batch_delete, execute_tree_delete, sync_env_vars, ImpexModal, WorkbenchState,
};
use crate::ui::{
    CommentableTextarea, EnvDiffView, LintPanel, ModalShell, PerformanceModal, TeamModal,
};

#[component]
pub fn VarsPage(st: WorkbenchState) -> Element {
    let mut show_vars = st.show_vars;
    let mut vars_tab_open = st.vars_tab_open;
    let mut vars_active_tab = st.vars_active_tab;
    let mut environments = st.environments;
    let mut active_env_id = st.active_env_id;
    let mut editing_env_id = st.editing_env_id;
    let mut vars = st.vars;
    let mut drag_env_idx = use_signal(|| None::<usize>);
    let mut drop_env_idx = use_signal(|| None::<usize>);

    if !show_vars() {
        return rsx! {};
    }

    rsx! {
        div {
            class: "vars-page",
            tabindex: "-1",
            onkeydown: move |evt: KeyboardEvent| {
                if evt.key() == Key::Escape {
                    let mut st = st;
                    show_vars.set(false);
                    st.persist();
                }
            },
            div { class: "vars-page-head",
                div { class: "vars-page-title-wrap",
                    h2 { "Environments & Variables" }
                    div { class: "vars-tabs",
                        button {
                            class: if vars_active_tab() == 0 { "vars-tab active" } else { "vars-tab" },
                            onclick: move |_| vars_active_tab.set(0),
                            "Environments ({environments().len()})"
                        }
                        button {
                            class: if vars_active_tab() == 1 { "vars-tab active" } else { "vars-tab" },
                            onclick: move |_| vars_active_tab.set(1),
                            "Globals & Vault"
                        }
                        button {
                            class: if vars_active_tab() == 2 { "vars-tab active" } else { "vars-tab" },
                            onclick: move |_| vars_active_tab.set(2),
                            "Diff & Drift Inspector"
                        }
                    }
                }
                button {
                    class: "btn btn-ghost vars-close-btn",
                    title: "Close (Esc)",
                    onclick: move |_| {
                        let mut st = st;
                        vars_tab_open.set(false);
                        show_vars.set(false);
                        st.persist();
                    },
                    "✕ Close (Esc)"
                }
            }
            if vars_active_tab() == 0 {
                div { class: "vars-env-layout",
                    div { class: "env-sidebar-col",
                        div { class: "env-sidebar-header",
                            span { "Environments" }
                            button {
                                class: "btn-icon",
                                title: "Add environment",
                                onclick: move |_| {
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
                                },
                                crate::ui::icons::ActionIcon { name: "add", size: 16 }
                            }
                        }
                        div { class: "env-sidebar-list",
                            for (e_idx, env) in environments().into_iter().enumerate() {
                                {
                                    let id = env.id;
                                    let name = env.name.clone();
                                    let icon = env.icon.clone();
                                    let color = env.color.clone();
                                    let tier = crate::ui::icons::env_tier(&name);
                                    let is_editing = editing_env_id() == Some(id);
                                    let is_active = active_env_id() == Some(id);
                                    let is_dragging = drag_env_idx() == Some(e_idx);
                                    let is_drop_target = drop_env_idx() == Some(e_idx) && !is_dragging;
                                    let item_class = match (is_editing, is_dragging, is_drop_target) {
                                        (true, true, _) => "env-sidebar-item active dragging",
                                        (true, false, true) => "env-sidebar-item active drag-over",
                                        (true, false, false) => "env-sidebar-item active",
                                        (false, true, _) => "env-sidebar-item dragging",
                                        (false, false, true) => "env-sidebar-item drag-over",
                                        (false, false, false) => "env-sidebar-item",
                                    };
                                    rsx! {
                                        div {
                                            key: "{id}",
                                            class: "{item_class}",
                                            draggable: true,
                                            style: if let Some(c) = color.as_deref().filter(|s| !s.trim().is_empty()) {
                                                format!("background: {c}33; border-left: 3px solid {c};")
                                            } else {
                                                String::new()
                                            },
                                            ondragstart: move |_| {
                                                drag_env_idx.set(Some(e_idx));
                                            },
                                            ondragover: move |evt| {
                                                evt.prevent_default();
                                                if drop_env_idx() != Some(e_idx) {
                                                    drop_env_idx.set(Some(e_idx));
                                                }
                                            },
                                            ondragleave: move |_| {
                                                if drop_env_idx() == Some(e_idx) {
                                                    drop_env_idx.set(None);
                                                }
                                            },
                                            ondrop: move |evt| {
                                                evt.prevent_default();
                                                if let Some(from) = drag_env_idx() {
                                                    let mut envs = environments();
                                                    if reorder_environments(&mut envs, from, e_idx) {
                                                        environments.set(envs);
                                                        let mut st = st;
                                                        st.persist();
                                                    }
                                                }
                                                drag_env_idx.set(None);
                                                drop_env_idx.set(None);
                                            },
                                            ondragend: move |_| {
                                                drag_env_idx.set(None);
                                                drop_env_idx.set(None);
                                            },
                                            onclick: move |_| {
                                                editing_env_id.set(Some(id));
                                            },
                                            span {
                                                class: "env-drag-handle",
                                                title: "Drag to reorder",
                                                "⠿"
                                            }
                                            crate::ui::icons::EnvIcon { icon, color: color.clone(), tier, size: 15 }
                                            span { class: "env-name-text", "{name}" }
                                            if is_active {
                                                span { class: "env-active-indicator", "Active" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    div { class: "env-editor-col",
                        {
                            let envs = environments();
                            let cur_edit_id = editing_env_id()
                                .or(active_env_id())
                                .or_else(|| envs.first().map(|e| e.id));
                            match cur_edit_id.and_then(|id| envs.iter().find(|e| e.id == id).cloned()) {
                                Some(env) => {
                                    let env_id = env.id;
                                    let env_name = env.name.clone();
                                    let env_icon = env.icon.clone();
                                    let env_color = env.color.clone();
                                    let env_tier = crate::ui::icons::env_tier(&env_name);
                                    let is_active = active_env_id() == Some(env_id);
                                    let values_len = env.values.len();
                                    rsx! {
                                        div {
                                            class: "env-editor-top",
                                            style: if let Some(c) = env_color.as_deref().filter(|s| !s.trim().is_empty()) {
                                                format!("background: {c}2e; border-color: {c}77; border-radius: 8px; padding: 8px 10px;")
                                            } else {
                                                String::new()
                                            },
                                            div { class: "env-editor-identity",
                                                crate::ui::icons::EnvIconPicker {
                                                    icon: env_icon,
                                                    color: env_color.clone(),
                                                    tier: env_tier,
                                                    on_select: move |(new_icon, new_color): (Option<String>, Option<String>)| {
                                                        let mut envs = environments();
                                                        if let Some(e) = envs.iter_mut().find(|e| e.id == env_id) {
                                                            e.icon = new_icon;
                                                            e.color = new_color;
                                                        }
                                                        environments.set(envs);
                                                        let mut st = st;
                                                        st.persist();
                                                    },
                                                }
                                                input {
                                                    class: "env-editor-name",
                                                    value: "{env_name}",
                                                    title: "Click to rename environment",
                                                    oninput: move |evt: Event<FormData>| {
                                                        let mut envs = environments();
                                                        if let Some(e) = envs.iter_mut().find(|e| e.id == env_id) {
                                                            e.name = evt.value();
                                                        }
                                                        environments.set(envs);
                                                    }
                                                }
                                            }
                                            div { style: "display: flex; gap: 8px; align-items: center;",
                                                if !is_active {
                                                    button {
                                                        class: "btn btn-ghost",
                                                        title: "Set as active environment",
                                                        onclick: move |_| {
                                                            switch_to_env(st, Some(env_id));
                                                        },
                                                        "Set Active"
                                                    }
                                                }
                                                button {
                                                    class: "env-delete-btn",
                                                    title: "Delete environment",
                                                    onclick: move |_| {
                                                        let mut envs = environments();
                                                        envs.retain(|e| e.id != env_id);
                                                        if active_env_id() == Some(env_id) {
                                                            active_env_id.set(envs.first().map(|e| e.id));
                                                        }
                                                        environments.set(envs.clone());
                                                        let mut v = vars();
                                                        sync_env_vars(&envs, active_env_id(), &mut v);
                                                        vars.set(v);
                                                    },
                                                    "Delete"
                                                }
                                            }
                                        }
                                        p { class: "vars-section-desc", style: "margin-bottom: 8px;",
                                            "Environments let you switch between setups (e.g. Localhost, Staging, Production). Initial values are committed to Git for your team. Current values stay strictly local on this machine."
                                        }
                                        table { class: "kv-table",
                                            thead {
                                                tr {
                                                    th { "Variable" }
                                                    th { "Initial Value" }
                                                    th { "Current Value" }
                                                    th { style: "width: 40px;", "" }
                                                }
                                            }
                                            tbody {
                                                for i in 0..values_len {
                                                    tr { key: "{env_id}-{i}",
                                                        td {
                                                            input {
                                                                value: "{env.values.get(i).map(|r| r.key.clone()).unwrap_or_default()}",
                                                                placeholder: "Key",
                                                                oninput: {
                                                                    let idx = i;
                                                                    move |evt: Event<FormData>| {
                                                                        let mut envs = environments();
                                                                        if let Some(e) = envs.iter_mut().find(|e| e.id == env_id) {
                                                                            if let Some(r) = e.values.get_mut(idx) {
                                                                                r.key = evt.value();
                                                                            }
                                                                        }
                                                                        environments.set(envs);
                                                                    }
                                                                }
                                                            }
                                                        }
                                                        td {
                                                            VarHoverInput {
                                                                class: "kv-var-input".to_string(),
                                                                value: env.values.get(i).map(|r| r.initial_value.clone()).unwrap_or_default(),
                                                                placeholder: "Initial value (synced)".to_string(),
                                                                input_id: format!("env-init-val-{env_id}-{i}"),
                                                                vars: vars(),
                                                                on_input: {
                                                                    let idx = i;
                                                                    move |val: String| {
                                                                        let mut envs = environments();
                                                                        if let Some(e) = envs.iter_mut().find(|e| e.id == env_id) {
                                                                            if let Some(r) = e.values.get_mut(idx) {
                                                                                r.initial_value = val;
                                                                            }
                                                                        }
                                                                        environments.set(envs);
                                                                    }
                                                                },
                                                                on_var_edit: move |_| {},
                                                            }
                                                        }
                                                        td {
                                                            VarHoverInput {
                                                                class: "kv-var-input".to_string(),
                                                                value: env.values.get(i).map(|r| r.current_value.clone()).unwrap_or_default(),
                                                                placeholder: "Current value (active)".to_string(),
                                                                input_id: format!("env-cur-val-{env_id}-{i}"),
                                                                vars: vars(),
                                                                on_input: {
                                                                    let idx = i;
                                                                    move |val: String| {
                                                                        let mut envs = environments();
                                                                        if let Some(e) = envs.iter_mut().find(|e| e.id == env_id) {
                                                                            if let Some(r) = e.values.get_mut(idx) {
                                                                                r.current_value = val;
                                                                            }
                                                                        }
                                                                        environments.set(envs);
                                                                        if active_env_id() == Some(env_id) {
                                                                            let mut v = vars();
                                                                            sync_env_vars(&environments(), Some(env_id), &mut v);
                                                                            vars.set(v);
                                                                        }
                                                                    }
                                                                },
                                                                on_var_edit: move |_| {},
                                                            }
                                                        }
                                                        td {
                                                            button {
                                                                class: "btn btn-ghost",
                                                                title: "Remove row",
                                                                onclick: {
                                                                    let idx = i;
                                                                    move |_| {
                                                                        let mut envs = environments();
                                                                        if let Some(e) = envs.iter_mut().find(|e| e.id == env_id) {
                                                                            if idx < e.values.len() {
                                                                                e.values.remove(idx);
                                                                            }
                                                                        }
                                                                        environments.set(envs);
                                                                        if active_env_id() == Some(env_id) {
                                                                            let mut v = vars();
                                                                            sync_env_vars(&environments(), Some(env_id), &mut v);
                                                                            vars.set(v);
                                                                        }
                                                                    }
                                                                },
                                                                "×"
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        div { class: "kv-actions", style: "margin-top: 10px;",
                                            button {
                                                class: "btn btn-ghost",
                                                onclick: move |_| {
                                                    let mut envs = environments();
                                                    if let Some(e) = envs.iter_mut().find(|e| e.id == env_id) {
                                                        e.values.push(EnvVar {
                                                            key: String::new(),
                                                            initial_value: String::new(),
                                                            current_value: String::new(),
                                                            enabled: true,
                                                        });
                                                    }
                                                    environments.set(envs);
                                                },
                                                "+ Add variable"
                                            }
                                        }
                                    }
                                }
                                None => rsx! {
                                    div { style: "display: flex; flex-direction: column; align-items: center; justify-content: center; height: 100%; gap: 12px;",
                                        p { class: "hint", "No environment selected." }
                                        button {
                                            class: "btn btn-ghost",
                                            onclick: move |_| {
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
                                            },
                                            "Create an environment"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            } else if vars_active_tab() == 1 {
                div { class: "vars-page-inner",
                    div { class: "vars-scope-banner",
                        div { class: "vars-scope-banner-title",
                            crate::ui::icons::ActionIcon { name: "bolt", size: 16, style: "margin-right: 6px;".to_string() }
                            span { "Globals & Vault Secrets" }
                        }
                        p { class: "vars-scope-banner-text",
                            "Manage workspace-wide Global variables ({{name}}) and local-only Vault secrets ({{vault:key}}). Environment variables live in the Environments tab."
                        }
                    }

                    div { class: "vars-section",
                        h4 { "Global" }
                        p { class: "vars-section-desc",
                            "Universal variables accessible across all workspaces, environments, and collections."
                        }
                        MapVarEditor {
                            rows: sorted_map_rows(&vars().global),
                            on_change: move |rows: Vec<(String, String)>| {
                                let mut v = vars();
                                v.global = rows_to_map(rows);
                                vars.set(v);
                            },
                        }
                    }

                    div { class: "vars-section",
                        h4 { "Vault (Hardware & Local Secrets)" }
                        p { class: "vars-section-desc",
                            "Sensitive credentials (passwords, tokens, private keys) stored in vault.json or OS Keychain. NEVER committed to Git or pushed to remotes. Reference via {{vault:key}} or blitz.vault.get('key')."
                        }
                        MapVarEditor {
                            rows: sorted_map_rows(&vars().vault.entries),
                            on_change: move |rows: Vec<(String, String)>| {
                                let mut v = vars();
                                v.vault.entries = rows_to_map(rows);
                                if let Ok(store) = JsonStore::open_default() {
                                    let _ = v.vault.save(store.root());
                                }
                                vars.set(v);
                            },
                        }
                    }

                    div { class: "kv-actions",
                        button {
                            class: "btn btn-send",
                            onclick: move |_| {
                                let mut st = st;
                                show_vars.set(false);
                                st.persist();
                            },
                            "✕ Close"
                        }
                    }
                }
            }
            if vars_active_tab() == 2 {
                EnvDiffView {
                    environments,
                    active_env_id,
                    on_done: move |_| {
                        let mut st = st;
                        show_vars.set(false);
                        st.persist();
                    },
                }
            }
        }
    }
}

#[component]
pub fn StageToolPages(st: WorkbenchState) -> Element {
    let mut show_lint = st.show_lint;
    let mut lint_tab_open = st.lint_tab_open;
    let mut show_runner = st.show_runner;
    let mut runner_tab_open = st.runner_tab_open;
    let mut runner_running = st.runner_running;
    let mut runner_report = st.runner_report;
    let mut runner_cancel = use_signal(|| None::<Arc<AtomicBool>>);
    let mut runner_progress = use_signal(|| None::<RunnerProgressSnapshot>);
    let mut show_perf = st.show_perf;
    let mut perf_tab_open = st.perf_tab_open;
    let mut perf_running = st.perf_running;
    let mut perf_report = st.perf_report;
    let mut perf_vus = st.perf_vus;
    let mut perf_secs = st.perf_secs;
    let mut perf_cancel = use_signal(|| None::<Arc<AtomicBool>>);
    let mut show_team = st.show_team;
    let mut team_tab_open = st.team_tab_open;
    let mut team_syncing = st.team_syncing;
    let mut team_activity = st.team_activity;

    let mut collections = st.collections;
    let mut environments = st.environments;
    let mut active_collection_id = st.active_collection_id;
    let mut active_request_id = st.active_request_id;
    let mut editing_collection_id = st.editing_collection_id;
    let mut draft = st.draft;
    let mut vars = st.vars;
    let mut cookies = st.cookies;
    let mut console_lines = st.console_lines;
    let mut console_open = st.console_open;
    let mut status_msg = st.status_msg;
    let app_settings = st.app_settings;
    let active_env_id = st.active_env_id;

    rsx! {
        if show_runner() {
            RunnerPanel {
                collections: collections(),
                environments: environments(),
                active_env_id: active_env_id(),
                selected_id: active_collection_id(),
                running: runner_running(),
                progress: runner_progress(),
                report: runner_report(),
                on_event: move |evt| match evt {
                    RunnerEvent::Close => {
                        runner_tab_open.set(false);
                        show_runner.set(false);
                    }
                    RunnerEvent::Cancel => {
                        if let Some(flag) = runner_cancel() {
                            flag.store(true, Ordering::SeqCst);
                            status_msg.set("stopping collection run…".into());
                        }
                    }
                    RunnerEvent::OpenRequest {
                        collection_id,
                        request_id,
                    } => {
                        let cols = collections();
                        if let Some((col_id, req)) = find_request_across(&cols, request_id) {
                            active_collection_id.set(Some(col_id));
                            active_request_id.set(Some(req.id));
                            editing_collection_id.set(None);
                            draft.set(req.clone());
                            let mut tabs = (st.open_tabs)();
                            if !tabs.iter().any(|t| t.id == req.id) {
                                tabs.push(req.clone());
                                st.open_tabs.set(tabs);
                            }
                            let cached = (st.responses)().get(&req.id).cloned();
                            let mut resp_sig = st.response;
                            resp_sig.set(cached);
                            show_runner.set(false);
                        } else {
                            active_collection_id.set(Some(collection_id));
                            show_runner.set(false);
                        }
                    }
                    RunnerEvent::Run {
                        collection_id,
                        folder_filter,
                        selected_request_ids,
                        environment_id,
                        iterations,
                        delay_ms,
                        data_file,
                        stop_on_error,
                        persist_vars,
                    } => {
                        if runner_running() {
                            return;
                        }
                        let cancel_flag = Arc::new(AtomicBool::new(false));
                        let progress_tracker = Arc::new(RunnerProgress::default());
                        runner_cancel.set(Some(cancel_flag.clone()));
                        runner_progress.set(Some(RunnerProgressSnapshot::default()));
                        runner_running.set(true);
                        runner_report.set(None);
                        let cols = collections();
                        let Some(col) = cols.into_iter().find(|c| c.id == collection_id) else {
                            runner_running.set(false);
                            runner_cancel.set(None);
                            runner_progress.set(None);
                            status_msg.set("collection not found".into());
                            return;
                        };
                        let mut ctx = vars();
                        ctx.collection = col.variables_map();
                        if let Some(env) = find_environment(&environments(), environment_id) {
                            ctx.environment = env.to_map();
                        } else {
                            ctx.environment.clear();
                        }
                        let env_id = environment_id;
                        let col_id = col.id;
                        let mut jar = cookies();
                        let settings = app_settings();
                        let cfg = RunnerConfig {
                            iterations,
                            delay_ms,
                            data_file,
                            folder_filter,
                            selected_request_ids,
                            stop_on_error,
                            cancel: Some(cancel_flag),
                            progress: Some(progress_tracker.clone()),
                        };
                        let mut st = st;
                        spawn(async move {
                            while runner_running() {
                                runner_progress.set(Some(progress_tracker.snapshot()));
                                tokio::time::sleep(Duration::from_millis(75)).await;
                            }
                        });
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
                                    if persist_vars {
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
                                                "no active environment — blitz.environment.set ignored"
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
                                    }
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
                            runner_cancel.set(None);
                            runner_progress.set(None);
                            if persist_vars {
                                st.persist();
                            }
                        });
                    }
                },
            }
        } else if show_lint() {
            LintPanel {
                collections: collections(),
                vars: vars(),
                on_close: move |_| {
                    lint_tab_open.set(false);
                    show_lint.set(false);
                },
            }
        } else if show_perf() {
            PerformanceModal {
                collections: collections(),
                active_collection_id: active_collection_id(),
                active_request_id: active_request_id(),
                running: perf_running(),
                report: perf_report(),
                initial_vus: perf_vus(),
                initial_secs: perf_secs(),
                on_close: move |_| {
                    perf_tab_open.set(false);
                    show_perf.set(false);
                },
                on_cancel: move |_| {
                    if let Some(flag) = perf_cancel() {
                        flag.store(true, Ordering::SeqCst);
                        status_msg.set("stopping benchmark…".into());
                    }
                },
                on_run: move |(col_id, req_id, vus, secs)| {
                    if perf_running() {
                        return;
                    }
                    let cancel_flag = Arc::new(AtomicBool::new(false));
                    perf_cancel.set(Some(cancel_flag.clone()));
                    perf_vus.set(vus);
                    perf_secs.set(secs);
                    perf_running.set(true);
                    perf_report.set(None);

                    let cols = collections();
                    let Some(col) = cols.into_iter().find(|c| c.id == col_id) else {
                        perf_running.set(false);
                        perf_cancel.set(None);
                        status_msg.set("collection not found".into());
                        return;
                    };

                    let target_col = if let Some(r_id) = req_id {
                        let reqs = crate::runtime::ordered_requests(&col);
                        if let Some(target_req) = reqs.into_iter().find(|r| r.id == r_id) {
                            Collection {
                                id: Uuid::new_v4(),
                                name: format!("Perf: {}", target_req.name),
                                auth: col.auth.clone(),
                                items: vec![crate::core::CollectionItem::Request(target_req)],
                                ..Collection::new("PerfTarget")
                            }
                        } else {
                            col.clone()
                        }
                    } else {
                        col.clone()
                    };

                    let mut ctx = vars();
                    ctx.collection = col.variables_map();
                    if let Some(env) = find_environment(&environments(), active_env_id()) {
                        ctx.environment = env.to_map();
                    } else {
                        ctx.environment.clear();
                    }

                    spawn(async move {
                        let report = tokio::task::spawn_blocking(move || {
                            run_performance_with_cancel(
                                &target_col,
                                &ctx,
                                vus,
                                Duration::from_secs(secs),
                                Some(cancel_flag),
                            )
                        })
                        .await;

                        perf_running.set(false);
                        perf_cancel.set(None);
                        match report {
                            Ok(r) => {
                                status_msg.set(format!(
                                    "perf completed: {} reqs, {} errs, avg {:.1}ms (p95 {}ms), {:.1} rps",
                                    r.total_requests,
                                    r.errors,
                                    r.avg_latency_ms(),
                                    r.p95_latency_ms,
                                    r.rps(),
                                ));
                                perf_report.set(Some(r));
                            }
                            Err(e) => status_msg.set(format!("perf failed: {e}")),
                        }
                    });
                },
            }
        } else if show_team() {
            TeamModal {
                activity: team_activity(),
                is_syncing: team_syncing() || (st.cloud_busy)(),
                on_close: move |_| {
                    team_tab_open.set(false);
                    show_team.set(false);
                },
                on_refresh: move |_| {
                    if (st.cloud_busy)() || team_syncing() {
                        return;
                    }
                    let Ok(store) = JsonStore::open_default() else {
                        status_msg.set("team: cannot open store".into());
                        return;
                    };
                    let settings = app_settings();
                    let clone_path = settings.cloud_sync.resolved_clone_path(store.root());
                    team_syncing.set(true);
                    let mut st_busy = st;
                    st_busy.cloud_busy.set(true);
                    spawn(async move {
                        let res = tokio::task::spawn_blocking(move || {
                            get_team_activity_ex(&clone_path, 30, true).map_err(|e| e.to_string())
                        })
                        .await;
                        match res {
                            Ok(Ok(act)) => {
                                team_activity.set(act);
                                status_msg.set("team: refreshed activity".into());
                            }
                            Ok(Err(e)) => status_msg.set(format!("team refresh: {e}")),
                            Err(e) => status_msg.set(format!("team join: {e}")),
                        }
                        team_syncing.set(false);
                        st_busy.cloud_busy.set(false);
                    });
                },
                on_pull: move |_| {
                    if (st.cloud_busy)() || team_syncing() {
                        return;
                    }
                    let Ok(store) = JsonStore::open_default() else {
                        return;
                    };
                    let root = store.root().to_path_buf();
                    let mut settings = app_settings();
                    if settings.cloud_sync.remote_url.trim().is_empty() {
                        status_msg.set("Configure Remote Git URL in Cloud Settings first".into());
                        return;
                    }
                    settings.cloud_sync.enabled = true;
                    let clone_path = settings.cloud_sync.resolved_clone_path(&root);
                    let envs = environments();
                    let cols = collections();
                    let globals = vars().global;
                    team_syncing.set(true);
                    let mut st_busy = st;
                    st_busy.cloud_busy.set(true);
                    spawn(async move {
                        let cp_bg = clone_path.clone();
                        let res = tokio::task::spawn_blocking(move || {
                            let r = pull_workspace(&root, &mut settings, &cols, &envs, &globals);
                            let act = get_team_activity_ex(&cp_bg, 30, false).ok();
                            let need_conflicts =
                                r.is_err() || matches!(&r, Ok(sw) if !sw.result.ok);
                            let (conflicts, rebasing) = if need_conflicts {
                                (
                                    detect_git_conflicts(&cp_bg).unwrap_or_default(),
                                    is_rebase_in_progress(&cp_bg),
                                )
                            } else {
                                (Vec::new(), false)
                            };
                            (r, settings, act, conflicts, rebasing)
                        })
                        .await;
                        team_syncing.set(false);
                        st_busy.cloud_busy.set(false);
                        match res {
                            Ok((Ok(synced), updated_settings, act, conflicts, rebasing)) => {
                                if !synced.result.ok {
                                    st_busy.patch_cloud_meta(
                                        &updated_settings.cloud_sync.last_status,
                                        &updated_settings.cloud_sync.last_branch,
                                        Some(true),
                                        None,
                                        None,
                                    );
                                    if crate::ui::workbench::cloud::open_conflicts_if_any(
                                        st_busy,
                                        clone_path,
                                        conflicts,
                                        rebasing,
                                    ) {
                                        show_team.set(false);
                                    } else {
                                        status_msg.set(format!("pull: {}", synced.result.message));
                                    }
                                    return;
                                }
                                st_busy.apply_synced_workspace(
                                    synced.collections,
                                    synced.environments,
                                    synced.globals,
                                    None,
                                    &synced.result.message,
                                    &synced.result.branch,
                                );
                                st_busy.patch_cloud_meta(
                                    &synced.result.message,
                                    &synced.result.branch,
                                    Some(true),
                                    None,
                                    None,
                                );
                                if let Some(act) = act {
                                    team_activity.set(act);
                                }
                                status_msg.set(synced.result.message);
                            }
                            Ok((Err(e), _, _, conflicts, rebasing)) => {
                                if crate::ui::workbench::cloud::open_conflicts_if_any(
                                    st_busy,
                                    clone_path,
                                    conflicts,
                                    rebasing,
                                ) {
                                    show_team.set(false);
                                } else {
                                    status_msg.set(format!("pull: {e}"));
                                }
                            }
                            Err(e) => status_msg.set(format!("pull join: {e}")),
                        }
                    });
                },
                on_push: move |_| {
                    if (st.cloud_busy)() || team_syncing() {
                        return;
                    }
                    let Ok(store) = JsonStore::open_default() else {
                        return;
                    };
                    let root = store.root().to_path_buf();
                    let mut settings = app_settings();
                    if settings.cloud_sync.remote_url.trim().is_empty() {
                        status_msg.set("Configure Remote Git URL in Cloud Settings first".into());
                        return;
                    }
                    settings.cloud_sync.enabled = true;
                    let clone_path = settings.cloud_sync.resolved_clone_path(&root);
                    let envs = environments();
                    let cols = collections();
                    let v = vars();
                    let active_env_name = find_environment(&envs, active_env_id())
                        .map(|e| e.name.clone())
                        .unwrap_or_else(|| "No Environment".into());
                    let commit = settings.cloud_sync.commit_message.clone();
                    team_syncing.set(true);
                    let mut st_busy = st;
                    st_busy.cloud_busy.set(true);
                    spawn(async move {
                        let cp_bg = clone_path.clone();
                        let res = tokio::task::spawn_blocking(move || {
                            let r = push_workspace(
                                &root,
                                &mut settings,
                                &cols,
                                &envs,
                                &active_env_name,
                                Some(commit.as_str()),
                                &v,
                            );
                            let act = get_team_activity_ex(&cp_bg, 30, false).ok();
                            let (conflicts, rebasing) = if !r.ok {
                                (
                                    detect_git_conflicts(&cp_bg).unwrap_or_default(),
                                    is_rebase_in_progress(&cp_bg),
                                )
                            } else {
                                (Vec::new(), false)
                            };
                            (r, settings, act, conflicts, rebasing)
                        })
                        .await;
                        team_syncing.set(false);
                        st_busy.cloud_busy.set(false);
                        if let Ok((r, updated_settings, act, conflicts, rebasing)) = res {
                            st_busy.patch_cloud_meta(
                                &updated_settings.cloud_sync.last_status,
                                &updated_settings.cloud_sync.last_branch,
                                Some(true),
                                None,
                                None,
                            );
                            if let Some(act) = act {
                                team_activity.set(act);
                            }
                            if !r.ok
                                && crate::ui::workbench::cloud::open_conflicts_if_any(
                                    st_busy,
                                    clone_path,
                                    conflicts,
                                    rebasing,
                                )
                            {
                                show_team.set(false);
                            } else {
                                status_msg.set(r.message);
                                st_busy.persist_local();
                            }
                        }
                    });
                },
            }
        }
    }
}

#[component]
pub fn WorkbenchModals(
    st: WorkbenchState,
    on_send: EventHandler<()>,
    on_save: EventHandler<()>,
    on_new_tab: EventHandler<()>,
) -> Element {
    let mut show_palette = st.show_palette;
    let mut pending_delete = st.pending_delete;
    let mut pending_batch_move = st.pending_batch_move;
    let mut pending_close_tab = st.pending_close_tab;
    let mut show_trash = st.show_trash;
    let mut show_snippets = st.show_snippets;
    let mut snippet_lang = st.snippet_lang;
    let mut snippet_resolve_vars = st.snippet_resolve_vars;
    let mut show_cookies = st.show_cookies;
    let mut show_col_vars = st.show_col_vars;
    let mut col_vars_all = st.col_vars_all;
    let mut col_vars_open_id = st.col_vars_open_id;
    let mut impex_modal = st.impex_modal;
    let mut collections = st.collections;
    let mut active_collection_id = st.active_collection_id;
    let mut active_request_id = st.active_request_id;
    let mut draft = st.draft;
    let mut trash = st.trash;
    let vars = st.vars;
    let mut cookies = st.cookies;
    let mut status_msg = st.status_msg;

    rsx! {
        if show_palette() {
            crate::ui::command_palette::CommandPalette {
                st,
                on_send: move |_| on_send.call(()),
                on_save: move |_| on_save.call(()),
                on_new_tab: move |_| on_new_tab.call(()),
                on_close: move |_| {
                    show_palette.set(false);
                    spawn(async move {
                        let script = r#"
                            setTimeout(() => {
                                let target = window.__blitz_last_focused;
                                if (!target || !document.contains(target) || target.classList.contains('palette-search-input')) {
                                    target = document.getElementById('np-url-input') || document.querySelector('.url-input');
                                }
                                if (target && typeof target.focus === 'function') {
                                    target.focus();
                                    if (target.id === 'np-url-input' && typeof target.select === 'function') {
                                        target.select();
                                    }
                                } else {
                                    const shell = document.querySelector('.app-shell') || document.body;
                                    if (shell) {
                                        if (!shell.hasAttribute('tabindex')) shell.setAttribute('tabindex', '-1');
                                        shell.focus();
                                    }
                                }
                            }, 10);
                        "#;
                        let _ = document::eval(script).await;
                    });
                },
            }
        }

        if let Some(del) = pending_delete() {
            {
                let del_id = del.id;
                let del_ids = del.ids.clone();
                let del_label = del.label.clone();
                let is_batch = del_ids.len() > 1;
                let title = if is_batch {
                    format!("Delete {} items?", del_ids.len())
                } else {
                    "Delete item?".to_string()
                };
                let prompt = if is_batch {
                    format!("Delete {} selected items? This cannot be undone.", del_ids.len())
                } else {
                    format!("Delete “{del_label}”? This cannot be undone.")
                };
                rsx! {
                    ModalShell {
                        title,
                        z_index: 2400,
                        on_close: move |_| pending_delete.set(None),
                        div { class: "modal-body",
                            p { "{prompt}" }
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
                                    let mut st = st;
                                    pending_delete.set(None);
                                    let mut cols = collections();
                                    let mut tr = trash();
                                    let mut col_id = active_collection_id();
                                    let mut req_id = active_request_id();
                                    let mut d = draft();
                                    let mut tabs = (st.open_tabs)();
                                    if is_batch {
                                        let count = execute_batch_delete(
                                            &del_ids,
                                            &mut cols,
                                            &mut tr,
                                            &mut col_id,
                                            &mut req_id,
                                            &mut d,
                                            &mut tabs,
                                        );
                                        st.open_tabs.set(tabs);
                                        status_msg.set(format!("Deleted {count} items"));
                                    } else {
                                        if let Some(msg) = execute_tree_delete(
                                            del_id,
                                            &mut cols,
                                            &mut tr,
                                            &mut col_id,
                                            &mut req_id,
                                            &mut d,
                                            &mut tabs,
                                        ) {
                                            status_msg.set(msg);
                                        }
                                        st.open_tabs.set(tabs);
                                    }
                                    trash.set(tr);
                                    collections.set(cols);
                                    active_collection_id.set(col_id);
                                    active_request_id.set(req_id);
                                    draft.set(d);
                                    st.persist();
                                },
                                "Delete"
                            }
                        }
                    }
                }
            }
        }

        if let Some(move_ids) = pending_batch_move() {
            {
                let cols = collections();
                let destinations = valid_move_destinations(&cols, &move_ids);
                let count = move_ids.len();
                let title = format!("Move {count} item{} to...", if count == 1 { "" } else { "s" });
                rsx! {
                    ModalShell {
                        title,
                        z_index: 2400,
                        on_close: move |_| pending_batch_move.set(None),
                        div { class: "modal-body move-modal-body",
                            if destinations.is_empty() {
                                p { class: "hint", "No available destination folders or collections." }
                            } else {
                                for (di, dest) in destinations.into_iter().enumerate() {
                                    {
                                        let did = dest.id;
                                        let pad = dest.depth * 14 + 10;
                                        let icon = if dest.is_collection { "📦" } else { "📁" };
                                        rsx! {
                                            button {
                                                key: "{did}-{di}",
                                                class: "move-target-row",
                                                style: "padding-left: {pad}px;",
                                                onclick: {
                                                    let move_ids = move_ids.clone();
                                                    move |_| {
                                                        let mut st = st;
                                                        pending_batch_move.set(None);
                                                        let mut cols = collections();
                                                        let count = batch_move_items(&mut cols, &move_ids, did);
                                                        if count > 0 {
                                                            collections.set(cols);
                                                            status_msg.set(format!(
                                                                "moved {} item{}",
                                                                count,
                                                                if count == 1 { "" } else { "s" }
                                                            ));
                                                            st.persist();
                                                        } else {
                                                            status_msg.set("cannot move there".into());
                                                        }
                                                    }
                                                },
                                                span { class: "move-target-icon", "{icon}" }
                                                span { class: "move-target-name", "{dest.name}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "modal-actions",
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| pending_batch_move.set(None),
                                "Cancel"
                            }
                        }
                    }
                }
            }
        }

        if let Some(pending) = pending_close_tab() {
            {
                let req_name = if pending.draft.name.trim().is_empty() {
                    "Untitled Request".to_string()
                } else {
                    pending.draft.name.clone()
                };
                rsx! {
                    ModalShell {
                        title: "Save Changes?".to_string(),
                        z_index: 2400,
                        on_close: move |_| pending_close_tab.set(None),
                        div { class: "modal-body",
                            p {
                                "Do you want to save the changes you made to "
                                strong { "“{req_name}”" }
                                "?"
                            }
                            p { class: "hint", "Your changes will be lost if you discard them." }
                        }
                        div { class: "modal-actions",
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| pending_close_tab.set(None),
                                "Cancel"
                            }
                            button {
                                class: "btn btn-ghost danger",
                                onclick: {
                                    let tid = pending.tab_id;
                                    move |_| {
                                        pending_close_tab.set(None);
                                        let mut tabs = (st.open_tabs)();
                                        let cur = (st.draft)();
                                        let Some(idx) = tabs.iter().position(|t| t.id == tid) else {
                                            return;
                                        };
                                        let closing_active = cur.id == tid;
                                        tabs.remove(idx);
                                        if tabs.is_empty() {
                                            let mut st = st;
                                            st.active_request_id.set(None);
                                            st.response.set(None);
                                            st.draft.set(RequestDraft::default());
                                            st.open_tabs.set(tabs.clone());
                                            st.cleanup_closed_tabs(&tabs, None);
                                            st.persist_now();
                                            st.status_msg.set("tab closed".into());
                                            return;
                                        }
                                        let mut next_active = Some(cur.id);
                                        if closing_active {
                                            let mut st = st;
                                            let next_idx = if idx < tabs.len() { idx } else { tabs.len() - 1 };
                                            let next = tabs[next_idx].clone();
                                            next_active = Some(next.id);
                                            st.draft.set(next.clone());
                                            st.active_request_id.set(Some(next.id));
                                            let resp_map = (st.responses)();
                                            if let Some(cached) = resp_map.get(&next.id).cloned() {
                                                st.response.set(Some(cached));
                                            } else {
                                                st.response.set(None);
                                            }
                                        }
                                        let mut st = st;
                                        st.open_tabs.set(tabs.clone());
                                        st.cleanup_closed_tabs(&tabs, next_active);
                                        st.persist_now();
                                        st.status_msg.set("changes discarded".into());
                                    }
                                },
                                "Discard Changes"
                            }
                            button {
                                class: "btn btn-send",
                                onclick: {
                                    let pd = pending.draft.clone();
                                    let tid = pending.tab_id;
                                    move |_| {
                                        let mut st = st;
                                        pending_close_tab.set(None);
                                        let mut cols = collections();
                                        let owner = save_request_to_collections(&mut cols, active_collection_id(), pd.clone());
                                        collections.set(cols);
                                        active_collection_id.set(Some(owner));
                                        let mut tabs = (st.open_tabs)();
                                        let cur = (st.draft)();
                                        let Some(idx) = tabs.iter().position(|t| t.id == tid) else {
                                            st.persist_now();
                                            return;
                                        };
                                        let closing_active = cur.id == tid;
                                        tabs.remove(idx);
                                        if tabs.is_empty() {
                                            st.active_request_id.set(None);
                                            st.response.set(None);
                                            st.draft.set(RequestDraft::default());
                                            st.open_tabs.set(tabs.clone());
                                            st.cleanup_closed_tabs(&tabs, None);
                                            st.persist_now();
                                            st.status_msg.set(format!("saved & closed '{}'", pd.name));
                                            return;
                                        }
                                        let mut next_active = Some(cur.id);
                                        if closing_active {
                                            let next_idx = if idx < tabs.len() { idx } else { tabs.len() - 1 };
                                            let next = tabs[next_idx].clone();
                                            next_active = Some(next.id);
                                            st.draft.set(next.clone());
                                            st.active_request_id.set(Some(next.id));
                                            let resp_map = (st.responses)();
                                            if let Some(cached) = resp_map.get(&next.id).cloned() {
                                                st.response.set(Some(cached));
                                            } else {
                                                st.response.set(None);
                                            }
                                        }
                                        st.open_tabs.set(tabs.clone());
                                        st.cleanup_closed_tabs(&tabs, next_active);
                                        st.persist_now();
                                        st.status_msg.set(format!("saved & closed '{}'", pd.name));
                                    }
                                },
                                "Save & Close"
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
                        for (ti, c) in trash().into_iter().enumerate() {
                            div { key: "{c.id}-{ti}", class: "trash-row",
                                span { "{c.name}" }
                                button {
                                    class: "btn btn-ghost",
                                    r#type: "button",
                                    onclick: move |_| {
                                        let mut st = st;
                                        let mut cols = collections();
                                        let mut tr = trash();
                                        if restore_from_trash(&mut cols, &mut tr, c.id) {
                                            collections.set(cols);
                                            trash.set(tr);
                                            st.persist();
                                        }
                                    },
                                    "Restore"
                                }
                                button {
                                    class: "btn btn-ghost",
                                    onclick: move |_| {
                                        let mut st = st;
                                        let mut tr = trash();
                                        if purge_from_trash(&mut tr, c.id) {
                                            trash.set(tr);
                                            st.persist();
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
            {
                let cur_d = draft();
                let codegen_d = st.effective_draft_for_codegen(&cur_d);
                let code_content = if snippet_resolve_vars() {
                    let v = st.effective_vars_for_request(&cur_d);
                    generate_snippet_resolved(&snippet_lang(), &codegen_d, Some(&v))
                } else {
                    generate_snippet_resolved(&snippet_lang(), &codegen_d, None)
                };
                let code_for_copy = code_content.clone();

                rsx! {
                    ModalShell {
                        title: "Generate Code Snippet".to_string(),
                        on_close: move |_| show_snippets.set(false),
                        div { class: "modal-body",
                            div { style: "display: flex; gap: 12px; align-items: center; margin-bottom: 12px; justify-content: space-between;",
                                div { style: "display: flex; gap: 8px; align-items: center;",
                                    span { class: "hint", "Language:" }
                                    select {
                                        value: "{snippet_lang()}",
                                        onchange: move |evt| snippet_lang.set(evt.value()),
                                        for lang in snippet_languages() {
                                            option { key: "{lang}", value: "{lang}", "{lang}" }
                                        }
                                    }
                                }
                                label { style: "display: flex; gap: 6px; align-items: center; cursor: pointer; font-size: 12px;",
                                    input {
                                        r#type: "checkbox",
                                        checked: "{snippet_resolve_vars()}",
                                        onchange: move |evt| snippet_resolve_vars.set(evt.checked()),
                                    }
                                    span { "Resolve Variables" }
                                }
                            }
                            pre {
                                class: "response-body",
                                style: "max-height: 380px; overflow-y: auto; user-select: text;",
                                "{code_content}"
                            }
                        }
                        div { class: "modal-actions",
                            button {
                                class: "btn btn-primary",
                                onclick: move |_| {
                                    let c = code_for_copy.clone();
                                    let script = format!(
                                        "navigator.clipboard.writeText({});",
                                        serde_json::to_string(&c).unwrap_or_else(|_| "\"\"".into())
                                    );
                                    spawn(async move {
                                        let _ = document::eval(&script).await;
                                    });
                                },
                                "Copy to Clipboard"
                            }
                            button { class: "btn btn-ghost", onclick: move |_| show_snippets.set(false), "Close" }
                        }
                    }
                }
            }
        }

        if show_cookies() {
            ModalShell {
                title: "Cookie manager".to_string(),
                on_close: move |_| {
                    let mut st = st;
                    show_cookies.set(false);
                    st.persist();
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
                                    key: "{c.domain}-{c.name}-{i}",
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
                                                let mut st = st;
                                                let mut jar = cookies();
                                                jar.remove_at(i);
                                                cookies.set(jar);
                                                st.persist();
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
                                let mut st = st;
                                let mut jar = cookies();
                                jar.clear();
                                cookies.set(jar);
                                st.persist();
                            },
                            "Clear all"
                        }
                        button {
                            class: "btn btn-send",
                            onclick: move |_| {
                                let mut st = st;
                                show_cookies.set(false);
                                st.persist();
                            },
                            "✕ Close"
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
                    let mut st = st;
                    show_col_vars.set(false);
                    st.persist();
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
                                let mut st = st;
                                show_col_vars.set(false);
                                st.persist();
                            },
                            "✕ Close"
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
                    let mut st = st;
                    match import_auto_bundle(&text) {
                        Ok(bundle) => {
                            let summary = st.apply_import_bundle(bundle);
                            impex_modal.set(None);
                            status_msg.set(summary);
                        }
                        Err(e) => status_msg.set(format!("import error: {e}")),
                    }
                },
            }
        }

        if (st.show_update_modal)() {
            if let Some(upd) = (st.available_update)() {
                UpdateAvailableModal {
                    st,
                    info: upd,
                }
            }
        }
    }
}

fn format_update_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

#[component]
fn UpdateAvailableModal(st: WorkbenchState, info: crate::core::UpdateInfo) -> Element {
    let mut show_update_modal = st.show_update_modal;
    let installing = (st.installing_update)();
    let progress = (st.update_progress)();
    let err_msg = (st.update_error)();
    let rel_name = info.release_name.trim().to_string();

    rsx! {
        ModalShell {
            title: format!("Update Available — Blitz v{}", info.latest_version),
            z_index: 2400,
            on_close: move |_| {
                if !(st.installing_update)() {
                    show_update_modal.set(false);
                }
            },
            div { class: "update-modal",
                div { class: "update-modal-head",
                    span { class: "update-version-pill", "Current: v{info.current_version} → New: v{info.latest_version}" }
                    if !rel_name.is_empty() {
                        span { class: "hint", "{rel_name}" }
                    }
                }
                if let Some(p) = &progress {
                    div { class: "update-progress-wrap",
                        div { class: "update-progress-meta",
                            span { "{p.message}" }
                            if matches!(p.stage, crate::core::UpdateStage::Downloading) {
                                if let (Some(pct), Some(total)) = (p.percent(), p.total_bytes) {
                                    span { class: "update-progress-pct",
                                        "{format_update_bytes(p.downloaded_bytes)} / {format_update_bytes(total)} ({pct}%)"
                                    }
                                } else if p.downloaded_bytes > 0 {
                                    span { class: "update-progress-pct",
                                        "{format_update_bytes(p.downloaded_bytes)}"
                                    }
                                }
                            } else if matches!(p.stage, crate::core::UpdateStage::Verifying) {
                                span { class: "update-progress-pct", "Verifying…" }
                            } else if matches!(p.stage, crate::core::UpdateStage::Installing) {
                                span { class: "update-progress-pct", "Installing…" }
                            }
                        }
                        div { class: "update-progress-track",
                            if matches!(p.stage, crate::core::UpdateStage::Downloading) {
                                if let Some(pct) = p.percent() {
                                    div {
                                        class: "update-progress-fill",
                                        style: "width: {pct}%;",
                                    }
                                } else {
                                    div { class: "update-progress-fill update-progress-indeterminate" }
                                }
                            } else {
                                div {
                                    class: "update-progress-fill update-progress-indeterminate",
                                    style: "width: 100%;",
                                }
                            }
                        }
                    }
                }
                if let Some(err) = &err_msg {
                    div { class: "update-error-box", "{err}" }
                }
                div { class: "kv-actions",
                    if !installing {
                        button {
                            class: "btn btn-send",
                            onclick: {
                                let info = info.clone();
                                move |_| st.start_update_install(info.clone())
                            },
                            if err_msg.is_some() {
                                "Retry Update"
                            } else {
                                "Download & Install"
                            }
                        }
                        button {
                            class: "btn btn-ghost",
                            onclick: {
                                let url = info.release_url.clone();
                                move |_| crate::core::open_browser_url(&url)
                            },
                            "Release Notes"
                        }
                        button {
                            class: "btn btn-ghost",
                            onclick: move |_| show_update_modal.set(false),
                            "Later"
                        }
                    } else {
                        span { class: "hint", "Please wait — Blitz will restart automatically when ready." }
                    }
                }
            }
        }
    }
}

pub fn sorted_map_rows(map: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut rows: Vec<_> = map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

pub fn rows_to_map(rows: Vec<(String, String)>) -> HashMap<String, String> {
    rows.into_iter().filter(|(k, _)| !k.is_empty()).collect()
}

/// Editable flat key/value map (global / vault).
#[component]
pub fn MapVarEditor(
    rows: Vec<(String, String)>,
    on_change: EventHandler<Vec<(String, String)>>,
) -> Element {
    let mut local_rows = use_signal(|| rows.clone());

    let incoming = rows.clone();
    use_effect(move || {
        let current = local_rows();
        let incoming_map: HashMap<_, _> = incoming.iter().cloned().collect();
        let current_map: HashMap<_, _> = current
            .iter()
            .filter(|(k, _)| !k.is_empty())
            .cloned()
            .collect();
        if incoming_map != current_map && (current.is_empty() || incoming.len() != current.len()) {
            local_rows.set(incoming.clone());
        }
    });

    rsx! {
        table { class: "kv-table",
            thead { tr { th { "Key" } th { "Value" } th { "" } } }
            tbody {
                for (i, (key, val)) in local_rows().iter().enumerate() {
                    tr {
                        key: "{i}",
                        td {
                            input {
                                value: "{key}",
                                placeholder: "Key",
                                oninput: move |evt: Event<FormData>| {
                                    let mut next = local_rows();
                                    if let Some(r) = next.get_mut(i) {
                                        r.0 = evt.value();
                                    }
                                    local_rows.set(next.clone());
                                    on_change.call(next);
                                }
                            }
                        }
                        td {
                            input {
                                value: "{val}",
                                placeholder: "Value",
                                oninput: move |evt: Event<FormData>| {
                                    let mut next = local_rows();
                                    if let Some(r) = next.get_mut(i) {
                                        r.1 = evt.value();
                                    }
                                    local_rows.set(next.clone());
                                    on_change.call(next);
                                }
                            }
                        }
                        td {
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| {
                                    let mut next = local_rows();
                                    if i < next.len() {
                                        next.remove(i);
                                    }
                                    local_rows.set(next.clone());
                                    on_change.call(next);
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
                    let mut next = local_rows();
                    next.push((String::new(), String::new()));
                    local_rows.set(next.clone());
                    on_change.call(next);
                },
                "+ Add row"
            }
        }
    }
}

/// Editable key/value rows for one collection's variables.
#[component]
pub fn ColVarEditor(
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
                        key: "{cid}-{i}",
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
                            VarHoverInput {
                                class: "kv-var-input".to_string(),
                                value: row.value.clone(),
                                placeholder: "Value".to_string(),
                                input_id: format!("modal-col-var-{cid}-{i}"),
                                vars: vars(),
                                on_input: move |val: String| {
                                    let mut cols = collections();
                                    if let Some(col) = cols.iter_mut().find(|c| c.id == cid) {
                                        if let Some(r) = col.variables.get_mut(i) {
                                            r.value = val;
                                        }
                                        if sync_active {
                                            let mut v = vars();
                                            v.collection = col.variables_map();
                                            vars.set(v);
                                        }
                                    }
                                    collections.set(cols);
                                },
                                on_var_edit: move |_| {},
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
pub fn ImpexDialog(
    modal: ImpexModal,
    on_close: EventHandler<()>,
    on_import: EventHandler<String>,
) -> Element {
    let mut text = use_signal(|| match &modal {
        ImpexModal::Import { text } | ImpexModal::Export { text, .. } => text.clone(),
    });
    let is_import = matches!(modal, ImpexModal::Import { .. });
    let title = match &modal {
        ImpexModal::Import { .. } => {
            "Import (Postman / Bruno / Yaak / cURL / OpenAPI / HAR)".to_string()
        }
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
