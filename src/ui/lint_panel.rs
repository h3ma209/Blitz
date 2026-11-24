use dioxus::prelude::*;

use crate::core::{lint_collections, lint_error_count, LintIssue, LintSeverity, VariableContext};

#[component]
pub fn LintPanel(
    collections: Vec<crate::core::Collection>,
    vars: VariableContext,
    on_close: EventHandler<()>,
) -> Element {
    let issues = lint_collections(&collections, &vars);
    let errors = lint_error_count(&issues);
    let warnings = issues.len() - errors;
    let mut lint_scroll_row = use_signal(|| 0usize);

    rsx! {
        div { class: "stage-page lint-stage-page",
            div { class: "stage-page-head",
                h2 { "Collection Lint & API Audit" }
                button {
                    class: "btn btn-ghost vars-close-btn",
                    r#type: "button",
                    title: "Close (Esc)",
                    onclick: move |_| on_close.call(()),
                    "✕ Close (Esc)"
                }
            }
            div { class: "stage-page-body",
                div { class: "lint-summary",
                    span { class: if errors > 0 { "lint-stat lint-stat-err" } else { "lint-stat" },
                        "{errors} error(s)"
                    }
                    span { class: "lint-stat lint-stat-warn", "{warnings} warning(s)" }
                }
                if issues.is_empty() {
                    p { class: "hint", "No issues found for current variables and environments." }
                } else {
                    {
                        let len = issues.len();
                        let (start, end) = crate::ui::list_window(len, lint_scroll_row(), 100, 100);
                        let top_pad = start * 64;
                        let bot_pad = len.saturating_sub(end) * 64;
                        rsx! {
                            div {
                                class: "lint-list",
                                onscroll: move |evt: Event<ScrollData>| {
                                    if len <= 100 {
                                        return;
                                    }
                                    let row = (evt.data().scroll_top() / 64.0) as usize;
                                    let bucket = (row / 25) * 25;
                                    if lint_scroll_row() != bucket {
                                        lint_scroll_row.set(bucket);
                                    }
                                },
                                if top_pad > 0 {
                                    div { style: "height: {top_pad}px; flex-shrink: 0;" }
                                }
                                for item in &issues[start..end] {
                                    { lint_row(item) }
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
    }
}

fn lint_row(item: &LintIssue) -> Element {
    let sev = match item.severity {
        LintSeverity::Error => "lint-row lint-row-error",
        LintSeverity::Warning => "lint-row lint-row-warn",
    };
    let req = item
        .request_name
        .as_deref()
        .unwrap_or("— (collection/folder)");
    rsx! {
        div { class: "{sev}",
            div { class: "lint-row-head",
                span { class: "lint-collection", "{item.collection_name}" }
                span { class: "lint-request", "{req}" }
            }
            div { class: "lint-row-body",
                span { class: "lint-field", "{item.field}" }
                span { class: "lint-msg", "{item.message}" }
            }
        }
    }
}
