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
    #[props(default)] vars: Option<crate::core::VariableContext>,
    on_change: EventHandler<Option<Uuid>>,
    on_sync: EventHandler<Uuid>,
    on_manage: EventHandler<()>,
    #[props(default)] on_compare: EventHandler<()>,
) -> Element {
    let mut menu_open = use_signal(|| false);
    let mut quick_look_open = use_signal(|| false);
    let mut filter_query = use_signal(String::new);
    let mut copied_key = use_signal(|| None::<String>);

    let active_env = environments.iter().find(|e| Some(e.id) == active_id);
    let active_name = active_env
        .map(|e| e.name.clone())
        .unwrap_or_else(|| "No Environment".into());
    let active_tier = if active_id.is_none() { "local" } else { env_tier(&active_name) };

    let query_lower = filter_query().trim().to_lowercase();

    rsx! {
        div { class: "env-selector-wrap env-tier-{active_tier}",
            if menu_open() {
                DismissBackdrop {
                    z_index: 900,
                    on_dismiss: move |_| menu_open.set(false),
                }
            }
            if quick_look_open() {
                DismissBackdrop {
                    z_index: 900,
                    on_dismiss: move |_| quick_look_open.set(false),
                }
            }
            button {
                class: "env-select-trigger",
                title: "Select environment ({active_tier})",
                onclick: move |_| {
                    quick_look_open.set(false);
                    menu_open.set(!menu_open());
                },
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
                        quick_look_open.set(false);
                        if let Some(id) = active_id {
                            on_sync.call(id);
                        }
                    },
                    "Sync"
                }
            }
            // Quick Look (Eye) Icon
            button {
                class: if quick_look_open() { "btn-icon btn-icon-active" } else { "btn-icon" },
                title: "Environment Quick Look (inspect active & global variables)",
                aria_label: "Environment Quick Look",
                onclick: move |_| {
                    menu_open.set(false);
                    quick_look_open.set(!quick_look_open());
                },
                span { class: "action-icon action-icon-eye", aria_hidden: "true" }
            }
            button {
                class: "btn-icon",
                title: "Manage environments",
                aria_label: "Manage environments",
                onclick: move |_| {
                    menu_open.set(false);
                    quick_look_open.set(false);
                    on_manage.call(());
                },
                span { class: "action-icon action-icon-edit", aria_hidden: "true" }
            }

            if quick_look_open() {
                div { class: "env-quicklook-popover",
                    div { class: "env-quicklook-header",
                        span { class: "env-quicklook-title",
                            span { class: "action-icon action-icon-eye", aria_hidden: "true", style: "width: 12px; height: 12px;" }
                            span { "Quick Look" }
                        }
                        span { class: "env-quicklook-badge env-quicklook-badge-{active_tier}", "{active_name}" }
                    }
                    div { class: "env-quicklook-search-wrap",
                        input {
                            class: "env-quicklook-search",
                            placeholder: "Filter variables…",
                            value: "{filter_query()}",
                            oninput: move |evt: Event<FormData>| filter_query.set(evt.value()),
                        }
                    }
                    div { class: "env-quicklook-body",
                        // Active Environment variables
                        if let Some(env) = active_env {
                            {
                                let vars_matching: Vec<_> = env.values.iter()
                                    .filter(|v| query_lower.is_empty() || v.key.to_lowercase().contains(&query_lower) || v.effective().to_lowercase().contains(&query_lower))
                                    .collect();
                                rsx! {
                                    div { class: "env-quicklook-section-title", "{env.name} ({vars_matching.len()})" }
                                    if vars_matching.is_empty() {
                                        div { class: "env-quicklook-empty",
                                            if query_lower.is_empty() { "No variables in this environment." } else { "No matching variables." }
                                        }
                                    } else {
                                        for v in vars_matching {
                                            {
                                                let key = v.key.clone();
                                                let eff = v.effective().to_string();
                                                let is_copied = copied_key() == Some(key.clone());
                                                rsx! {
                                                    div { key: "{key}", class: "env-quicklook-row",
                                                        span { class: "env-quicklook-key", title: "{{{key}}}", "{{{{{key}}}}}" }
                                                        span { class: "env-quicklook-val", title: "{eff}", "{eff}" }
                                                        button {
                                                            class: if is_copied { "env-quicklook-copy-btn copied" } else { "env-quicklook-copy-btn" },
                                                            title: "Copy variable value",
                                                            onclick: {
                                                                let key_c = key.clone();
                                                                let eff_c = eff.clone();
                                                                move |_| {
                                                                    let script = format!(
                                                                        "navigator.clipboard.writeText({});",
                                                                        serde_json::to_string(&eff_c).unwrap_or_else(|_| "\"\"".into())
                                                                    );
                                                                    let k = key_c.clone();
                                                                    spawn(async move {
                                                                        let _ = document::eval(&script).await;
                                                                        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                                                                        if copied_key() == Some(k) {
                                                                            copied_key.set(None);
                                                                        }
                                                                    });
                                                                    copied_key.set(Some(key_c.clone()));
                                                                }
                                                            },
                                                            if is_copied { "Copied" } else { "Copy" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            div { class: "env-quicklook-empty",
                                "No active environment selected."
                            }
                        }

                        // Globals
                        if let Some(v_ctx) = vars.as_ref() {
                            if !v_ctx.global.is_empty() {
                                {
                                    let globals_matching: Vec<_> = v_ctx.global.iter()
                                        .filter(|(k, v)| query_lower.is_empty() || k.to_lowercase().contains(&query_lower) || v.to_lowercase().contains(&query_lower))
                                        .collect();
                                    rsx! {
                                        div { class: "env-quicklook-section-title", "Globals ({globals_matching.len()})" }
                                        if globals_matching.is_empty() {
                                            div { class: "env-quicklook-empty", "No matching globals." }
                                        } else {
                                            for (k, v) in globals_matching {
                                                {
                                                    let key = k.clone();
                                                    let val = v.clone();
                                                    let is_copied = copied_key() == Some(format!("global_{key}"));
                                                    rsx! {
                                                        div { key: "global-{key}", class: "env-quicklook-row",
                                                            span { class: "env-quicklook-key", title: "{{{key}}}", "{{{{{key}}}}}" }
                                                            span { class: "env-quicklook-val", title: "{val}", "{val}" }
                                                            button {
                                                                class: if is_copied { "env-quicklook-copy-btn copied" } else { "env-quicklook-copy-btn" },
                                                                title: "Copy global value",
                                                                onclick: {
                                                                    let key_c = format!("global_{key}");
                                                                    let val_c = val.clone();
                                                                    move |_| {
                                                                        let script = format!(
                                                                            "navigator.clipboard.writeText({});",
                                                                            serde_json::to_string(&val_c).unwrap_or_else(|_| "\"\"".into())
                                                                        );
                                                                        let k = key_c.clone();
                                                                        spawn(async move {
                                                                            let _ = document::eval(&script).await;
                                                                            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                                                                            if copied_key() == Some(k) {
                                                                                copied_key.set(None);
                                                                            }
                                                                        });
                                                                        copied_key.set(Some(key_c.clone()));
                                                                    }
                                                                },
                                                                if is_copied { "Copied" } else { "Copy" }
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
                    }
                    div { class: "env-quicklook-footer",
                        button {
                            class: "env-quicklook-manage-link",
                            onclick: move |_| {
                                quick_look_open.set(false);
                                on_compare.call(());
                            },
                            span { "⇄ Compare Envs" }
                        }
                        button {
                            class: "env-quicklook-manage-link",
                            onclick: move |_| {
                                quick_look_open.set(false);
                                on_manage.call(());
                            },
                            span { "Manage" }
                            span { "→" }
                        }
                    }
                }
            }

            if menu_open() {
                div { class: "env-menu",
                    div { class: "env-menu-header", "Default Scope" }
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
                        div { class: "env-menu-info",
                            span { class: "env-menu-name", "No Environment" }
                            span { class: "env-menu-sub", "Global & collection variables only" }
                        }
                        if active_id.is_none() {
                            span { class: "env-menu-check", "✓" }
                        }
                    }

                    if !environments.is_empty() {
                        div { class: "env-menu-header", "Environments" }
                        for env in environments.iter() {
                            {
                                let id = env.id;
                                let name = env.name.clone();
                                let tier = env_tier(&name);
                                let var_count = env.values.len();
                                let active = active_id == Some(id);
                                let row_class = if active {
                                    "env-menu-row env-menu-row-active"
                                } else {
                                    "env-menu-row"
                                };
                                rsx! {
                                    button {
                                        key: "{id}",
                                        class: "{row_class}",
                                        title: "Select {name} ({tier})",
                                        onclick: move |_| {
                                            menu_open.set(false);
                                            on_change.call(Some(id));
                                        },
                                        span { class: "env-indicator env-indicator-{tier}", aria_hidden: "true" }
                                        div { class: "env-menu-info",
                                            span { class: "env-menu-name", "{name}" }
                                            span { class: "env-menu-sub", "{var_count} var(s) · {tier}" }
                                        }
                                        if active {
                                            span { class: "env-menu-check", "✓" }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    div { class: "env-menu-divider" }
                    button {
                        class: "env-menu-row env-menu-action-row",
                        onclick: move |_| {
                            menu_open.set(false);
                            on_compare.call(());
                        },
                        span { class: "action-icon action-icon-branch", aria_hidden: "true" }
                        span { class: "env-menu-action-label", "Compare Environments (Diff & Drift)…" }
                    }
                    button {
                        class: "env-menu-row env-menu-action-row",
                        onclick: move |_| {
                            menu_open.set(false);
                            on_manage.call(());
                        },
                        span { class: "action-icon action-icon-edit", aria_hidden: "true" }
                        span { class: "env-menu-action-label", "Manage Environments…" }
                    }
                }
            }
        }
    }
}
