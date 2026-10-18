use dioxus::prelude::*;
use uuid::Uuid;

use crate::core::Environment;
use super::DismissBackdrop;

#[component]
pub fn EnvSelector(
    environments: Vec<Environment>,
    active_id: Option<Uuid>,
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
                title: "Select environment",
                onclick: move |_| menu_open.set(!menu_open()),
                span { class: "env-select-label", "{active_name}" }
                span { class: "env-select-caret", "▾" }
            }
            // Sync only for team/git envs — hide for No Environment and Local.
            if active_id.is_some()
                && !active_name.trim().eq_ignore_ascii_case("Local")
            {
                button {
                    class: "env-sync-btn",
                    title: "Sync with team: pull → commit local → push. Local/script vars stay on this machine.",
                    onclick: move |_| {
                        menu_open.set(false);
                        if let Some(id) = active_id {
                            on_sync.call(id);
                        }
                    },
                    "Sync"
                }
            }
            button {
                class: "btn-icon",
                title: "Manage environments",
                aria_label: "Manage environments",
                onclick: move |_| {
                    menu_open.set(false);
                    on_manage.call(());
                },
                span { class: "action-icon action-icon-settings", aria_hidden: "true" }
            }

            if menu_open() {
                DismissBackdrop {
                    z_index: 900,
                    on_dismiss: move |_| menu_open.set(false),
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
                        {
                            let id = env.id;
                            let name = env.name.clone();
                            let active = active_id == Some(id);
                            let row_class = if active {
                                "env-menu-row env-menu-row-active"
                            } else {
                                "env-menu-row"
                            };
                            rsx! {
                                button {
                                    class: "{row_class}",
                                    title: "Select {name}",
                                    onclick: move |_| {
                                        menu_open.set(false);
                                        on_change.call(Some(id));
                                    },
                                    span { class: "env-menu-name", "{name}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
