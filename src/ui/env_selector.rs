use dioxus::prelude::*;
use uuid::Uuid;

use crate::core::Environment;
use super::DismissBackdrop;

fn env_tier(name: &str) -> &'static str {
    let lower = name.trim().to_lowercase();
    if lower.contains("prod") || lower.contains("live") {
        "prod"
    } else if lower.contains("stag") || lower.contains("uat") || lower.contains("qa") || lower.contains("test") {
        "stage"
    } else {
        "local"
    }
}

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
    let active_tier = if active_id.is_none() { "local" } else { env_tier(&active_name) };

    rsx! {
        div { class: "env-selector-wrap env-tier-{active_tier}",
            if menu_open() {
                DismissBackdrop {
                    z_index: 900,
                    on_dismiss: move |_| menu_open.set(false),
                }
            }
            button {
                class: "env-select-trigger",
                title: "Select environment ({active_tier})",
                onclick: move |_| menu_open.set(!menu_open()),
                span { class: "env-indicator env-indicator-{active_tier}", aria_hidden: "true" }
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
                span { class: "action-icon action-icon-edit", aria_hidden: "true" }
            }

            if menu_open() {
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
                        span { class: "env-indicator env-indicator-local", aria_hidden: "true" }
                        span { class: "env-menu-name", "No Environment" }
                    }
                    for env in environments.iter() {
                        {
                            let id = env.id;
                            let name = env.name.clone();
                            let tier = env_tier(&name);
                            let active = active_id == Some(id);
                            let row_class = if active {
                                "env-menu-row env-menu-row-active"
                            } else {
                                "env-menu-row"
                            };
                            rsx! {
                                button {
                                    class: "{row_class}",
                                    title: "Select {name} ({tier})",
                                    onclick: move |_| {
                                        menu_open.set(false);
                                        on_change.call(Some(id));
                                    },
                                    span { class: "env-indicator env-indicator-{tier}", aria_hidden: "true" }
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
