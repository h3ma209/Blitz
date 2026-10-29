use std::collections::HashSet;

use dioxus::prelude::*;

use super::DismissBackdrop;
use uuid::Uuid;

use crate::core::{
    ancestor_ids_for_request, collapsed_except_expanded, filter_tree_rows,
    flatten_collections_filtered, flatten_collections_searchable, paste_target_parent_id,
    search_highlight_term, Collection, HistoryEntry, HttpMethod, TreeRowKind,
};

use super::sidebar_shortcuts::{
    label_copy, label_cut, label_delete, label_duplicate, label_open, label_paste, label_rename,
    tree_key_action, CtxMenuItem, TreeKeyAction,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SidebarAction {
    SelectRequest,
    SelectCollection,
    ToggleExpand,
    NewCollection,
    NewFolder,
    NewRequest,
    Rename,
    Delete,
    Duplicate,
    Favorite,
    Sort,
    MoveItem,
    BatchDelete,
    BatchMove,
    BatchDuplicate,
    CopyId,
    Copy,
    Cut,
    Paste,
    OpenHistory,
    SaveHistoryToCollection,
    ImportCollection,
    ExportCollection,
    ExportStaticDoc,
    ExportSwaggerDoc,
    ToggleHistoryCollapse,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SidebarEvent {
    pub action: SidebarAction,
    pub id: Option<Uuid>,
    pub ids: Vec<Uuid>,
    pub collection_id: Option<Uuid>,
    /// Drop target (collection or folder) for [`SidebarAction::MoveItem`] or [`SidebarAction::BatchMove`].
    pub target_id: Option<Uuid>,
    /// New name for [`SidebarAction::Rename`] (set on Enter confirm).
    pub name: Option<String>,
}

impl SidebarEvent {
    fn new(action: SidebarAction, id: Option<Uuid>, collection_id: Option<Uuid>) -> Self {
        Self {
            action,
            id,
            ids: id.map(|i| vec![i]).unwrap_or_default(),
            collection_id,
            target_id: None,
            name: None,
        }
    }

    pub fn batch(action: SidebarAction, ids: Vec<Uuid>, target_id: Option<Uuid>) -> Self {
        Self {
            action,
            id: ids.first().copied(),
            ids,
            collection_id: None,
            target_id,
            name: None,
        }
    }

    fn rename(id: Uuid, collection_id: Uuid, name: String) -> Self {
        Self {
            action: SidebarAction::Rename,
            id: Some(id),
            ids: vec![id],
            collection_id: Some(collection_id),
            target_id: None,
            name: Some(name),
        }
    }
}

#[derive(Clone, PartialEq)]
struct TreeSelection {
    id: Uuid,
    collection_id: Uuid,
    kind: TreeRowKind,
}

#[derive(Clone, PartialEq)]
struct ContextMenuState {
    x: f64,
    y: f64,
    kind: TreeRowKind,
    id: Uuid,
    collection_id: Uuid,
}

#[component]
pub fn Sidebar(
    collections: Vec<Collection>,
    active_collection_id: Option<Uuid>,
    active_request_id: Option<Uuid>,
    history: Vec<HistoryEntry>,
    #[props(default = true)] history_collapsed: bool,
    on_event: EventHandler<SidebarEvent>,
) -> Element {
    // Empty = all folded (Postman-style). Parent remounts via `key=fold_epoch` to refold.
    let mut expanded = use_signal(HashSet::<Uuid>::new);
    let mut dragging = use_signal(|| None::<Uuid>);
    let mut drop_target = use_signal(|| None::<Uuid>);
    let mut ctx_menu = use_signal(|| None::<ContextMenuState>);
    let mut renaming_id = use_signal(|| None::<Uuid>);
    let mut rename_draft = use_signal(String::new);
    let mut search = use_signal(String::new);
    let mut focused_row = use_signal(|| None::<TreeSelection>);
    let mut selected_ids = use_signal(HashSet::<Uuid>::new);
    let mut last_selected_id = use_signal(|| None::<Uuid>);
    let mut last_scrolled_req = use_signal(|| None::<Uuid>);

    let query = search();
    let query_active = !query.trim().is_empty();
    let rows = if query_active {
        // While searching, uncollapse all nodes with empty collapsed set and build search index
        let flat = flatten_collections_searchable(&collections, &HashSet::new());
        filter_tree_rows(flat, &query)
    } else {
        let collapsed = collapsed_except_expanded(&collections, &expanded());
        flatten_collections_filtered(&collections, &collapsed)
    };

    let total_matches = rows.len();
    const MAX_SEARCH_RENDER: usize = 150;
    let (rendered_rows, truncated_search) = if query_active && total_matches > MAX_SEARCH_RENDER {
        (rows[..MAX_SEARCH_RENDER].to_vec(), true)
    } else {
        (rows.clone(), false)
    };
    // Auto-track and reveal active request (expand ancestor folders/collections and scroll into view)
    // Redirection ONLY happens when switching between request files, NOT when clicking or opening folders/collections.
    let collections_for_track = collections.clone();
    use_effect(move || {
        let cur_req = active_request_id;
        if cur_req == last_scrolled_req() {
            return;
        }
        last_scrolled_req.set(cur_req);
        if let Some(req_id) = cur_req {
            let ancestors = ancestor_ids_for_request(&collections_for_track, req_id);
            if !ancestors.is_empty() {
                let mut set = expanded.peek().clone();
                let mut changed = false;
                for aid in ancestors {
                    if set.insert(aid) {
                        changed = true;
                    }
                }
                if changed {
                    expanded.set(set);
                }
            }
            // Smoothly scroll active request row into view if needed
            let script = format!(
                r#"
                setTimeout(() => {{
                    const el = document.getElementById("tree-row-" + {id});
                    if (el) {{
                        el.scrollIntoView({{ behavior: 'smooth', block: 'nearest' }});
                    }}
                }}, 30);
                "#,
                id = serde_json::to_string(&req_id.to_string()).unwrap_or_else(|_| "\"\"".into())
            );
            spawn(async move {
                let _ = document::eval(&script).await;
            });
        }
    });

    let empty = collections.is_empty() || (query_active && rows.is_empty());
    let all_rows = rows.clone();
    let search_raw = search();
    let highlight_term = search_highlight_term(&search_raw);

    rsx! {
        aside {
            class: "sidebar",
            tabindex: "0",
            onkeydown: {
                let all_rows_for_key = all_rows.clone();
                move |evt| {
                    if evt.key() == Key::Escape {
                        ctx_menu.set(None);
                        if !selected_ids().is_empty() {
                            selected_ids.set(HashSet::new());
                        }
                        if renaming_id().is_some() {
                            renaming_id.set(None);
                            rename_draft.set(String::new());
                        }
                    } else if (evt.modifiers().meta() || evt.modifiers().ctrl())
                        && !evt.modifiers().shift()
                        && renaming_id().is_none()
                        && (evt.code() == Code::KeyA
                            || matches!(evt.key(), Key::Character(ref c) if c.eq_ignore_ascii_case("a")))
                    {
                        evt.prevent_default();
                        selected_ids.set(all_rows_for_key.iter().map(|r| r.id).collect());
                    }
                }
            },
            onclick: move |_| {
                ctx_menu.set(None);
                if renaming_id().is_some() {
                    renaming_id.set(None);
                    rename_draft.set(String::new());
                }
            },
            oncontextmenu: move |evt| {
                // Bubbled from chrome / empty area (tree rows stop_propagation).
                evt.prevent_default();
                let coords = evt.client_coordinates();
                ctx_menu.set(Some(ContextMenuState {
                    x: coords.x,
                    y: coords.y,
                    kind: TreeRowKind::Collection,
                    id: Uuid::nil(),
                    collection_id: Uuid::nil(),
                }));
            },

            div { class: "sidebar-collections",
            div { class: "sidebar-section-head",
                h3 { class: "type-label", "Collections" }
                div { class: "sidebar-actions",
                    button {
                        class: "btn-icon",
                        title: "Import Postman v2.1 JSON",
                        aria_label: "Import Postman v2.1 JSON",
                        onclick: move |_| on_event.call(SidebarEvent::new(
                            SidebarAction::ImportCollection,
                            None,
                            active_collection_id,
                        )),
                        span { class: "action-icon action-icon-import", aria_hidden: "true" }
                    }
                    button {
                        class: "btn-icon",
                        title: "Export active collection",
                        aria_label: "Export active collection",
                        onclick: move |_| on_event.call(SidebarEvent::new(
                            SidebarAction::ExportCollection,
                            None,
                            active_collection_id,
                        )),
                        span { class: "action-icon action-icon-export", aria_hidden: "true" }
                    }
                    button {
                        class: "btn-icon",
                        title: "Sort favorites + folders first",
                        aria_label: "Sort favorites and folders first",
                        onclick: move |_| on_event.call(SidebarEvent::new(
                            SidebarAction::Sort,
                            None,
                            active_collection_id,
                        )),
                        span { class: "action-icon action-icon-sort", aria_hidden: "true" }
                    }
                    button {
                        class: "btn-icon",
                        title: "New collection",
                        aria_label: "New collection",
                        onclick: move |_| on_event.call(SidebarEvent::new(
                            SidebarAction::NewCollection,
                            None,
                            None,
                        )),
                        span { class: "action-icon action-icon-add", aria_hidden: "true" }
                    }
                }
            }
            SidebarSearchInput {
                on_search: move |val: String| search.set(val),
            }

            if !selected_ids().is_empty() {
                {
                    let sel_count = selected_ids().len();
                    let sel_ids_del: Vec<Uuid> = selected_ids().into_iter().collect();
                    let sel_ids_dup: Vec<Uuid> = selected_ids().into_iter().collect();
                    let sel_ids_move: Vec<Uuid> = selected_ids().into_iter().collect();
                    let all_row_ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
                    let all_selected = !all_row_ids.is_empty() && all_row_ids.iter().all(|id| selected_ids().contains(id));
                    rsx! {
                        div { class: "sidebar-multiselect-bar",
                            div { class: "sidebar-multiselect-count",
                                input {
                                    r#type: "checkbox",
                                    class: "tree-checkbox multiselect-master-check",
                                    checked: all_selected,
                                    title: if all_selected { "Deselect all" } else { "Select all" },
                                    onclick: {
                                        let all_row_ids = all_row_ids.clone();
                                        move |evt| {
                                            evt.stop_propagation();
                                            if all_selected {
                                                selected_ids.set(HashSet::new());
                                            } else {
                                                selected_ids.set(all_row_ids.iter().copied().collect());
                                            }
                                        }
                                    },
                                }
                                span { class: "multiselect-badge", "{sel_count}" }
                                span { "selected" }
                            }
                            div { class: "sidebar-multiselect-actions",
                                button {
                                    class: "sidebar-multiselect-btn",
                                    title: if all_selected { "Deselect all items" } else { "Select all visible items (⌘A)" },
                                    onclick: {
                                        let all_row_ids = all_row_ids.clone();
                                        move |evt| {
                                            evt.stop_propagation();
                                            if all_selected {
                                                selected_ids.set(HashSet::new());
                                            } else {
                                                selected_ids.set(all_row_ids.iter().copied().collect());
                                            }
                                        }
                                    },
                                    if all_selected { "Deselect All" } else { "Select All" }
                                }
                                button {
                                    class: "sidebar-multiselect-btn",
                                    title: "Move selected items to a folder or collection",
                                    onclick: move |evt| {
                                        evt.stop_propagation();
                                        on_event.call(SidebarEvent::batch(
                                            SidebarAction::BatchMove,
                                            sel_ids_move.clone(),
                                            None,
                                        ));
                                    },
                                    "Move"
                                }
                                button {
                                    class: "sidebar-multiselect-btn",
                                    title: "Duplicate selected items",
                                    onclick: move |evt| {
                                        evt.stop_propagation();
                                        on_event.call(SidebarEvent::batch(
                                            SidebarAction::BatchDuplicate,
                                            sel_ids_dup.clone(),
                                            None,
                                        ));
                                        selected_ids.set(HashSet::new());
                                    },
                                    "Duplicate"
                                }
                                button {
                                    class: "sidebar-multiselect-btn danger",
                                    title: "Delete selected items",
                                    onclick: move |evt| {
                                        evt.stop_propagation();
                                        on_event.call(SidebarEvent::batch(
                                            SidebarAction::BatchDelete,
                                            sel_ids_del.clone(),
                                            None,
                                        ));
                                        selected_ids.set(HashSet::new());
                                    },
                                    "Delete"
                                }
                                button {
                                    class: "sidebar-multiselect-btn btn-clear",
                                    title: "Clear selection (Esc)",
                                    onclick: move |evt| {
                                        evt.stop_propagation();
                                        selected_ids.set(HashSet::new());
                                    },
                                    "✕"
                                }
                            }
                        }
                    }
                }
            }

            if empty {
                div {
                    class: "empty-state",
                    oncontextmenu: move |evt| {
                        evt.prevent_default();
                        evt.stop_propagation();
                        let coords = evt.client_coordinates();
                        ctx_menu.set(Some(ContextMenuState {
                            x: coords.x,
                            y: coords.y,
                            kind: TreeRowKind::Collection,
                            id: Uuid::nil(),
                            collection_id: Uuid::nil(),
                        }));
                    },
                    p {
                        if search().trim().is_empty() {
                            "No collections yet."
                        } else {
                            "No items match \"{search}\"."
                        }
                    }
                    if !search().trim().is_empty() {
                        span { class: "tree-search-hint",
                            "Tip: Use folder: or req: to filter (e.g. folder:auth or req:users)"
                        }
                    }
                    button {
                        class: "btn btn-ghost",
                        onclick: move |_| on_event.call(SidebarEvent::new(
                            SidebarAction::NewCollection,
                            None,
                            None,
                        )),
                        "Create collection"
                    }
                }
            } else {
                div {
                    class: if !selected_ids().is_empty() { "tree multi-selecting" } else { "tree" },
                    tabindex: "0",
                    onkeydown: {
                        let collections = collections.clone();
                        let all_rows = all_rows.clone();
                        move |evt| {
                        if !selected_ids().is_empty() {
                            if let Some(action) = tree_key_action(&evt) {
                                match action {
                                    TreeKeyAction::Delete => {
                                        evt.prevent_default();
                                        evt.stop_propagation();
                                        let ids: Vec<Uuid> = selected_ids().into_iter().collect();
                                        on_event.call(SidebarEvent::batch(
                                            SidebarAction::BatchDelete,
                                            ids,
                                            None,
                                        ));
                                        selected_ids.set(HashSet::new());
                                        return;
                                    }
                                    TreeKeyAction::Duplicate => {
                                        evt.prevent_default();
                                        evt.stop_propagation();
                                        let ids: Vec<Uuid> = selected_ids().into_iter().collect();
                                        on_event.call(SidebarEvent::batch(
                                            SidebarAction::BatchDuplicate,
                                            ids,
                                            None,
                                        ));
                                        selected_ids.set(HashSet::new());
                                        return;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        let Some(action) = tree_key_action(&evt) else {
                            return;
                        };
                        evt.prevent_default();
                        evt.stop_propagation();
                        match action {
                            TreeKeyAction::NavDown => {
                                let next_row = if let Some(sel) = focused_row() {
                                    if let Some(pos) = all_rows.iter().position(|r| r.id == sel.id) {
                                        all_rows.get(pos + 1).cloned()
                                    } else {
                                        all_rows.first().cloned()
                                    }
                                } else {
                                    all_rows.first().cloned()
                                };
                                if let Some(row) = next_row {
                                    focused_row.set(Some(TreeSelection {
                                        id: row.id,
                                        collection_id: row.collection_id,
                                        kind: row.kind,
                                    }));
                                    let rid = row.id;
                                    let script = format!(
                                        r#"setTimeout(() => {{
                                            const el = document.getElementById("tree-row-" + {id});
                                            if (el) el.scrollIntoView({{ behavior: 'smooth', block: 'nearest' }});
                                        }}, 10);"#,
                                        id = serde_json::to_string(&rid.to_string()).unwrap_or_else(|_| "\"\"".into())
                                    );
                                    spawn(async move {
                                        let _ = document::eval(&script).await;
                                    });
                                }
                            }
                            TreeKeyAction::NavUp => {
                                let prev_row = if let Some(sel) = focused_row() {
                                    if let Some(pos) = all_rows.iter().position(|r| r.id == sel.id) {
                                        if pos > 0 {
                                            all_rows.get(pos - 1).cloned()
                                        } else {
                                            None
                                        }
                                    } else {
                                        all_rows.first().cloned()
                                    }
                                } else {
                                    all_rows.first().cloned()
                                };
                                if let Some(row) = prev_row {
                                    focused_row.set(Some(TreeSelection {
                                        id: row.id,
                                        collection_id: row.collection_id,
                                        kind: row.kind,
                                    }));
                                    let rid = row.id;
                                    let script = format!(
                                        r#"setTimeout(() => {{
                                            const el = document.getElementById("tree-row-" + {id});
                                            if (el) el.scrollIntoView({{ behavior: 'smooth', block: 'nearest' }});
                                        }}, 10);"#,
                                        id = serde_json::to_string(&rid.to_string()).unwrap_or_else(|_| "\"\"".into())
                                    );
                                    spawn(async move {
                                        let _ = document::eval(&script).await;
                                    });
                                }
                            }
                            TreeKeyAction::NavRight => {
                                if let Some(sel) = focused_row() {
                                    if matches!(sel.kind, TreeRowKind::Collection | TreeRowKind::Folder) {
                                        if !expanded().contains(&sel.id) {
                                            let mut set = expanded();
                                            set.insert(sel.id);
                                            expanded.set(set);
                                            on_event.call(SidebarEvent::new(
                                                SidebarAction::ToggleExpand,
                                                Some(sel.id),
                                                Some(sel.collection_id),
                                            ));
                                        } else if let Some(pos) = all_rows.iter().position(|r| r.id == sel.id) {
                                            if let Some(child) = all_rows.get(pos + 1) {
                                                focused_row.set(Some(TreeSelection {
                                                    id: child.id,
                                                    collection_id: child.collection_id,
                                                    kind: child.kind.clone(),
                                                }));
                                            }
                                        }
                                    }
                                }
                            }
                            TreeKeyAction::NavLeft => {
                                if let Some(sel) = focused_row() {
                                    if matches!(sel.kind, TreeRowKind::Collection | TreeRowKind::Folder) && expanded().contains(&sel.id) {
                                        let mut set = expanded();
                                        set.remove(&sel.id);
                                        expanded.set(set);
                                        on_event.call(SidebarEvent::new(
                                            SidebarAction::ToggleExpand,
                                            Some(sel.id),
                                            Some(sel.collection_id),
                                        ));
                                    } else if let Some(parent_id) = paste_target_parent_id(&collections, sel.id) {
                                        if let Some(parent_row) = all_rows.iter().find(|r| r.id == parent_id) {
                                            focused_row.set(Some(TreeSelection {
                                                id: parent_row.id,
                                                collection_id: parent_row.collection_id,
                                                kind: parent_row.kind.clone(),
                                            }));
                                        }
                                    }
                                }
                            }
                            TreeKeyAction::Delete => {
                                if let Some(sel) = focused_row() {
                                    on_event.call(SidebarEvent::new(
                                        SidebarAction::Delete,
                                        Some(sel.id),
                                        Some(sel.collection_id),
                                    ));
                                }
                            }
                            TreeKeyAction::Duplicate => {
                                if let Some(sel) = focused_row() {
                                    on_event.call(SidebarEvent::new(
                                        SidebarAction::Duplicate,
                                        Some(sel.id),
                                        Some(sel.collection_id),
                                    ));
                                }
                            }
                            TreeKeyAction::Copy => {
                                if let Some(sel) = focused_row() {
                                    on_event.call(SidebarEvent::new(
                                        SidebarAction::Copy,
                                        Some(sel.id),
                                        Some(sel.collection_id),
                                    ));
                                }
                            }
                            TreeKeyAction::Cut => {
                                if let Some(sel) = focused_row() {
                                    on_event.call(SidebarEvent::new(
                                        SidebarAction::Cut,
                                        Some(sel.id),
                                        Some(sel.collection_id),
                                    ));
                                }
                            }
                            TreeKeyAction::Paste => {
                                if let Some(sel) = focused_row() {
                                    if let Some(parent) =
                                        paste_target_parent_id(&collections, sel.id)
                                    {
                                        on_event.call(SidebarEvent::new(
                                            SidebarAction::Paste,
                                            Some(parent),
                                            Some(sel.collection_id),
                                        ));
                                    }
                                }
                            }
                            TreeKeyAction::Rename => {
                                if let Some(sel) = focused_row() {
                                    let current = tree_row_name(&collections, sel.id);
                                    renaming_id.set(Some(sel.id));
                                    rename_draft.set(current);
                                }
                            }
                            TreeKeyAction::Open => {
                                if let Some(sel) = focused_row() {
                                    if matches!(sel.kind, TreeRowKind::Request { .. }) {
                                        on_event.call(SidebarEvent::new(
                                            SidebarAction::SelectRequest,
                                            Some(sel.id),
                                            Some(sel.collection_id),
                                        ));
                                    } else if matches!(sel.kind, TreeRowKind::Collection | TreeRowKind::Folder) {
                                        let mut set = expanded();
                                        if set.contains(&sel.id) {
                                            set.remove(&sel.id);
                                        } else {
                                            set.insert(sel.id);
                                        }
                                        expanded.set(set);
                                        on_event.call(SidebarEvent::new(
                                            SidebarAction::ToggleExpand,
                                            Some(sel.id),
                                            Some(sel.collection_id),
                                        ));
                                    }
                                }
                            }
                        }
                        }
                    },
                    for row in rendered_rows {
                        {
                            let row_id = row.id;
                            let col_id = row.collection_id;
                            let depth = row.depth;
                            let name = row.name.clone();
                            let kind = row.kind.clone();
                            let favorite = row.favorite;
                            let is_req = matches!(kind, TreeRowKind::Request { .. });
                            let is_col = matches!(kind, TreeRowKind::Collection);
                            let is_folder = matches!(kind, TreeRowKind::Folder);
                            let row_active = if is_req {
                                active_request_id == Some(row_id)
                            } else if is_col {
                                active_collection_id == Some(row_id) && active_request_id.is_none()
                            } else {
                                false
                            };
                            let is_collapsed = !expanded().contains(&row_id);
                            let is_drop = drop_target() == Some(row_id);
                            let can_drop = is_col || is_folder;
                            let is_renaming = renaming_id() == Some(row_id);
                            let pad = 8 + (depth as i64) * 12;
                            let glyph = if is_folder || is_col {
                                if is_collapsed { "▸" } else { "▾" }
                            } else {
                                glyph_for(&kind)
                            };
                            let label = if favorite {
                                format!("★ {name}")
                            } else {
                                name.clone()
                            };
                            let start_name = name.clone();
                            let kind_for_ctx = kind.clone();
                            let kind_for_click = kind.clone();

                            let is_selected_multi = selected_ids().contains(&row_id);
                            let mut row_class = String::from("tree-row");
                            if row_active {
                                row_class.push_str(" active");
                            }
                            if is_selected_multi {
                                row_class.push_str(" tree-selected-multi");
                            }
                            if is_drop && can_drop {
                                row_class.push_str(" drop-target");
                            }
                            if dragging() == Some(row_id) {
                                row_class.push_str(" dragging");
                            }
                            if focused_row().map(|s| s.id) == Some(row_id) {
                                row_class.push_str(" tree-selected");
                            }

                            rsx! {
                                div {
                                    id: "tree-row-{row_id}",
                                    class: "{row_class}",
                                    style: "padding-left: {pad}px;",
                                    draggable: if !is_col { "true" } else { "false" },
                                    ondragstart: move |evt| {
                                        if !is_col {
                                            evt.stop_propagation();
                                            dragging.set(Some(row_id));
                                            if !selected_ids().contains(&row_id) {
                                                selected_ids.set(HashSet::from([row_id]));
                                            }
                                            let _ = evt.data.data_transfer().set_data(
                                                "text/plain",
                                                &row_id.to_string(),
                                            );
                                        }
                                    },
                                    ondragend: move |_| {
                                        dragging.set(None);
                                        drop_target.set(None);
                                    },
                                    ondragover: move |evt| {
                                        if can_drop {
                                            if let Some(src) = dragging() {
                                                if src != row_id && !selected_ids().contains(&row_id) {
                                                    evt.prevent_default();
                                                    drop_target.set(Some(row_id));
                                                }
                                            }
                                        }
                                    },
                                    ondragleave: move |_| {
                                        if drop_target() == Some(row_id) {
                                            drop_target.set(None);
                                        }
                                    },
                                    ondrop: move |evt| {
                                        evt.prevent_default();
                                        evt.stop_propagation();
                                        if can_drop {
                                            if let Some(src) = dragging() {
                                                let sel = selected_ids();
                                                if sel.contains(&src) && sel.len() > 1 {
                                                    let ids: Vec<Uuid> = sel.into_iter().collect();
                                                    on_event.call(SidebarEvent::batch(
                                                        SidebarAction::BatchMove,
                                                        ids,
                                                        Some(row_id),
                                                    ));
                                                    selected_ids.set(HashSet::new());
                                                } else {
                                                    on_event.call(SidebarEvent {
                                                        action: SidebarAction::MoveItem,
                                                        id: Some(src),
                                                        ids: vec![src],
                                                        collection_id: Some(col_id),
                                                        target_id: Some(row_id),
                                                        name: None,
                                                    });
                                                }
                                            }
                                        }
                                        dragging.set(None);
                                        drop_target.set(None);
                                    },
                                    oncontextmenu: move |evt| {
                                        evt.prevent_default();
                                        evt.stop_propagation();
                                        let coords = evt.client_coordinates();
                                        let sel = selected_ids();
                                        if !sel.contains(&row_id) {
                                            selected_ids.set(HashSet::from([row_id]));
                                        }
                                        ctx_menu.set(Some(ContextMenuState {
                                            x: coords.x,
                                            y: coords.y,
                                            kind: kind_for_ctx.clone(),
                                            id: row_id,
                                            collection_id: col_id,
                                        }));
                                    },
                                    div {
                                        class: "tree-main",
                                        onclick: {
                                            let rows_clone = all_rows.clone();
                                            move |evt| {
                                                if is_renaming {
                                                    evt.stop_propagation();
                                                    return;
                                                }
                                                evt.stop_propagation();
                                                ctx_menu.set(None);

                                                let is_meta = evt.modifiers().meta() || evt.modifiers().ctrl();
                                                let is_shift = evt.modifiers().shift();

                                                if is_meta {
                                                    let mut set = selected_ids();
                                                    if set.contains(&row_id) {
                                                        set.remove(&row_id);
                                                    } else {
                                                        set.insert(row_id);
                                                    }
                                                    last_selected_id.set(Some(row_id));
                                                    selected_ids.set(set);
                                                    focused_row.set(Some(TreeSelection {
                                                        id: row_id,
                                                        collection_id: col_id,
                                                        kind: kind_for_click.clone(),
                                                    }));
                                                    return;
                                                } else if is_shift {
                                                    let mut set = selected_ids();
                                                    if let Some(last_id) = last_selected_id() {
                                                        let p1 = rows_clone.iter().position(|r| r.id == last_id);
                                                        let p2 = rows_clone.iter().position(|r| r.id == row_id);
                                                        if let (Some(a), Some(b)) = (p1, p2) {
                                                            let (start, end) = if a < b { (a, b) } else { (b, a) };
                                                            for r in &rows_clone[start..=end] {
                                                                set.insert(r.id);
                                                            }
                                                        } else {
                                                            set.insert(row_id);
                                                        }
                                                    } else {
                                                        set.insert(row_id);
                                                    }
                                                    last_selected_id.set(Some(row_id));
                                                    selected_ids.set(set);
                                                    focused_row.set(Some(TreeSelection {
                                                        id: row_id,
                                                        collection_id: col_id,
                                                        kind: kind_for_click.clone(),
                                                    }));
                                                    return;
                                                } else {
                                                    let sel = selected_ids();
                                                    if !sel.is_empty() {
                                                        if is_col || is_folder {
                                                            // In multi-select mode, clicking a collection or folder
                                                            // toggles expand/collapse WITHOUT exiting multi-select mode!
                                                            let mut set = expanded();
                                                            if set.contains(&row_id) {
                                                                set.remove(&row_id);
                                                            } else {
                                                                set.insert(row_id);
                                                            }
                                                            expanded.set(set);
                                                            on_event.call(SidebarEvent::new(
                                                                SidebarAction::ToggleExpand,
                                                                Some(row_id),
                                                                Some(col_id),
                                                            ));
                                                            return;
                                                        } else {
                                                            // In multi-select mode, clicking a request row toggles
                                                            // selection (prevents misclick off checkbox from dropping selection)
                                                            let mut set = sel;
                                                            if set.contains(&row_id) {
                                                                set.remove(&row_id);
                                                            } else {
                                                                set.insert(row_id);
                                                            }
                                                            last_selected_id.set(Some(row_id));
                                                            selected_ids.set(set);
                                                            focused_row.set(Some(TreeSelection {
                                                                id: row_id,
                                                                collection_id: col_id,
                                                                kind: kind_for_click.clone(),
                                                            }));
                                                            return;
                                                        }
                                                    }
                                                    last_selected_id.set(Some(row_id));
                                                }

                                                focused_row.set(Some(TreeSelection {
                                                    id: row_id,
                                                    collection_id: col_id,
                                                    kind: kind_for_click.clone(),
                                                }));
                                                if is_req {
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::SelectRequest,
                                                        Some(row_id),
                                                        Some(col_id),
                                                    ));
                                                } else if is_col || is_folder {
                                                    let mut set = expanded();
                                                    if set.contains(&row_id) {
                                                        set.remove(&row_id);
                                                    } else {
                                                        set.insert(row_id);
                                                    }
                                                    expanded.set(set);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::ToggleExpand,
                                                        Some(row_id),
                                                        Some(col_id),
                                                    ));
                                                }
                                            }
                                        },
                                        input {
                                            r#type: "checkbox",
                                            class: "tree-checkbox",
                                            checked: is_selected_multi,
                                            title: "Select item",
                                            onclick: move |evt| {
                                                evt.stop_propagation();
                                                let mut set = selected_ids();
                                                if set.contains(&row_id) {
                                                    set.remove(&row_id);
                                                } else {
                                                    set.insert(row_id);
                                                }
                                                last_selected_id.set(Some(row_id));
                                                selected_ids.set(set);
                                            },
                                        }
                                        span {
                                            class: "tree-glyph",
                                            onclick: move |evt| {
                                                if is_col || is_folder {
                                                    evt.stop_propagation();
                                                    let mut set = expanded();
                                                    if set.contains(&row_id) {
                                                        set.remove(&row_id);
                                                    } else {
                                                        set.insert(row_id);
                                                    }
                                                    expanded.set(set);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::ToggleExpand,
                                                        Some(row_id),
                                                        Some(col_id),
                                                    ));
                                                }
                                            },
                                            "{glyph}"
                                        }
                                        if is_col {
                                            span {
                                                class: "tree-icon tree-icon-collection",
                                                title: "Collection",
                                                aria_hidden: "true",
                                            }
                                        } else if is_folder {
                                            span {
                                                class: "tree-icon tree-icon-folder",
                                                title: "Folder",
                                                aria_hidden: "true",
                                            }
                                        } else if let TreeRowKind::Request { method } = kind {
                                            span { class: "method-badge method-{method_class(method)}",
                                                "{method.as_str()}"
                                            }
                                        }
                                        if is_renaming {
                                            input {
                                                class: "tree-rename-input",
                                                value: "{rename_draft()}",
                                                autofocus: true,
                                                onclick: move |evt| evt.stop_propagation(),
                                                onmousedown: move |evt| evt.stop_propagation(),
                                                oninput: move |evt| {
                                                    rename_draft.set(evt.value());
                                                },
                                                onkeydown: move |evt| {
                                                    evt.stop_propagation();
                                                    match evt.key() {
                                                        Key::Enter => {
                                                            let raw = rename_draft();
                                                            let next = sanitize_item_name(&raw);
                                                            renaming_id.set(None);
                                                            rename_draft.set(String::new());
                                                            if !next.is_empty() {
                                                                on_event.call(SidebarEvent::rename(
                                                                    row_id, col_id, next,
                                                                ));
                                                            }
                                                        }
                                                        Key::Escape => {
                                                            renaming_id.set(None);
                                                            rename_draft.set(String::new());
                                                        }
                                                        _ => {}
                                                    }
                                                },
                                            }
                                        } else {
                                            {
                                                highlight_tree_label(&label, highlight_term)
                                            }
                                        }
                                    }
                                    div { class: "tree-row-actions",
                                        if is_col {
                                            button {
                                                class: "btn-icon",
                                                title: "Edit collection (Overview, Auth, Scripts, Vars, Runs)",
                                                aria_label: "Edit collection",
                                                onclick: move |evt| {
                                                    evt.stop_propagation();
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::SelectCollection,
                                                        Some(row_id),
                                                        Some(col_id),
                                                    ));
                                                },
                                                span { class: "action-icon action-icon-sliders", aria_hidden: "true" }
                                            }
                                            button {
                                                class: "btn-icon",
                                                title: "Favorite",
                                                aria_label: "Favorite",
                                                onclick: move |evt| {
                                                    evt.stop_propagation();
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::Favorite,
                                                        Some(row_id),
                                                        Some(col_id),
                                                    ));
                                                },
                                                span { class: "action-icon action-icon-star", aria_hidden: "true" }
                                            }
                                            button {
                                                class: "btn-icon",
                                                title: "Duplicate",
                                                aria_label: "Duplicate",
                                                onclick: move |evt| {
                                                    evt.stop_propagation();
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::Duplicate,
                                                        Some(row_id),
                                                        Some(col_id),
                                                    ));
                                                },
                                                span { class: "action-icon action-icon-duplicate", aria_hidden: "true" }
                                            }
                                            button {
                                                class: "btn-icon",
                                                title: "New request",
                                                aria_label: "New request",
                                                onclick: move |evt| {
                                                    evt.stop_propagation();
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::NewRequest,
                                                        Some(row_id),
                                                        Some(col_id),
                                                    ));
                                                },
                                                span { class: "action-icon action-icon-request", aria_hidden: "true" }
                                            }
                                            button {
                                                class: "btn-icon",
                                                title: "New folder",
                                                aria_label: "New folder",
                                                onclick: move |evt| {
                                                    evt.stop_propagation();
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::NewFolder,
                                                        Some(row_id),
                                                        Some(col_id),
                                                    ));
                                                },
                                                span { class: "action-icon action-icon-folder-new", aria_hidden: "true" }
                                            }
                                        }
                                        if is_folder {
                                            button {
                                                class: "btn-icon",
                                                title: "New request",
                                                aria_label: "New request",
                                                onclick: move |evt| {
                                                    evt.stop_propagation();
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::NewRequest,
                                                        Some(row_id),
                                                        Some(col_id),
                                                    ));
                                                },
                                                span { class: "action-icon action-icon-request", aria_hidden: "true" }
                                            }
                                            button {
                                                class: "btn-icon",
                                                title: "New folder",
                                                aria_label: "New folder",
                                                onclick: move |evt| {
                                                    evt.stop_propagation();
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::NewFolder,
                                                        Some(row_id),
                                                        Some(col_id),
                                                    ));
                                                },
                                                span { class: "action-icon action-icon-folder-new", aria_hidden: "true" }
                                            }
                                            button {
                                                class: "btn-icon",
                                                title: "Duplicate",
                                                aria_label: "Duplicate",
                                                onclick: move |evt| {
                                                    evt.stop_propagation();
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::Duplicate,
                                                        Some(row_id),
                                                        Some(col_id),
                                                    ));
                                                },
                                                span { class: "action-icon action-icon-duplicate", aria_hidden: "true" }
                                            }
                                        }
                                        if is_req {
                                            button {
                                                class: "btn-icon",
                                                title: "Duplicate",
                                                aria_label: "Duplicate",
                                                onclick: move |evt| {
                                                    evt.stop_propagation();
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::Duplicate,
                                                        Some(row_id),
                                                        Some(col_id),
                                                    ));
                                                },
                                                span { class: "action-icon action-icon-duplicate", aria_hidden: "true" }
                                            }
                                        }
                                        button {
                                            class: "btn-icon",
                                            title: "Rename",
                                            aria_label: "Rename",
                                            onclick: move |evt| {
                                                evt.stop_propagation();
                                                ctx_menu.set(None);
                                                renaming_id.set(Some(row_id));
                                                rename_draft.set(start_name.clone());
                                            },
                                            span { class: "action-icon action-icon-edit", aria_hidden: "true" }
                                        }
                                        button {
                                            class: "btn-icon danger",
                                            title: "Delete",
                                            aria_label: "Delete",
                                            onclick: move |evt| {
                                                evt.stop_propagation();
                                                let sel = selected_ids();
                                                if sel.contains(&row_id) && sel.len() > 1 {
                                                    let ids: Vec<Uuid> = sel.into_iter().collect();
                                                    on_event.call(SidebarEvent::batch(
                                                        SidebarAction::BatchDelete,
                                                        ids,
                                                        None,
                                                    ));
                                                    selected_ids.set(HashSet::new());
                                                } else {
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::Delete,
                                                        Some(row_id),
                                                        Some(col_id),
                                                    ));
                                                }
                                            },
                                            span { class: "action-icon action-icon-delete", aria_hidden: "true" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if truncated_search {
                        div { class: "tree-search-limit-notice",
                            "Showing first {MAX_SEARCH_RENDER} of {total_matches} matches. Refine query to narrow."
                        }
                    }
                }
            }
            } // .sidebar-collections

            div { class: "sidebar-history",
                button {
                    class: "sidebar-history-toggle",
                    r#type: "button",
                    title: if history_collapsed { "Expand history" } else { "Collapse history" },
                    onclick: move |_| on_event.call(SidebarEvent::new(
                        SidebarAction::ToggleHistoryCollapse,
                        None,
                        None,
                    )),
                    span { class: "sidebar-history-chevron",
                        if history_collapsed { "▸" } else { "▾" }
                    }
                    span { class: "type-label", "History" }
                }
                if !history_collapsed {
                    if history.is_empty() {
                        p { class: "hint", "Empty — Send to log." }
                    } else {
                        div { class: "history-list",
                            for entry in history.iter().take(40) {
                                {
                                    let id = entry.id;
                                    let method = entry.method;
                                    let url = truncate(&entry.url, 42);
                                    let status = entry
                                        .status
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| "—".into());
                                    let status_cls = match entry.status {
                                        Some(s) if (200..400).contains(&s) => "history-status status-ok",
                                        Some(_) => "history-status status-err",
                                        None => "history-status status-pending",
                                    };
                                    let when = entry.at.format("%H:%M:%S").to_string();
                                    rsx! {
                                        div { class: "history-row",
                                            button {
                                                class: "history-main",
                                                title: "{entry.url}",
                                                onclick: move |_| on_event.call(SidebarEvent::new(
                                                    SidebarAction::OpenHistory,
                                                    Some(id),
                                                    None,
                                                )),
                                                span { class: "method-badge method-{method_class(method)}",
                                                    "{method.as_str()}"
                                                }
                                                span { class: "{status_cls}", "{status}" }
                                                span { class: "tree-label", "{url}" }
                                                span { class: "history-time", "{when}" }
                                            }
                                            button {
                                                class: "btn-icon",
                                                title: "Save to collection",
                                                aria_label: "Save to collection",
                                                onclick: move |_| on_event.call(SidebarEvent::new(
                                                    SidebarAction::SaveHistoryToCollection,
                                                    Some(id),
                                                    active_collection_id,
                                                )),
                                                span { class: "action-icon action-icon-save", aria_hidden: "true" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if let Some(menu) = ctx_menu() {
                {
                    let mx = menu.x;
                    let my = menu.y;
                    let mid = menu.id;
                    let mcol = menu.collection_id;
                    let mkind = menu.kind.clone();
                    let is_empty_menu = mid.is_nil();
                    let collections_menu = collections.clone();
                    rsx! {
                        DismissBackdrop {
                            z_index: 900,
                            on_dismiss: move |_| ctx_menu.set(None),
                        }
                        div {
                            class: "ctx-menu",
                            style: "left: {mx}px; top: {my}px;",
                            onclick: move |evt| evt.stop_propagation(),
                            if is_empty_menu {
                                CtxMenuItem {
                                    label: "New Collection",
                                    onclick: move |_| {
                                        ctx_menu.set(None);
                                        on_event.call(SidebarEvent::new(
                                            SidebarAction::NewCollection,
                                            None,
                                            None,
                                        ));
                                    },
                                }
                            } else if selected_ids().len() > 1 {
                                {
                                    let sel_count = selected_ids().len();
                                    let sel_ids_del: Vec<Uuid> = selected_ids().into_iter().collect();
                                    let sel_ids_dup: Vec<Uuid> = selected_ids().into_iter().collect();
                                    let sel_ids_move: Vec<Uuid> = selected_ids().into_iter().collect();
                                    rsx! {
                                        CtxMenuItem {
                                            label: format!("Move ({sel_count} items) to..."),
                                            onclick: move |_| {
                                                ctx_menu.set(None);
                                                on_event.call(SidebarEvent::batch(
                                                    SidebarAction::BatchMove,
                                                    sel_ids_move.clone(),
                                                    None,
                                                ));
                                            },
                                        }
                                        CtxMenuItem {
                                            label: format!("Duplicate ({sel_count} items)"),
                                            shortcut: Some(label_duplicate()),
                                            onclick: move |_| {
                                                ctx_menu.set(None);
                                                on_event.call(SidebarEvent::batch(
                                                    SidebarAction::BatchDuplicate,
                                                    sel_ids_dup.clone(),
                                                    None,
                                                ));
                                                selected_ids.set(HashSet::new());
                                            },
                                        }
                                        CtxMenuItem {
                                            label: format!("Delete ({sel_count} items)"),
                                            shortcut: Some(label_delete()),
                                            danger: true,
                                            onclick: move |_| {
                                                ctx_menu.set(None);
                                                on_event.call(SidebarEvent::batch(
                                                    SidebarAction::BatchDelete,
                                                    sel_ids_del.clone(),
                                                    None,
                                                ));
                                                selected_ids.set(HashSet::new());
                                            },
                                        }
                                        CtxMenuItem {
                                            label: "Clear selection".to_string(),
                                            onclick: move |_| {
                                                ctx_menu.set(None);
                                                selected_ids.set(HashSet::new());
                                            },
                                        }
                                    }
                                }
                            } else {
                                match mkind {
                                    TreeRowKind::Collection => {
                                        rsx! {
                                            CtxMenuItem {
                                                label: "Edit Collection",
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::SelectCollection,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "New Request",
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::NewRequest,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "New Folder",
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::NewFolder,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Rename",
                                                shortcut: Some(label_rename().to_string()),
                                                onclick: {
                                                    let collections_menu = collections_menu.clone();
                                                    move |_| {
                                                        let current = tree_row_name(&collections_menu, mid);
                                                        ctx_menu.set(None);
                                                        renaming_id.set(Some(mid));
                                                        rename_draft.set(current);
                                                    }
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Duplicate",
                                                shortcut: Some(label_duplicate()),
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::Duplicate,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Favorite",
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::Favorite,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Export",
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::ExportCollection,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Export Static HTML Docs",
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::ExportStaticDoc,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Export Swagger UI Docs",
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::ExportSwaggerDoc,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Delete",
                                                shortcut: Some(label_delete()),
                                                danger: true,
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::Delete,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                        }
                                    }
                                    TreeRowKind::Folder => {
                                        let cols_rename = collections_menu.clone();
                                        let cols_paste = collections_menu.clone();
                                        rsx! {
                                            CtxMenuItem {
                                                label: "New Request",
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::NewRequest,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "New Folder",
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::NewFolder,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Rename",
                                                shortcut: Some(label_rename().to_string()),
                                                onclick: move |_| {
                                                    let current = tree_row_name(&cols_rename, mid);
                                                    ctx_menu.set(None);
                                                    renaming_id.set(Some(mid));
                                                    rename_draft.set(current);
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Duplicate",
                                                shortcut: Some(label_duplicate()),
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::Duplicate,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Cut",
                                                shortcut: Some(label_cut()),
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::Cut,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Copy",
                                                shortcut: Some(label_copy()),
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::Copy,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Paste",
                                                shortcut: Some(label_paste()),
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    if let Some(parent) =
                                                        paste_target_parent_id(&cols_paste, mid)
                                                    {
                                                        on_event.call(SidebarEvent::new(
                                                            SidebarAction::Paste,
                                                            Some(parent),
                                                            Some(mcol),
                                                        ));
                                                    }
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Delete",
                                                shortcut: Some(label_delete()),
                                                danger: true,
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::Delete,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                        }
                                    }
                                    TreeRowKind::Request { .. } => {
                                        let cols_rename = collections_menu.clone();
                                        let cols_paste = collections_menu.clone();
                                        rsx! {
                                            CtxMenuItem {
                                                label: "Open",
                                                shortcut: Some(label_open().to_string()),
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::SelectRequest,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Rename",
                                                shortcut: Some(label_rename().to_string()),
                                                onclick: move |_| {
                                                    let current = tree_row_name(&cols_rename, mid);
                                                    ctx_menu.set(None);
                                                    renaming_id.set(Some(mid));
                                                    rename_draft.set(current);
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Duplicate",
                                                shortcut: Some(label_duplicate()),
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::Duplicate,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Cut",
                                                shortcut: Some(label_cut()),
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::Cut,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Copy",
                                                shortcut: Some(label_copy()),
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::Copy,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Paste",
                                                shortcut: Some(label_paste()),
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    if let Some(parent) =
                                                        paste_target_parent_id(&cols_paste, mid)
                                                    {
                                                        on_event.call(SidebarEvent::new(
                                                            SidebarAction::Paste,
                                                            Some(parent),
                                                            Some(mcol),
                                                        ));
                                                    }
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Copy ID",
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::CopyId,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
                                            }
                                            CtxMenuItem {
                                                label: "Delete",
                                                shortcut: Some(label_delete()),
                                                danger: true,
                                                onclick: move |_| {
                                                    ctx_menu.set(None);
                                                    on_event.call(SidebarEvent::new(
                                                        SidebarAction::Delete,
                                                        Some(mid),
                                                        Some(mcol),
                                                    ));
                                                },
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

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

/// Strip display-only favorite markers; never persist ★ in stored names.
fn sanitize_item_name(name: &str) -> String {
    name.trim()
        .trim_start_matches('★')
        .trim()
        .trim_end_matches(" *")
        .trim()
        .to_string()
}

fn tree_row_name(collections: &[Collection], id: Uuid) -> String {
    flatten_collections_filtered(collections, &HashSet::new())
        .into_iter()
        .find(|r| r.id == id)
        .map(|r| r.name)
        .unwrap_or_else(|| "Item".into())
}

fn glyph_for(kind: &TreeRowKind) -> &'static str {
    match kind {
        TreeRowKind::Collection => "▣",
        TreeRowKind::Folder => "▸",
        TreeRowKind::Request { .. } => "·",
    }
}

fn method_class(m: HttpMethod) -> &'static str {
    match m {
        HttpMethod::Get => "get",
        HttpMethod::Post => "post",
        HttpMethod::Put => "put",
        HttpMethod::Patch => "patch",
        HttpMethod::Delete => "delete",
        HttpMethod::Head => "head",
        HttpMethod::Options => "options",
    }
}

fn highlight_tree_label(label: &str, query: &str) -> Element {
    let q = query.trim();
    if q.is_empty() {
        return rsx! { span { class: "tree-label", "{label}" } };
    }
    // Fast path: ASCII substring matching without heap allocation
    if label.is_ascii() && q.is_ascii() {
        let lower_label = label.to_ascii_lowercase();
        let lower_q = q.to_ascii_lowercase();
        if let Some(idx) = lower_label.find(&lower_q) {
            let end = idx + q.len();
            let before = &label[..idx];
            let matched = &label[idx..end];
            let after = &label[end..];
            return rsx! {
                span { class: "tree-label",
                    "{before}"
                    mark { class: "tree-search-match", "{matched}" }
                    "{after}"
                }
            };
        }
    } else {
        let lower_label = label.to_lowercase();
        let lower_q = q.to_lowercase();
        if let Some(byte_idx) = lower_label.find(&lower_q) {
            let char_start = lower_label[..byte_idx].chars().count();
            let char_len = lower_q.chars().count();
            let mut chars = label.chars();
            let before: String = chars.by_ref().take(char_start).collect();
            let matched: String = chars.by_ref().take(char_len).collect();
            let after: String = chars.collect();
            return rsx! {
                span { class: "tree-label",
                    "{before}"
                    mark { class: "tree-search-match", "{matched}" }
                    "{after}"
                }
            };
        }
    }
    rsx! { span { class: "tree-label", "{label}" } }
}

#[component]
fn SidebarSearchInput(on_search: EventHandler<String>) -> Element {
    let mut draft = use_signal(String::new);
    let mut seq = use_signal(|| 0u64);

    rsx! {
        input {
            class: "sidebar-search",
            r#type: "search",
            placeholder: "Search… (e.g. folder:auth, req:users)",
            title: "Search tree. Type folder: (or f:) or req: (or r:) to filter by type.",
            value: "{draft}",
            oninput: move |evt: Event<FormData>| {
                let val = evt.value();
                draft.set(val.clone());
                let next_seq = seq() + 1;
                seq.set(next_seq);
                if val.trim().is_empty() {
                    on_search.call(String::new());
                } else {
                    spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                        if seq() == next_seq {
                            on_search.call(val);
                        }
                    });
                }
            },
            onkeydown: move |evt: KeyboardEvent| {
                if evt.key() == Key::Enter {
                    seq.set(seq() + 1);
                    on_search.call(draft());
                } else if evt.key() == Key::Escape {
                    draft.set(String::new());
                    seq.set(seq() + 1);
                    on_search.call(String::new());
                }
            },
        }
    }
}
