use dioxus::prelude::*;
use std::path::PathBuf;

use crate::core::{get_conflict_file_versions, resolve_git_conflicts};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictState {
    pub files: Vec<String>,
    pub selected_idx: usize,
}

#[component]
pub fn ConflictResolver(
    root: PathBuf,
    conflict: ConflictState,
    on_resolved: EventHandler<()>,
    on_close: EventHandler<()>,
) -> Element {
    let mut selected_idx = use_signal(|| conflict.selected_idx);
    let mut status_msg = use_signal(String::new);

    let clamped_idx = selected_idx().min(conflict.files.len().saturating_sub(1));
    let active_file = conflict.files.get(clamped_idx).cloned().unwrap_or_default();
    let (ours, theirs) = if !active_file.is_empty() {
        get_conflict_file_versions(&root, &active_file)
    } else {
        (String::new(), String::new())
    };

    let files_len = conflict.files.len();

    rsx! {
        div { class: "conflict-resolver-pane", style: "display: flex; flex-direction: column; height: 100%; width: 100%; background: var(--bg-surface); overflow: hidden;",
            // Header
            div { class: "col-editor-header", style: "border-bottom: var(--glass-border); padding: 12px 20px; display: flex; justify-content: space-between; align-items: center; background: var(--bg-surface-elevated);",
                div {
                    h3 { style: "margin: 0; font-size: 15px; font-weight: 700; display: flex; align-items: center; gap: 8px;",
                        span { "⚔️ Git Merge Conflict Resolver" }
                        span { class: "status-chip status-err", style: "font-size: 11px;", "{files_len} conflicting files" }
                    }
                    p { class: "hint", style: "margin: 3px 0 0;",
                        "Review differences between your workstation and team commits. Resolve per file or accept all."
                    }
                }
                div { style: "display: flex; gap: 8px; align-items: center;",
                    button {
                        class: "btn btn-ghost",
                        title: "Keep all local workstation changes and overwrite incoming remote",
                        onclick: {
                            let root = root.clone();
                            let files = conflict.files.clone();
                            move |_| {
                                let _ = resolve_git_conflicts(&root, &files, true);
                                on_resolved.call(());
                            }
                        },
                        "Keep All Local (Mine)"
                    }
                    button {
                        class: "btn btn-primary",
                        title: "Accept all incoming team changes from git remote",
                        onclick: {
                            let root = root.clone();
                            let files = conflict.files.clone();
                            move |_| {
                                let _ = resolve_git_conflicts(&root, &files, false);
                                on_resolved.call(());
                            }
                        },
                        "Accept All Remote (Team)"
                    }
                    button {
                        class: "btn btn-ghost",
                        style: "font-size: 14px; padding: 4px 10px;",
                        onclick: move |_| on_close.call(()),
                        "✕"
                    }
                }
            }

            // File selection tabs / strip
            if files_len > 1 {
                div { class: "conflict-file-tabs", style: "display: flex; gap: 4px; padding: 6px 16px; background: rgba(0,0,0,0.2); border-bottom: var(--glass-border); overflow-x: auto;",
                    for (i, file) in conflict.files.iter().enumerate() {
                        {
                            let is_active = i == clamped_idx;
                            let file_name = file.clone();
                            rsx! {
                                button {
                                    class: if is_active { "builder-tab active" } else { "builder-tab" },
                                    style: "font-size: 12px; padding: 4px 12px; border-radius: 4px;",
                                    onclick: move |_| selected_idx.set(i),
                                    "📄 {file_name}"
                                }
                            }
                        }
                    }
                }
            }

            // Active file action bar
            div { style: "padding: 8px 20px; display: flex; justify-content: space-between; align-items: center; background: var(--bg-surface); border-bottom: var(--glass-border); font-size: 12px;",
                span { class: "hint",
                    "Active File: "
                    code { style: "color: var(--text-primary); font-weight: 600;", "{active_file}" }
                }
                div { style: "display: flex; gap: 8px;",
                    button {
                        class: "btn btn-ghost",
                        style: "font-size: 11px; padding: 3px 10px;",
                        onclick: {
                            let root = root.clone();
                            let file = active_file.clone();
                            move |_| {
                                if !file.is_empty() {
                                    let _ = resolve_git_conflicts(&root, std::slice::from_ref(&file), true);
                                    status_msg.set(format!("Resolved '{file}' with local version"));
                                    on_resolved.call(());
                                }
                            }
                        },
                        "Keep Mine for this file"
                    }
                    button {
                        class: "btn btn-ghost",
                        style: "font-size: 11px; padding: 3px 10px;",
                        onclick: {
                            let root = root.clone();
                            let file = active_file.clone();
                            move |_| {
                                if !file.is_empty() {
                                    let _ = resolve_git_conflicts(&root, std::slice::from_ref(&file), false);
                                    status_msg.set(format!("Resolved '{file}' with team version"));
                                    on_resolved.call(());
                                }
                            }
                        },
                        "Accept Team for this file"
                    }
                }
            }

            // Side-by-side diff comparison
            div { style: "flex: 1; display: grid; grid-template-columns: 1fr 1fr; gap: 1px; background: var(--border-color); overflow: hidden;",
                // Local (Ours) Column
                div { style: "display: flex; flex-direction: column; background: var(--bg-surface); height: 100%; overflow: hidden;",
                    div { style: "padding: 8px 16px; background: rgba(30, 215, 96, 0.08); border-bottom: var(--glass-border); font-size: 12px; font-weight: 700; color: #1ed760; display: flex; justify-content: space-between;",
                        span { "Your Version (Local / HEAD)" }
                        span { class: "hint", "{ours.lines().count()} lines" }
                    }
                    pre {
                        style: "flex: 1; margin: 0; padding: 12px 16px; overflow: auto; font-family: var(--mono); font-size: 12px; line-height: 1.5; color: var(--text-primary); background: transparent;",
                        if ours.is_empty() {
                            "(empty or deleted locally)"
                        } else {
                            "{ours}"
                        }
                    }
                }

                // Remote (Theirs) Column
                div { style: "display: flex; flex-direction: column; background: var(--bg-surface); height: 100%; overflow: hidden;",
                    div { style: "padding: 8px 16px; background: rgba(59, 130, 246, 0.08); border-bottom: var(--glass-border); font-size: 12px; font-weight: 700; color: #60a5fa; display: flex; justify-content: space-between;",
                        span { "Incoming Team Version (Remote / MERGE_HEAD)" }
                        span { class: "hint", "{theirs.lines().count()} lines" }
                    }
                    pre {
                        style: "flex: 1; margin: 0; padding: 12px 16px; overflow: auto; font-family: var(--mono); font-size: 12px; line-height: 1.5; color: var(--text-primary); background: transparent;",
                        if theirs.is_empty() {
                            "(empty or deleted remotely)"
                        } else {
                            "{theirs}"
                        }
                    }
                }
            }
        }
    }
}
