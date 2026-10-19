use dioxus::prelude::*;
use uuid::Uuid;

use crate::core::{AuthConfig, Collection, KeyValue, VariableContext};
use crate::runtime::{run_collection_with_config, RunnerConfig, RunnerReport};
use crate::ui::request_builder::{AuthEditor, ScriptsEditor};
use crate::ui::CommentableTextarea;
use crate::ui::loading::Spinner;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CollectionEditorTab {
    Overview,
    Authorization,
    Scripts,
    Variables,
    Runs,
}

#[component]
pub fn CollectionEditor(
    collection: Collection,
    vars: VariableContext,
    active_env_id: Option<Uuid>,
    on_change: EventHandler<Collection>,
    on_close: EventHandler<()>,
) -> Element {
    let mut tab = use_signal(|| CollectionEditorTab::Overview);

    // Runs tab runner state
    let mut runner_iterations = use_signal(|| 1usize);
    let mut runner_delay = use_signal(|| 0u64);
    let mut runner_data_file = use_signal(String::new);
    let mut runner_running = use_signal(|| false);
    let mut runner_report = use_signal(|| None::<RunnerReport>);

    let col_id = collection.id;
    let col_name = collection.name.clone();

    rsx! {
        div { class: "collection-editor-pane",
            // Header Bar
            div { class: "col-editor-header",
                div { class: "col-editor-title-wrap",
                    span { class: "tree-icon tree-icon-collection", aria_hidden: "true" }
                    input {
                        class: "col-editor-name-input",
                        value: "{col_name}",
                        placeholder: "Collection name",
                        oninput: {
                            let collection = collection.clone();
                            let on_change = on_change.clone();
                            move |evt: Event<FormData>| {
                                let mut c = collection.clone();
                                c.name = evt.value();
                                on_change.call(c);
                            }
                        }
                    }
                }
                button {
                    class: "btn btn-ghost btn-sm",
                    title: "Close collection editor",
                    onclick: move |_| on_close.call(()),
                    "Done"
                }
            }

            // Tab Bar: Overview, Authorization, Scripts, Variables, Runs
            div { class: "col-editor-tabs",
                button {
                    class: if tab() == CollectionEditorTab::Overview { "col-editor-tab active" } else { "col-editor-tab" },
                    onclick: move |_| tab.set(CollectionEditorTab::Overview),
                    "Overview"
                }
                button {
                    class: if tab() == CollectionEditorTab::Authorization { "col-editor-tab active" } else { "col-editor-tab" },
                    onclick: move |_| tab.set(CollectionEditorTab::Authorization),
                    "Authorization"
                }
                button {
                    class: if tab() == CollectionEditorTab::Scripts { "col-editor-tab active" } else { "col-editor-tab" },
                    onclick: move |_| tab.set(CollectionEditorTab::Scripts),
                    "Scripts"
                }
                button {
                    class: if tab() == CollectionEditorTab::Variables { "col-editor-tab active" } else { "col-editor-tab" },
                    onclick: move |_| tab.set(CollectionEditorTab::Variables),
                    "Variables ({collection.variables.len()})"
                }
                button {
                    class: if tab() == CollectionEditorTab::Runs { "col-editor-tab active" } else { "col-editor-tab" },
                    onclick: move |_| tab.set(CollectionEditorTab::Runs),
                    "Runs"
                }
            }

            // Tab Content Body
            div { class: "col-editor-body",
                match tab() {
                    CollectionEditorTab::Overview => rsx! {
                        div { class: "col-editor-section",
                            label { class: "col-field-label", "Description & Documentation" }
                            p { class: "hint",
                                "Document what this collection does, API base URLs, or onboarding instructions for team members. Supports Markdown."
                            }
                            CommentableTextarea {
                                class: "body-editor col-doc-editor".to_string(),
                                value: collection.description.clone(),
                                rows: Some("12"),
                                placeholder: "Describe the API endpoints in this collection...".to_string(),
                                on_change: {
                                    let collection = collection.clone();
                                    let on_change = on_change.clone();
                                    move |desc: String| {
                                        let mut c = collection.clone();
                                        c.description = desc;
                                        on_change.call(c);
                                    }
                                }
                            }
                            div { class: "col-meta-box",
                                div { class: "col-meta-row",
                                    span { class: "col-meta-key", "Collection ID:" }
                                    code { "{collection.id}" }
                                }
                                div { class: "col-meta-row",
                                    span { class: "col-meta-key", "Total Items:" }
                                    span { "{collection.items.len()} root items" }
                                }
                                div { class: "col-meta-row",
                                    span { class: "col-meta-key", "Variables:" }
                                    span { "{collection.variables.len()} defined" }
                                }
                            }
                        }
                    },
                    CollectionEditorTab::Authorization => rsx! {
                        div { class: "col-editor-section",
                            label { class: "col-field-label", "Collection Authorization" }
                            p { class: "hint",
                                "Requests in this collection with auth set to 'Inherit from parent' automatically inherit these credentials. Tip: use {{{{vault:key}}}} to protect tokens from being committed to Git."
                            }
                            div { class: "col-auth-wrapper",
                                AuthEditor {
                                    auth: collection.auth.clone(),
                                    ancestor_origin: None,
                                    on_edit_parent_auth: move |_| {},
                                    on_change: {
                                        let collection = collection.clone();
                                        let on_change = on_change.clone();
                                        move |new_auth: AuthConfig| {
                                            let mut c = collection.clone();
                                            c.auth = new_auth;
                                            on_change.call(c);
                                        }
                                    }
                                }
                            }
                        }
                    },
                    CollectionEditorTab::Scripts => rsx! {
                        div { class: "col-editor-section",
                            label { class: "col-field-label", "Collection Scripts" }
                            p { class: "hint",
                                "Collection scripts execute uniformly: Pre-request script runs before each child request; Test script runs after each child response."
                            }
                            ScriptsEditor {
                                pre: collection.pre_request_script.clone(),
                                test: collection.test_script.clone(),
                                vars: vars.clone(),
                                on_change: {
                                    let collection = collection.clone();
                                    let on_change = on_change.clone();
                                    move |(pre, test): (String, String)| {
                                        let mut c = collection.clone();
                                        c.pre_request_script = pre;
                                        c.test_script = test;
                                        on_change.call(c);
                                    }
                                }
                            }
                        }
                    },
                    CollectionEditorTab::Variables => rsx! {
                        div { class: "col-editor-section",
                            label { class: "col-field-label", "Collection Variables" }
                            p { class: "hint",
                                "Variables scoped to this collection. Resolves in all child requests via {{{{variable_name}}}}. Synced with Git for team sharing. For private tokens or passwords, use Vault instead."
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
                                    for (i, row) in collection.variables.iter().enumerate() {
                                        {
                                            let k = row.key.clone();
                                            let v = row.value.clone();
                                            rsx! {
                                                tr { key: "{col_id}-var-{i}",
                                                    td {
                                                        input {
                                                            value: "{k}",
                                                            placeholder: "Key",
                                                            oninput: {
                                                                let collection = collection.clone();
                                                                let on_change = on_change.clone();
                                                                move |evt: Event<FormData>| {
                                                                    let mut c = collection.clone();
                                                                    if let Some(r) = c.variables.get_mut(i) {
                                                                        r.key = evt.value();
                                                                    }
                                                                    on_change.call(c);
                                                                }
                                                            }
                                                        }
                                                    }
                                                    td {
                                                        input {
                                                            value: "{v}",
                                                            placeholder: "Initial value (synced)",
                                                            oninput: {
                                                                let collection = collection.clone();
                                                                let on_change = on_change.clone();
                                                                move |evt: Event<FormData>| {
                                                                    let mut c = collection.clone();
                                                                    if let Some(r) = c.variables.get_mut(i) {
                                                                        r.value = evt.value();
                                                                    }
                                                                    on_change.call(c);
                                                                }
                                                            }
                                                        }
                                                    }
                                                    td {
                                                        input {
                                                            value: "{vars.collection.get(&k).cloned().unwrap_or_else(|| v.clone())}",
                                                            placeholder: "Current value (active)",
                                                            disabled: true,
                                                            title: "Current value resolved from active runtime"
                                                        }
                                                    }
                                                    td {
                                                        button {
                                                            class: "btn btn-ghost",
                                                            title: "Remove variable",
                                                            onclick: {
                                                                let collection = collection.clone();
                                                                let on_change = on_change.clone();
                                                                move |_| {
                                                                    let mut c = collection.clone();
                                                                    if i < c.variables.len() {
                                                                        c.variables.remove(i);
                                                                    }
                                                                    on_change.call(c);
                                                                }
                                                            },
                                                            "×"
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            div { class: "kv-actions", style: "margin-top: 10px;",
                                button {
                                    class: "btn btn-ghost",
                                    onclick: {
                                        let collection = collection.clone();
                                        let on_change = on_change.clone();
                                        move |_| {
                                            let mut c = collection.clone();
                                            c.variables.push(KeyValue {
                                                key: String::new(),
                                                value: String::new(),
                                                enabled: true,
                                            });
                                            on_change.call(c);
                                        }
                                    },
                                    "+ Add variable"
                                }
                            }
                        }
                    },
                    CollectionEditorTab::Runs => rsx! {
                        div { class: "col-editor-section",
                            label { class: "col-field-label", "Collection Runner" }
                            p { class: "hint",
                                "Execute requests sequentially in tree order with full pre-request scripts, tests, cookies, and dynamic assertions."
                            }

                            div { class: "runner-config-row",
                                div { class: "runner-config-item",
                                    label { class: "hint", "Iterations" }
                                    input {
                                        r#type: "number",
                                        min: "1",
                                        class: "runner-input-number",
                                        value: "{runner_iterations()}",
                                        oninput: move |evt: Event<FormData>| {
                                            if let Ok(n) = evt.value().parse::<usize>() {
                                                runner_iterations.set(n.max(1));
                                            }
                                        }
                                    }
                                }
                                div { class: "runner-config-item",
                                    label { class: "hint", "Delay (ms)" }
                                    input {
                                        r#type: "number",
                                        min: "0",
                                        step: "50",
                                        class: "runner-input-number",
                                        value: "{runner_delay()}",
                                        oninput: move |evt: Event<FormData>| {
                                            if let Ok(d) = evt.value().parse::<u64>() {
                                                runner_delay.set(d);
                                            }
                                        }
                                    }
                                }
                                div { class: "runner-config-item flex-1",
                                    label { class: "hint", "Data File (CSV or JSON optional)" }
                                    input {
                                        r#type: "text",
                                        placeholder: "/path/to/data.json",
                                        value: "{runner_data_file()}",
                                        oninput: move |evt: Event<FormData>| {
                                            runner_data_file.set(evt.value());
                                        }
                                    }
                                }
                                button {
                                    class: if runner_running() { "btn btn-send disabled" } else { "btn btn-send" },
                                    style: "align-self: flex-end; margin-bottom: 2px;",
                                    disabled: runner_running(),
                                    onclick: {
                                        let col = collection.clone();
                                        let mut vars = vars.clone();
                                        move |_| {
                                            if runner_running() {
                                                return;
                                            }
                                            runner_running.set(true);
                                            runner_report.set(None);
                                            let col = col.clone();
                                            let mut ctx = vars.clone();
                                            ctx.collection = col.variables_map();
                                            let cfg = RunnerConfig {
                                                iterations: runner_iterations(),
                                                delay_ms: runner_delay(),
                                                data_file: runner_data_file(),
                                            };
                                            spawn(async move {
                                                let rep = tokio::task::spawn_blocking(move || {
                                                    let mut jar = crate::core::CookieJar::default();
                                                    run_collection_with_config(
                                                        &col,
                                                        &mut ctx,
                                                        &mut jar,
                                                        None,
                                                        None,
                                                        &cfg,
                                                    )
                                                })
                                                .await
                                                .unwrap_or_else(|_| RunnerReport::default());
                                                runner_report.set(Some(rep));
                                                runner_running.set(false);
                                            });
                                        }
                                    },
                                    if runner_running() { "Running…" } else { "Run Collection" }
                                }
                            }

                            if runner_running() && runner_report().is_none() {
                                div { class: "loading-row", style: "margin: 20px 0;",
                                    Spinner {}
                                    span { class: "loading-text", "Executing requests and scripts across collection…" }
                                }
                            }

                            if let Some(rep) = runner_report() {
                                div { class: "runner-results-card",
                                    div { class: "runner-results-summary",
                                        span { class: "runner-stat-pill stat-pass", "✓ {rep.passed_count()} Passed" }
                                        span { class: "runner-stat-pill stat-fail", "✗ {rep.failed_count()} Failed" }
                                        span { class: "runner-stat-pill stat-total", "Total: {rep.results.len()} Tests" }
                                    }
                                    table { class: "kv-table runner-table",
                                        thead {
                                            tr {
                                                th { "Iter" }
                                                th { "Result" }
                                                th { "Method" }
                                                th { "Name" }
                                                th { "Status" }
                                                th { "Time" }
                                            }
                                        }
                                        tbody {
                                            for r in rep.results.iter() {
                                                tr {
                                                    td { "{r.iteration}" }
                                                    td {
                                                        span {
                                                            class: if r.passed { "status-chip status-ok" } else { "status-chip status-err" },
                                                            if r.passed { "PASS" } else { "FAIL" }
                                                        }
                                                    }
                                                    td { "{r.method}" }
                                                    td { "{r.name}" }
                                                    td {
                                                        if let Some(s) = r.status {
                                                            "{s}"
                                                        } else if let Some(e) = &r.error {
                                                            span { class: "status-err", "{e}" }
                                                        } else {
                                                            "—"
                                                        }
                                                    }
                                                    td {
                                                        if let Some(ms) = r.elapsed_ms {
                                                            "{ms} ms"
                                                        } else {
                                                            "—"
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    if !rep.console.is_empty() {
                                        details { class: "runner-console-details",
                                            summary { "Console Output ({rep.console.len()} lines)" }
                                            pre { class: "response-body", "{rep.console.join(\"\\n\")}" }
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
