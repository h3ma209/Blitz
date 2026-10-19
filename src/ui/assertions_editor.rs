//! Visual test builder — no-code response assertions (Tests tab).
//!
//! Assertions are stored on the request (`RequestDraft::assertions`) and compiled
//! to a `blitz.test(...)` script at send time (see `core::assertions`).

use dioxus::prelude::*;

use crate::core::{Assertion, AssertionKind};

#[component]
pub fn AssertionsEditor(
    assertions: Vec<Assertion>,
    on_change: EventHandler<Vec<Assertion>>,
) -> Element {
    let count = assertions.len();
    rsx! {
        p { class: "hint",
            "Tests with no code. Each assertion runs after the response arrives and shows pass/fail in the response Tests tab, the Collection Runner, and the CLI. Values accept JSON (5, true, \"5\") or plain text."
        }

        div { class: "assert-list",
            if count == 0 {
                div { class: "assert-empty",
                    div { class: "assert-empty-title", "No assertions yet" }
                    div { class: "hint", "Add one below — e.g. \"Status code equals 200\" or \"JSON path exists data.id\"." }
                }
            }
            for (i, a) in assertions.iter().enumerate() {
                AssertionRow {
                    key: "{a.id}",
                    assertion: a.clone(),
                    on_change: {
                        let list = assertions.clone();
                        move |updated: Assertion| {
                            let mut next = list.clone();
                            if let Some(slot) = next.get_mut(i) {
                                *slot = updated;
                            }
                            on_change.call(next);
                        }
                    },
                    on_remove: {
                        let list = assertions.clone();
                        move |_| {
                            let mut next = list.clone();
                            if i < next.len() {
                                next.remove(i);
                            }
                            on_change.call(next);
                        }
                    },
                }
            }
        }

        div { class: "assert-add-row",
            select {
                class: "assert-add-select",
                value: "",
                title: "Add an assertion",
                onchange: {
                    let list = assertions.clone();
                    move |evt: Event<FormData>| {
                        if let Some(kind) = AssertionKind::default_for(&evt.value()) {
                            let mut next = list.clone();
                            next.push(Assertion::new(kind));
                            on_change.call(next);
                        }
                    }
                },
                option { value: "", selected: true, "+ Add assertion…" }
                for (key, label) in AssertionKind::picker_options() {
                    option { value: "{key}", "{label}" }
                }
            }
        }
    }
}

#[component]
fn AssertionRow(
    assertion: Assertion,
    on_change: EventHandler<Assertion>,
    on_remove: EventHandler<()>,
) -> Element {
    let id = assertion.id;
    let enabled = assertion.enabled;
    let kind = assertion.kind.clone();
    let toggle_kind = assertion.kind.clone();

    let set_kind = move |k: AssertionKind| {
        on_change.call(Assertion {
            id,
            enabled,
            kind: k,
        });
    };

    let row_class = if enabled {
        "assert-row"
    } else {
        "assert-row assert-row-off"
    };
    let type_label = AssertionKind::picker_options()
        .iter()
        .find(|(k, _)| *k == kind.type_key())
        .map(|(_, l)| *l)
        .unwrap_or("Assertion");

    rsx! {
        div { class: "{row_class}",
            input {
                class: "assert-toggle",
                r#type: "checkbox",
                checked: enabled,
                title: if enabled { "Enabled" } else { "Disabled" },
                onchange: move |evt: Event<FormData>| {
                    on_change.call(Assertion {
                        id,
                        enabled: evt.checked(),
                        kind: toggle_kind.clone(),
                    });
                },
            }
            span { class: "assert-type", "{type_label}" }
            div { class: "assert-fields",
                match kind.clone() {
                    AssertionKind::StatusEquals { code } => rsx! {
                        input {
                            class: "assert-input assert-input-num",
                            r#type: "number",
                            value: "{code}",
                            oninput: move |evt: Event<FormData>| {
                                set_kind(AssertionKind::StatusEquals {
                                    code: evt.value().trim().parse().unwrap_or(0),
                                });
                            },
                        }
                    },
                    AssertionKind::StatusInRange { min, max } => rsx! {
                        input {
                            class: "assert-input assert-input-num",
                            r#type: "number",
                            value: "{min}",
                            oninput: move |evt: Event<FormData>| {
                                set_kind(AssertionKind::StatusInRange {
                                    min: evt.value().trim().parse().unwrap_or(0),
                                    max,
                                });
                            },
                        }
                        span { class: "assert-sep", "to" }
                        input {
                            class: "assert-input assert-input-num",
                            r#type: "number",
                            value: "{max}",
                            oninput: move |evt: Event<FormData>| {
                                set_kind(AssertionKind::StatusInRange {
                                    min,
                                    max: evt.value().trim().parse().unwrap_or(0),
                                });
                            },
                        }
                    },
                    AssertionKind::JsonPathEquals { path, expected } => rsx! {
                        input {
                            class: "assert-input assert-input-mono",
                            r#type: "text",
                            placeholder: "data.items[0].id",
                            value: "{path}",
                            oninput: {
                                let expected = expected.clone();
                                move |evt: Event<FormData>| {
                                    set_kind(AssertionKind::JsonPathEquals {
                                        path: evt.value(),
                                        expected: expected.clone(),
                                    });
                                }
                            },
                        }
                        span { class: "assert-sep", "=" }
                        input {
                            class: "assert-input",
                            r#type: "text",
                            placeholder: "expected value",
                            value: "{expected}",
                            oninput: {
                                let path = path.clone();
                                move |evt: Event<FormData>| {
                                    set_kind(AssertionKind::JsonPathEquals {
                                        path: path.clone(),
                                        expected: evt.value(),
                                    });
                                }
                            },
                        }
                    },
                    AssertionKind::JsonPathExists { path } => rsx! {
                        input {
                            class: "assert-input assert-input-mono",
                            r#type: "text",
                            placeholder: "data.token",
                            value: "{path}",
                            oninput: move |evt: Event<FormData>| {
                                set_kind(AssertionKind::JsonPathExists { path: evt.value() });
                            },
                        }
                    },
                    AssertionKind::HeaderExists { name } => rsx! {
                        input {
                            class: "assert-input",
                            r#type: "text",
                            placeholder: "content-type",
                            value: "{name}",
                            oninput: move |evt: Event<FormData>| {
                                set_kind(AssertionKind::HeaderExists { name: evt.value() });
                            },
                        }
                    },
                    AssertionKind::HeaderEquals { name, value } => rsx! {
                        input {
                            class: "assert-input",
                            r#type: "text",
                            placeholder: "content-type",
                            value: "{name}",
                            oninput: {
                                let value = value.clone();
                                move |evt: Event<FormData>| {
                                    set_kind(AssertionKind::HeaderEquals {
                                        name: evt.value(),
                                        value: value.clone(),
                                    });
                                }
                            },
                        }
                        span { class: "assert-sep", "=" }
                        input {
                            class: "assert-input",
                            r#type: "text",
                            placeholder: "application/json",
                            value: "{value}",
                            oninput: {
                                let name = name.clone();
                                move |evt: Event<FormData>| {
                                    set_kind(AssertionKind::HeaderEquals {
                                        name: name.clone(),
                                        value: evt.value(),
                                    });
                                }
                            },
                        }
                    },
                    AssertionKind::BodyContains { text } => rsx! {
                        input {
                            class: "assert-input",
                            r#type: "text",
                            placeholder: "text the body must contain",
                            value: "{text}",
                            oninput: move |evt: Event<FormData>| {
                                set_kind(AssertionKind::BodyContains { text: evt.value() });
                            },
                        }
                    },
                    AssertionKind::TimeUnder { ms } => rsx! {
                        input {
                            class: "assert-input assert-input-num",
                            r#type: "number",
                            value: "{ms}",
                            oninput: move |evt: Event<FormData>| {
                                set_kind(AssertionKind::TimeUnder {
                                    ms: evt.value().trim().parse().unwrap_or(0),
                                });
                            },
                        }
                        span { class: "assert-sep", "ms" }
                    },
                }
            }
            button {
                class: "assert-remove",
                r#type: "button",
                title: "Remove assertion",
                onclick: move |_| on_remove.call(()),
                "✕"
            }
        }
    }
}
