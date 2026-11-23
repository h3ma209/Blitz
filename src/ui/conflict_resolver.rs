use dioxus::prelude::*;
use std::path::PathBuf;

use crate::core::{abort_git_rebase, get_conflict_file_versions, resolve_git_conflicts};
use super::icons::ActionIcon;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictState {
    pub clone_path: PathBuf,
    pub files: Vec<String>,
    pub selected_idx: usize,
}

#[component]
pub fn ConflictResolver(
    conflict: ConflictState,
    on_resolved: EventHandler<()>,
    on_close: EventHandler<()>,
) -> Element {
    let root = conflict.clone_path.clone();
    let mut selected_idx = use_signal(|| conflict.selected_idx);
    let mut status_msg = use_signal(String::new);
    let mut busy = use_signal(|| false);

    let clamped_idx = selected_idx().min(conflict.files.len().saturating_sub(1));
    let active_file = conflict.files.get(clamped_idx).cloned().unwrap_or_default();

    let versions_res = use_resource(use_reactive((&root, &active_file), |(r, f)| async move {
        if f.is_empty() {
            return (String::new(), String::new());
        }
        tokio::task::spawn_blocking(move || get_conflict_file_versions(&r, &f))
            .await
            .unwrap_or_default()
    }));
    let (ours, theirs) = versions_res.read().clone().unwrap_or_default();

    let files_len = conflict.files.len();

    rsx! {
        div { class: "conflict-resolver-pane",
            // Header
            div { class: "col-editor-header", style: "border-bottom: var(--glass-border); padding: 12px 20px; display: flex; justify-content: space-between; align-items: center; background: var(--bg-surface-elevated);",
                div {
                    h3 { style: "margin: 0; font-size: 15px; font-weight: 700; display: flex; align-items: center; gap: 8px;",
                        ActionIcon { name: "git-merge", size: 17 }
                        span { "Git Merge Conflict Resolver" }
                        span { class: "status-chip status-err", style: "font-size: 11px;", "{files_len} conflicting files" }
                    }
                    p { class: "hint", style: "margin: 3px 0 0;",
                        "Review differences between your local changes and incoming team commits, resolve per file, or abort the rebase."
                    }
                }
                div { style: "display: flex; gap: 8px; align-items: center;",
                    button {
                        class: "btn btn-ghost",
                        disabled: busy(),
                        title: "Abort in-progress rebase/merge and restore clean branch state",
                        onclick: {
                            let root = root.clone();
                            move |_| {
                                if busy() {
                                    return;
                                }
                                busy.set(true);
                                status_msg.set("Aborting rebase…".into());
                                let r = root.clone();
                                spawn(async move {
                                    let res = tokio::task::spawn_blocking(move || abort_git_rebase(&r)).await;
                                    busy.set(false);
                                    match res {
                                        Ok(Ok(())) => {
                                            status_msg.set("Rebase aborted cleanly".into());
                                            on_resolved.call(());
                                        }
                                        Ok(Err(e)) => status_msg.set(format!("abort error: {e}")),
                                        Err(e) => status_msg.set(format!("abort join error: {e}")),
                                    }
                                });
                            }
                        },
                        "Abort Rebase"
                    }
                    button {
                        class: "btn btn-ghost",
                        disabled: busy(),
                        title: "Keep all local workstation changes and overwrite incoming remote",
                        onclick: {
                            let root = root.clone();
                            let files = conflict.files.clone();
                            move |_| {
                                if busy() {
                                    return;
                                }
                                busy.set(true);
                                status_msg.set("Resolving keeping local versions…".into());
                                let r = root.clone();
                                let fs = files.clone();
                                spawn(async move {
                                    let res = tokio::task::spawn_blocking(move || resolve_git_conflicts(&r, &fs, true)).await;
                                    busy.set(false);
                                    match res {
                                        Ok(Ok(_)) => on_resolved.call(()),
                                        Ok(Err(e)) => status_msg.set(format!("resolve error: {e}")),
                                        Err(e) => status_msg.set(format!("resolve join error: {e}")),
                                    }
                                });
                            }
                        },
                        "Keep All Local (Mine)"
                    }
                    button {
                        class: "btn btn-primary",
                        disabled: busy(),
                        title: "Accept all incoming team changes from git remote",
                        onclick: {
                            let root = root.clone();
                            let files = conflict.files.clone();
                            move |_| {
                                if busy() {
                                    return;
                                }
                                busy.set(true);
                                status_msg.set("Resolving accepting team versions…".into());
                                let r = root.clone();
                                let fs = files.clone();
                                spawn(async move {
                                    let res = tokio::task::spawn_blocking(move || resolve_git_conflicts(&r, &fs, false)).await;
                                    busy.set(false);
                                    match res {
                                        Ok(Ok(_)) => on_resolved.call(()),
                                        Ok(Err(e)) => status_msg.set(format!("resolve error: {e}")),
                                        Err(e) => status_msg.set(format!("resolve join error: {e}")),
                                    }
                                });
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

            if !status_msg().is_empty() {
                div { class: "hint", style: "padding: 6px 20px; border-bottom: var(--glass-border);", "{status_msg()}" }
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
                                    key: "{file_name}-{i}",
                                    class: if is_active { "builder-tab active" } else { "builder-tab" },
                                    r#type: "button",
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
                        disabled: busy() || active_file.is_empty(),
                        style: "font-size: 11px; padding: 3px 10px;",
                        onclick: {
                            let root = root.clone();
                            let file = active_file.clone();
                            move |_| {
                                if busy() || file.is_empty() {
                                    return;
                                }
                                busy.set(true);
                                let r = root.clone();
                                let f = file.clone();
                                let f_label = file.clone();
                                spawn(async move {
                                    let res = tokio::task::spawn_blocking(move || resolve_git_conflicts(&r, std::slice::from_ref(&f), true)).await;
                                    busy.set(false);
                                    match res {
                                        Ok(Ok(_)) => {
                                            status_msg.set(format!("Resolved '{f_label}' with local version"));
                                            on_resolved.call(());
                                        }
                                        Ok(Err(e)) => status_msg.set(format!("resolve error: {e}")),
                                        Err(e) => status_msg.set(format!("resolve join error: {e}")),
                                    }
                                });
                            }
                        },
                        "Keep Mine for this file"
                    }
                    button {
                        class: "btn btn-ghost",
                        disabled: busy() || active_file.is_empty(),
                        style: "font-size: 11px; padding: 3px 10px;",
                        onclick: {
                            let root = root.clone();
                            let file = active_file.clone();
                            move |_| {
                                if busy() || file.is_empty() {
                                    return;
                                }
                                busy.set(true);
                                let r = root.clone();
                                let f = file.clone();
                                let f_label = file.clone();
                                spawn(async move {
                                    let res = tokio::task::spawn_blocking(move || resolve_git_conflicts(&r, std::slice::from_ref(&f), false)).await;
                                    busy.set(false);
                                    match res {
                                        Ok(Ok(_)) => {
                                            status_msg.set(format!("Resolved '{f_label}' with team version"));
                                            on_resolved.call(());
                                        }
                                        Ok(Err(e)) => status_msg.set(format!("resolve error: {e}")),
                                        Err(e) => status_msg.set(format!("resolve join error: {e}")),
                                    }
                                });
                            }
                        },
                        "Accept Team for this file"
                    }
                }
            }

            // Side-by-side diff comparison
            div { style: "flex: 1; display: grid; grid-template-columns: 1fr 1fr; gap: 1px; background: var(--border-color); overflow: hidden;",
                // Local Column
                div { style: "display: flex; flex-direction: column; background: var(--bg-surface); height: 100%; overflow: hidden;",
                    div { style: "padding: 8px 16px; background: rgba(30, 215, 96, 0.08); border-bottom: var(--glass-border); font-size: 12px; font-weight: 700; color: #1ed760; display: flex; justify-content: space-between;",
                        span { "Your Local Changes" }
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

                // Remote Column
                div { style: "display: flex; flex-direction: column; background: var(--bg-surface); height: 100%; overflow: hidden;",
                    div { style: "padding: 8px 16px; background: rgba(59, 130, 246, 0.08); border-bottom: var(--glass-border); font-size: 12px; font-weight: 700; color: #60a5fa; display: flex; justify-content: space-between;",
                        span { "Incoming Team (Remote)" }
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
