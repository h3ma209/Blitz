use dioxus::prelude::*;

use crate::core::{DefaultResponseTab, ResponseView};
use super::json_highlight::{render_json_highlight_html, should_highlight_json};
use super::loading::{LoadingOverlay, SkeletonBars, Spinner};

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
                            title: "Save as example",
                            onclick: move |_| on_save_example.call(()),
                            "Save example"
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
                    },
                    RespTab::Raw => rsx! {
                        pre { class: "response-body",
                            if resp.body.is_empty() && resp.error.is_none() {
                                "(empty body)"
                            } else {
                                "{resp.body}"
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
                                    "No test results. Add pm.test(\"name\", function() {{ … }}) in Scripts."
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
                    RespTab::Visualize => rsx! {
                        div { class: "visualizer-pane",
                            if resp.visualizer_html.is_empty() {
                                p { class: "hint",
                                    "No visualizer output. In Tests: pm.visualizer.set(\"<h1>{{{{title}}}}</h1>\", {{title:\"hi\"}}) — or return HTML body."
                                }
                            } else {
                                p { class: "hint", "Visualizer HTML (source — desktop has no iframe sandbox):" }
                                pre { class: "response-body", "{resp.visualizer_html}" }
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
