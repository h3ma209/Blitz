use dioxus::prelude::*;

use crate::core::{DefaultResponseTab, ResponseView};
use super::json_highlight::{render_json_highlight_html, should_highlight_json};
use super::loading::{SkeletonBars, Spinner};

#[derive(Clone, Copy, PartialEq, Eq)]
enum RespTab {
    Pretty,
    Raw,
    Preview,
    Headers,
    Cookies,
    Tests,
    Visualize,
    Timeline,
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
    sending: bool,
    #[props(default)] default_tab: DefaultResponseTab,
    on_save_example: EventHandler<()>,
) -> Element {
    let mut tab = use_signal(|| resp_tab_from_default(default_tab));
    let mut search_query = use_signal(String::new);
    let mut search_open = use_signal(|| false);
    let mut copied_body = use_signal(|| false);

    rsx! {
        section { class: "response-pane",
            if sending {
                div { class: "response-sending",
                    Spinner { }
                    p { class: "hint", "Sending…" }
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
                            let ok = resp.status >= 200 && resp.status < 400;
                            let tag = if ok { "OK" } else { "Fail" };
                            let cls = if ok { "status-chip status-ok" } else { "status-chip status-err" };
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
                                    });
                                    copied_body.set(true);
                                }
                            },
                            if copied_body() { "✓ Copied" } else { "📋 Copy" }
                        }
                        button {
                            class: if search_open() { "btn btn-ghost active" } else { "btn btn-ghost" },
                            title: "Find in response (⌘F)",
                            onclick: move |_| search_open.set(!search_open()),
                            "🔍 Find"
                        }
                        button {
                            class: "btn btn-ghost",
                            title: "Save response as mock route example (served by local Mock Server)",
                            onclick: move |_| on_save_example.call(()),
                            "⚡ Save to Mock"
                        }
                    }
                }

                if search_open() {
                    div { class: "response-search-bar",
                        input {
                            class: "response-search-input",
                            placeholder: "Search response body…",
                            value: "{search_query()}",
                            oninput: move |evt: Event<FormData>| search_query.set(evt.value()),
                        }
                        if !search_query().is_empty() {
                            {
                                let count = count_matches(&resp.body, &search_query());
                                let text = if count == 0 {
                                    "No matches".to_string()
                                } else if count == 1 {
                                    "1 match".to_string()
                                } else {
                                    format!("{count} matches")
                                };
                                rsx! { span { class: "hint", "{text}" } }
                            }
                        }
                        button {
                            class: "btn btn-ghost",
                            style: "padding: 0 6px;",
                            title: "Close search",
                            onclick: move |_| {
                                search_query.set(String::new());
                                search_open.set(false);
                            },
                            "✕"
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
                    RespTab::Visualize => {
                        let render_mode = use_signal(|| true);
                        rsx! {
                            div { class: "visualizer-pane",
                                if resp.visualizer_html.is_empty() {
                                    p { class: "hint",
                                        "No visualizer output. In Tests: pm.visualizer.set(\"<h1>{{{{title}}}}</h1>\", {{title:\"Hello\"}}) — or return HTML body."
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
                p { class: "hint", "Response appears here after Send." }
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
        0
    } else {
        text.to_lowercase().match_indices(&query.to_lowercase()).count()
    }
}

fn highlight_search_matches(text: &str, query: &str) -> String {
    if query.is_empty() {
        return html_escape(text);
    }
    let lower_text = text.to_lowercase();
    let lower_query = query.to_lowercase();
    let mut result = String::new();
    let mut last_idx = 0;

    for (idx, _) in lower_text.match_indices(&lower_query) {
        result.push_str(&html_escape(&text[last_idx..idx]));
        result.push_str("<mark class=\"search-match\">");
        result.push_str(&html_escape(&text[idx..idx + query.len()]));
        result.push_str("</mark>");
        last_idx = idx + query.len();
    }
    result.push_str(&html_escape(&text[last_idx..]));
    result
}
