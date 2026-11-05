use dioxus::prelude::*;
use uuid::Uuid;

use crate::core::{Collection, PerfReport};
use crate::runtime::ordered_requests;
use crate::ui::{loading::Spinner, ModalShell};

#[component]
pub fn PerformanceModal(
    collections: Vec<Collection>,
    active_collection_id: Option<Uuid>,
    active_request_id: Option<Uuid>,
    running: bool,
    report: Option<PerfReport>,
    initial_vus: u32,
    initial_secs: u64,
    on_close: EventHandler<()>,
    on_run: EventHandler<(Uuid, Option<Uuid>, u32, u64)>,
) -> Element {
    let mut selected_col_id = use_signal(|| {
        active_collection_id.or_else(|| collections.first().map(|c| c.id))
    });
    let mut selected_req_id = use_signal(|| active_request_id);
    let mut vus = use_signal(|| initial_vus.clamp(1, 100));
    let mut duration_secs = use_signal(|| initial_secs.clamp(1, 120));
    let mut copied_json = use_signal(|| false);

    let current_col = collections.iter().find(|c| Some(c.id) == selected_col_id());
    let requests = current_col.map(ordered_requests).unwrap_or_default();

    rsx! {
        ModalShell {
            title: "Performance Benchmark & Load Tester".to_string(),
            class: "perf-runner-modal".to_string(),
            on_close: move |_| on_close.call(()),
            div { class: "modal-body perf-modal-body",
                p { class: "hint",
                    "Simulate concurrent virtual users (VUs) hitting your endpoints. Pure native Tokio/Rust threads with sub-millisecond telemetry."
                }

                // Target Configuration Card
                div { class: "perf-config-card",
                    div { class: "perf-form-group",
                        label { class: "perf-label", "Target Collection" }
                        select {
                            class: "perf-select",
                            value: "{selected_col_id().map(|id| id.to_string()).unwrap_or_default()}",
                            disabled: running,
                            onchange: move |evt: Event<FormData>| {
                                if let Ok(id) = Uuid::parse_str(&evt.value()) {
                                    selected_col_id.set(Some(id));
                                    selected_req_id.set(None);
                                }
                            },
                            for c in collections.iter() {
                                option {
                                    value: "{c.id}",
                                    selected: selected_col_id() == Some(c.id),
                                    "{c.name}"
                                }
                            }
                        }
                    }

                    div { class: "perf-form-group",
                        label { class: "perf-label", "Benchmark Target" }
                        select {
                            class: "perf-select",
                            value: "{selected_req_id().map(|id| id.to_string()).unwrap_or_default()}",
                            disabled: running,
                            onchange: move |evt: Event<FormData>| {
                                let val = evt.value();
                                if val.is_empty() {
                                    selected_req_id.set(None);
                                } else if let Ok(id) = Uuid::parse_str(&val) {
                                    selected_req_id.set(Some(id));
                                }
                            },
                            option { value: "", "⚡ All endpoints in collection (sequential loop)" }
                            for req in requests.iter() {
                                option {
                                    value: "{req.id}",
                                    selected: selected_req_id() == Some(req.id),
                                    "[{req.method.as_str()}] {req.name}"
                                }
                            }
                        }
                    }
                }

                // Concurrency & Duration Settings
                div { class: "perf-settings-row",
                    div { class: "perf-setting-col",
                        div { class: "perf-label-bar",
                            label { class: "perf-label", "Virtual Users (VUs)" }
                            span { class: "perf-val-pill", "{vus()} concurrent" }
                        }
                        input {
                            class: "perf-input",
                            r#type: "number",
                            min: "1",
                            max: "100",
                            value: "{vus()}",
                            disabled: running,
                            oninput: move |evt: Event<FormData>| {
                                if let Ok(n) = evt.value().parse::<u32>() {
                                    vus.set(n.clamp(1, 100));
                                }
                            }
                        }
                        div { class: "perf-pills",
                            for count in [5u32, 10, 25, 50] {
                                button {
                                    class: if vus() == count { "perf-pill active" } else { "perf-pill" },
                                    disabled: running,
                                    onclick: move |_| vus.set(count),
                                    "{count} VUs"
                                }
                            }
                        }
                    }

                    div { class: "perf-setting-col",
                        div { class: "perf-label-bar",
                            label { class: "perf-label", "Duration" }
                            span { class: "perf-val-pill", "{duration_secs()}s" }
                        }
                        input {
                            class: "perf-input",
                            r#type: "number",
                            min: "1",
                            max: "120",
                            value: "{duration_secs()}",
                            disabled: running,
                            oninput: move |evt: Event<FormData>| {
                                if let Ok(n) = evt.value().parse::<u64>() {
                                    duration_secs.set(n.clamp(1, 120));
                                }
                            }
                        }
                        div { class: "perf-pills",
                            for secs in [5u64, 10, 30, 60] {
                                button {
                                    class: if duration_secs() == secs { "perf-pill active" } else { "perf-pill" },
                                    disabled: running,
                                    onclick: move |_| duration_secs.set(secs),
                                    "{secs}s"
                                }
                            }
                        }
                    }
                }

                // In-Flight Status
                if running {
                    div { class: "perf-running-banner",
                        Spinner { }
                        div { class: "perf-running-info",
                            span { class: "perf-running-title", "Benchmarking target in progress…" }
                            span { class: "perf-running-sub", "Generating continuous load with {vus()} concurrent workers" }
                        }
                    }
                }

                // Results Dashboard
                if let Some(rep) = report.as_ref() {
                    {
                        let total = rep.total_requests;
                        let errors = rep.errors;
                        let success_count = total.saturating_sub(errors);
                        let success_pct = if total > 0 { (success_count as f64 / total as f64 * 100.0).round() as u32 } else { 0 };
                        let error_pct = 100u32.saturating_sub(success_pct);
                        let is_clean = errors == 0;
                        let rps = rep.rps();
                        let avg_ms = rep.avg_latency_ms();
                        let min_ms = rep.latency_min_ms;
                        let max_ms = rep.latency_max_ms;

                        rsx! {
                            div { class: "perf-results-container",
                                div { class: "perf-results-header",
                                    span { class: "perf-results-title", "Benchmark Telemetry" }
                                    span { class: if is_clean { "perf-status-badge status-ok" } else { "perf-status-badge status-err" },
                                        if is_clean { "100% Success" } else { "{rep.error_rate():.1}% Error Rate" }
                                    }
                                }

                                // 4-Metric Grid
                                div { class: "perf-metrics-grid",
                                    div { class: "perf-metric-card",
                                        span { class: "perf-metric-label", "Throughput" }
                                        span { class: "perf-metric-val val-green", "{rps:.1}" }
                                        span { class: "perf-metric-unit", "requests / sec" }
                                    }
                                    div { class: "perf-metric-card",
                                        span { class: "perf-metric-label", "Average Latency" }
                                        span { class: "perf-metric-val", "{avg_ms:.1} ms" }
                                        span { class: "perf-metric-unit", "response time" }
                                    }
                                    div { class: "perf-metric-card",
                                        span { class: "perf-metric-label", "Latency Range" }
                                        span { class: "perf-metric-val val-mono", "{min_ms}–{max_ms} ms" }
                                        span { class: "perf-metric-unit", "min … max" }
                                    }
                                    div { class: "perf-metric-card",
                                        span { class: "perf-metric-label", "Total Volume" }
                                        span { class: "perf-metric-val", "{total}" }
                                        span { class: "perf-metric-unit", "{errors} errors" }
                                    }
                                }

                                // Visual Ratio Bar
                                if total > 0 {
                                    div { class: "perf-ratio-bar-wrap",
                                        div { class: "perf-ratio-bar",
                                            div {
                                                class: "perf-ratio-seg seg-success",
                                                style: "width: {success_pct}%;",
                                                title: "Success: {success_count} ({success_pct}%)",
                                            }
                                            if error_pct > 0 {
                                                div {
                                                    class: "perf-ratio-seg seg-error",
                                                    style: "width: {error_pct}%;",
                                                    title: "Errors: {errors} ({error_pct}%)",
                                                }
                                            }
                                        }
                                        div { class: "perf-ratio-labels",
                                            span { class: "ratio-lbl lbl-success", "✓ {success_count} succeeded ({success_pct}%)" }
                                            if errors > 0 {
                                                span { class: "ratio-lbl lbl-error", "✕ {errors} failed ({error_pct}%)" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Modal Actions
                div { class: "modal-actions perf-actions",
                    if let Some(rep) = report.as_ref() {
                        {
                            let rep_clone = rep.clone();
                            rsx! {
                                button {
                                    class: "btn btn-ghost",
                                    title: "Copy JSON report to clipboard",
                                    onclick: move |_| {
                                        let json_str = serde_json::to_string_pretty(&rep_clone).unwrap_or_default();
                                        let script = format!(
                                            "navigator.clipboard.writeText({});",
                                            serde_json::to_string(&json_str).unwrap_or_else(|_| "\"\"".into())
                                        );
                                        spawn(async move {
                                            let _ = document::eval(&script).await;
                                            tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
                                            copied_json.set(false);
                                        });
                                        copied_json.set(true);
                                    },
                                    if copied_json() {
                                        span { class: "action-icon action-icon-check", aria_hidden: "true", style: "width: 12px; height: 12px; margin-right: 4px;" }
                                        span { "Copied!" }
                                    } else {
                                        span { class: "action-icon action-icon-copy", aria_hidden: "true", style: "width: 12px; height: 12px; margin-right: 4px;" }
                                        span { "Copy JSON" }
                                    }
                                }
                            }
                        }
                    }
                    button {
                        class: "btn btn-ghost",
                        onclick: move |_| on_close.call(()),
                        "Close"
                    }
                    button {
                        class: "btn btn-send perf-run-btn",
                        disabled: running || selected_col_id().is_none(),
                        onclick: move |_| {
                            if let Some(col_id) = selected_col_id() {
                                on_run.call((col_id, selected_req_id(), vus(), duration_secs()));
                            }
                        },
                        if running {
                            div { class: "loading-row",
                                Spinner { }
                                span { class: "loading-text", "Benchmarking…" }
                            }
                        } else if report.is_some() {
                            span { "⚡ Rerun Benchmark" }
                        } else {
                            span { "⚡ Start Benchmark" }
                        }
                    }
                }
            }
        }
    }
}
