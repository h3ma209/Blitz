//! Environment Variable Diff & Drift Inspector.
//! Side-by-side comparison between environments, drift inspection, and 1-click synchronization.

use dioxus::prelude::*;
use uuid::Uuid;
use crate::core::{
    compare_environments, export_diff_markdown, sync_all_missing_to_target, sync_var_to_target,
    DiffStatus, Environment,
};

#[component]
pub fn EnvDiffView(
    mut environments: Signal<Vec<Environment>>,
    active_env_id: Signal<Option<Uuid>>,
    on_done: EventHandler<()>,
) -> Element {
    let envs = environments();
    let default_baseline = active_env_id().or_else(|| envs.first().map(|e| e.id));
    let default_target = envs
        .iter()
        .find(|e| Some(e.id) != default_baseline)
        .map(|e| e.id)
        .or(default_baseline);

    let mut baseline_id = use_signal(|| default_baseline);
    let mut target_id = use_signal(|| default_target);
    let mut search_query = use_signal(String::new);
    let mut diffs_only = use_signal(|| false);
    let mut copied_report = use_signal(|| false);
    let mut action_flash = use_signal(|| None::<String>);

    if envs.len() < 2 {
        return rsx! {
            div { class: "env-diff-empty-state",
                span { class: "action-icon action-icon-branch", style: "width: 32px; height: 32px; color: var(--text-muted); margin-bottom: 12px;", aria_hidden: "true" }
                h3 { "At least 2 environments are needed to compare" }
                p { class: "hint", "Create multiple environments (e.g. Local, Development, Production) in the 'Environments' tab to compare variables and inspect drift." }
            }
        };
    }

    let b_env = envs.iter().find(|e| Some(e.id) == baseline_id());
    let t_env = envs.iter().find(|e| Some(e.id) == target_id());

    let (Some(baseline), Some(target)) = (b_env, t_env) else {
        return rsx! { div { class: "hint", "Select environments to compare." } };
    };

    let report = compare_environments(baseline, target);
    let query_lower = search_query().trim().to_lowercase();

    let filtered_rows: Vec<_> = report.rows.iter().filter(|r| {
        let matches_search = query_lower.is_empty()
            || r.key.to_lowercase().contains(&query_lower)
            || r.baseline_var.as_ref().map(|v| v.effective().to_lowercase().contains(&query_lower)).unwrap_or(false)
            || r.target_var.as_ref().map(|v| v.effective().to_lowercase().contains(&query_lower)).unwrap_or(false);

        let matches_diff = !diffs_only() || r.status != DiffStatus::Matching;

        matches_search && matches_diff
    }).collect();

    rsx! {
        div { class: "env-diff-view",
            // Toolbar: Dropdowns & Action Buttons
            div { class: "env-diff-toolbar",
                div { class: "env-diff-select-group",
                    label { class: "env-diff-label", "Baseline:" }
                    select {
                        class: "env-diff-select",
                        value: "{baseline.id}",
                        onchange: move |evt: Event<FormData>| {
                            if let Ok(id) = Uuid::parse_str(&evt.value()) {
                                baseline_id.set(Some(id));
                            }
                        },
                        for e in &envs {
                            option {
                                value: "{e.id}",
                                selected: Some(e.id) == baseline_id(),
                                "{e.name}"
                            }
                        }
                    }
                    button {
                        class: "btn btn-ghost btn-sm env-diff-swap-btn",
                        title: "Swap baseline and target environments",
                        onclick: move |_| {
                            let b = baseline_id();
                            let t = target_id();
                            baseline_id.set(t);
                            target_id.set(b);
                        },
                        span { "⇄" }
                    }
                    label { class: "env-diff-label", "Target:" }
                    select {
                        class: "env-diff-select",
                        value: "{target.id}",
                        onchange: move |evt: Event<FormData>| {
                            if let Ok(id) = Uuid::parse_str(&evt.value()) {
                                target_id.set(Some(id));
                            }
                        },
                        for e in &envs {
                            option {
                                value: "{e.id}",
                                selected: Some(e.id) == target_id(),
                                "{e.name}"
                            }
                        }
                    }
                }

                div { class: "env-diff-search-group",
                    input {
                        class: "env-diff-search-input",
                        placeholder: "Filter variables…",
                        value: "{search_query()}",
                        oninput: move |evt: Event<FormData>| search_query.set(evt.value()),
                    }
                    if !search_query().is_empty() {
                        button {
                            class: "btn btn-ghost btn-sm",
                            onclick: move |_| search_query.set(String::new()),
                            "✕"
                        }
                    }
                    button {
                        class: if diffs_only() { "btn btn-ghost btn-sm active" } else { "btn btn-ghost btn-sm" },
                        title: "Show only variables with drift or missing status",
                        onclick: move |_| diffs_only.set(!diffs_only()),
                        if diffs_only() { "Drift Only" } else { "All Variables" }
                    }
                }

                div { class: "env-diff-actions-group",
                    if report.missing_in_target_count > 0 {
                        button {
                            class: "btn btn-primary btn-sm",
                            title: "Copy all variables missing in {target.name} from {baseline.name}",
                            onclick: {
                                let b = baseline.clone();
                                let t_id = target.id;
                                move |_| {
                                    let mut cur_envs = environments();
                                    if let Some(t) = cur_envs.iter_mut().find(|e| e.id == t_id) {
                                        let added = sync_all_missing_to_target(t, &b, true);
                                        action_flash.set(Some(format!("Added {added} missing variables to {}", t.name)));
                                    }
                                    environments.set(cur_envs);
                                }
                            },
                            span { "+ Sync All Missing ({report.missing_in_target_count})" }
                        }
                    }
                    button {
                        class: "btn btn-ghost btn-sm",
                        title: "Export comparison summary as Markdown",
                        onclick: {
                            let rep = report.clone();
                            move |_| {
                                let md = export_diff_markdown(&rep);
                                let script = format!(
                                    "navigator.clipboard.writeText({});",
                                    serde_json::to_string(&md).unwrap_or_else(|_| "\"\"".into())
                                );
                                spawn(async move {
                                    let _ = document::eval(&script).await;
                                    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                                    copied_report.set(false);
                                });
                                copied_report.set(true);
                            }
                        },
                        if copied_report() {
                            span { "✓ Copied MD" }
                        } else {
                            span { "Copy Report" }
                        }
                    }
                }
            }

            if let Some(msg) = action_flash() {
                div { class: "env-diff-flash-banner",
                    span { "✓ {msg}" }
                    button {
                        class: "env-diff-flash-close",
                        onclick: move |_| action_flash.set(None),
                        "✕"
                    }
                }
            }

            // Telemetry Gauges / Stat Chips
            div { class: "env-diff-telemetry",
                div { class: "env-telemetry-chip",
                    span { class: "telemetry-num", "{report.total_keys}" }
                    span { class: "telemetry-title", "Total Keys" }
                }
                div { class: "env-telemetry-chip chip-match",
                    span { class: "telemetry-num", "{report.matching_count}" }
                    span { class: "telemetry-title", "Matching" }
                }
                div { class: "env-telemetry-chip chip-drift",
                    span { class: "telemetry-num", "{report.drift_count}" }
                    span { class: "telemetry-title", "Value Drift" }
                }
                div { class: "env-telemetry-chip chip-missing",
                    span { class: "telemetry-num", "{report.missing_in_target_count}" }
                    span { class: "telemetry-title", "Missing in Target" }
                }
                div { class: "env-telemetry-chip chip-extra",
                    span { class: "telemetry-num", "{report.extra_in_target_count}" }
                    span { class: "telemetry-title", "Extra in Target" }
                }
            }

            // Comparison Table
            div { class: "env-diff-table-wrap",
                table { class: "env-diff-table",
                    thead {
                        tr {
                            th { style: "width: 90px;", "Status" }
                            th { style: "width: 22%;", "Variable" }
                            th { style: "width: 32%;", "{baseline.name} (Baseline)" }
                            th { style: "width: 32%;", "{target.name} (Target)" }
                            th { style: "width: 14%; text-align: right;", "Action" }
                        }
                    }
                    tbody {
                        if filtered_rows.is_empty() {
                            tr {
                                td { colspan: "5", class: "env-diff-empty-row",
                                    if diffs_only() {
                                        "No differences found! Environments are completely in sync."
                                    } else {
                                        "No matching variables found."
                                    }
                                }
                            }
                        } else {
                            for row in filtered_rows {
                                {
                                    let key = row.key.clone();
                                    let status = row.status;
                                    let b_var = row.baseline_var.clone();
                                    let t_var = row.target_var.clone();

                                    let (b_val, b_class) = match &b_var {
                                        Some(v) => (v.effective().to_string(), "val-present"),
                                        None => ("(not set)".to_string(), "val-none"),
                                    };
                                    let (t_val, t_class) = match &t_var {
                                        Some(v) => (v.effective().to_string(), "val-present"),
                                        None => ("(missing)".to_string(), "val-none val-missing"),
                                    };

                                    let row_class = match status {
                                        DiffStatus::Matching => "row-match",
                                        DiffStatus::Drift => "row-drift",
                                        DiffStatus::MissingInTarget => "row-missing",
                                        DiffStatus::ExtraInTarget => "row-extra",
                                    };

                                    rsx! {
                                        tr { key: "{key}", class: "env-diff-row {row_class}",
                                            td {
                                                span { class: "diff-status-pill {status.css_class()}",
                                                    "{status.label()}"
                                                }
                                            }
                                            td {
                                                code { class: "env-diff-key", "{key}" }
                                            }
                                            td {
                                                span { class: "env-diff-val {b_class}", title: "{b_val}",
                                                    "{b_val}"
                                                }
                                            }
                                            td {
                                                span { class: "env-diff-val {t_class}", title: "{t_val}",
                                                    "{t_val}"
                                                }
                                            }
                                            td { style: "text-align: right;",
                                                match status {
                                                    DiffStatus::MissingInTarget => {
                                                        let b_v = b_var.clone();
                                                        let t_id = target.id;
                                                        rsx! {
                                                            button {
                                                                class: "btn btn-ghost btn-sm btn-sync-var",
                                                                title: "Copy this variable to {target.name}",
                                                                onclick: move |_| {
                                                                    if let Some(bv) = &b_v {
                                                                        let mut cur_envs = environments();
                                                                        if let Some(t) = cur_envs.iter_mut().find(|e| e.id == t_id) {
                                                                            sync_var_to_target(t, bv, true);
                                                                        }
                                                                        environments.set(cur_envs);
                                                                    }
                                                                },
                                                                "+ Copy"
                                                            }
                                                        }
                                                    }
                                                    DiffStatus::Drift => {
                                                        let b_v = b_var.clone();
                                                        let t_id = target.id;
                                                        rsx! {
                                                            button {
                                                                class: "btn btn-ghost btn-sm btn-sync-var",
                                                                title: "Sync value from {baseline.name} to {target.name}",
                                                                onclick: move |_| {
                                                                    if let Some(bv) = &b_v {
                                                                        let mut cur_envs = environments();
                                                                        if let Some(t) = cur_envs.iter_mut().find(|e| e.id == t_id) {
                                                                            sync_var_to_target(t, bv, true);
                                                                        }
                                                                        environments.set(cur_envs);
                                                                    }
                                                                },
                                                                "⎘ Sync"
                                                            }
                                                        }
                                                    }
                                                    DiffStatus::ExtraInTarget => {
                                                        let t_v = t_var.clone();
                                                        let b_id = baseline.id;
                                                        rsx! {
                                                            button {
                                                                class: "btn btn-ghost btn-sm btn-sync-var",
                                                                title: "Copy extra variable into {baseline.name}",
                                                                onclick: move |_| {
                                                                    if let Some(tv) = &t_v {
                                                                        let mut cur_envs = environments();
                                                                        if let Some(b) = cur_envs.iter_mut().find(|e| e.id == b_id) {
                                                                            sync_var_to_target(b, tv, true);
                                                                        }
                                                                        environments.set(cur_envs);
                                                                    }
                                                                },
                                                                "+ To Base"
                                                            }
                                                        }
                                                    }
                                                    DiffStatus::Matching => rsx! {
                                                        span { class: "env-diff-match-check", "✓" }
                                                    },
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

            div { class: "env-diff-footer",
                button {
                    class: "btn btn-send",
                    onclick: move |_| on_done.call(()),
                    "Done"
                }
            }
        }
    }
}
