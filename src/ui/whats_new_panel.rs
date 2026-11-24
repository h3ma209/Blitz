//! Post-update "What's New" stage tab showing release highlights, version history, and quick actions.

use dioxus::prelude::*;

use crate::core::{
    fetch_github_release_notes, open_browser_url, release_notes, RemoteReleaseNote,
    CURRENT_VERSION,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WhatsNewAction {
    Close,
    OpenSettings,
    OpenEnvironments,
    OpenRunner,
    OpenLint,
}

#[component]
pub fn WhatsNewPanel(
    upgraded_from: Option<String>,
    on_action: EventHandler<WhatsNewAction>,
) -> Element {
    let notes = release_notes();
    let mut selected_idx = use_signal(|| 0usize);
    let mut remote_notes = use_signal(|| None::<Vec<RemoteReleaseNote>>);
    let mut remote_loading = use_signal(|| false);
    let mut remote_err = use_signal(|| None::<String>);
    let mut show_github_tab = use_signal(|| false);

    let active_entry = notes.get(selected_idx()).or_else(|| notes.first());

    let fetch_remote = move |_| {
        if remote_loading() {
            return;
        }
        remote_loading.set(true);
        remote_err.set(None);
        spawn(async move {
            let res = tokio::task::spawn_blocking(|| fetch_github_release_notes(10)).await;
            remote_loading.set(false);
            match res {
                Ok(Ok(list)) => {
                    remote_notes.set(Some(list));
                    show_github_tab.set(true);
                }
                Ok(Err(e)) => remote_err.set(Some(e)),
                Err(e) => remote_err.set(Some(format!("Task error: {e}"))),
            }
        });
    };

    rsx! {
        div {
            class: "stage-page whats-new-stage-page",
            div { class: "stage-page-head",
                div { class: "whats-new-title-row",
                    h2 { "What's New in Blitz" }
                    span { class: "badge whats-new-ver-badge", "v{CURRENT_VERSION}" }
                    if let Some(prev) = upgraded_from.as_ref().filter(|s| !s.is_empty()) {
                        span { class: "badge whats-new-upgrade-badge", "Updated from v{prev}" }
                    }
                }
                div { class: "modal-actions",
                    button {
                        class: "btn btn-sm",
                        disabled: remote_loading(),
                        onclick: fetch_remote,
                        if remote_loading() { "Loading GitHub…" } else { "Load GitHub Releases" }
                    }
                    button {
                        class: "btn btn-sm",
                        onclick: move |_| {
                            open_browser_url("https://github.com/h3ma209/Blitz/releases");
                        },
                        "GitHub Releases ↗"
                    }
                    button {
                        class: "btn btn-sm btn-primary",
                        onclick: move |_| on_action.call(WhatsNewAction::Close),
                        "Done"
                    }
                }
            }

            div { class: "stage-page-body whats-new-body",
                div { class: "whats-new-layout",
                    aside { class: "whats-new-sidebar",
                        div { class: "whats-new-sidebar-label", "Release History" }
                        for (idx, entry) in notes.iter().enumerate() {
                            button {
                                key: "{entry.version}",
                                class: if !show_github_tab() && selected_idx() == idx {
                                    "whats-new-version-btn active"
                                } else {
                                    "whats-new-version-btn"
                                },
                                onclick: move |_| {
                                    show_github_tab.set(false);
                                    selected_idx.set(idx);
                                },
                                div { class: "whats-new-version-top",
                                    span { class: "whats-new-version-num", "v{entry.version}" }
                                    if entry.version == CURRENT_VERSION {
                                        span { class: "whats-new-current-pill", "Current" }
                                    }
                                }
                                div { class: "whats-new-version-sub", "{entry.headline}" }
                            }
                        }
                        if let Some(list) = remote_notes() {
                            div { class: "whats-new-sidebar-label", style: "margin-top:10px;", "GitHub Feed" }
                            button {
                                class: if show_github_tab() { "whats-new-version-btn active" } else { "whats-new-version-btn" },
                                onclick: move |_| show_github_tab.set(true),
                                div { class: "whats-new-version-top",
                                    span { class: "whats-new-version-num", "All Releases ({list.len()})" }
                                }
                                div { class: "whats-new-version-sub", "Live from GitHub Releases" }
                            }
                        }
                        if let Some(err) = remote_err() {
                            div { class: "whats-new-err", "{err}" }
                        }
                    }

                    main { class: "whats-new-detail",
                        if show_github_tab() {
                            if let Some(list) = remote_notes() {
                                for rel in list {
                                    div { key: "{rel.tag}", class: "whats-new-card",
                                        div { class: "whats-new-card-head",
                                            h3 { "{rel.name}" }
                                            span { class: "badge", "{rel.tag}" }
                                        }
                                        if !rel.published_at.is_empty() {
                                            div { class: "whats-new-date", "Published: {rel.published_at}" }
                                        }
                                        pre { class: "whats-new-remote-pre", "{rel.body}" }
                                    }
                                }
                            }
                        } else if let Some(entry) = active_entry {
                            div { class: "whats-new-hero",
                                div { class: "whats-new-hero-meta",
                                    span { class: "badge whats-new-ver-badge", "v{entry.version}" }
                                    span { class: "whats-new-date", "{entry.date}" }
                                }
                                h3 { class: "whats-new-headline", "{entry.headline}" }
                                p { class: "whats-new-summary", "{entry.summary}" }
                                div { class: "whats-new-quick-links",
                                    button {
                                        class: "btn btn-sm",
                                        onclick: move |_| on_action.call(WhatsNewAction::OpenSettings),
                                        "Open Settings"
                                    }
                                    button {
                                        class: "btn btn-sm",
                                        onclick: move |_| on_action.call(WhatsNewAction::OpenEnvironments),
                                        "Open Environments"
                                    }
                                    button {
                                        class: "btn btn-sm",
                                        onclick: move |_| on_action.call(WhatsNewAction::OpenRunner),
                                        "Open Runner"
                                    }
                                    button {
                                        class: "btn btn-sm",
                                        onclick: move |_| on_action.call(WhatsNewAction::OpenLint),
                                        "Open Lint"
                                    }
                                }
                            }

                            for sec in entry.sections {
                                div { key: "{sec.title}", class: "whats-new-card",
                                    h4 { class: "whats-new-sec-title", "{sec.title}" }
                                    ul { class: "whats-new-list",
                                        for item in sec.items {
                                            li { key: "{item}", "{item}" }
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
