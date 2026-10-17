use dioxus::prelude::*;

use crate::core::ResponseView;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RespTab {
    Pretty,
    Raw,
    Preview,
    Headers,
    Cookies,
    Tests,
}

#[component]
pub fn ResponseViewer(
    response: Option<ResponseView>,
    sending: bool,
    on_save_example: EventHandler<()>,
) -> Element {
    let mut tab = use_signal(|| RespTab::Pretty);

    rsx! {
        section { class: "response-pane",
            if sending {
                p { class: "hint", "Sending…" }
            } else if let Some(resp) = response.as_ref() {
                div { class: "response-meta",
                    if let Some(err) = &resp.error {
                        span { class: "status-err", "Error: {err}" }
                    } else {
                        span {
                            class: if resp.status >= 200 && resp.status < 400 { "status-ok" } else { "status-err" },
                            "{resp.status} {resp.status_text}"
                        }
                        span { "{resp.elapsed_ms} ms" }
                        span { "{resp.size_bytes()} B" }
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
                    ] {
                        button {
                            class: if tab() == t { "builder-tab active" } else { "builder-tab" },
                            onclick: move |_| tab.set(t),
                            "{label}"
                        }
                    }
                }

                match tab() {
                    RespTab::Pretty => rsx! {
                        pre { class: "response-body",
                            "{pretty_body(&resp.body)}"
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
                            // iframe srcdoc not always available in dioxus — show escaped HTML note + raw
                            div {
                                p { class: "hint", "HTML preview (source):" }
                                pre { class: "response-body", "{resp.body}" }
                            }
                        } else {
                            p { class: "hint", "Preview is for HTML responses. Showing Pretty instead." }
                            pre { class: "response-body", "{pretty_body(&resp.body)}" }
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
                                    rsx! { "(no Set-Cookie headers — see Cookies jar in footer state)" }
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
                }
            } else {
                p { class: "hint", "Response appears here after Send." }
            }
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
