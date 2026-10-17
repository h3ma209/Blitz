use dioxus::prelude::*;
use uuid::Uuid;

use crate::core::Environment;

#[component]
pub fn EnvSelector(
    environments: Vec<Environment>,
    active_id: Option<Uuid>,
    /// When true, show Sync per env (cloud sync enabled).
    cloud_enabled: bool,
    on_change: EventHandler<Option<Uuid>>,
    on_sync: EventHandler<Uuid>,
    on_manage: EventHandler<()>,
) -> Element {
    let mut menu_open = use_signal(|| false);

    let active_name = environments
        .iter()
        .find(|e| Some(e.id) == active_id)
        .map(|e| e.name.clone())
        .unwrap_or_else(|| "No Environment".into());

    rsx! {
        div { class: "env-selector-wrap",
            button {
                class: "env-select-trigger",
                title: "Environment",
                onclick: move |_| menu_open.set(!menu_open()),
                span { class: "env-select-label", "{active_name}" }
                span { class: "env-select-caret", "▾" }
            }
            button {
                class: "btn-icon",
                title: "Manage environments",
                onclick: move |_| {
                    menu_open.set(false);
                    on_manage.call(());
                },
                "⚙"
            }

            if menu_open() {
                div {
                    class: "env-menu-backdrop",
                    onclick: move |_| menu_open.set(false),
                }
                div { class: "env-menu",
                    button {
                        class: if active_id.is_none() {
                            "env-menu-row env-menu-row-active"
                        } else {
                            "env-menu-row"
                        },
                        onclick: move |_| {
                            menu_open.set(false);
                            on_change.call(None);
                        },
                        span { class: "env-menu-name", "No Environment" }
                    }
                    for env in environments.iter() {
                        EnvMenuRow {
                            id: env.id,
                            name: env.name.clone(),
                            active: active_id == Some(env.id),
                            cloud_enabled,
                            on_change,
                            on_sync,
                            on_close: move |_| menu_open.set(false),
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn EnvMenuRow(
    id: Uuid,
    name: String,
    active: bool,
    cloud_enabled: bool,
    on_change: EventHandler<Option<Uuid>>,
    on_sync: EventHandler<Uuid>,
    on_close: EventHandler<()>,
) -> Element {
    let row_class = if active {
        "env-menu-row env-menu-row-active"
    } else {
        "env-menu-row"
    };
    rsx! {
        div { class: "{row_class}",
            button {
                class: "env-menu-name-btn",
                title: "Select {name}",
                onclick: move |_| {
                    on_close.call(());
                    on_change.call(Some(id));
                },
                "{name}"
            }
            if cloud_enabled {
                button {
                    class: "env-sync-btn",
                    title: "Sync {name} → git branch + push",
                    onclick: move |_| {
                        on_close.call(());
                        on_sync.call(id);
                    },
                    "Sync"
                }
            }
        }
    }
}
