use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

use dioxus::prelude::*;

use crate::core::{
    apply_collection_map, apply_environment_map, detect_git_conflicts, find_environment,
    generate_snippet_resolved, get_team_activity, import_collection_v21, import_curl, import_har,
    import_openapi3, limitations_blurb, pull_workspace, purge_from_trash, push_console,
    push_workspace, restore_from_trash, run_performance, snippet_languages, Collection,
    ConsoleLine, Cookie, EnvVar, Environment, KeyValue, VariableContext,
};
use crate::runtime::{run_collection_with_config, RunnerConfig};
use crate::storage::JsonStore;
use crate::ui::loading::Spinner;
use crate::ui::runner_panel::{RunnerEvent, RunnerPanel};
use crate::ui::var_hover::VarHoverInput;
use crate::ui::workbench::cloud::switch_to_env;
use crate::ui::workbench::state::{
    execute_tree_delete, sync_env_vars, ImpexModal, WorkbenchState,
};
use crate::ui::{CommentableTextarea, ConflictState, ModalShell, TeamModal};

#[component]
pub fn VarsPage(st: WorkbenchState) -> Element {
    let mut show_vars = st.show_vars;
    let mut vars_active_tab = st.vars_active_tab;
    let mut environments = st.environments;
    let mut active_env_id = st.active_env_id;
    let mut editing_env_id = st.editing_env_id;
    let mut vars = st.vars;
    let mut col_vars_all = st.col_vars_all;
    let mut col_vars_open_id = st.col_vars_open_id;
    let mut show_col_vars = st.show_col_vars;
    let collections = st.collections;
    let active_collection_id = st.active_collection_id;

    if !show_vars() {
        return rsx! {};
    }

    rsx! {
        div { class: "vars-page",
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
                            "Variables Overview"
                        }
                    }
                }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| {
                        let mut st = st;
                        show_vars.set(false);
                        st.persist();
                    },
                    "Done"
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
                                span { class: "action-icon action-icon-add", aria_hidden: "true" }
                            }
                        }
                        div { class: "env-sidebar-list",
                            for env in environments() {
                                {
                                    let id = env.id;
                                    let name = env.name.clone();
                                    let is_editing = editing_env_id() == Some(id);
                                    let is_active = active_env_id() == Some(id);
                                    rsx! {
                                        div {
                                            key: "{id}",
                                            class: if is_editing { "env-sidebar-item active" } else { "env-sidebar-item" },
                                            onclick: move |_| {
                                                editing_env_id.set(Some(id));
                                            },
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
                                    let is_active = active_env_id() == Some(env_id);
                                    let values_len = env.values.len();
                                    rsx! {
                                        div { class: "env-editor-top",
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
            } else {
                div { class: "vars-page-inner",
                    div { class: "vars-scope-banner",
                        div { class: "vars-scope-banner-title", "⚡ Variable System & Scopes" }
                        p { class: "vars-scope-banner-text",
                            "Variables parameterize URLs, headers, auth, and bodies via {{name}}. Scopes resolve from highest priority: Local → Environment → Collection → Global. Hardware & device secrets resolve via {{vault:key}}."
                        }
                    }

                    div { class: "vars-section",
                        div { class: "vars-section-head",
                            h4 { "Environment" }
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| {
                                    vars_active_tab.set(0);
                                    editing_env_id.set(active_env_id());
                                },
                                "Manage…"
                            }
                        }
                        p { class: "vars-section-desc",
                            "Target configurations (e.g. Localhost, Staging, Prod). Initial values sync to Git; current values stay local on this machine."
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
                                                for i in 0..values_len {
                                                    tr {
                                                        td {
                                                            input {
                                                                value: "{env.values.get(i).map(|r| r.key.clone()).unwrap_or_default()}",
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
                                                            input {
                                                                value: "{env.values.get(i).map(|r| r.initial_value.clone()).unwrap_or_default()}",
                                                                oninput: {
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
                        p { class: "vars-section-desc",
                            "Scoped to the active collection. Shared with your team and committed inside the collection file."
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

                    div { class: "vars-section",
                        h4 { "Local (Runtime)" }
                        p { class: "vars-section-desc",
                            "Ephemeral in-memory variables set during script execution via blitz.variables.set(). Discarded when request completes."
                        }
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
                                let mut st = st;
                                show_vars.set(false);
                                st.persist();
                            },
                            "Done"
                        }
                    }
                }
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
    let mut show_trash = st.show_trash;
    let mut show_snippets = st.show_snippets;
    let mut snippet_lang = st.snippet_lang;
    let mut snippet_resolve_vars = st.snippet_resolve_vars;
    let mut show_runner = st.show_runner;
    let mut runner_running = st.runner_running;
    let mut runner_report = st.runner_report;
    let mut show_perf = st.show_perf;
    let mut perf_running = st.perf_running;
    let mut perf_report = st.perf_report;
    let mut perf_vus = st.perf_vus;
    let mut perf_secs = st.perf_secs;
    let mut show_cookies = st.show_cookies;
    let mut show_team = st.show_team;
    let mut team_syncing = st.team_syncing;
    let mut team_activity = st.team_activity;
    let mut show_col_vars = st.show_col_vars;
    let mut col_vars_all = st.col_vars_all;
    let mut col_vars_open_id = st.col_vars_open_id;
    let mut impex_modal = st.impex_modal;
    let mut collections = st.collections;
    let mut environments = st.environments;
    let mut active_collection_id = st.active_collection_id;
    let mut active_request_id = st.active_request_id;
    let mut editing_collection_id = st.editing_collection_id;
    let mut conflict_state = st.conflict_state;
    let mut draft = st.draft;
    let mut trash = st.trash;
    let mut vars = st.vars;
    let mut cookies = st.cookies;
    let mut console_lines = st.console_lines;
    let mut console_open = st.console_open;
    let mut status_msg = st.status_msg;
    let mut fold_epoch = st.fold_epoch;
    let mut app_settings = st.app_settings;
    let active_env_id = st.active_env_id;

    rsx! {
        if show_palette() {
            crate::ui::command_palette::CommandPalette {
                st,
                on_send: move |_| on_send.call(()),
                on_save: move |_| on_save.call(()),
                on_new_tab: move |_| on_new_tab.call(()),
                on_close: move |_| show_palette.set(false),
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
                                    let mut st = st;
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
                                    st.persist();
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
                let code_content = if snippet_resolve_vars() {
                    let v = vars();
                    generate_snippet_resolved(&snippet_lang(), &draft(), Some(&v))
                } else {
                    generate_snippet_resolved(&snippet_lang(), &draft(), None)
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
                                            option { value: "{lang}", "{lang}" }
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
                        let mut st = st;
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
                            st.persist();
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
                                perf_vus.set(n.clamp(1, 50));
                            }
                        }
                    }
                    label { class: "hint", "Duration (seconds)" }
                    input {
                        r#type: "number",
                        value: "{perf_secs()}",
                        oninput: move |evt| {
                            if let Ok(n) = evt.value().parse::<u64>() {
                                perf_secs.set(n.clamp(1, 120));
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
                            "Done"
                        }
                    }
                }
            }
        }

        if show_team() {
            TeamModal {
                activity: team_activity(),
                is_syncing: team_syncing(),
                on_close: move |_| show_team.set(false),
                on_refresh: move |_| {
                    team_syncing.set(true);
                    spawn(async move {
                        let Ok(store) = JsonStore::open_default() else {
                            team_syncing.set(false);
                            return;
                        };
                        let settings = app_settings();
                        let clone_path = settings.cloud_sync.resolved_clone_path(store.root());
                        let res = tokio::task::spawn_blocking(move || {
                            get_team_activity(&clone_path, 30)
                        })
                        .await;
                        if let Ok(Ok(act)) = res {
                            team_activity.set(act);
                            status_msg.set("team: refreshed activity".into());
                        }
                        team_syncing.set(false);
                    });
                },
                on_pull: move |_| {
                    team_syncing.set(true);
                    spawn(async move {
                        let Ok(store) = JsonStore::open_default() else {
                            team_syncing.set(false);
                            return;
                        };
                        let root = store.root().to_path_buf();
                        let mut settings = app_settings();
                        let envs = environments();
                        let res = tokio::task::spawn_blocking(move || {
                            let r = pull_workspace(&root, &mut settings, &envs);
                            let clone_path = settings.cloud_sync.resolved_clone_path(&root);
                            let act = get_team_activity(&clone_path, 30).ok();
                            let conflicts = if r.is_err() {
                                detect_git_conflicts(&root).unwrap_or_default()
                            } else {
                                vec![]
                            };
                            (r, settings, act, conflicts)
                        })
                        .await;
                        match res {
                            Ok((Ok((cols, envs, r)), updated_settings, act, _)) => {
                                app_settings.set(updated_settings);
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
                                if let Some(act) = act {
                                    team_activity.set(act);
                                }
                                status_msg.set(r.message);
                                st.persist_local();
                            }
                            Ok((Err(e), _, _, conflicts)) => {
                                if !conflicts.is_empty() {
                                    show_team.set(false);
                                    conflict_state.set(Some(ConflictState {
                                        files: conflicts,
                                        selected_idx: 0,
                                    }));
                                    active_request_id.set(None);
                                    editing_collection_id.set(None);
                                    status_msg.set(
                                        "⚔️ Merge conflicts detected — opened in main section".into(),
                                    );
                                } else {
                                    status_msg.set(format!("pull: {e}"));
                                }
                            }
                            Err(e) => status_msg.set(format!("pull join: {e}")),
                        }
                        team_syncing.set(false);
                    });
                },
                on_push: move |_| {
                    team_syncing.set(true);
                    spawn(async move {
                        let Ok(store) = JsonStore::open_default() else {
                            team_syncing.set(false);
                            return;
                        };
                        let root = store.root().to_path_buf();
                        let mut settings = app_settings();
                        let envs = environments();
                        let cols = collections();
                        let v = vars();
                        let active_env_name = find_environment(&envs, active_env_id())
                            .map(|e| e.name.clone())
                            .unwrap_or_else(|| "No Environment".into());
                        let commit = settings.cloud_sync.commit_message.clone();
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
                            let clone_path = settings.cloud_sync.resolved_clone_path(&root);
                            let act = get_team_activity(&clone_path, 30).ok();
                            (r, settings, act)
                        })
                        .await;
                        if let Ok((r, updated_settings, act)) = res {
                            app_settings.set(updated_settings);
                            if let Some(act) = act {
                                team_activity.set(act);
                            }
                            status_msg.set(r.message);
                            st.persist_local();
                        }
                        team_syncing.set(false);
                    });
                },
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
                    let mut st = st;
                    let text = text.trim().to_string();
                    let imported = if text.contains("\"log\"") && text.contains("\"entries\"") {
                        import_har(&text)
                    } else if (text.trim_start().starts_with('{')
                        && (text.contains("\"openapi\"") || text.contains("\"paths\"")))
                        || (!text.trim_start().starts_with('{')
                            && (text.contains("openapi:") || text.contains("paths:")))
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
                            st.persist();
                            status_msg.set("imported collection".into());
                        }
                        Err(e) => status_msg.set(format!("import error: {e}")),
                    }
                },
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
            "Import from Postman export (v2.1 JSON / cURL / OpenAPI)".to_string()
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
