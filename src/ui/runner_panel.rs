use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;
use uuid::Uuid;

use crate::core::{Collection, CollectionItem, Environment, HttpMethod, RequestDraft, RequestProtocol};
use crate::runtime::{
    load_data_rows, ordered_requests_with_folders, to_html, to_junit_xml, RunnerItemResult,
    RunnerProgressSnapshot, RunnerReport,
};
use crate::ui::loading::{SkeletonBars, Spinner};
use crate::ui::EmptyState;

#[derive(Clone, PartialEq)]
pub enum RunnerEvent {
    Close,
    Cancel,
    OpenRequest {
        collection_id: Uuid,
        request_id: Uuid,
    },
    Run {
        collection_id: Uuid,
        folder_filter: Option<String>,
        selected_request_ids: Vec<Uuid>,
        environment_id: Option<Uuid>,
        iterations: usize,
        delay_ms: u64,
        data_file: String,
        stop_on_error: bool,
        persist_vars: bool,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResultFilter {
    All,
    Passed,
    Failed,
}

fn collect_folder_breadcrumbs(items: &[CollectionItem], prefix: &str, out: &mut Vec<String>) {
    for item in items {
        if let CollectionItem::Folder(f) = item {
            let path = if prefix.is_empty() {
                f.name.clone()
            } else {
                format!("{prefix}/{}", f.name)
            };
            out.push(path.clone());
            collect_folder_breadcrumbs(&f.items, &path, out);
        }
    }
}

fn method_badge_class(method: &str) -> &'static str {
    match method.to_ascii_uppercase().as_str() {
        "GET" => "get",
        "POST" => "post",
        "PUT" => "put",
        "PATCH" => "patch",
        "DELETE" => "delete",
        "HEAD" => "head",
        "OPTIONS" => "options",
        "GRPC" => "grpc",
        "SSE" => "sse",
        _ => "get",
    }
}

fn request_method_rank(r: &RequestDraft) -> u8 {
    match r.protocol {
        RequestProtocol::Http => match r.method {
            HttpMethod::Get => 0,
            HttpMethod::Post => 1,
            HttpMethod::Put => 2,
            HttpMethod::Patch => 3,
            HttpMethod::Delete => 4,
            HttpMethod::Head => 5,
            HttpMethod::Options => 6,
        },
        _ => 10,
    }
}

fn format_markdown_summary(col_name: &str, rep: &RunnerReport) -> String {
    let total = rep.results.len();
    let passed = rep.passed_count();
    let failed = rep.failed_count();
    let pct = if total > 0 {
        (passed as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    let mut out = format!(
        "### ⚡ Blitz Collection Run: **{col_name}**\n\n- **Result:** {passed}/{total} passed ({pct:.1}%), **{failed} failed**\n- **Assertions:** {}/{}\n- **Duration:** {} ms (avg {} ms, p95 {} ms)\n\n| Iter | Result | Method | Request | Status | Time |\n|---|---|---|---|---|---|\n",
        rep.passed_assertions(),
        rep.total_assertions(),
        rep.total_duration_ms(),
        rep.avg_duration_ms(),
        rep.p95_duration_ms(),
    );
    for r in &rep.results {
        let icon = if r.passed { "✅ PASS" } else { "❌ FAIL" };
        let status = r
            .status
            .map(|s| s.to_string())
            .unwrap_or_else(|| if r.error.is_some() { "ERR".into() } else { "—".into() });
        let time = r
            .elapsed_ms
            .map(|m| format!("{m} ms"))
            .unwrap_or_else(|| "—".into());
        let label = if r.folder_path.is_empty() {
            r.name.clone()
        } else {
            format!("{} / {}", r.folder_path, r.name)
        };
        out.push_str(&format!(
            "| #{} | {} | `{}` | {} | {} | {} |\n",
            r.iteration, icon, r.method, label, status, time
        ));
    }
    out
}

#[component]
pub fn RunnerPanel(
    collections: Vec<Collection>,
    environments: Vec<Environment>,
    active_env_id: Option<Uuid>,
    selected_id: Option<Uuid>,
    running: bool,
    #[props(default)] progress: Option<RunnerProgressSnapshot>,
    report: Option<RunnerReport>,
    on_event: EventHandler<RunnerEvent>,
) -> Element {
    let mut sel_col = use_signal(|| selected_id.or_else(|| collections.first().map(|c| c.id)));
    let mut sel_env = use_signal(|| active_env_id);
    let mut folder_filter = use_signal(|| None::<String>);
    let mut excluded_ids = use_signal(HashSet::<Uuid>::new);
    let mut custom_order = use_signal(Vec::<Uuid>::new);
    let mut dragging_id = use_signal(|| None::<Uuid>);
    let mut drop_target_id = use_signal(|| None::<Uuid>);
    let mut queue_search = use_signal(String::new);
    let mut iterations = use_signal(|| 1usize);
    let mut delay_ms = use_signal(|| 0u64);
    let mut data_file = use_signal(String::new);
    let mut stop_on_error = use_signal(|| false);
    let mut persist_vars = use_signal(|| true);
    let mut right_tab = use_signal(|| 0usize); // 0 = Sequence, 1 = Results
    let mut queue_scroll_row = use_signal(|| 0usize);

    let active_col = sel_col().and_then(|id| collections.iter().find(|c| c.id == id));
    let mut folder_paths = Vec::new();
    if let Some(col) = active_col {
        collect_folder_breadcrumbs(&col.items, "", &mut folder_paths);
    }

    let all_rows = active_col
        .map(ordered_requests_with_folders)
        .unwrap_or_default();

    let scoped_rows: Vec<(String, RequestDraft)> = match folder_filter().as_deref() {
        Some(f) if !f.is_empty() => {
            let f_low = f.to_ascii_lowercase();
            let prefix = format!("{f_low}/");
            all_rows
                .into_iter()
                .filter(|(fp, _)| {
                    let fp_low = fp.to_ascii_lowercase();
                    fp_low == f_low || fp_low.starts_with(&prefix)
                })
                .collect()
        }
        _ => all_rows,
    };

    // Apply custom drag/drop / preset ordering cleanly while preserving new/unseen IDs in tree order.
    let mut by_id: HashMap<Uuid, (String, RequestDraft)> =
        scoped_rows.iter().map(|(fp, r)| (r.id, (fp.clone(), r.clone()))).collect();
    let mut ordered_queue: Vec<(String, RequestDraft)> = Vec::with_capacity(scoped_rows.len());
    for id in custom_order().iter() {
        if let Some(item) = by_id.remove(id) {
            ordered_queue.push(item);
        }
    }
    for (fp, r) in &scoped_rows {
        if let Some(item) = by_id.remove(&r.id) {
            let _ = (fp, r);
            ordered_queue.push(item);
        }
    }

    let ordered_ids: Vec<Uuid> = ordered_queue.iter().map(|(_, r)| r.id).collect();
    let selected_ids: Vec<Uuid> = ordered_queue
        .iter()
        .filter(|(_, r)| !excluded_ids().contains(&r.id))
        .map(|(_, r)| r.id)
        .collect();

    let q_low = queue_search().trim().to_ascii_lowercase();
    let displayed_queue: Vec<(usize, String, RequestDraft)> = ordered_queue
        .iter()
        .enumerate()
        .filter(|(_, (fp, r))| {
            q_low.is_empty()
                || r.name.to_ascii_lowercase().contains(&q_low)
                || fp.to_ascii_lowercase().contains(&q_low)
                || r.url.to_ascii_lowercase().contains(&q_low)
                || r.method.as_str().to_ascii_lowercase().contains(&q_low)
        })
        .map(|(idx, (fp, r))| (idx, fp.clone(), r.clone()))
        .collect();

    let col_name = active_col
        .map(|c| c.name.clone())
        .unwrap_or_else(|| "Collection".to_string());

    let data_status = if data_file().trim().is_empty() {
        None
    } else {
        Some(load_data_rows(&data_file()))
    };

    let move_in_order = {
        let base_ids = ordered_ids.clone();
        move |from_id: Uuid, to_idx: usize| {
            let mut list = base_ids.clone();
            if let Some(pos) = list.iter().position(|id| *id == from_id) {
                let item = list.remove(pos);
                let insert_at = to_idx.min(list.len());
                list.insert(insert_at, item);
                custom_order.set(list);
            }
        }
    };

    rsx! {
        div { class: "stage-page runner-stage-page",
            div { class: "stage-page-head",
                h2 { "Collection Runner" }
                button {
                    class: "btn btn-ghost vars-close-btn",
                    r#type: "button",
                    title: "Close (Esc)",
                    onclick: move |_| on_event.call(RunnerEvent::Close),
                    "✕ Close (Esc)"
                }
            }
            div { class: "stage-page-body runner-modal-body",
                div { class: "runner-workspace-grid",
                    // Left Pane: Target, Flow Options & Interactive Drag-Drop Queue
                    div { class: "runner-setup-pane",
                        div { class: "runner-section-title", "Target & Execution Flow" }

                        label { class: "hint", "Collection" }
                        select {
                            class: "runner-select",
                            value: "{sel_col().map(|id| id.to_string()).unwrap_or_default()}",
                            onchange: move |evt: Event<FormData>| {
                                if let Ok(id) = Uuid::parse_str(&evt.value()) {
                                    sel_col.set(Some(id));
                                    folder_filter.set(None);
                                    excluded_ids.set(HashSet::new());
                                    custom_order.set(Vec::new());
                                }
                            },
                            for (ci, c) in collections.iter().enumerate() {
                                option {
                                    key: "{c.id}-{ci}",
                                    value: "{c.id}",
                                    selected: sel_col() == Some(c.id),
                                    "{c.name}"
                                }
                            }
                        }

                        div { class: "runner-two-col",
                            div { class: "runner-field",
                                label { class: "hint", "Folder Scope" }
                                select {
                                    class: "runner-select",
                                    value: "{folder_filter().unwrap_or_default()}",
                                    onchange: move |evt: Event<FormData>| {
                                        let val = evt.value();
                                        folder_filter.set(if val.is_empty() { None } else { Some(val) });
                                        excluded_ids.set(HashSet::new());
                                        custom_order.set(Vec::new());
                                    },
                                    option { value: "", selected: folder_filter().is_none(), "All Folders" }
                                    for (fi, fp) in folder_paths.iter().enumerate() {
                                        option {
                                            key: "fp-{fi}",
                                            value: "{fp}",
                                            selected: folder_filter().as_deref() == Some(fp.as_str()),
                                            "{fp}"
                                        }
                                    }
                                }
                            }
                            div { class: "runner-field",
                                label { class: "hint", "Environment" }
                                select {
                                    class: "runner-select",
                                    value: "{sel_env().map(|id| id.to_string()).unwrap_or_default()}",
                                    onchange: move |evt: Event<FormData>| {
                                        let v = evt.value();
                                        sel_env.set(Uuid::parse_str(&v).ok());
                                    },
                                    option { value: "", selected: sel_env().is_none(), "No Environment" }
                                    for (ei, e) in environments.iter().enumerate() {
                                        option {
                                            key: "{e.id}-{ei}",
                                            value: "{e.id}",
                                            selected: sel_env() == Some(e.id),
                                            "{e.name}"
                                        }
                                    }
                                }
                            }
                        }

                        div { class: "runner-two-col",
                            div { class: "runner-field",
                                label { class: "hint", "Iterations" }
                                input {
                                    r#type: "number",
                                    min: "1",
                                    max: "500",
                                    value: "{iterations()}",
                                    oninput: move |evt| {
                                        if let Ok(n) = evt.value().parse::<usize>() {
                                            iterations.set(n.clamp(1, 500));
                                        }
                                    }
                                }
                            }
                            div { class: "runner-field",
                                label { class: "hint", "Delay (ms)" }
                                input {
                                    r#type: "number",
                                    min: "0",
                                    step: "50",
                                    value: "{delay_ms()}",
                                    oninput: move |evt| {
                                        if let Ok(n) = evt.value().parse::<u64>() {
                                            delay_ms.set(n.min(60_000));
                                        }
                                    }
                                }
                            }
                        }

                        div { class: "runner-field",
                            div { class: "runner-field-header",
                                label { class: "hint", "Data File (CSV or JSON array)" }
                                match &data_status {
                                    Some(Ok(rows)) => rsx! {
                                        span { class: "runner-data-badge ok", "{rows.len()} row(s)" }
                                    },
                                    Some(Err(_)) => rsx! {
                                        span { class: "runner-data-badge err", "Unreadable" }
                                    },
                                    None => rsx! {},
                                }
                            }
                            div { class: "runner-datafile-row",
                                input {
                                    r#type: "text",
                                    placeholder: "/path/to/data.csv or .json",
                                    value: "{data_file()}",
                                    oninput: move |evt| data_file.set(evt.value()),
                                }
                                button {
                                    class: "btn btn-sm btn-ghost",
                                    r#type: "button",
                                    title: "Browse CSV or JSON file",
                                    onclick: move |_| {
                                        spawn(async move {
                                            if let Some(path) = rfd::FileDialog::new()
                                                .set_title("Select Iteration Data File")
                                                .add_filter("Data Files", &["csv", "json"])
                                                .pick_file()
                                            {
                                                data_file.set(path.to_string_lossy().into_owned());
                                            }
                                        });
                                    },
                                    "Browse…"
                                }
                                if !data_file().is_empty() {
                                    button {
                                        class: "btn btn-sm btn-ghost",
                                        r#type: "button",
                                        title: "Clear data file",
                                        onclick: move |_| data_file.set(String::new()),
                                        "✕"
                                    }
                                }
                            }
                            if let Some(Ok(rows)) = &data_status {
                                if !rows.is_empty() && !rows[0].is_empty() {
                                    {
                                        let mut keys: Vec<String> = rows[0].keys().cloned().collect();
                                        keys.sort();
                                        let preview_rows: Vec<_> = rows.iter().take(4).cloned().collect();
                                        rsx! {
                                            details { class: "runner-data-preview",
                                                summary { "Preview Data ({keys.len()} vars × {rows.len()} rows)" }
                                                div { class: "runner-data-table-wrap",
                                                    table { class: "runner-data-table",
                                                        thead {
                                                            tr {
                                                                th { "#" }
                                                                for k in &keys {
                                                                    th { key: "dh-{k}", "{k}" }
                                                                }
                                                            }
                                                        }
                                                        tbody {
                                                            for (ri, rmap) in preview_rows.iter().enumerate() {
                                                                tr { key: "dr-{ri}",
                                                                    td { "{ri + 1}" }
                                                                    for k in &keys {
                                                                        td { key: "dc-{ri}-{k}", "{rmap.get(k).map(String::as_str).unwrap_or(\"\")}" }
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
                        }

                        div { class: "runner-toggles",
                            label { class: "runner-check-row",
                                input {
                                    r#type: "checkbox",
                                    checked: stop_on_error(),
                                    onchange: move |evt| stop_on_error.set(evt.checked()),
                                }
                                span { "Stop run on first error / failed test" }
                            }
                            label { class: "runner-check-row",
                                input {
                                    r#type: "checkbox",
                                    checked: persist_vars(),
                                    onchange: move |evt| persist_vars.set(evt.checked()),
                                }
                                span { "Persist env, collection & cookie updates" }
                            }
                        }

                        div { class: "runner-run-footer",
                            if running {
                                button {
                                    class: "btn btn-ghost btn-cancel",
                                    style: "flex: 1; justify-content: center;",
                                    r#type: "button",
                                    onclick: move |_| on_event.call(RunnerEvent::Cancel),
                                    span { class: "loading-spinner-cancel" }
                                    span { "⏹ Stop Collection Run" }
                                }
                            } else {
                                button {
                                    class: "btn btn-send",
                                    style: "flex: 1; justify-content: center;",
                                    r#type: "button",
                                    disabled: sel_col().is_none() || selected_ids.is_empty(),
                                    onclick: {
                                        let sel_vec = selected_ids.clone();
                                        move |_| {
                                            if let Some(id) = sel_col() {
                                                right_tab.set(1);
                                                on_event.call(RunnerEvent::Run {
                                                    collection_id: id,
                                                    folder_filter: folder_filter(),
                                                    selected_request_ids: sel_vec.clone(),
                                                    environment_id: sel_env(),
                                                    iterations: iterations(),
                                                    delay_ms: delay_ms(),
                                                    data_file: data_file(),
                                                    stop_on_error: stop_on_error(),
                                                    persist_vars: persist_vars(),
                                                });
                                            }
                                        }
                                    },
                                    "▶ Run {col_name} ({selected_ids.len()} reqs)"
                                }
                            }
                        }
                    }

                    // Right Pane: Execution Sequence (pre-run) & Live Results Inspector (during/post-run)
                    div { class: "runner-output-pane",
                        if running || report.is_some() {
                            div { class: "runner-right-tabs",
                                button {
                                    class: if right_tab() == 1 { "vars-tab active" } else { "vars-tab" },
                                    r#type: "button",
                                    onclick: move |_| right_tab.set(1),
                                    if running && report.is_none() {
                                        "Live Run"
                                    } else {
                                        "Results"
                                    }
                                }
                                button {
                                    class: if right_tab() == 0 { "vars-tab active" } else { "vars-tab" },
                                    r#type: "button",
                                    onclick: move |_| right_tab.set(0),
                                    "Execution Sequence ({selected_ids.len()}/{ordered_queue.len()})"
                                }
                            }
                        }

                        if (!running && report.is_none()) || right_tab() == 0 {
                            div { class: "runner-queue-header",
                                span { class: "runner-section-title", style: "margin: 0;",
                                    "Execution Sequence ({selected_ids.len()}/{ordered_queue.len()}) · Drag to Reorder"
                                }
                            }

                            div { class: "runner-queue-toolbar",
                                input {
                                    class: "runner-queue-search",
                                    r#type: "text",
                                    placeholder: "Filter requests by name, folder, method…",
                                    value: "{queue_search()}",
                                    oninput: move |evt| queue_search.set(evt.value()),
                                }
                                div { class: "runner-preset-bar",
                                    button {
                                        class: "runner-preset-chip",
                                        r#type: "button",
                                        onclick: move |_| excluded_ids.set(HashSet::new()),
                                        "All"
                                    }
                                    button {
                                        class: "runner-preset-chip",
                                        r#type: "button",
                                        onclick: {
                                            let all_vis = ordered_ids.clone();
                                            move |_| excluded_ids.set(all_vis.iter().copied().collect())
                                        },
                                        "None"
                                    }
                                    button {
                                        class: "runner-preset-chip",
                                        r#type: "button",
                                        title: "Invert checked selection",
                                        onclick: {
                                            let all_vis = ordered_ids.clone();
                                            move |_| {
                                                let cur = excluded_ids();
                                                let next: HashSet<Uuid> = all_vis
                                                    .iter()
                                                    .copied()
                                                    .filter(|id| !cur.contains(id))
                                                    .collect();
                                                excluded_ids.set(next);
                                            }
                                        },
                                        "Invert"
                                    }
                                    button {
                                        class: "runner-preset-chip",
                                        r#type: "button",
                                        title: "Select safe read-only GET requests only",
                                        onclick: {
                                            let items = ordered_queue.clone();
                                            move |_| {
                                                let next: HashSet<Uuid> = items
                                                    .iter()
                                                    .filter(|(_, r)| r.method != HttpMethod::Get)
                                                    .map(|(_, r)| r.id)
                                                    .collect();
                                                excluded_ids.set(next);
                                            }
                                        },
                                        "GET Only"
                                    }
                                    button {
                                        class: "runner-preset-chip",
                                        r#type: "button",
                                        title: "Select requests that have test assertions",
                                        onclick: {
                                            let items = ordered_queue.clone();
                                            move |_| {
                                                let next: HashSet<Uuid> = items
                                                    .iter()
                                                    .filter(|(_, r)| r.test_script.trim().is_empty())
                                                    .map(|(_, r)| r.id)
                                                    .collect();
                                                excluded_ids.set(next);
                                            }
                                        },
                                        "Has Tests"
                                    }
                                    span { class: "runner-preset-sep" }
                                    button {
                                        class: "runner-preset-chip",
                                        r#type: "button",
                                        title: "Sort queue A → Z",
                                        onclick: {
                                            let mut items = ordered_queue.clone();
                                            move |_| {
                                                items.sort_by_key(|a| a.1.name.to_lowercase());
                                                custom_order.set(items.iter().map(|(_, r)| r.id).collect());
                                            }
                                        },
                                        "A→Z"
                                    }
                                    button {
                                        class: "runner-preset-chip",
                                        r#type: "button",
                                        title: "Sort queue by HTTP method",
                                        onclick: {
                                            let mut items = ordered_queue.clone();
                                            move |_| {
                                                items.sort_by(|a, b| {
                                                    request_method_rank(&a.1)
                                                        .cmp(&request_method_rank(&b.1))
                                                        .then_with(|| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase()))
                                                });
                                                custom_order.set(items.iter().map(|(_, r)| r.id).collect());
                                            }
                                        },
                                        "Method"
                                    }
                                    button {
                                        class: "runner-preset-chip",
                                        r#type: "button",
                                        title: "Reverse current queue order",
                                        onclick: {
                                            let mut ids = ordered_ids.clone();
                                            move |_| {
                                                ids.reverse();
                                                custom_order.set(ids.clone());
                                            }
                                        },
                                        "Reverse"
                                    }
                                    button {
                                        class: "runner-preset-chip",
                                        r#type: "button",
                                        title: "Reset queue back to collection tree order",
                                        onclick: move |_| custom_order.set(Vec::new()),
                                        "Reset"
                                    }
                                }
                            }

                            {
                                let q_len = displayed_queue.len();
                                let (q_start, q_end) = crate::ui::list_window(q_len, queue_scroll_row(), 100, 120);
                                let top_pad = q_start * 30;
                                let bot_pad = q_len.saturating_sub(q_end) * 30;
                                rsx! {
                                    div {
                                        class: "runner-req-checklist",
                                        onscroll: move |evt: Event<ScrollData>| {
                                            if q_len <= 100 {
                                                return;
                                            }
                                            let row = (evt.data().scroll_top() / 30.0) as usize;
                                            let bucket = (row / 30) * 30;
                                            if queue_scroll_row() != bucket {
                                                queue_scroll_row.set(bucket);
                                            }
                                        },
                                        if displayed_queue.is_empty() {
                                            div { class: "runner-queue-empty", "No matching requests in scope." }
                                        } else {
                                            {
                                                let slice: Vec<_> = displayed_queue.into_iter().skip(q_start).take(q_end - q_start).collect();
                                                rsx! {
                                                    if top_pad > 0 {
                                                        div { style: "height: {top_pad}px; flex-shrink: 0;" }
                                                    }
                                            for (pos, fp, req) in slice {
                                                {
                                                    let rid = req.id;
                                                    let checked = !excluded_ids().contains(&rid);
                                                    let is_drag = dragging_id() == Some(rid);
                                                    let is_drop = drop_target_id() == Some(rid) && dragging_id() != Some(rid);
                                                    let badge = if req.protocol == RequestProtocol::Http {
                                                        req.method.as_str()
                                                    } else {
                                                        req.protocol.badge_label()
                                                    };
                                                    let m_cls = method_badge_class(badge);
                                                    let has_tests = !req.test_script.trim().is_empty();
                                                    let has_pre = !req.pre_request_script.trim().is_empty();
                                                    let total_len = ordered_ids.len();
                                                    let row_cls = match (checked, is_drag, is_drop) {
                                                        (_, true, _) => "runner-req-row is-dragging",
                                                        (_, _, true) => "runner-req-row is-drop-target",
                                                        (false, _, _) => "runner-req-row is-dim",
                                                        (true, false, false) => "runner-req-row",
                                                    };
                                                    let move_up = {
                                                        let mut move_fn = move_in_order.clone();
                                                        move |evt: Event<MouseData>| {
                                                            evt.stop_propagation();
                                                            evt.prevent_default();
                                                            if pos > 0 {
                                                                move_fn(rid, pos - 1);
                                                            }
                                                        }
                                                    };
                                                    let move_dn = {
                                                        let mut move_fn = move_in_order.clone();
                                                        move |evt: Event<MouseData>| {
                                                            evt.stop_propagation();
                                                            evt.prevent_default();
                                                            if pos + 1 < total_len {
                                                                move_fn(rid, pos + 1);
                                                            }
                                                        }
                                                    };
                                                    let handle_drop = {
                                                        let mut move_fn = move_in_order.clone();
                                                        move |evt: Event<DragData>| {
                                                            evt.prevent_default();
                                                            evt.stop_propagation();
                                                            if let Some(src) = dragging_id() {
                                                                if src != rid {
                                                                    move_fn(src, pos);
                                                                }
                                                            }
                                                            dragging_id.set(None);
                                                            drop_target_id.set(None);
                                                        }
                                                    };
                                                    let cur_col_id = sel_col().unwrap_or_default();
                                                    rsx! {
                                                        div {
                                                            key: "{rid}-{pos}",
                                                            class: "{row_cls}",
                                                            draggable: "true",
                                                            ondragstart: move |evt| {
                                                                evt.stop_propagation();
                                                                dragging_id.set(Some(rid));
                                                                let _ = evt.data.data_transfer().set_data(
                                                                    "text/plain",
                                                                    &rid.to_string(),
                                                                );
                                                            },
                                                            ondragover: move |evt| {
                                                                if dragging_id().is_some_and(|s| s != rid) {
                                                                    evt.prevent_default();
                                                                    drop_target_id.set(Some(rid));
                                                                }
                                                            },
                                                            ondragleave: move |_| {
                                                                if drop_target_id() == Some(rid) {
                                                                    drop_target_id.set(None);
                                                                }
                                                            },
                                                            ondragend: move |_| {
                                                                dragging_id.set(None);
                                                                drop_target_id.set(None);
                                                            },
                                                            ondrop: handle_drop,
                                                            span { class: "runner-drag-handle", title: "Drag to reorder", "⋮⋮" }
                                                            input {
                                                                r#type: "checkbox",
                                                                checked,
                                                                onclick: move |evt| evt.stop_propagation(),
                                                                onchange: move |evt| {
                                                                    let mut set = excluded_ids();
                                                                    if evt.checked() {
                                                                        set.remove(&rid);
                                                                    } else {
                                                                        set.insert(rid);
                                                                    }
                                                                    excluded_ids.set(set);
                                                                }
                                                            }
                                                            span { class: "runner-req-idx", "{pos + 1}" }
                                                            span { class: "method-badge method-{m_cls}", "{badge}" }
                                                            span { class: "runner-req-label",
                                                                if !fp.is_empty() {
                                                                    span { class: "runner-req-folder", "{fp} / " }
                                                                }
                                                                "{req.name}"
                                                            }
                                                            if has_pre {
                                                                span { class: "runner-mini-tag", "pre" }
                                                            }
                                                            if has_tests {
                                                                span { class: "runner-mini-tag test", "tests" }
                                                            }
                                                            div { class: "runner-row-actions",
                                                                button {
                                                                    class: "runner-row-btn",
                                                                    r#type: "button",
                                                                    title: "Move up",
                                                                    disabled: pos == 0,
                                                                    onclick: move_up,
                                                                    "↑"
                                                                }
                                                                button {
                                                                    class: "runner-row-btn",
                                                                    r#type: "button",
                                                                    title: "Move down",
                                                                    disabled: pos + 1 >= total_len,
                                                                    onclick: move_dn,
                                                                    "↓"
                                                                }
                                                                button {
                                                                    class: "runner-row-btn",
                                                                    r#type: "button",
                                                                    title: "Open request in workbench",
                                                                    onclick: move |evt| {
                                                                        evt.stop_propagation();
                                                                        on_event.call(RunnerEvent::OpenRequest {
                                                                            collection_id: cur_col_id,
                                                                            request_id: rid,
                                                                        });
                                                                    },
                                                                    "↗"
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                                    if bot_pad > 0 {
                                                        div { style: "height: {bot_pad}px; flex-shrink: 0;" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        } else if running && report.is_none() {
                            div { class: "runner-running-state",
                                {
                                    let snap = progress.clone().unwrap_or_default();
                                    let tot = if snap.total > 0 {
                                        snap.total
                                    } else {
                                        selected_ids.len().saturating_mul(iterations())
                                    };
                                    let done = snap.completed.min(tot);
                                    let pct = if tot > 0 {
                                        (done as f64 / tot as f64) * 100.0
                                    } else {
                                        0.0
                                    };
                                    let cur = if snap.current_label.is_empty() {
                                        format!(
                                            "Executing {} request(s) × {} iteration(s)…",
                                            selected_ids.len(),
                                            iterations()
                                        )
                                    } else {
                                        snap.current_label
                                    };
                                    rsx! {
                                        div { class: "loading-row",
                                            Spinner {}
                                            span { class: "loading-text",
                                                "Executing {done} / {tot} ({pct:.0}%)"
                                            }
                                            if snap.passed > 0 || snap.failed > 0 {
                                                span { class: "runner-assert-pill ok", "✓ {snap.passed}" }
                                                if snap.failed > 0 {
                                                    span { class: "runner-assert-pill fail", "✕ {snap.failed}" }
                                                }
                                            }
                                        }
                                        div { class: "runner-detail-url", "{cur}" }
                                        div { class: "runner-progress-track", style: "max-width: 480px;",
                                            div {
                                                class: "runner-progress-pass",
                                                style: "width: {pct:.1}%;",
                                            }
                                        }
                                    }
                                }
                                SkeletonBars { n: 5 }
                            }
                        } else if let Some(rep) = report.as_ref() {
                            {
                                let all_vis_for_retry = ordered_ids.clone();
                                let col_for_open = sel_col().unwrap_or_default();
                                let f_filter = folder_filter();
                                let s_env = sel_env();
                                let iters = iterations();
                                let del = delay_ms();
                                let df = data_file();
                                let stop_e = stop_on_error();
                                let pers_v = persist_vars();
                                rsx! {
                                    RunnerResultsView {
                                        collection_name: col_name.clone(),
                                        report: rep.clone(),
                                        on_retry_failed: Some(EventHandler::new(move |failed_ids: Vec<Uuid>| {
                                            let failed_set: HashSet<Uuid> = failed_ids.iter().copied().collect();
                                            let excl: HashSet<Uuid> = all_vis_for_retry
                                                .iter()
                                                .copied()
                                                .filter(|id| !failed_set.contains(id))
                                                .collect();
                                            excluded_ids.set(excl);
                                            if let Some(id) = sel_col() {
                                                on_event.call(RunnerEvent::Run {
                                                    collection_id: id,
                                                    folder_filter: f_filter.clone(),
                                                    selected_request_ids: failed_ids,
                                                    environment_id: s_env,
                                                    iterations: iters,
                                                    delay_ms: del,
                                                    data_file: df.clone(),
                                                    stop_on_error: stop_e,
                                                    persist_vars: pers_v,
                                                });
                                            }
                                        })),
                                        on_open_request: Some(EventHandler::new(move |rid: Uuid| {
                                            on_event.call(RunnerEvent::OpenRequest {
                                                collection_id: col_for_open,
                                                request_id: rid,
                                            });
                                        })),
                                    }
                                }
                            }
                        } else {
                            EmptyState {
                                class: "runner-empty-state".to_string(),
                                icon: "play",
                                title: "Ready to run collection".to_string(),
                                description: "Drag rows or use ↑/↓ on the left to customize execution order. Pick scope, data files, and presets (GET Only / Has Tests), then click ▶ Run.".to_string(),
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn RunnerResultsView(
    collection_name: String,
    report: RunnerReport,
    #[props(default)] on_retry_failed: Option<EventHandler<Vec<Uuid>>>,
    #[props(default)] on_open_request: Option<EventHandler<Uuid>>,
) -> Element {
    let mut filter = use_signal(|| ResultFilter::All);
    let mut iter_filter = use_signal(|| 0usize); // 0 = All iterations
    let mut result_search = use_signal(String::new);
    let mut expanded_rows = use_signal(HashSet::<usize>::new);
    let mut copied_md = use_signal(|| false);

    let total = report.results.len();
    let passed = report.passed_count();
    let failed = report.failed_count();
    let pass_pct = if total > 0 {
        (passed as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    let total_asserts = report.total_assertions();
    let passed_asserts = report.passed_assertions();
    let total_ms = report.total_duration_ms();
    let avg_ms = report.avg_duration_ms();
    let p95_ms = report.p95_duration_ms();
    let slowest = report
        .max_duration_ms()
        .map(|(ms, n)| (ms, n.to_string()));
    let max_iter = report.results.iter().map(|r| r.iteration).max().unwrap_or(1);

    // Unique failed request IDs for Re-run Failed
    let mut failed_req_ids: Vec<Uuid> = Vec::new();
    for r in &report.results {
        if !r.passed && !failed_req_ids.contains(&r.request_id) {
            failed_req_ids.push(r.request_id);
        }
    }

    let r_search_low = result_search().trim().to_ascii_lowercase();
    let filtered_results: Vec<(usize, &RunnerItemResult)> = report
        .results
        .iter()
        .enumerate()
        .filter(|(_, r)| match filter() {
            ResultFilter::All => true,
            ResultFilter::Passed => r.passed,
            ResultFilter::Failed => !r.passed,
        })
        .filter(|(_, r)| iter_filter() == 0 || r.iteration == iter_filter())
        .filter(|(_, r)| {
            r_search_low.is_empty()
                || r.name.to_ascii_lowercase().contains(&r_search_low)
                || r.folder_path.to_ascii_lowercase().contains(&r_search_low)
                || r.url.to_ascii_lowercase().contains(&r_search_low)
                || r.method.to_ascii_lowercase().contains(&r_search_low)
                || r.status
                    .is_some_and(|s| s.to_string().contains(&r_search_low))
        })
        .collect();

    let rep_xml = report.clone();
    let col_xml = collection_name.clone();
    let rep_html = report.clone();
    let col_html = collection_name.clone();
    let rep_json = report.clone();
    let col_json = collection_name.clone();
    let rep_md = report.clone();
    let col_md = collection_name.clone();

    rsx! {
        div { class: "runner-results-view",
            // Standout KPI Cards Grid
            div { class: "runner-kpi-grid",
                div { class: if failed == 0 { "runner-kpi-card is-good" } else { "runner-kpi-card is-warn" },
                    span { class: "runner-kpi-label", "Pass Rate" }
                    span { class: "runner-kpi-val", "{pass_pct:.0}%" }
                    span { class: "runner-kpi-sub", "{passed} passed · {failed} failed" }
                }
                div { class: "runner-kpi-card",
                    span { class: "runner-kpi-label", "Assertions" }
                    span { class: "runner-kpi-val", "{passed_asserts}/{total_asserts}" }
                    span { class: "runner-kpi-sub",
                        if total_asserts == 0 {
                            "No test scripts"
                        } else if passed_asserts == total_asserts {
                            "All assertions passing"
                        } else {
                            "{total_asserts.saturating_sub(passed_asserts)} assertion(s) failed"
                        }
                    }
                }
                div { class: "runner-kpi-card",
                    span { class: "runner-kpi-label", "Latency (Avg / P95)" }
                    span { class: "runner-kpi-val", "{avg_ms} / {p95_ms} ms" }
                    span { class: "runner-kpi-sub", "Total network time: {total_ms} ms" }
                }
                if let Some((slow_ms, slow_name)) = slowest {
                    div { class: "runner-kpi-card",
                        span { class: "runner-kpi-label", "Slowest Endpoint" }
                        span { class: "runner-kpi-val", "{slow_ms} ms" }
                        span { class: "runner-kpi-sub runner-kpi-truncate", title: "{slow_name}", "{slow_name}" }
                    }
                }
            }

            // Progress Bar
            div { class: "runner-progress-track",
                div {
                    class: "runner-progress-pass",
                    style: "width: {pass_pct:.1}%;",
                }
            }

            // Controls, Retry Failed, Search & Export Toolbar
            div { class: "runner-results-header",
                div { class: "runner-filter-bar",
                    button {
                        class: if filter() == ResultFilter::All { "runner-filter-btn active" } else { "runner-filter-btn" },
                        r#type: "button",
                        onclick: move |_| filter.set(ResultFilter::All),
                        "All ({total})"
                    }
                    button {
                        class: if filter() == ResultFilter::Passed { "runner-filter-btn active pass" } else { "runner-filter-btn" },
                        r#type: "button",
                        onclick: move |_| filter.set(ResultFilter::Passed),
                        "Passed ({passed})"
                    }
                    button {
                        class: if filter() == ResultFilter::Failed { "runner-filter-btn active fail" } else { "runner-filter-btn" },
                        r#type: "button",
                        onclick: move |_| filter.set(ResultFilter::Failed),
                        "Failed ({failed})"
                    }
                    if max_iter > 1 {
                        select {
                            class: "runner-iter-select",
                            value: "{iter_filter()}",
                            onchange: move |evt: Event<FormData>| {
                                iter_filter.set(evt.value().parse().unwrap_or(0));
                            },
                            option { value: "0", "All Iters ({max_iter})" }
                            for n in 1..=max_iter {
                                option { key: "iter-opt-{n}", value: "{n}", "Iter #{n}" }
                            }
                        }
                    }
                    input {
                        class: "runner-results-search",
                        r#type: "text",
                        placeholder: "Filter results…",
                        value: "{result_search()}",
                        oninput: move |evt| result_search.set(evt.value()),
                    }
                }
                div { class: "runner-export-actions",
                    if !failed_req_ids.is_empty() {
                        if let Some(retry_cb) = on_retry_failed {
                            button {
                                class: "btn btn-sm runner-retry-btn",
                                r#type: "button",
                                title: "Re-run only the failed requests",
                                onclick: {
                                    let ids = failed_req_ids.clone();
                                    move |_| retry_cb.call(ids.clone())
                                },
                                "↻ Re-run Failed ({failed_req_ids.len()})"
                            }
                        }
                    }
                    button {
                        class: "btn btn-sm btn-ghost",
                        r#type: "button",
                        title: "Copy Markdown summary for PR / Slack",
                        onclick: move |_| {
                            let md = format_markdown_summary(&col_md, &rep_md);
                            let js = format!(
                                "navigator.clipboard.writeText({});",
                                serde_json::to_string(&md).unwrap_or_else(|_| "\"\"".into())
                            );
                            copied_md.set(true);
                            spawn(async move {
                                let _ = document::eval(&js).await;
                            });
                        },
                        if copied_md() { "✓ Copied MD" } else { "Copy MD" }
                    }
                    button {
                        class: "btn btn-sm btn-ghost",
                        r#type: "button",
                        title: "Export structured JSON report",
                        onclick: move |_| {
                            let payload = serde_json::json!({
                                "collection": col_json,
                                "passed": rep_json.passed_count(),
                                "failed": rep_json.failed_count(),
                                "avg_ms": rep_json.avg_duration_ms(),
                                "p95_ms": rep_json.p95_duration_ms(),
                                "results": rep_json.results,
                            });
                            let raw = serde_json::to_string_pretty(&payload).unwrap_or_default();
                            let slug = col_json.to_lowercase().replace(' ', "_");
                            spawn(async move {
                                if let Some(path) = rfd::FileDialog::new()
                                    .set_title("Export JSON Runner Report")
                                    .set_file_name(format!("{slug}_report.json"))
                                    .add_filter("JSON", &["json"])
                                    .save_file()
                                {
                                    let _ = std::fs::write(path, raw);
                                }
                            });
                        },
                        "JSON…"
                    }
                    button {
                        class: "btn btn-sm btn-ghost",
                        r#type: "button",
                        title: "Export runner report as JUnit XML for CI/CD",
                        onclick: move |_| {
                            let xml = to_junit_xml(&col_xml, &rep_xml);
                            let slug = col_xml.to_lowercase().replace(' ', "_");
                            spawn(async move {
                                if let Some(path) = rfd::FileDialog::new()
                                    .set_title("Export JUnit XML Report")
                                    .set_file_name(format!("{slug}_junit.xml"))
                                    .add_filter("XML", &["xml"])
                                    .save_file()
                                {
                                    let _ = std::fs::write(path, xml);
                                }
                            });
                        },
                        "JUnit XML…"
                    }
                    button {
                        class: "btn btn-sm btn-ghost",
                        r#type: "button",
                        title: "Export runner report as self-contained HTML",
                        onclick: move |_| {
                            let html = to_html(&col_html, &rep_html);
                            let slug = col_html.to_lowercase().replace(' ', "_");
                            spawn(async move {
                                if let Some(path) = rfd::FileDialog::new()
                                    .set_title("Export HTML Report")
                                    .set_file_name(format!("{slug}_report.html"))
                                    .add_filter("HTML", &["html"])
                                    .save_file()
                                {
                                    let _ = std::fs::write(path, html);
                                }
                            });
                        },
                        "HTML…"
                    }
                }
            }

            // Expandable Result Rows
            div { class: "runner-results-list",
                if filtered_results.is_empty() {
                    div { class: "runner-queue-empty", "No matching results for this filter." }
                } else {
                    for (idx, r) in filtered_results {
                        {
                            let is_open = expanded_rows().contains(&idx);
                            let m_cls = method_badge_class(&r.method);
                            let r_assert_total = r.test_results.len();
                            let r_assert_pass = r.test_results.iter().filter(|t| t.passed).count();
                            let rid = r.request_id;
                            rsx! {
                                div {
                                    key: "res-{idx}-{r.iteration}",
                                    class: if r.passed { "runner-result-item is-pass" } else { "runner-result-item is-fail" },
                                    div {
                                        class: "runner-result-summary",
                                        onclick: move |_| {
                                            let mut set = expanded_rows();
                                            if set.contains(&idx) {
                                                set.remove(&idx);
                                            } else {
                                                set.insert(idx);
                                            }
                                            expanded_rows.set(set);
                                        },
                                        span { class: "runner-chevron", if is_open { "▾" } else { "▸" } }
                                        span { class: "runner-iter-pill", "#{r.iteration}" }
                                        span {
                                            class: if r.passed { "status-chip status-ok" } else { "status-chip status-err" },
                                            if r.passed { "PASS" } else { "FAIL" }
                                        }
                                        span { class: "method-badge method-{m_cls}", "{r.method}" }
                                        div { class: "runner-result-title",
                                            if !r.folder_path.is_empty() {
                                                span { class: "runner-req-folder", "{r.folder_path} / " }
                                            }
                                            span { class: "runner-result-name", "{r.name}" }
                                        }
                                        if r_assert_total > 0 {
                                            span {
                                                class: if r_assert_pass == r_assert_total { "runner-assert-pill ok" } else { "runner-assert-pill fail" },
                                                "{r_assert_pass}/{r_assert_total} tests"
                                            }
                                        }
                                        span { class: "runner-result-status",
                                            if let Some(s) = r.status {
                                                "{s}"
                                            } else if r.error.is_some() {
                                                "ERR"
                                            } else {
                                                "—"
                                            }
                                        }
                                        span { class: "runner-result-time",
                                            if let Some(ms) = r.elapsed_ms {
                                                "{ms} ms"
                                            } else {
                                                "—"
                                            }
                                        }
                                    }

                                    if is_open {
                                        div { class: "runner-result-details",
                                            div { class: "runner-detail-top",
                                                div { class: "runner-detail-url", "{r.url}" }
                                                if let Some(open_cb) = on_open_request {
                                                    button {
                                                        class: "btn btn-sm btn-ghost",
                                                        r#type: "button",
                                                        onclick: move |_| open_cb.call(rid),
                                                        "↗ Open in Workbench"
                                                    }
                                                }
                                            }
                                            if let Some(err) = &r.error {
                                                div { class: "runner-detail-error", "{err}" }
                                            }
                                            if !r.test_results.is_empty() {
                                                div { class: "runner-assertion-list",
                                                    for (ti, t) in r.test_results.iter().enumerate() {
                                                        div {
                                                            key: "t-{idx}-{ti}",
                                                            class: if t.passed { "runner-assertion-row ok" } else { "runner-assertion-row fail" },
                                                            span { class: "runner-assertion-icon", if t.passed { "✓" } else { "✗" } }
                                                            span { class: "runner-assertion-name", "{t.name}" }
                                                            if !t.passed && !t.message.is_empty() {
                                                                span { class: "runner-assertion-msg", "— {t.message}" }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            if !r.response_headers.is_empty() {
                                                details { class: "runner-preview-details",
                                                    summary { "Response Headers ({r.response_headers.len()})" }
                                                    div { class: "runner-headers-kv",
                                                        for (hi, (hk, hv)) in r.response_headers.iter().enumerate() {
                                                            div { key: "hdr-{idx}-{hi}", class: "runner-hdr-row",
                                                                span { class: "runner-hdr-k", "{hk}" }
                                                                span { class: "runner-hdr-v", "{hv}" }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            if !r.response_preview.is_empty() {
                                                details { class: "runner-preview-details",
                                                    summary { "Response Preview" }
                                                    pre { class: "runner-preview-body", "{r.response_preview}" }
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

            if !report.console.is_empty() {
                details { class: "runner-console-details",
                    summary { "Console Output ({report.console.len()} lines)" }
                    pre { class: "response-body runner-console", "{report.console.join(\"\\n\")}" }
                }
            }
        }
    }
}
