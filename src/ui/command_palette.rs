use dioxus::prelude::*;
use uuid::Uuid;

use crate::core::{
    find_request_across, folder_path_to_request, Collection, CollectionItem, HttpMethod,
    RequestDraft, UiTheme,
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
        request: Box<RequestDraft>,
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

/// Fast subsequence fuzzy matcher.
/// Returns `Some(score)` when every char of `query` appears in `hay` in order (case-insensitive).
/// Higher scores indicate tighter, earlier, or word-boundary matches.
pub fn fuzzy_score(query: &str, hay: &str) -> Option<i32> {
    let q_trimmed = query.trim();
    if q_trimmed.is_empty() {
        return Some(0);
    }

    let q_chars: Vec<char> = q_trimmed.chars().flat_map(|c| c.to_lowercase()).collect();
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

            // Bonus for matching at start of string
            if hi == 0 {
                char_score += 25;
            } else {
                // Word boundary bonus
                let prev = h_chars.get(hi - 1).copied().unwrap_or(' ');
                if !prev.is_alphanumeric() {
                    char_score += 18;
                } else if prev.is_lowercase()
                    && h_chars.get(hi).map(|c| c.is_uppercase()).unwrap_or(false)
                {
                    char_score += 14;
                }
            }

            // Consecutive match bonus
            if let Some(prev_idx) = prev_match_idx {
                if hi == prev_idx + 1 {
                    char_score += 20;
                } else {
                    let gap = (hi - prev_idx - 1) as i32;
                    char_score -= gap.min(8);
                }
            } else {
                // Slight penalty for late first match
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

fn collect_requests(cols: &[Collection]) -> Vec<(Uuid, String, RequestDraft)> {
    fn walk_items(
        col: &Collection,
        items: &[CollectionItem],
        out: &mut Vec<(Uuid, String, RequestDraft)>,
    ) {
        for item in items {
            match item {
                CollectionItem::Request(r) => {
                    let folders = folder_path_to_request(col, r.id);
                    let path = if folders.is_empty() {
                        col.name.clone()
                    } else {
                        let f_names: Vec<&str> = folders.iter().map(|f| f.name.as_str()).collect();
                        format!("{} / {}", col.name, f_names.join(" / "))
                    };
                    out.push((col.id, path, r.clone()));
                }
                CollectionItem::Folder(f) => {
                    walk_items(col, &f.items, out);
                }
            }
        }
    }

    let mut out = Vec::new();
    for col in cols {
        walk_items(col, &col.items, &mut out);
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

    // 2. Requests across all collections
    for (col_id, folder_path, req) in collect_requests(&(st.collections)()) {
        let subtitle = if req.url.is_empty() {
            folder_path
        } else {
            format!("{folder_path} · {}", req.url)
        };
        entries.push(PaletteEntry {
            category: "Request",
            title: req.name.clone(),
            subtitle,
            shortcut: None,
            method_badge: Some(req.method),
            action: PaletteAction::OpenRequest {
                collection_id: col_id,
                request: Box::new(req),
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

    let all_entries = build_entries(&st);
    let q = query();
    let mut scored: Vec<(i32, PaletteEntry)> = all_entries
        .into_iter()
        .filter_map(|entry| {
            if q.trim().is_empty() {
                return Some((0, entry));
            }
            let hay = format!("{} {} {}", entry.category, entry.title, entry.subtitle);
            let title_score = fuzzy_score(&q, &entry.title).map(|s| s + 25);
            let full_score = fuzzy_score(&q, &hay);
            match (title_score, full_score) {
                (Some(a), Some(b)) => Some((a.max(b), entry)),
                (Some(a), None) => Some((a, entry)),
                (None, Some(b)) => Some((b, entry)),
                (None, None) => None,
            }
        })
        .collect();

    if !q.trim().is_empty() {
        scored.sort_by_key(|b| std::cmp::Reverse(b.0));
    }
    let filtered: Vec<PaletteEntry> = scored.into_iter().take(40).map(|(_, e)| e).collect();
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
                request,
            } => {
                let req_id = request.id;
                let cols = (st.collections)();
                let resolved_req = find_request_across(&cols, req_id)
                    .map(|(_, r)| r.clone())
                    .unwrap_or(*request);

                let mut tabs = (st.open_tabs)();
                let cur = (st.draft)();
                if let Some(slot) = tabs.iter_mut().find(|t| t.id == cur.id) {
                    *slot = cur;
                } else {
                    tabs.push(cur);
                }
                if let Some(slot) = tabs.iter_mut().find(|t| t.id == resolved_req.id) {
                    *slot = resolved_req.clone();
                } else {
                    tabs.push(resolved_req.clone());
                }
                st.open_tabs.set(tabs);
                st.draft.set(resolved_req.clone());
                st.active_request_id.set(Some(req_id));
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
                if let Some(cached) = resp_map.get(&req_id).cloned() {
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
                            }
                        }
                        Key::ArrowUp => {
                            evt.prevent_default();
                            if item_count > 0 {
                                selected_idx.set(if cur_sel == 0 { item_count - 1 } else { cur_sel - 1 });
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
                        autofocus: true,
                        placeholder: "Type a command, request name, URL, environment, or theme…",
                        value: "{query()}",
                        oninput: move |evt: Event<FormData>| {
                            query.set(evt.value());
                            selected_idx.set(0);
                        },
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
