//! Environments list + variable editor (initial / current).

use dioxus::prelude::*;
use uuid::Uuid;

use crate::core::{EnvVar, Environment};

#[derive(Clone, PartialEq)]
pub enum EnvManagerEvent {
    Select(Option<Uuid>),
    Create,
    Rename { id: Uuid, name: String },
    Delete(Uuid),
    SetVars { id: Uuid, values: Vec<EnvVar> },
    Close,
}

#[component]
pub fn EnvManager(
    environments: Vec<Environment>,
    active_id: Option<Uuid>,
    editing_id: Option<Uuid>,
    on_event: EventHandler<EnvManagerEvent>,
) -> Element {
    let editing = editing_id
        .and_then(|id| environments.iter().find(|e| e.id == id).cloned())
        .or_else(|| {
            active_id
                .and_then(|id| environments.iter().find(|e| e.id == id).cloned())
        })
        .or_else(|| environments.first().cloned());

    rsx! {
        div { class: "env-manager",
            div { class: "env-manager-head",
                h3 { "Environments" }
                div { class: "sidebar-actions",
                    button {
                        class: "btn-icon",
                        title: "New environment",
                        onclick: move |_| on_event.call(EnvManagerEvent::Create),
                        "+"
                    }
                    button {
                        class: "btn-icon",
                        title: "Close",
                        onclick: move |_| on_event.call(EnvManagerEvent::Close),
                        "×"
                    }
                }
            }
            div { class: "env-list",
                button {
                    class: if active_id.is_none() { "sidebar-item active" } else { "sidebar-item" },
                    onclick: move |_| on_event.call(EnvManagerEvent::Select(None)),
                    "No Environment"
                }
                for env in environments.iter() {
                    {
                        let id = env.id;
                        let name = env.name.clone();
                        let selected = active_id == Some(id);
                        rsx! {
                            div { class: "env-list-row",
                                button {
                                    class: if selected { "sidebar-item active" } else { "sidebar-item" },
                                    onclick: move |_| on_event.call(EnvManagerEvent::Select(Some(id))),
                                    "{name}"
                                }
                                button {
                                    class: "btn-icon danger",
                                    title: "Delete",
                                    onclick: move |_| on_event.call(EnvManagerEvent::Delete(id)),
                                    "×"
                                }
                            }
                        }
                    }
                }
            }

            if let Some(env) = editing {
                {
                    let env_id = env.id;
                    let env_name = env.name.clone();
                    let values = env.values.clone();
                    rsx! {
                        div { class: "env-editor",
                            input {
                                class: "env-name-input",
                                value: "{env_name}",
                                oninput: move |evt| {
                                    on_event.call(EnvManagerEvent::Rename {
                                        id: env_id,
                                        name: evt.value(),
                                    });
                                }
                            }
                            table { class: "kv-table",
                                thead {
                                    tr {
                                        th { "Variable" }
                                        th { "Initial" }
                                        th { "Current" }
                                    }
                                }
                                tbody {
                                    for (i, row) in values.iter().enumerate() {
                                        tr {
                                            td {
                                                input {
                                                    r#type: "text",
                                                    value: "{row.key}",
                                                    oninput: {
                                                        let values = values.clone();
                                                        move |evt: Event<FormData>| {
                                                            let mut next = values.clone();
                                                            if let Some(r) = next.get_mut(i) {
                                                                r.key = evt.value();
                                                            }
                                                            ensure_trailing(&mut next);
                                                            on_event.call(EnvManagerEvent::SetVars {
                                                                id: env_id,
                                                                values: next,
                                                            });
                                                        }
                                                    }
                                                }
                                            }
                                            td {
                                                input {
                                                    r#type: "text",
                                                    value: "{row.initial_value}",
                                                    oninput: {
                                                        let values = values.clone();
                                                        move |evt: Event<FormData>| {
                                                            let mut next = values.clone();
                                                            if let Some(r) = next.get_mut(i) {
                                                                r.initial_value = evt.value();
                                                            }
                                                            ensure_trailing(&mut next);
                                                            on_event.call(EnvManagerEvent::SetVars {
                                                                id: env_id,
                                                                values: next,
                                                            });
                                                        }
                                                    }
                                                }
                                            }
                                            td {
                                                input {
                                                    r#type: "text",
                                                    value: "{row.current_value}",
                                                    oninput: {
                                                        let values = values.clone();
                                                        move |evt: Event<FormData>| {
                                                            let mut next = values.clone();
                                                            if let Some(r) = next.get_mut(i) {
                                                                r.current_value = evt.value();
                                                            }
                                                            ensure_trailing(&mut next);
                                                            on_event.call(EnvManagerEvent::SetVars {
                                                                id: env_id,
                                                                values: next,
                                                            });
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            p { class: "hint", "Active env vars resolve as {{{{name}}}} (current → initial)." }
                        }
                    }
                }
            }
        }
    }
}

fn ensure_trailing(rows: &mut Vec<EnvVar>) {
    let needs = rows
        .last()
        .map(|r| !r.key.is_empty() || !r.initial_value.is_empty() || !r.current_value.is_empty())
        .unwrap_or(true);
    if needs {
        rows.push(EnvVar {
            key: String::new(),
            initial_value: String::new(),
            current_value: String::new(),
            enabled: true,
        });
    }
}
