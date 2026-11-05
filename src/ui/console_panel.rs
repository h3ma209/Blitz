use dioxus::prelude::*;

use crate::core::{ConsoleLevel, ConsoleLine};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConsoleFilter {
    #[default]
    All,
    Network,
    Cloud,
    Errors,
    Scripts,
}

#[component]
pub fn ConsolePanel(
    open: bool,
    lines: Vec<ConsoleLine>,
    resizing: bool,
    on_toggle: EventHandler<()>,
    on_clear: EventHandler<()>,
    on_resize_start: EventHandler<f64>,
) -> Element {
    let mut filter = use_signal(|| ConsoleFilter::All);
    let mut search = use_signal(String::new);
    let mut copied = use_signal(|| false);

    let err_count = lines
        .iter()
        .filter(|l| l.level == ConsoleLevel::Error || l.level == ConsoleLevel::Warn)
        .count();
    let net_count = lines
        .iter()
        .filter(|l| l.level == ConsoleLevel::Network)
        .count();
    let cloud_count = lines
        .iter()
        .filter(|l| l.level == ConsoleLevel::Cloud)
        .count();
    let script_count = lines
        .iter()
        .filter(|l| l.level == ConsoleLevel::Script)
        .count();
    let all_count = lines.len();

    let cur_filter = filter();
    let cur_search = search().trim().to_lowercase();

    let filtered: Vec<&ConsoleLine> = lines
        .iter()
        .filter(|l| match cur_filter {
            ConsoleFilter::All => true,
            ConsoleFilter::Network => l.level == ConsoleLevel::Network,
            ConsoleFilter::Cloud => l.level == ConsoleLevel::Cloud,
            ConsoleFilter::Errors => {
                l.level == ConsoleLevel::Error || l.level == ConsoleLevel::Warn
            }
            ConsoleFilter::Scripts => l.level == ConsoleLevel::Script,
        })
        .filter(|l| {
            if cur_search.is_empty() {
                true
            } else {
                l.message.to_lowercase().contains(&cur_search)
            }
        })
        .collect();

    let copy_text = filtered
        .iter()
        .map(|l| {
            let tag = match l.level {
                ConsoleLevel::Error => "ERR",
                ConsoleLevel::Warn => "WARN",
                ConsoleLevel::Script => "SCRIPT",
                ConsoleLevel::Network => "NET",
                ConsoleLevel::Cloud => "CLOUD",
                ConsoleLevel::Info => "INFO",
            };
            format!("[{}] [{tag}] {}", l.at.format("%H:%M:%S"), l.message)
        })
        .collect::<Vec<_>>()
        .join("\n");

    rsx! {
        div { class: "console-wrap",
            div {
                class: if resizing {
                    "console-resize active"
                } else {
                    "console-resize"
                },
                title: "Drag to resize console",
                onmousedown: move |evt| {
                    evt.prevent_default();
                    evt.stop_propagation();
                    on_resize_start.call(evt.client_coordinates().y);
                },
            }
            div { class: "console-bar",
                div { class: "console-bar-left",
                    button {
                        class: "builder-tab console-toggle-tab",
                        onclick: move |_| on_toggle.call(()),
                        if open { "Console ▾" } else { "Console ▸" }
                        if !open && err_count > 0 {
                            span { class: "console-err-badge-inline", "⚠ {err_count}" }
                        }
                    }
                    if open {
                        div { class: "console-filters",
                            button {
                                class: if cur_filter == ConsoleFilter::All { "console-filter-pill active" } else { "console-filter-pill" },
                                onclick: move |_| filter.set(ConsoleFilter::All),
                                "All ({all_count})"
                            }
                            button {
                                class: if cur_filter == ConsoleFilter::Network { "console-filter-pill active" } else { "console-filter-pill" },
                                onclick: move |_| filter.set(ConsoleFilter::Network),
                                "Net ({net_count})"
                            }
                            button {
                                class: if cur_filter == ConsoleFilter::Cloud { "console-filter-pill active" } else { "console-filter-pill" },
                                onclick: move |_| filter.set(ConsoleFilter::Cloud),
                                "Cloud ({cloud_count})"
                            }
                            button {
                                class: if cur_filter == ConsoleFilter::Errors { "console-filter-pill active" } else { "console-filter-pill" },
                                onclick: move |_| filter.set(ConsoleFilter::Errors),
                                "Errors ({err_count})"
                            }
                            button {
                                class: if cur_filter == ConsoleFilter::Scripts { "console-filter-pill active" } else { "console-filter-pill" },
                                onclick: move |_| filter.set(ConsoleFilter::Scripts),
                                "Scripts ({script_count})"
                            }
                        }
                    }
                }
                if open {
                    div { class: "console-bar-right",
                        input {
                            class: "console-search-input",
                            r#type: "text",
                            placeholder: "Search console…",
                            value: "{search()}",
                            oninput: move |evt| search.set(evt.value()),
                        }
                        if !cur_search.is_empty() {
                            button {
                                class: "console-btn-text",
                                title: "Clear search query",
                                onclick: move |_| search.set(String::new()),
                                "✕"
                            }
                        }
                        button {
                            class: "console-btn-text",
                            title: "Copy filtered logs to clipboard",
                            onclick: {
                                let txt = copy_text.clone();
                                move |_| {
                                    if !txt.is_empty() {
                                        let script = format!(
                                            "navigator.clipboard.writeText({});",
                                            serde_json::to_string(&txt).unwrap_or_default()
                                        );
                                        spawn(async move {
                                            let _ = document::eval(&script).await;
                                        });
                                        copied.set(true);
                                    }
                                }
                            },
                            if copied() { "✓ Copied" } else { "Copy" }
                        }
                        button {
                            class: "console-btn-text",
                            title: "Clear all logs",
                            onclick: move |_| on_clear.call(()),
                            "Clear"
                        }
                    }
                }
            }
            if open {
                pre { class: "console-body",
                    if filtered.is_empty() {
                        span { class: "hint",
                            if all_count == 0 {
                                "Send a request or sync cloud — network telemetry, errors, cloud events, and script logs appear here."
                            } else {
                                "No logs match the current filter or search query."
                            }
                        }
                    } else {
                        for (idx, line) in filtered.iter().enumerate() {
                            {
                                let cls = match line.level {
                                    ConsoleLevel::Error => "console-err",
                                    ConsoleLevel::Warn => "console-warn",
                                    ConsoleLevel::Script => "console-script",
                                    ConsoleLevel::Network => "console-network",
                                    ConsoleLevel::Cloud => "console-cloud",
                                    ConsoleLevel::Info => "console-info",
                                };
                                let ts = line.at.format("%H:%M:%S").to_string();
                                rsx! {
                                    div { key: "{idx}", class: "{cls}",
                                        span { class: "console-ts", "[{ts}] " }
                                        "{line.message}"
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
