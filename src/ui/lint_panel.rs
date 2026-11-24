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
                    div { class: "lint-list",
                        for item in issues {
                            { lint_row(&item) }
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
