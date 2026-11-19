use std::sync::Arc;

use dioxus::prelude::*;
use uuid::Uuid;

use crate::core::{diff_bodies, DefaultResponseTab, DiffLine, ResponseView, SavedExample};
use super::json_highlight::{
    highlight_json_fragment, json_paint_range, render_json_highlight_html,
    render_json_highlight_html_with_fold, render_json_value_html, should_highlight_json,
    JSON_PAINT_CAP,
};
use super::loading::SkeletonBars;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RespTab {
    Pretty,
    Raw,
    Preview,
    Headers,
    Cookies,
    Tests,
    Diff,
    Visualize,
    Timeline,
}

#[derive(Clone, PartialEq, Eq)]
enum DiffBaseline {
    PreviousRun,
    Example(Uuid),
}

fn resp_tab_from_default(tab: DefaultResponseTab) -> RespTab {
    match tab {
        DefaultResponseTab::Pretty => RespTab::Pretty,
        DefaultResponseTab::Raw => RespTab::Raw,
        DefaultResponseTab::Headers => RespTab::Headers,
    }
}

#[component]
pub fn ResponseViewer(
    response: Option<Arc<ResponseView>>,
    #[props(default)] prev_response: Option<Arc<ResponseView>>,
    #[props(default)] examples: Vec<SavedExample>,
    sending: bool,
    #[props(default)] default_tab: DefaultResponseTab,
    on_save_example: EventHandler<()>,
    #[props(default)] on_auto_assert: EventHandler<()>,
    #[props(default)] on_set_variable: EventHandler<(String, String)>,
) -> Element {
    let mut tab = use_signal(|| resp_tab_from_default(default_tab));
    let mut search_query = use_signal(String::new);
    let mut current_match_idx = use_signal(|| 0usize);
    let mut search_open = use_signal(|| false);
    let mut copied_body = use_signal(|| false);
    let mut copied_extract = use_signal(|| false);
    let mut bound_feedback = use_signal(|| None::<String>);
    let render_mode = use_signal(|| true);
    let mut diff_baseline = use_signal(|| DiffBaseline::PreviousRun);
    let mut json_filter = use_signal(String::new);
    let mut filter_open = use_signal(|| false);
    let mut all_collapsed = use_signal(|| false);
    let mut enable_folding = use_signal(|| true);
    let paint_off = use_signal(|| (0usize, 0usize));

    rsx! {
        section { class: "response-pane",
            if sending {
                div { class: "response-sending",
                    super::icons::ActionIcon { name: "bolt", size: 28, class: "bolt-flight-pulse".to_string(), style: "color: var(--spot-green-bright); margin-bottom: 8px;".to_string() }
                    p { class: "hint", style: "font-weight: 600; font-size: 13px; color: var(--text); margin-bottom: 12px;", "Sending request…" }
                    SkeletonBars { n: 4 }
                }
            } else if let Some(resp) = response.as_ref() {
                div { class: "response-meta",
                    if let Some(err) = &resp.error {
                        span { class: "status-chip status-err",
                            span { class: "status-tag", "Error" }
                            "{err}"
                        }
                    } else {
                        {
                            let (cls, tag) = match resp.status {
                                200..=299 => ("status-chip status-2xx", "OK"),
                                300..=399 => ("status-chip status-3xx", "REDIR"),
                                400..=499 => ("status-chip status-4xx", "CLIENT ERR"),
                                500..=599 => ("status-chip status-5xx", "SERVER ERR"),
                                _ => ("status-chip status-err", "ERR"),
                            };
                            rsx! {
                                span { class: "{cls}",
                                    span { class: "status-tag", "{tag}" }
                                    "{resp.status} {resp.status_text}"
                                }
                            }
                        }
                        span { class: "meta-pair",
                            span { class: "meta-label", "Time" }
                            span { class: "meta-value", "{resp.elapsed_ms} ms" }
                        }
                        span { class: "meta-pair",
                            span { class: "meta-label", "Size" }
                            span { class: "meta-value", "{resp.size_bytes()} B" }
                        }
                        button {
                            class: "btn btn-ghost",
                            title: "Copy response body to clipboard",
                            onclick: {
                                let body = resp.body.clone();
                                move |_| {
                                    let script = format!(
                                        "navigator.clipboard.writeText({});",
                                        serde_json::to_string(&body).unwrap_or_else(|_| "\"\"".into())
                                    );
                                    spawn(async move {
                                        let _ = document::eval(&script).await;
                                        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                                        copied_body.set(false);
                                    });
                                    copied_body.set(true);
                                }
                            },
                            if copied_body() {
                                super::icons::ActionIcon { name: "check", size: 15 }
                                span { "Copied" }
                            } else {
                                super::icons::ActionIcon { name: "copy", size: 15 }
                                span { "Copy" }
                            }
                        }
                        button {
                            class: "btn btn-ghost",
                            title: "Save response body to disk",
                            onclick: {
                                let body = resp.body.clone();
                                let status = resp.status;
                                move |_| {
                                    let filename = format!("response-{status}.json");
                                    let script = format!(
                                        r#"(() => {{
                                            const blob = new Blob([{}], {{ type: 'application/json' }});
                                            const url = URL.createObjectURL(blob);
                                            const a = document.createElement('a');
                                            a.href = url;
                                            a.download = {};
                                            document.body.appendChild(a);
                                            a.click();
                                            setTimeout(() => {{
                                                document.body.removeChild(a);
                                                URL.revokeObjectURL(url);
                                            }}, 100);
                                        }})()"#,
                                        serde_json::to_string(&body).unwrap_or_else(|_| "\"\"".into()),
                                        serde_json::to_string(&filename).unwrap_or_else(|_| "\"response.json\"".into())
                                    );
                                    spawn(async move {
                                        let _ = document::eval(&script).await;
                                    });
                                }
                            },
                            super::icons::ActionIcon { name: "save", size: 15 }
                            span { "Save" }
                        }
                        button {
                            class: if search_open() { "btn btn-ghost active" } else { "btn btn-ghost" },
                            title: "Find in response (⌘F)",
                            onclick: move |_| search_open.set(!search_open()),
                            super::icons::ActionIcon { name: "search", size: 15 }
                            span { "Find" }
                        }
                        if tab() == RespTab::Pretty && resp.error.is_none() {
                            button {
                                class: if filter_open() || !json_filter().is_empty() { "btn btn-ghost active" } else { "btn btn-ghost" },
                                title: "Filter response using JSONPath (e.g. $.items[0])",
                                onclick: move |_| filter_open.set(!filter_open()),
                                super::icons::ActionIcon { name: "filter", size: 15 }
                                span { "Filter" }
                                if !json_filter().is_empty() {
                                    span { class: "meta-active-badge", "1" }
                                }
                            }
                            if enable_folding() {
                                button {
                                    class: "btn btn-ghost",
                                    title: if all_collapsed() { "Expand all JSON blocks" } else { "Collapse all JSON blocks" },
                                    onclick: move |_| {
                                        let next = !all_collapsed();
                                        all_collapsed.set(next);
                                        spawn(async move {
                                            let script = format!(
                                                "document.querySelectorAll('.response-body-json details.json-fold').forEach(d => d.open = {});",
                                                !next
                                            );
                                            let _ = document::eval(&script).await;
                                        });
                                    },
                                    if all_collapsed() {
                                        super::icons::ActionIcon { name: "expand", size: 15 }
                                        span { "Expand" }
                                    } else {
                                        super::icons::ActionIcon { name: "fold", size: 15 }
                                        span { "Collapse" }
                                    }
                                }
                            }
                            button {
                                class: if enable_folding() { "btn btn-ghost" } else { "btn btn-ghost active" },
                                title: if enable_folding() { "Disable tree folding (view plain syntax-highlighted JSON)" } else { "Enable interactive tree folding" },
                                onclick: move |_| enable_folding.set(!enable_folding()),
                                super::icons::ActionIcon { name: "format", size: 15 }
                                span { if enable_folding() { "Fold: On" } else { "Fold: Off" } }
                            }
                        }
                        button {
                            class: "btn btn-ghost",
                            title: "Generate Tests from this response: status, content-type, JSON fields, time ceiling. Review them in the Tests tab.",
                            onclick: move |_| on_auto_assert.call(()),
                            super::icons::ActionIcon { name: "check", size: 15 }
                            span { "Auto-assert" }
                        }
                        button {
                            class: "btn btn-ghost",
                            title: "Save response as mock route example (served by local Mock Server)",
                            onclick: move |_| on_save_example.call(()),
                            super::icons::ActionIcon { name: "bolt", size: 15 }
                            span { "Save to Mock" }
                        }
                    }
                }

                if search_open() {
                    {
                        let count = count_matches(&resp.body, &search_query());
                        rsx! {
                            div { class: "response-search-bar",
                                input {
                                    class: "response-search-input",
                                    placeholder: "Search response body…",
                                    value: "{search_query()}",
                                    oninput: move |evt: Event<FormData>| {
                                        search_query.set(evt.value());
                                        current_match_idx.set(0);
                                    },
                                    onkeydown: move |evt: KeyboardEvent| {
                                        if evt.key() == Key::Enter && count > 0 {
                                            if evt.modifiers().shift() {
                                                current_match_idx.set((current_match_idx() + count - 1) % count);
                                            } else {
                                                current_match_idx.set((current_match_idx() + 1) % count);
                                            }
                                        }
                                    },
                                }
                                if !search_query().is_empty() {
                                    if count == 0 {
                                        span { class: "hint", "No matches" }
                                    } else {
                                        span { class: "hint", "{current_match_idx() + 1} of {count}" }
                                        button {
                                            class: "btn btn-ghost",
                                            style: "padding: 0 4px;",
                                            title: "Previous match (Shift+Enter)",
                                            onclick: move |_| {
                                                current_match_idx.set((current_match_idx() + count - 1) % count);
                                            },
                                            "▲"
                                        }
                                        button {
                                            class: "btn btn-ghost",
                                            style: "padding: 0 4px;",
                                            title: "Next match (Enter)",
                                            onclick: move |_| {
                                                current_match_idx.set((current_match_idx() + 1) % count);
                                            },
                                            "▼"
                                        }
                                    }
                                }
                                button {
                                    class: "btn btn-ghost",
                                    style: "padding: 0 6px;",
                                    title: "Close search",
                                    onclick: move |_| {
                                        search_query.set(String::new());
                                        current_match_idx.set(0);
                                        search_open.set(false);
                                    },
                                    "✕"
                                }
                            }
                        }
                    }
                }

                div { class: "builder-tabs",
                    for (label, t) in [
                        ("Pretty", RespTab::Pretty),
                        ("Raw", RespTab::Raw),
                        ("Preview", RespTab::Preview),
                        ("Headers", RespTab::Headers),
                        ("Cookies", RespTab::Cookies),
                        ("Test Results", RespTab::Tests),
                        ("Diff", RespTab::Diff),
                        ("Visualize", RespTab::Visualize),
                        ("Timeline", RespTab::Timeline),
                    ] {
                        button {
                            class: if tab() == t { "builder-tab active" } else { "builder-tab" },
                            onclick: move |_| tab.set(t),
                            "{label}"
                        }
                    }
                }

                match tab() {
                    RespTab::Pretty => {
                        let paint_key = resp.body.len();
                        let off = if paint_off().0 == paint_key { paint_off().1 } else { 0 };
                        let (pretty, start, end, total_len, parsed_json) =
                            pretty_body(&resp.body, off);
                        let paged = start > 0 || end < total_len;
                        let is_json = parsed_json.is_some()
                            || (!paged && should_highlight_json(&resp.body, &resp.headers));

                        let filter_text = json_filter().trim().to_string();
                        let filter_result = if let (Some(root_val), false) = (&parsed_json, filter_text.is_empty()) {
                            Some(crate::core::query_json(root_val, &filter_text))
                        } else {
                            None
                        };

                        let root_pills = parsed_json.as_ref().map(|v| crate::core::root_keys(v, 6)).unwrap_or_default();
                        let scalar_root_pairs: Vec<(String, String)> = match parsed_json.as_ref() {
                            Some(serde_json::Value::Object(map)) => map
                                .iter()
                                .filter(|(_, v)| v.is_string() || v.is_number() || v.is_boolean())
                                .take(5)
                                .map(|(k, v)| (k.clone(), crate::core::scalar_or_compact_json(v)))
                                .collect(),
                            _ => Vec::new(),
                        };

                        rsx! {
                            if let Some(msg) = bound_feedback() {
                                div {
                                    class: "jsonpath-status status-ok",
                                    style: "display: flex; align-items: center; justify-content: space-between; padding: 4px 10px;",
                                    span { "✓ Bound {{{{{msg}}}}} to variables" }
                                }
                            }
                            if is_json && (filter_open() || !json_filter().is_empty()) {
                                div { class: "response-jsonpath-bar",
                                    span { class: "jsonpath-label", "Path" }
                                    div { class: "jsonpath-input-wrap",
                                        input {
                                            class: "jsonpath-input",
                                            placeholder: "Dot-path / JSONPath (e.g. data.users.0.id, items.length, users.*.email)…",
                                            value: "{json_filter()}",
                                            oninput: move |evt: Event<FormData>| json_filter.set(evt.value()),
                                        }
                                        if !json_filter().is_empty() {
                                            button {
                                                class: "jsonpath-clear-btn",
                                                title: "Clear filter",
                                                onclick: move |_| json_filter.set(String::new()),
                                                "✕"
                                            }
                                        }
                                    }
                                    if !root_pills.is_empty() && json_filter().is_empty() {
                                        div { class: "jsonpath-pills",
                                            for (pi, key) in root_pills.into_iter().enumerate() {
                                                {
                                                    let k = key.clone();
                                                    rsx! {
                                                        button {
                                                            key: "{k}-{pi}",
                                                            class: "jsonpath-pill",
                                                            title: "Extract .{k}",
                                                            onclick: move |_| json_filter.set(k.clone()),
                                                            ".{k}"
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            } else if is_json && !scalar_root_pairs.is_empty() {
                                div {
                                    class: "response-jsonpath-bar",
                                    style: "padding: 4px 10px; gap: 6px; flex-wrap: wrap;",
                                    span { class: "jsonpath-label", "Quick → {{var}}" }
                                    div { class: "jsonpath-pills",
                                        for (si, (k, v)) in scalar_root_pairs.into_iter().enumerate() {
                                            {
                                                let key_for_bind = k.clone();
                                                let val_for_bind = v.clone();
                                                let preview_v: String = v.chars().take(24).collect();
                                                rsx! {
                                                    button {
                                                        key: "bind-{key_for_bind}-{si}",
                                                        class: "jsonpath-pill",
                                                        title: "Bind {{{{{key_for_bind}}}}} = {val_for_bind}",
                                                        onclick: move |_| {
                                                            on_set_variable.call((key_for_bind.clone(), val_for_bind.clone()));
                                                            bound_feedback.set(Some(key_for_bind.clone()));
                                                            spawn(async move {
                                                                tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
                                                                bound_feedback.set(None);
                                                            });
                                                        },
                                                        "+ {{{{{k}}}}} ({preview_v})"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            if let Some(res) = &filter_result {
                                match res {
                                    Ok(val) => {
                                        let item_count = match val {
                                            serde_json::Value::Array(a) => format!("{} items", a.len()),
                                            serde_json::Value::Object(m) => format!("{} keys", m.len()),
                                            _ => "1 value".to_string(),
                                        };
                                        let extracted_raw = crate::core::scalar_or_compact_json(val);
                                        let suggested_var: String = filter_text
                                            .rsplit(['.', '['])
                                            .map(|s| s.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_'))
                                            .find(|s| !s.is_empty() && s.chars().any(|c| c.is_ascii_alphabetic()))
                                            .unwrap_or("extracted")
                                            .to_string();
                                        let pretty_val = serde_json::to_string_pretty(val).unwrap_or_default();
                                        let html = render_json_highlight_html_with_fold(&pretty_val, enable_folding());
                                        let copy_val = extracted_raw.clone();
                                        let bind_val = extracted_raw;
                                        let bind_var = suggested_var.clone();
                                        rsx! {
                                            div {
                                                class: "jsonpath-status status-ok",
                                                style: "display: flex; align-items: center; justify-content: space-between; gap: 8px; flex-wrap: wrap;",
                                                span { "✓ Matched '{filter_text}' ({item_count})" }
                                                div { style: "display: flex; align-items: center; gap: 6px;",
                                                    button {
                                                        class: "btn btn-sm btn-ghost",
                                                        r#type: "button",
                                                        title: "Copy extracted value to clipboard",
                                                        onclick: move |_| {
                                                            let script = format!(
                                                                "navigator.clipboard.writeText({});",
                                                                serde_json::to_string(&copy_val).unwrap_or_else(|_| "\"\"".into())
                                                            );
                                                            spawn(async move {
                                                                let _ = document::eval(&script).await;
                                                                tokio::time::sleep(std::time::Duration::from_millis(1600)).await;
                                                                copied_extract.set(false);
                                                            });
                                                            copied_extract.set(true);
                                                        },
                                                        if copied_extract() {
                                                            "✓ Copied"
                                                        } else {
                                                            "📋 Copy Value"
                                                        }
                                                    }
                                                    button {
                                                        class: "btn btn-sm btn-ghost",
                                                        r#type: "button",
                                                        title: "Save extracted value to {{{{{suggested_var}}}}} in active Environment / Variables",
                                                        onclick: move |_| {
                                                            on_set_variable.call((bind_var.clone(), bind_val.clone()));
                                                            bound_feedback.set(Some(bind_var.clone()));
                                                            spawn(async move {
                                                                tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
                                                                bound_feedback.set(None);
                                                            });
                                                        },
                                                        "→ {{{{{suggested_var}}}}}"
                                                    }
                                                }
                                            }
                                            pre {
                                                class: "response-body response-body-json",
                                                dangerous_inner_html: "{html}",
                                            }
                                        }
                                    }
                                    Err(err) => {
                                        let html = render_json_highlight_html_with_fold(&pretty, enable_folding());
                                        rsx! {
                                            div { class: "jsonpath-status status-err",
                                                span { "⚠ {err}" }
                                            }
                                            pre {
                                                class: "response-body response-body-json",
                                                dangerous_inner_html: "{html}",
                                            }
                                        }
                                    }
                                }
                            } else {
                                {
                                    let query = search_query();
                                    if !query.is_empty() {
                                        let highlighted = highlight_search_matches(&pretty, &query);
                                        rsx! {
                                            {paint_controls(resp.body.len(), total_len, start, end, paint_off)}
                                            pre {
                                                class: "response-body",
                                                dangerous_inner_html: "{highlighted}",
                                            }
                                        }
                                    } else if paged {
                                        let html = highlight_json_fragment(&pretty);
                                        rsx! {
                                            {paint_controls(resp.body.len(), total_len, start, end, paint_off)}
                                            pre {
                                                class: "response-body response-body-json",
                                                dangerous_inner_html: "{html}",
                                            }
                                        }
                                    } else if let Some(ref val) = parsed_json {
                                        let html = render_json_value_html(val, enable_folding());
                                        rsx! {
                                            pre {
                                                class: "response-body response-body-json",
                                                dangerous_inner_html: "{html}",
                                            }
                                        }
                                    } else if is_json {
                                        let html = render_json_highlight_html_with_fold(&pretty, enable_folding());
                                        rsx! {
                                            pre {
                                                class: "response-body response-body-json",
                                                dangerous_inner_html: "{html}",
                                            }
                                        }
                                    } else {
                                        rsx! {
                                            pre { class: "response-body", "{pretty}" }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    RespTab::Raw => {
                        let query = search_query();
                        let paint_key = resp.body.len();
                        let off = if paint_off().0 == paint_key { paint_off().1 } else { 0 };
                        let (start, end) = json_paint_range(&resp.body, off);
                        let slice = &resp.body[start..end];
                        let paged = start > 0 || end < paint_key;
                        if !query.is_empty() {
                            let highlighted = highlight_search_matches(slice, &query);
                            rsx! {
                                if paged {
                                    {paint_controls(paint_key, paint_key, start, end, paint_off)}
                                }
                                pre {
                                    class: "response-body",
                                    dangerous_inner_html: "{highlighted}",
                                }
                            }
                        } else {
                            rsx! {
                                if paged {
                                    {paint_controls(paint_key, paint_key, start, end, paint_off)}
                                }
                                pre { class: "response-body",
                                    if resp.body.is_empty() && resp.error.is_none() {
                                        "(empty body)"
                                    } else {
                                        "{slice}"
                                    }
                                }
                            }
                        }
                    },
                    RespTab::Preview => rsx! {
                        if looks_like_html(&resp.body) {
                            div {
                                p { class: "hint", "HTML preview (source):" }
                                pre { class: "response-body", "{resp.body}" }
                            }
                        } else {
                            p { class: "hint", "Preview is for HTML responses. Showing Pretty instead." }
                            {
                                let paint_key = resp.body.len();
                                let off = if paint_off().0 == paint_key { paint_off().1 } else { 0 };
                                let (pretty, start, end, total_len, parsed_preview) = pretty_body(&resp.body, off);
                                let full = start == 0 && end == total_len;
                                if full && parsed_preview.is_some() {
                                    let html = render_json_value_html(parsed_preview.as_ref().unwrap(), true);
                                    rsx! {
                                        pre {
                                            class: "response-body response-body-json",
                                            dangerous_inner_html: "{html}",
                                        }
                                    }
                                } else if full && should_highlight_json(&resp.body, &resp.headers) {
                                    let html = render_json_highlight_html(&pretty);
                                    rsx! {
                                        pre {
                                            class: "response-body response-body-json",
                                            dangerous_inner_html: "{html}",
                                        }
                                    }
                                } else {
                                    rsx! {
                                        {paint_controls(resp.body.len(), total_len, start, end, paint_off)}
                                        pre { class: "response-body", "{pretty}" }
                                    }
                                }
                            }
                        }
                    },
                    RespTab::Headers => rsx! {
                        div { class: "response-body",
                            if let Some(msg) = bound_feedback() {
                                div {
                                    class: "jsonpath-status status-ok",
                                    style: "margin-bottom: 8px; padding: 4px 10px;",
                                    "✓ Bound {{{{{msg}}}}} to variables"
                                }
                            }
                            if resp.headers.is_empty() {
                                "(no headers)"
                            } else {
                                div {
                                    style: "display: flex; flex-direction: column; gap: 4px;",
                                    for (hi, (k, v)) in resp.headers.iter().enumerate() {
                                        {
                                            let hk = k.clone();
                                            let hv = v.clone();
                                            let var_key: String = k
                                                .chars()
                                                .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
                                                .collect::<String>()
                                                .trim_matches('_')
                                                .to_string();
                                            let var_key = if var_key.is_empty() { "header_val".to_string() } else { var_key };
                                            let bind_k = var_key.clone();
                                            let bind_v = hv.clone();
                                            let copy_v = hv.clone();
                                            rsx! {
                                                div {
                                                    key: "resp-hdr-{hi}-{hk}",
                                                    style: "display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 4px 8px; border-radius: 4px; background: rgba(255,255,255,0.025); border: 1px solid rgba(255,255,255,0.05);",
                                                    div {
                                                        style: "min-width: 0; flex: 1; font-family: var(--mono); font-size: 11.5px; word-break: break-all;",
                                                        span { style: "color: #93c5fd; font-weight: 600;", "{hk}: " }
                                                        span { style: "color: var(--text);", "{hv}" }
                                                    }
                                                    div {
                                                        style: "display: flex; align-items: center; gap: 4px; flex-shrink: 0;",
                                                        button {
                                                            class: "btn btn-sm btn-ghost",
                                                            r#type: "button",
                                                            title: "Copy header value",
                                                            onclick: move |_| {
                                                                let script = format!(
                                                                    "navigator.clipboard.writeText({});",
                                                                    serde_json::to_string(&copy_v).unwrap_or_else(|_| "\"\"".into())
                                                                );
                                                                spawn(async move {
                                                                    let _ = document::eval(&script).await;
                                                                });
                                                            },
                                                            "Copy"
                                                        }
                                                        button {
                                                            class: "btn btn-sm btn-ghost",
                                                            r#type: "button",
                                                            title: "Bind header value to {{{{{var_key}}}}}",
                                                            onclick: move |_| {
                                                                on_set_variable.call((bind_k.clone(), bind_v.clone()));
                                                                bound_feedback.set(Some(bind_k.clone()));
                                                                spawn(async move {
                                                                    tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
                                                                    bound_feedback.set(None);
                                                                });
                                                            },
                                                            "→ {{{{{var_key}}}}}"
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    RespTab::Cookies => rsx! {
                        pre { class: "response-body",
                            {
                                let cookies: Vec<_> = resp
                                    .headers
                                    .iter()
                                    .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
                                    .map(|(_, v)| v.clone())
                                    .collect();
                                if cookies.is_empty() {
                                    rsx! { "(no Set-Cookie — use Cookies manager in toolbar)" }
                                } else {
                                    let text = cookies.join("\n");
                                    rsx! { "{text}" }
                                }
                            }
                        }
                    },
                    RespTab::Tests => rsx! {
                        div { class: "test-results",
                            if resp.test_results.is_empty() {
                                p { class: "hint",
                                    "No test results. Add blitz.test(\"name\", function() {{ … }}) in Scripts."
                                }
                            } else {
                                for (i, t) in resp.test_results.iter().enumerate() {
                                    div {
                                        key: "{t.name}-{i}",
                                        class: if t.passed { "test-pass" } else { "test-fail" },
                                        span { if t.passed { "PASS" } else { "FAIL" } }
                                        " {t.name} — {t.message}"
                                    }
                                }
                            }
                            if !resp.pipeline_log.is_empty() {
                                p { class: "hint", "Pipeline: {resp.pipeline_log.join(\" → \")}" }
                            }
                        }
                    },
                    RespTab::Diff => {
                        let cur_base = diff_baseline();
                        let select_val = match cur_base {
                            DiffBaseline::PreviousRun => "prev".to_string(),
                            DiffBaseline::Example(id) => format!("ex:{id}"),
                        };

                        let baseline_data: Option<(String, u16, String, Option<u128>, String)> = match cur_base {
                            DiffBaseline::PreviousRun => prev_response.as_ref().map(|p| {
                                (
                                    "Previous run".to_string(),
                                    p.status,
                                    p.status_text.clone(),
                                    Some(p.elapsed_ms),
                                    p.body.clone(),
                                )
                            }),
                            DiffBaseline::Example(id) => examples
                                .iter()
                                .find(|e| e.id == id)
                                .or_else(|| examples.first())
                                .map(|ex| {
                                    (
                                        format!("Example: {}", ex.name),
                                        ex.status,
                                        ex.status_text.clone(),
                                        None,
                                        ex.body.clone(),
                                    )
                                }),
                        };

                        let computed_diff = baseline_data.as_ref().map(|(_, _, _, _, old_body)| {
                            diff_bodies(old_body, &resp.body)
                        });

                        rsx! {
                            div { class: "response-diff-pane",
                                div { class: "response-diff-toolbar",
                                    span { class: "hint", "Compare against:" }
                                    select {
                                        class: "response-diff-select",
                                        value: "{select_val}",
                                        onchange: move |evt: Event<FormData>| {
                                            let val = evt.value();
                                            if val == "prev" {
                                                diff_baseline.set(DiffBaseline::PreviousRun);
                                            } else if let Some(uuid_str) = val.strip_prefix("ex:") {
                                                if let Ok(id) = Uuid::parse_str(uuid_str) {
                                                    diff_baseline.set(DiffBaseline::Example(id));
                                                }
                                            }
                                        },
                                        option {
                                            value: "prev",
                                            selected: matches!(diff_baseline(), DiffBaseline::PreviousRun),
                                            if prev_response.is_some() {
                                                "Previous run"
                                            } else {
                                                "Previous run (none yet)"
                                            }
                                        }
                                        for (ei, ex) in examples.iter().enumerate() {
                                            option {
                                                key: "{ex.id}-{ei}",
                                                value: "ex:{ex.id}",
                                                selected: diff_baseline() == DiffBaseline::Example(ex.id),
                                                "Saved Example: {ex.name} ({ex.status})"
                                            }
                                        }
                                    }
                                    if let (Some((_, old_status, _, old_ms, _)), Some((_, ref stats))) = (&baseline_data, &computed_diff) {
                                        {
                                            let status_delta = if *old_status == resp.status {
                                                format!("Status {}", resp.status)
                                            } else {
                                                format!("Status {} → {}", old_status, resp.status)
                                            };
                                            let time_delta = old_ms.map(|old_t| {
                                                let diff_ms = resp.elapsed_ms as i128 - old_t as i128;
                                                if diff_ms >= 0 {
                                                    format!("+{diff_ms} ms")
                                                } else {
                                                    format!("{diff_ms} ms")
                                                }
                                            });
                                            rsx! {
                                                div { class: "response-diff-summary",
                                                    span { class: "diff-badge diff-badge-neutral", "{status_delta}" }
                                                    if let Some(td) = time_delta {
                                                        span { class: "diff-badge diff-badge-neutral", "{td}" }
                                                    }
                                                    span { class: "diff-badge diff-badge-add", "+{stats.added}" }
                                                    span { class: "diff-badge diff-badge-del", "-{stats.removed}" }
                                                }
                                            }
                                        }
                                    }
                                }

                                if let (Some((label, _, _, _, _)), Some((lines, stats))) = (baseline_data, computed_diff) {
                                    {
                                        if stats.added == 0 && stats.removed == 0 {
                                            rsx! {
                                                p { class: "hint", "No differences between {label} and current response (JSON keys normalized)." }
                                            }
                                        } else {
                                            let total_lines = lines.len();
                                            let max_diff_lines = 2_000usize;
                                            rsx! {
                                                div { class: "response-diff-lines",
                                                    for (idx, line) in lines.into_iter().take(max_diff_lines).enumerate() {
                                                        match line {
                                                            DiffLine::Same(text) => rsx! {
                                                                div { key: "{idx}", class: "diff-line diff-line-same",
                                                                    span { class: "diff-sign", " " }
                                                                    span { class: "diff-text", "{text}" }
                                                                }
                                                            },
                                                            DiffLine::Add(text) => rsx! {
                                                                div { key: "{idx}", class: "diff-line diff-line-add",
                                                                    span { class: "diff-sign", "+" }
                                                                    span { class: "diff-text", "{text}" }
                                                                }
                                                            },
                                                            DiffLine::Del(text) => rsx! {
                                                                div { key: "{idx}", class: "diff-line diff-line-del",
                                                                    span { class: "diff-sign", "-" }
                                                                    span { class: "diff-text", "{text}" }
                                                                }
                                                            },
                                                        }
                                                    }
                                                    if total_lines > max_diff_lines {
                                                        div { class: "hint", style: "padding: 8px;", "… showing first {max_diff_lines} of {total_lines} diff lines" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    p { class: "hint",
                                        "Send this request a second time or click 'Save to Mock' to create a baseline for comparison."
                                    }
                                }
                            }
                        }
                    },
                    RespTab::Visualize => {
                        rsx! {
                            div { class: "visualizer-pane",
                                if resp.visualizer_html.is_empty() {
                                    p { class: "hint",
                                        "No visualizer output. In Tests: blitz.visualizer.set(\"<h1>{{{{title}}}}</h1>\", {{title:\"Hello\"}}) — or return HTML body."
                                    }
                                } else {
                                    div { class: "visualizer-toolbar", style: "display: flex; gap: 8px; margin-bottom: 8px; align-items: center;",
                                        button {
                                            class: if render_mode() { "btn-ghost active" } else { "btn-ghost" },
                                            style: "padding: 2px 8px; font-size: 11px; border-radius: 4px; border: 1px solid rgba(255,255,255,0.1);",
                                            onclick: {
                                                let mut rm = render_mode;
                                                move |_| rm.set(true)
                                            },
                                            "Rendered View"
                                        }
                                        button {
                                            class: if !render_mode() { "btn-ghost active" } else { "btn-ghost" },
                                            style: "padding: 2px 8px; font-size: 11px; border-radius: 4px; border: 1px solid rgba(255,255,255,0.1);",
                                            onclick: {
                                                let mut rm = render_mode;
                                                move |_| rm.set(false)
                                            },
                                            "Source HTML"
                                        }
                                    }
                                    if render_mode() {
                                        iframe {
                                            class: "visualizer-rendered",
                                            "sandbox": "",
                                            srcdoc: "{resp.visualizer_html}",
                                            style: "width: 100%; min-height: 360px; border: none; border-radius: 6px; background: #fff; color: #111;",
                                        }
                                    } else {
                                        pre { class: "response-body", "{resp.visualizer_html}" }
                                    }
                                }
                            }
                        }
                    },
                    RespTab::Timeline => rsx! {
                        div { class: "timeline-pane",
                            p { class: "hint", "Timing phases approximated from total / TTFB (reqwest blocking)." }
                            {
                                let t = &resp.timing;
                                let total = t.total_ms.max(1);
                                rsx! {
                                    div { class: "timeline-bars",
                                        TimelineBar { label: "DNS", ms: t.dns_ms, total: total }
                                        TimelineBar { label: "Connect", ms: t.connect_ms, total: total }
                                        TimelineBar { label: "TLS", ms: t.tls_ms, total: total }
                                        TimelineBar { label: "TTFB", ms: t.ttfb_ms, total: total }
                                        TimelineBar { label: "Download", ms: t.download_ms, total: total }
                                        TimelineBar { label: "Total", ms: t.total_ms, total: total }
                                    }
                                }
                            }
                        }
                    },
                }
            } else {
                div { class: "response-empty-hero",
                    div { class: "response-empty-icon-wrap",
                        img {
                            src: crate::brand::LOGO_SRC.as_str(),
                            alt: "Blitz",
                            class: "response-empty-logo",
                        }
                    }
                    h4 { class: "response-empty-title", "Ready to Send" }
                    p { class: "response-empty-desc",
                        "Enter a URL above and click "
                        strong { "Send" }
                        " or press "
                        kbd { "⌘↵" }
                        " to inspect response headers, body, and test results."
                    }
                    div { class: "response-empty-shortcuts",
                        div { class: "empty-shortcut-chip",
                            kbd { "⌘↵" }
                            span { "Send Request" }
                        }
                        div { class: "empty-shortcut-chip",
                            kbd { "⌘S" }
                            span { "Save Draft" }
                        }
                        div { class: "empty-shortcut-chip",
                            kbd { "⌘K" }
                            span { "Command Palette" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TimelineBar(label: &'static str, ms: u128, total: u128) -> Element {
    let pct = ((ms as f64 / total as f64) * 100.0).clamp(2.0, 100.0);
    rsx! {
        div { class: "timeline-row",
            span { class: "timeline-label", "{label}" }
            div { class: "timeline-track",
                div {
                    class: "timeline-fill",
                    style: "width: {pct}%",
                }
            }
            span { class: "timeline-ms", "{ms} ms" }
        }
    }
}

fn pretty_body(body: &str, offset: usize) -> (String, usize, usize, usize, Option<serde_json::Value>) {
    let trimmed = body.trim().trim_start_matches('\u{feff}').trim();
    if !trimmed.is_empty() && trimmed.len() <= 2 * 1024 * 1024 {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Ok(beautified) = serde_json::to_string_pretty(&v) {
                let total_len = beautified.len();
                let (start, end) = json_paint_range(&beautified, offset);
                return (
                    beautified[start..end].to_string(),
                    start,
                    end,
                    total_len,
                    Some(v),
                );
            }
        }
    }
    let total_len = body.len();
    let (start, end) = json_paint_range(body, offset);
    (body[start..end].to_string(), start, end, total_len, None)
}

fn paint_controls(
    paint_key: usize,
    len: usize,
    start: usize,
    end: usize,
    mut paint_off: Signal<(usize, usize)>,
) -> Element {
    if start == 0 && end == len {
        return rsx! {};
    }
    let earlier = start.saturating_sub(JSON_PAINT_CAP);
    let later = end;
    rsx! {
        div { class: "json-large-warning",
            button {
                disabled: start == 0,
                onclick: move |_| paint_off.set((paint_key, earlier)),
                "Earlier"
            }
            " bytes {start}–{end} of {len}. Stored body is complete. "
            button {
                disabled: end == len,
                onclick: move |_| paint_off.set((paint_key, later)),
                "Later"
            }
        }
    }
}

fn looks_like_html(body: &str) -> bool {
    let t = body.trim_start().to_ascii_lowercase();
    t.starts_with("<!doctype html") || t.starts_with("<html") || t.contains("<body")
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn count_matches(text: &str, query: &str) -> usize {
    if query.is_empty() {
        return 0;
    }
    match regex::RegexBuilder::new(&regex::escape(query))
        .case_insensitive(true)
        .build()
    {
        Ok(re) => re.find_iter(text).count(),
        Err(_) => 0,
    }
}

fn highlight_search_matches(text: &str, query: &str) -> String {
    if query.is_empty() {
        return html_escape(text);
    }
    let Ok(re) = regex::RegexBuilder::new(&regex::escape(query))
        .case_insensitive(true)
        .build()
    else {
        return html_escape(text);
    };
    let mut result = String::with_capacity(text.len() + 64);
    let mut last_idx = 0;

    for m in re.find_iter(text) {
        result.push_str(&html_escape(&text[last_idx..m.start()]));
        result.push_str("<mark class=\"search-match\">");
        result.push_str(&html_escape(m.as_str()));
        result.push_str("</mark>");
        last_idx = m.end();
    }
    result.push_str(&html_escape(&text[last_idx..]));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_search_matches_handles_unicode_case_folding() {
        // 'İ' (2 bytes) lowercases to 'i\u{307}' (3 bytes) in to_lowercase()
        let text = "İstanbul and API key";
        let out = highlight_search_matches(text, "api");
        assert!(out.contains("<mark class=\"search-match\">API</mark>"));
        assert_eq!(count_matches(text, "api"), 1);
    }

    #[test]
    fn pretty_body_beautifies_bom_and_large_minified_json() {
        let bom_json = "\u{feff}{\"user\":{\"name\":\"ada\",\"roles\":[\"admin\"]}}";
        let (pretty, start, end, total, parsed) = pretty_body(bom_json, 0);
        assert!(parsed.is_some());
        assert_eq!(start, 0);
        assert_eq!(end, total);
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("  \"user\": {"));

        // Large minified JSON > JSON_PAINT_CAP still beautifies first before paging
        let mut items = Vec::new();
        for i in 0..12_000 {
            items.push(format!("{{\"id\":{i},\"name\":\"user_{i}\"}}"));
        }
        let huge = format!("[{}]", items.join(","));
        assert!(huge.len() > JSON_PAINT_CAP);
        let (pretty_huge, s2, e2, total2, parsed2) = pretty_body(&huge, 0);
        assert!(parsed2.is_some());
        assert_eq!(s2, 0);
        assert!(e2 < total2);
        assert!(pretty_huge.contains("{\n    \"id\": 0"));
    }
}
