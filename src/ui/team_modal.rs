//! Team Activity & Collaboration Modal.
//! Displays team members (contributors), recent git commits/changes, and local edited files.

use dioxus::prelude::*;
use crate::core::TeamActivity;
use crate::ui::loading::{SkeletonBars, Spinner};
use super::ModalShell;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TeamTab {
    Members,
    RecentChanges,
    EditedFiles,
}

#[component]
pub fn TeamModal(
    activity: TeamActivity,
    #[props(default)] is_syncing: bool,
    on_close: EventHandler<()>,
    on_refresh: EventHandler<()>,
    on_pull: EventHandler<()>,
    on_push: EventHandler<()>,
) -> Element {
    let mut active_tab = use_signal(|| TeamTab::RecentChanges);
    let mut selected_commit_idx = use_signal(|| None::<usize>);

    let member_count = activity.members.len();
    let commit_count = activity.recent_commits.len();
    let edited_count = activity.uncommitted_changes.len();
    let display_remote_url = crate::core::cloud_sync::mask_remote_url(&activity.remote_url);

    rsx! {
        ModalShell {
            title: "Team Collaboration & Workspace Activity".to_string(),
            on_close: move |_| on_close.call(()),
            class: "team-modal".to_string(),

            div { class: "team-modal-content",
                // Top Meta & Actions Bar
                div { class: "team-meta-bar",
                    div { class: "team-repo-info",
                        span { class: "team-badge", "Git Cloud" }
                        span { class: "team-branch-pill",
                            span { class: "action-icon action-icon-branch", style: "margin-right: 4px;", aria_hidden: "true" }
                            "{activity.branch}"
                        }
                        if !activity.remote_url.is_empty() {
                            span { class: "team-remote-url", title: "{display_remote_url}", "{display_remote_url}" }
                        }
                    }
                    div { class: "team-actions-group",
                        button {
                            class: "btn btn-ghost btn-sm",
                            disabled: is_syncing,
                            title: "Refresh team activity from Git",
                            onclick: move |_| on_refresh.call(()),
                            if is_syncing {
                                Spinner { class: "spinner-sm".to_string() }
                                span { style: "margin-left: 6px;", "Syncing…" }
                            } else {
                                "↻ Refresh"
                            }
                        }
                        button {
                            class: "btn btn-ghost btn-sm",
                            disabled: is_syncing,
                            title: "Pull latest team changes (git pull --rebase)",
                            onclick: move |_| on_pull.call(()),
                            "↓ Pull"
                        }
                        button {
                            class: "btn btn-send btn-sm",
                            disabled: is_syncing,
                            title: "Push local changes to team (git push)",
                            onclick: move |_| on_push.call(()),
                            "↑ Push"
                        }
                    }
                }

                if is_syncing {
                    div { class: "sync-progress-bar" }
                }

                // Subnav Tabs
                div { class: "team-tabs-bar",
                    button {
                        class: if active_tab() == TeamTab::RecentChanges { "team-tab active" } else { "team-tab" },
                        onclick: move |_| active_tab.set(TeamTab::RecentChanges),
                        "Recent Changes ({commit_count})"
                    }
                    button {
                        class: if active_tab() == TeamTab::EditedFiles { "team-tab active" } else { "team-tab" },
                        onclick: move |_| active_tab.set(TeamTab::EditedFiles),
                        "Local Edited Files ({edited_count})"
                    }
                    button {
                        class: if active_tab() == TeamTab::Members { "team-tab active" } else { "team-tab" },
                        onclick: move |_| active_tab.set(TeamTab::Members),
                        "Team Members ({member_count})"
                    }
                }

                // Tab Panels
                div { class: "team-tab-body",
                    match active_tab() {
                        TeamTab::RecentChanges => rsx! {
                            if activity.recent_commits.is_empty() {
                                if is_syncing {
                                    div { class: "team-loading-state",
                                        style: "display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; padding: 48px 16px;",
                                        Spinner { class: "spinner-lg".to_string() }
                                        p { class: "hint", "Fetching team commits from Git Cloud…" }
                                        SkeletonBars { n: 5 }
                                    }
                                } else {
                                    div { class: "team-empty",
                                        p { "No recent commits found in this repository branch." }
                                        p { class: "hint", "Commits and team history will appear here once saved or synced." }
                                    }
                                }
                            } else {
                                div { class: "team-commits-split",
                                    div { class: "team-commits-list",
                                        for (idx, commit) in activity.recent_commits.iter().enumerate() {
                                            div {
                                                key: "{commit.hash}",
                                                class: if selected_commit_idx() == Some(idx) { "commit-row selected" } else { "commit-row" },
                                                onclick: move |_| selected_commit_idx.set(Some(idx)),
                                                div { class: "commit-header",
                                                    span { class: "commit-hash", "{commit.hash}" }
                                                    span { class: "commit-author", "{commit.author_name}" }
                                                    span { class: "commit-date", "{commit.date}" }
                                                }
                                                div { class: "commit-msg", "{commit.message}" }
                                                if !commit.files.is_empty() {
                                                    div { class: "commit-files-count", "{commit.files.len()} file(s) changed" }
                                                }
                                            }
                                        }
                                    }

                                    // Detail Pane for Selected Commit
                                    div { class: "team-commit-detail",
                                        if let Some(idx) = selected_commit_idx() {
                                            if let Some(commit) = activity.recent_commits.get(idx) {
                                                {
                                                    let author_line = format!("👤 {} <{}>", commit.author_name, commit.author_email);
                                                    let files_len = commit.files.len();
                                                    rsx! {
                                                        div { class: "detail-header",
                                                            div { class: "detail-title", "{commit.message}" }
                                                            div { class: "detail-meta",
                                                                span { class: "author-badge", "{author_line}" }
                                                                span { " · " }
                                                                span { "{commit.date}" }
                                                                span { " · " }
                                                                code { "{commit.hash}" }
                                                            }
                                                        }
                                                        div { class: "detail-files-title",
                                                            "Touched Files ({files_len})"
                                                        }
                                                        ul { class: "detail-files-list",
                                                            for file in &commit.files {
                                                                li { key: "{file}",
                                                                    span { class: "file-icon", "📄" }
                                                                    span { class: "file-path", "{file}" }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        } else {
                                            div { class: "detail-empty", "Select a commit on the left to see affected files." }
                                        }
                                    }
                                }
                            }
                        },
                        TeamTab::EditedFiles => rsx! {
                            if activity.uncommitted_changes.is_empty() {
                                div { class: "team-empty",
                                    p { class: "text-emerald-400 font-semibold", "✓ Workspace clean — all local changes committed / synced." }
                                    p { class: "hint", "Any edits to collections, requests, or environments will show up here before committing." }
                                }
                            } else {
                                div { class: "team-files-table-container",
                                    table { class: "kv-table",
                                        thead {
                                            tr {
                                                th { style: "width: 80px;", "Status" }
                                                th { "File Path" }
                                            }
                                        }
                                        tbody {
                                            for file in &activity.uncommitted_changes {
                                                tr { key: "{file.path}",
                                                    td {
                                                        span {
                                                            class: match file.status.as_str() {
                                                                "M" => "status-badge status-m",
                                                                "A" | "??" => "status-badge status-a",
                                                                "D" => "status-badge status-d",
                                                                _ => "status-badge",
                                                            },
                                                            "{file.status}"
                                                        }
                                                    }
                                                    td {
                                                        span { class: "font-mono text-xs", "{file.path}" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        },
                        TeamTab::Members => rsx! {
                            if activity.members.is_empty() {
                                if is_syncing {
                                    div { class: "team-loading-state",
                                        style: "display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; padding: 48px 16px;",
                                        Spinner { class: "spinner-lg".to_string() }
                                        p { class: "hint", "Parsing contributors from Git history…" }
                                        SkeletonBars { n: 4 }
                                    }
                                } else {
                                    div { class: "team-empty",
                                        p { "No contributors discovered in git log yet." }
                                        p { class: "hint", "Authors will appear here automatically as commits are authored in this repository." }
                                    }
                                }
                            } else {
                                div { class: "team-members-grid",
                                    for member in &activity.members {
                                        div { key: "{member.email}", class: "team-member-card",
                                            div { class: "member-avatar",
                                                "{member.name.chars().next().unwrap_or('?')}"
                                            }
                                            div { class: "member-info",
                                                div { class: "member-name", "{member.name}" }
                                                div { class: "member-email", "{member.email}" }
                                                div { class: "member-stats",
                                                    span { class: "member-commits",
                                                        span { class: "action-icon action-icon-commit", style: "margin-right: 4px;", aria_hidden: "true" }
                                                        "{member.commit_count} commits"
                                                    }
                                                    span { class: "member-active", "Active {member.last_active}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        },
                    }
                }
            }
        }
    }
}
