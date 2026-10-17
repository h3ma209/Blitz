use dioxus::prelude::*;
use uuid::Uuid;

use crate::core::Environment;

#[component]
pub fn EnvSelector(
    environments: Vec<Environment>,
    active_id: Option<Uuid>,
    on_change: EventHandler<Option<Uuid>>,
    on_manage: EventHandler<()>,
) -> Element {
    let active_value = active_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "__none__".into());

    rsx! {
        div { class: "env-selector-wrap",
            select {
                class: "env-select",
                value: "{active_value}",
                onchange: move |evt| {
                    let v = evt.value();
                    if v == "__none__" {
                        on_change.call(None);
                    } else if let Ok(id) = Uuid::parse_str(&v) {
                        on_change.call(Some(id));
                    }
                },
                option { value: "__none__", selected: active_id.is_none(), "No Environment" }
                for env in environments {
                    option {
                        value: "{env.id}",
                        selected: active_id == Some(env.id),
                        "{env.name}"
                    }
                }
            }
            button {
                class: "btn-icon",
                title: "Manage environments",
                onclick: move |_| on_manage.call(()),
                "⚙"
            }
        }
    }
}
