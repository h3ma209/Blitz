use dioxus::prelude::*;
use uuid::Uuid;

use crate::core::{diff_bodies, DefaultResponseTab, DiffLine, ResponseView, SavedExample};
use super::json_highlight::{render_json_highlight_html, should_highlight_json};
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
    response: Option<ResponseView>,
    #[props(default)] prev_response: Option<ResponseView>,
    #[props(default)] examples: Vec<SavedExample>,
    sending: bool,
    #[props(default)] default_tab: DefaultResponseTab,
    on_save_example: EventHandler<()>,
    #[props(default)] on_auto_assert: EventHandler<()>,
) -> Element {
    let mut tab = use_signal(|| resp_tab_from_default(default_tab));
    let mut search_query = use_signal(String::new);
    let mut current_match_idx = use_signal(|| 0usize);
    let mut search_open = use_signal(|| false);
    let mut copied_body = use_signal(|| false);
    let render_mode = use_signal(|| true);
    let mut diff_baseline = use_signal(|| DiffBaseline::PreviousRun);

    rsx! {
        section { class: "response-pane",
            if sending {
                div { class: "response-sending",
                    span { class: "action-icon action-icon-bolt bolt-flight-pulse", style: "width: 28px; height: 28px; color: var(--spot-green-bright); margin-bottom: 8px;", aria_hidden: "true" }
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
                                span { class: "action-icon action-icon-check", aria_hidden: "true" }
                                span { "Copied" }
                            } else {
                                span { class: "action-icon action-icon-copy", aria_hidden: "true" }
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
                            span { class: "action-icon action-icon-save", aria_hidden: "true" }
                            span { "Save" }
                        }
                        button {
                            class: if search_open() { "btn btn-ghost active" } else { "btn btn-ghost" },
                            title: "Find in response (⌘F)",
                            onclick: move |_| search_open.set(!search_open()),
                            span { class: "action-icon action-icon-search", aria_hidden: "true" }
                            span { "Find" }
                        }
                        button {
                            class: "btn btn-ghost",
                            title: "Generate Tests from this response: status, content-type, JSON fields, time ceiling. Review them in the Tests tab.",
                            onclick: move |_| on_auto_assert.call(()),
                            span { class: "action-icon action-icon-check", aria_hidden: "true" }
                            span { "Auto-assert" }
                        }
                        button {
                            class: "btn btn-ghost",
                            title: "Save response as mock route example (served by local Mock Server)",
                            onclick: move |_| on_save_example.call(()),
                            span { class: "action-icon action-icon-bolt", aria_hidden: "true" }
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
                        let pretty = pretty_body(&resp.body);
                        let query = search_query();
                        if !query.is_empty() {
                            let highlighted = highlight_search_matches(&pretty, &query);
                            rsx! {
                                pre {
                                    class: "response-body",
                                    dangerous_inner_html: "{highlighted}",
                                }
                            }
                        } else if should_highlight_json(&resp.body, &resp.headers) {
                            let html = render_json_highlight_html(&pretty);
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
                    },
                    RespTab::Raw => {
                        let query = search_query();
                        if !query.is_empty() {
                            let highlighted = highlight_search_matches(&resp.body, &query);
                            rsx! {
                                pre {
                                    class: "response-body",
                                    dangerous_inner_html: "{highlighted}",
                                }
                            }
                        } else {
                            rsx! {
                                pre { class: "response-body",
                                    if resp.body.is_empty() && resp.error.is_none() {
                                        "(empty body)"
                                    } else {
                                        "{resp.body}"
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
                                let pretty = pretty_body(&resp.body);
                                if should_highlight_json(&resp.body, &resp.headers) {
                                    let html = render_json_highlight_html(&pretty);
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
                    },
                    RespTab::Headers => rsx! {
                        pre { class: "response-body",
                            if resp.headers.is_empty() {
                                "(no headers)"
                            } else {
                                {
                                    let text = resp
                                        .headers
                                        .iter()
                                        .map(|(k, v)| format!("{k}: {v}"))
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    rsx! { "{text}" }
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
                                for t in resp.test_results.iter() {
                                    div { class: if t.passed { "test-pass" } else { "test-fail" },
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
                                        for ex in examples.iter() {
                                            option {
                                                key: "{ex.id}",
                                                value: "ex:{ex.id}",
                                                selected: diff_baseline() == DiffBaseline::Example(ex.id),
                                                "Saved Example: {ex.name} ({ex.status})"
                                            }
                                        }
                                    }
                                    if let Some((_, old_status, _, old_ms, ref old_body)) = baseline_data {
                                        {
                                            let (_, stats) = diff_bodies(old_body, &resp.body);
                                            let status_delta = if old_status == resp.status {
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

                                if let Some((label, _, _, _, old_body)) = baseline_data {
                                    {
                                        let (lines, stats) = diff_bodies(&old_body, &resp.body);
                                        if stats.added == 0 && stats.removed == 0 {
                                            rsx! {
                                                p { class: "hint", "No differences between {label} and current response (JSON keys normalized)." }
                                            }
                                        } else {
                                            rsx! {
                                                div { class: "response-diff-lines",
                                                    for (idx, line) in lines.into_iter().enumerate() {
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
                                        div {
                                            class: "visualizer-rendered",
                                            style: "background: #fff; color: #111; padding: 16px; border-radius: 6px; overflow: auto; min-height: 200px;",
                                            dangerous_inner_html: "{resp.visualizer_html}",
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
                        span { class: "action-icon action-icon-bolt response-empty-bolt", aria_hidden: "true" }
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

fn pretty_body(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        serde_json::to_string_pretty(&v).unwrap_or_else(|_| body.to_string())
    } else {
        body.to_string()
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
}
