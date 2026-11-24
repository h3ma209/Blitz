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
    // Percentage share of the combined Baseline+Target value columns (50.0 = 50/50 split)
    let mut split_pct = use_signal(|| 50.0_f64);
    let mut split_drag = use_signal(|| None::<(f64, f64, f64)>); // (start_x, start_pct, table_px)
    let mut table_width_px = use_signal(|| 800.0_f64);

    if envs.len() < 2 {
        return rsx! {
            super::EmptyState {
                class: "env-diff-empty-state".to_string(),
                icon: "branch",
                title: "At least 2 environments are needed to compare".to_string(),
                description: "Create multiple environments (e.g. Local, Development, Production) in the 'Environments' tab to compare variables and inspect drift.".to_string(),
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| on_done.call(()),
                    "✕ Close"
                }
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

    let baseline_col_pct = (64.0 * (split_pct() / 100.0)).clamp(12.0, 52.0);
    let target_col_pct = (64.0 - baseline_col_pct).clamp(12.0, 52.0);
    let view_class = if split_drag().is_some() {
        "env-diff-view env-diff-resizing"
    } else {
        "env-diff-view"
    };

    rsx! {
        div {
            class: "{view_class}",
            onmousemove: move |evt| {
                if let Some((start_x, start_pct, tbl_w)) = split_drag() {
                    let combined_w = (tbl_w * 0.64).max(240.0);
                    let dx = evt.client_coordinates().x - start_x;
                    let next = (start_pct + (dx / combined_w) * 100.0).clamp(20.0, 80.0);
                    split_pct.set(next);
                }
            },
            onmouseup: move |_| {
                if split_drag().is_some() {
                    split_drag.set(None);
                }
            },
            onmouseleave: move |_| {
                if split_drag().is_some() {
                    split_drag.set(None);
                }
            },
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
                    button {
                        class: "btn btn-ghost btn-sm",
                        title: "Close comparison",
                        onclick: move |_| on_done.call(()),
                        "✕ Close"
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
                table {
                    id: "np-env-diff-table",
                    class: "env-diff-table",
                    colgroup {
                        col { style: "width: 90px;" }
                        col { style: "width: 22%;" }
                        col { style: "width: {baseline_col_pct:.2}%;" }
                        col { style: "width: {target_col_pct:.2}%;" }
                        col { style: "width: 14%;" }
                    }
                    thead {
                        tr {
                            th { "Status" }
                            th { "Variable" }
                            th {
                                class: "env-diff-th-resizable",
                                "{baseline.name} (Baseline)"
                                div {
                                    class: if split_drag().is_some() { "env-diff-col-resizer active" } else { "env-diff-col-resizer" },
                                    title: "Drag to resize Baseline / Target columns (double-click to reset 50/50)",
                                    onmousedown: move |evt| {
                                        evt.prevent_default();
                                        evt.stop_propagation();
                                        let start_x = evt.client_coordinates().x;
                                        let start_pct = split_pct();
                                        split_drag.set(Some((start_x, start_pct, table_width_px())));
                                        spawn(async move {
                                            if let Ok(v) = document::eval(
                                                "const t = document.getElementById('np-env-diff-table'); return t ? t.getBoundingClientRect().width : 800;",
                                            )
                                            .await
                                            {
                                                if let Some(w) = v.as_f64().filter(|w| *w > 100.0) {
                                                    table_width_px.set(w);
                                                    if let Some((sx, sp, _)) = split_drag() {
                                                        split_drag.set(Some((sx, sp, w)));
                                                    }
                                                }
                                            }
                                        });
                                    },
                                    ondoubleclick: move |evt| {
                                        evt.stop_propagation();
                                        split_pct.set(50.0);
                                        split_drag.set(None);
                                    },
                                }
                            }
                            th { "{target.name} (Target)" }
                            th { style: "text-align: right;", "Action" }
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
                            for (ri, row) in filtered_rows.into_iter().enumerate() {
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
                                        tr { key: "{ri}-{key}", class: "env-diff-row {row_class}",
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
