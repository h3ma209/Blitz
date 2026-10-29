use dioxus::prelude::*;
use uuid::Uuid;

use crate::core::{
    find_request_across, Collection, CollectionItem, HttpMethod,
    UiTheme,
};
use crate::ui::workbench::cloud::switch_to_env;
use crate::ui::workbench::method_class;
use crate::ui::workbench::state::WorkbenchState;
use crate::ui::ModalShell;

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteAction {
    SendRequest,
    SaveRequest,
    NewTab,
    OpenRunner,
    ToggleConsole,
    OpenVariables,
    OpenSettings,
    OpenCookies,
    OpenPerformance,
    OpenTrash,
    OpenSnippets,
    SwitchTheme(UiTheme),
    SelectEnvironment(Option<Uuid>),
    OpenRequest {
        collection_id: Uuid,
        request_id: Uuid,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaletteEntry {
    pub category: &'static str,
    pub title: String,
    pub subtitle: String,
    pub shortcut: Option<&'static str>,
    pub method_badge: Option<HttpMethod>,
    pub action: PaletteAction,
}

/// Fast subsequence fuzzy matcher with zero-allocation ASCII fast path.
/// Returns `Some(score)` when every char of `query` appears in `hay` in order (case-insensitive).
/// Higher scores indicate tighter, earlier, or word-boundary matches.
pub fn fuzzy_score(query: &str, hay: &str) -> Option<i32> {
    let q_trimmed = query.trim();
    if q_trimmed.is_empty() {
        return Some(0);
    }

    // Zero-allocation fast path for ASCII (majority of commands & requests)
    if q_trimmed.is_ascii() && hay.is_ascii() {
        return fuzzy_score_ascii(q_trimmed.as_bytes(), hay.as_bytes());
    }

    fuzzy_score_unicode(q_trimmed, hay)
}

fn fuzzy_score_ascii(q: &[u8], h: &[u8]) -> Option<i32> {
    if q.is_empty() {
        return Some(0);
    }
    if h.len() < q.len() {
        return None;
    }

    let mut score = 0i32;
    let mut qi = 0usize;
    let mut prev_match_idx: Option<usize> = None;

    for (hi, &hb) in h.iter().enumerate() {
        let hc = hb.to_ascii_lowercase();
        let qc = q[qi].to_ascii_lowercase();
        if hc == qc {
            let mut char_score = 10;
            if hi == 0 {
                char_score += 25;
            } else {
                let prev = h[hi - 1];
                if !prev.is_ascii_alphanumeric() {
                    char_score += 18;
                } else if prev.is_ascii_lowercase() && hb.is_ascii_uppercase() {
                    char_score += 14;
                }
            }

            if let Some(prev_idx) = prev_match_idx {
                if hi == prev_idx + 1 {
                    char_score += 20;
                } else {
                    let gap = (hi - prev_idx - 1) as i32;
                    char_score -= gap.min(8);
                }
            } else {
                char_score -= (hi as i32).min(15);
            }

            score += char_score;
            prev_match_idx = Some(hi);
            qi += 1;
            if qi == q.len() {
                return Some(score);
            }
        }
    }

    None
}

fn fuzzy_score_unicode(query: &str, hay: &str) -> Option<i32> {
    let q_chars: Vec<char> = query.chars().flat_map(|c| c.to_lowercase()).collect();
    let h_chars: Vec<char> = hay.chars().collect();
    let h_lower: Vec<char> = hay.chars().flat_map(|c| c.to_lowercase()).collect();

    if q_chars.is_empty() {
        return Some(0);
    }
    if h_lower.len() < q_chars.len() {
        return None;
    }

    let mut score: i32 = 0;
    let mut qi = 0usize;
    let mut prev_match_idx: Option<usize> = None;

    for (hi, &hc) in h_lower.iter().enumerate() {
        if qi < q_chars.len() && hc == q_chars[qi] {
            let mut char_score = 10;

            if hi == 0 {
                char_score += 25;
            } else {
                let prev = h_chars.get(hi - 1).copied().unwrap_or(' ');
                if !prev.is_alphanumeric() {
                    char_score += 18;
                } else if prev.is_lowercase()
                    && h_chars.get(hi).map(|c| c.is_uppercase()).unwrap_or(false)
                {
                    char_score += 14;
                }
            }

            if let Some(prev_idx) = prev_match_idx {
                if hi == prev_idx + 1 {
                    char_score += 20;
                } else {
                    let gap = (hi - prev_idx - 1) as i32;
                    char_score -= gap.min(8);
                }
            } else {
                char_score -= (hi as i32).min(15);
            }

            score += char_score;
            prev_match_idx = Some(hi);
            qi += 1;
        }
    }

    if qi == q_chars.len() {
        Some(score)
    } else {
        None
    }
}

/// O(N) single-pass request collection without cloning RequestDraft bodies or repeated tree searches.
fn collect_requests(cols: &[Collection]) -> Vec<(Uuid, String, Uuid, String, String, HttpMethod)> {
    fn walk_items(
        col_id: Uuid,
        col_name: &str,
        items: &[CollectionItem],
        cur_folder: &str,
        out: &mut Vec<(Uuid, String, Uuid, String, String, HttpMethod)>,
    ) {
        for item in items {
            match item {
                CollectionItem::Request(r) => {
                    let path = if cur_folder.is_empty() {
                        col_name.to_string()
                    } else {
                        format!("{col_name} / {cur_folder}")
                    };
                    out.push((col_id, path, r.id, r.name.clone(), r.url.clone(), r.method));
                }
                CollectionItem::Folder(f) => {
                    let next = if cur_folder.is_empty() {
                        f.name.clone()
                    } else {
                        format!("{cur_folder} / {}", f.name)
                    };
                    walk_items(col_id, col_name, &f.items, &next, out);
                }
            }
        }
    }

    let mut out = Vec::new();
    for col in cols {
        walk_items(col.id, &col.name, &col.items, "", &mut out);
    }
    out
}

fn build_entries(st: &WorkbenchState) -> Vec<PaletteEntry> {
    // 1. Core Actions
    let mut entries = vec![
        PaletteEntry {
            category: "Action",
            title: "Send Request".into(),
            subtitle: "Execute active request in builder".into(),
            shortcut: Some("⌘↵"),
            method_badge: None,
            action: PaletteAction::SendRequest,
        },
        PaletteEntry {
            category: "Action",
            title: "Save Request to Collection".into(),
            subtitle: "Persist current request draft".into(),
            shortcut: Some("⌘S"),
            method_badge: None,
            action: PaletteAction::SaveRequest,
        },
        PaletteEntry {
            category: "Action",
            title: "New Request Tab".into(),
            subtitle: "Open a blank request tab".into(),
            shortcut: Some("⌘T"),
            method_badge: None,
            action: PaletteAction::NewTab,
        },
        PaletteEntry {
            category: "Action",
            title: "Open Collection Runner".into(),
            subtitle: "Run collection sequence with iterations & data".into(),
            shortcut: None,
            method_badge: None,
            action: PaletteAction::OpenRunner,
        },
        PaletteEntry {
            category: "Action",
            title: "Toggle Console".into(),
            subtitle: "Show or hide bottom execution console".into(),
            shortcut: None,
            method_badge: None,
            action: PaletteAction::ToggleConsole,
        },
        PaletteEntry {
            category: "Action",
            title: "Manage Environments & Variables".into(),
            subtitle: "Edit environment, collection, global, and vault variables".into(),
            shortcut: None,
            method_badge: None,
            action: PaletteAction::OpenVariables,
        },
        PaletteEntry {
            category: "Action",
            title: "Open Settings & Preferences".into(),
            subtitle: "Configure theme, layout, timeouts, proxy, certificates, and cloud sync".into(),
            shortcut: None,
            method_badge: None,
            action: PaletteAction::OpenSettings,
        },
        PaletteEntry {
            category: "Action",
            title: "Generate Code Snippet".into(),
            subtitle: "Export current request to cURL, Rust, Python, JS, Go…".into(),
            shortcut: None,
            method_badge: None,
            action: PaletteAction::OpenSnippets,
        },
        PaletteEntry {
            category: "Action",
            title: "Open Cookie Jar".into(),
            subtitle: "Inspect or clear stored HTTP cookies".into(),
            shortcut: None,
            method_badge: None,
            action: PaletteAction::OpenCookies,
        },
        PaletteEntry {
            category: "Action",
            title: "Open Performance / Load Tester".into(),
            subtitle: "Benchmark endpoint latency & throughput with virtual users".into(),
            shortcut: None,
            method_badge: None,
            action: PaletteAction::OpenPerformance,
        },
        PaletteEntry {
            category: "Action",
            title: "Open Trash".into(),
            subtitle: "Restore or permanently purge deleted collections".into(),
            shortcut: None,
            method_badge: None,
            action: PaletteAction::OpenTrash,
        },
    ];

    // 2. Requests across all collections (zero clone of body/params/scripts)
    for (col_id, folder_path, req_id, req_name, req_url, req_method) in
        collect_requests(&(st.collections)())
    {
        let subtitle = if req_url.is_empty() {
            folder_path
        } else {
            format!("{folder_path} · {req_url}")
        };
        entries.push(PaletteEntry {
            category: "Request",
            title: req_name,
            subtitle,
            shortcut: None,
            method_badge: Some(req_method),
            action: PaletteAction::OpenRequest {
                collection_id: col_id,
                request_id: req_id,
            },
        });
    }

    // 3. Environments
    let active_env = (st.active_env_id)();
    for env in (st.environments)() {
        let is_active = active_env == Some(env.id);
        entries.push(PaletteEntry {
            category: "Environment",
            title: format!("Switch Environment: {}", env.name),
            subtitle: if is_active {
                format!("Active · {} variable(s)", env.values.len())
            } else {
                format!("{} variable(s)", env.values.len())
            },
            shortcut: None,
            method_badge: None,
            action: PaletteAction::SelectEnvironment(Some(env.id)),
        });
    }
    if active_env.is_some() {
        entries.push(PaletteEntry {
            category: "Environment",
            title: "Clear Active Environment (No Environment)".into(),
            subtitle: "Deactivate environment variables".into(),
            shortcut: None,
            method_badge: None,
            action: PaletteAction::SelectEnvironment(None),
        });
    }

    // 4. Themes
    for &theme in UiTheme::all() {
        entries.push(PaletteEntry {
            category: "Theme",
            title: format!("Theme: {}", theme.label()),
            subtitle: "Switch workbench appearance".into(),
            shortcut: None,
            method_badge: None,
            action: PaletteAction::SwitchTheme(theme),
        });
    }

    entries
}

/// Restore window and keyboard focus to the main workbench when palette closes.
pub fn restore_desktop_focus() {
    spawn(async move {
        let script = r#"
            window.focus();
            setTimeout(() => {
                let target = window.__blitz_last_focused;
                if (!target || !document.contains(target) || target.classList.contains('palette-search-input')) {
                    target = document.getElementById('np-url-input') || document.querySelector('.url-input');
                }
                if (target && typeof target.focus === 'function') {
                    target.focus();
                    if (target.id === 'np-url-input' && typeof target.select === 'function') {
                        target.select();
                    }
                } else {
                    const shell = document.querySelector('.app-shell') || document.body;
                    if (shell) {
                        if (!shell.hasAttribute('tabindex')) shell.setAttribute('tabindex', '-1');
                        shell.focus();
                    }
                }
            }, 10);
        "#;
        let _ = document::eval(script).await;
    });
}

#[component]
pub fn CommandPalette(
    st: WorkbenchState,
    on_send: EventHandler<()>,
    on_save: EventHandler<()>,
    on_new_tab: EventHandler<()>,
    on_close: EventHandler<()>,
) -> Element {
    let mut query = use_signal(String::new);
    let mut selected_idx = use_signal(|| 0usize);
    // Cached once on modal open — avoids rebuilding/cloning entries on every keystroke/hover
    let all_entries = use_signal(|| build_entries(&st));

    // Restore desktop & webview focus on unmount regardless of exit path
    use_drop(move || {
        restore_desktop_focus();
    });

    // Ensure autofocus search input reliably on open and record previous focus
    use_effect(move || {
        spawn(async move {
            let script = r#"
                const cur = document.activeElement;
                if (cur && cur !== document.body && !cur.classList.contains('palette-search-input')) {
                    window.__blitz_last_focused = cur;
                }
                window.focus();
                setTimeout(() => {
                    const el = document.querySelector('.palette-search-input');
                    if (el) {
                        el.focus();
                        el.select();
                    }
                }, 10);
            "#;
            let _ = document::eval(script).await;
        });
    });

    let q_raw = query();
    let q = q_raw.trim();
    let entries = all_entries.read();
    let filtered: Vec<PaletteEntry> = if q.is_empty() {
        entries.iter().take(40).cloned().collect()
    } else {
        let mut scored: Vec<(i32, &PaletteEntry)> = entries
            .iter()
            .filter_map(|entry| {
                if let Some(ts) = fuzzy_score(q, &entry.title) {
                    Some((ts + 25, entry))
                } else if let Some(ss) = fuzzy_score(q, &entry.subtitle) {
                    Some((ss + 5, entry))
                } else if let Some(cs) = fuzzy_score(q, entry.category) {
                    Some((cs, entry))
                } else {
                    None
                }
            })
            .collect();
        scored.sort_by_key(|b| std::cmp::Reverse(b.0));
        scored.into_iter().take(40).map(|(_, e)| e.clone()).collect()
    };
    let item_count = filtered.len();

    let cur_sel = if item_count == 0 {
        0
    } else {
        selected_idx().min(item_count - 1)
    };

    let execute_action = move |act: PaletteAction| {
        let mut st = st;
        on_close.call(());
        match act {
            PaletteAction::SendRequest => on_send.call(()),
            PaletteAction::SaveRequest => on_save.call(()),
            PaletteAction::NewTab => on_new_tab.call(()),
            PaletteAction::OpenRunner => st.show_runner.set(true),
            PaletteAction::ToggleConsole => st.console_open.set(!(st.console_open)()),
            PaletteAction::OpenVariables => {
                st.vars_active_tab.set(0);
                st.show_vars.set(true);
            }
            PaletteAction::OpenSettings => st.show_settings.set(true),
            PaletteAction::OpenCookies => st.show_cookies.set(true),
            PaletteAction::OpenPerformance => st.show_perf.set(true),
            PaletteAction::OpenTrash => st.show_trash.set(true),
            PaletteAction::OpenSnippets => st.show_snippets.set(true),
            PaletteAction::SwitchTheme(t) => {
                let mut s = (st.app_settings)();
                s.ui.theme = t;
                s.ui = s.ui.clamped();
                st.app_settings.set(s);
                st.persist();
            }
            PaletteAction::SelectEnvironment(env_id) => {
                switch_to_env(st, env_id);
            }
            PaletteAction::OpenRequest {
                collection_id,
                request_id,
            } => {
                let cols = (st.collections)();
                let Some((_, resolved_req)) = find_request_across(&cols, request_id) else {
                    return;
                };
                let resolved_req = resolved_req.clone();

                let mut tabs = (st.open_tabs)();
                let cur = (st.draft)();
                if tabs.iter().any(|t| t.id == cur.id) {
                    if let Some(slot) = tabs.iter_mut().find(|t| t.id == cur.id) {
                        *slot = cur;
                    }
                }
                if let Some(slot) = tabs.iter_mut().find(|t| t.id == resolved_req.id) {
                    *slot = resolved_req.clone();
                } else {
                    tabs.push(resolved_req.clone());
                }
                st.open_tabs.set(tabs);
                st.draft.set(resolved_req.clone());
                st.active_request_id.set(Some(request_id));
                st.active_collection_id.set(Some(collection_id));
                st.editing_collection_id.set(None);
                st.show_settings.set(false);
                st.show_vars.set(false);
                if let Some(col) = cols.iter().find(|c| c.id == collection_id) {
                    let mut v = (st.vars)();
                    v.collection = col.variables_map();
                    st.vars.set(v);
                }
                let resp_map = (st.responses)();
                if let Some(cached) = resp_map.get(&request_id).cloned() {
                    st.response.set(Some(cached));
                } else {
                    st.response.set(None);
                }
                st.persist_local();
            }
        }
    };

    let filtered_for_key = filtered.clone();

    rsx! {
        ModalShell {
            title: "Command Palette".to_string(),
            class: "palette-modal".to_string(),
            on_close: move |_| on_close.call(()),
            div {
                class: "palette-container",
                onkeydown: move |evt: Event<KeyboardData>| {
                    match evt.key() {
                        Key::ArrowDown => {
                            evt.prevent_default();
                            if item_count > 0 {
                                selected_idx.set((cur_sel + 1) % item_count);
                                spawn(async move {
                                    let _ = document::eval(
                                        "const el = document.querySelector('.palette-item.active'); if (el) el.scrollIntoView({ block: 'nearest' });"
                                    ).await;
                                });
                            }
                        }
                        Key::ArrowUp => {
                            evt.prevent_default();
                            if item_count > 0 {
                                selected_idx.set(if cur_sel == 0 { item_count - 1 } else { cur_sel - 1 });
                                spawn(async move {
                                    let _ = document::eval(
                                        "const el = document.querySelector('.palette-item.active'); if (el) el.scrollIntoView({ block: 'nearest' });"
                                    ).await;
                                });
                            }
                        }
                        Key::Enter => {
                            evt.prevent_default();
                            if let Some(item) = filtered_for_key.get(cur_sel).cloned() {
                                execute_action(item.action);
                            }
                        }
                        Key::Escape => {
                            evt.prevent_default();
                            on_close.call(());
                        }
                        _ => {}
                    }
                },
                div { class: "palette-search-wrap",
                    input {
                        class: "palette-search-input",
                        r#type: "text",
                        placeholder: "Type a command, request name, URL, environment, or theme…",
                        value: "{query()}",
                        oninput: move |evt: Event<FormData>| {
                            query.set(evt.value());
                            selected_idx.set(0);
                        },
                    }
                    button {
                        class: "palette-close-btn",
                        r#type: "button",
                        title: "Close (Esc)",
                        aria_label: "Close command palette",
                        onclick: move |_| on_close.call(()),
                        "✕"
                    }
                }
                div { class: "palette-list",
                    if filtered.is_empty() {
                        p { class: "hint palette-empty", "No matching commands or requests." }
                    } else {
                        for (idx, item) in filtered.into_iter().enumerate() {
                            {
                                let is_sel = idx == cur_sel;
                                let act = item.action.clone();
                                rsx! {
                                    button {
                                        key: "{idx}-{item.title}",
                                        class: if is_sel { "palette-item active" } else { "palette-item" },
                                        onmouseenter: move |_| selected_idx.set(idx),
                                        onclick: move |_| execute_action(act.clone()),
                                        div { class: "palette-item-main",
                                             span { class: "palette-category", "{item.category}" }
                                             if let Some(m) = item.method_badge {
                                                 span {
                                                     class: "method-badge method-{method_class(m)}",
                                                     "{m.as_str()}"
                                                 }
                                             }
                                             span { class: "palette-title", "{item.title}" }
                                         }
                                         div { class: "palette-item-meta",
                                             span { class: "palette-subtitle", "{item.subtitle}" }
                                             if let Some(sc) = item.shortcut {
                                                 kbd { class: "palette-kbd", "{sc}" }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_score_matches_subsequences_and_ranks_tighter_higher() {
        assert_eq!(fuzzy_score("", "Anything"), Some(0));
        assert!(fuzzy_score("xyz", "Get Users").is_none());

        let exact_prefix = fuzzy_score("get", "Get Users").unwrap();
        let scattered = fuzzy_score("get", "Generate Auth Token").unwrap();
        assert!(
            exact_prefix > scattered,
            "prefix match ({exact_prefix}) should beat scattered ({scattered})"
        );
    }

    #[test]
    fn fuzzy_score_handles_unicode_safely() {
        assert!(fuzzy_score("api", "İstanbul API Gateway").is_some());
        assert!(fuzzy_score("blueprint", "Theme: Hardware Blueprint Light").is_some());
    }
}
