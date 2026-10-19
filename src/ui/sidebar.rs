use std::collections::HashSet;

use dioxus::prelude::*;

use super::DismissBackdrop;
use uuid::Uuid;

use crate::core::{
    ancestor_ids_for_request, collapsible_tree_ids, collapsed_except_expanded, filter_tree_rows,
    flatten_collections_filtered, paste_target_parent_id, Collection, HistoryEntry, HttpMethod,
    TreeRowKind,
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
    CopyId,
    Copy,
    Cut,
    Paste,
    OpenHistory,
    SaveHistoryToCollection,
    ImportCollection,
    ExportCollection,
    ExportStaticDoc,
    ToggleHistoryCollapse,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SidebarEvent {
    pub action: SidebarAction,
    pub id: Option<Uuid>,
    pub collection_id: Option<Uuid>,
    /// Drop target (collection or folder) for [`SidebarAction::MoveItem`].
    pub target_id: Option<Uuid>,
    /// New name for [`SidebarAction::Rename`] (set on Enter confirm).
    pub name: Option<String>,
}

impl SidebarEvent {
    fn new(action: SidebarAction, id: Option<Uuid>, collection_id: Option<Uuid>) -> Self {
        Self {
            action,
            id,
            collection_id,
            target_id: None,
            name: None,
        }
    }

    fn rename(id: Uuid, collection_id: Uuid, name: String) -> Self {
        Self {
            action: SidebarAction::Rename,
            id: Some(id),
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

    let rows = {
        let query = search();
        let query_active = !query.trim().is_empty();
        // While searching, flatten the whole tree (expand every collapsible node)
        // so requests nested in collapsed folders/collections are reachable.
        // This only overrides the *local* fold set used for flattening; the user's
        // real `expanded` signal is untouched, so folds restore when the search clears.
        let expanded = if query_active {
            collapsible_tree_ids(&collections)
        } else {
            expanded()
        };
        let collapsed = collapsed_except_expanded(&collections, &expanded);
        let flat = flatten_collections_filtered(&collections, &collapsed);
        filter_tree_rows(flat, &query)
    };
    // Auto-track and reveal active request (expand ancestor folders/collections and scroll into view)
    let collections_for_track = collections.clone();
    use_effect(move || {
        if let Some(req_id) = active_request_id {
            let ancestors = ancestor_ids_for_request(&collections_for_track, req_id);
            if !ancestors.is_empty() {
                let mut set = expanded();
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

    let empty = collections.is_empty();

    rsx! {
        aside {
            class: "sidebar",
            tabindex: "0",
            onkeydown: move |evt| {
                if evt.key() == Key::Escape {
                    ctx_menu.set(None);
                    if renaming_id().is_some() {
                        renaming_id.set(None);
                        rename_draft.set(String::new());
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
            input {
                class: "sidebar-search",
                r#type: "search",
                placeholder: "Search collections & requests…",
                value: "{search}",
                oninput: move |evt: Event<FormData>| search.set(evt.value()),
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
                            "No requests match \"{search}\"."
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
                    class: "tree",
                    tabindex: "0",
                    onkeydown: {
                        let collections = collections.clone();
                        move |evt| {
                        let Some(sel) = focused_row() else {
                            return;
                        };
                        let Some(action) = tree_key_action(&evt) else {
                            return;
                        };
                        evt.prevent_default();
                        evt.stop_propagation();
                        match action {
                            TreeKeyAction::Delete => {
                                on_event.call(SidebarEvent::new(
                                    SidebarAction::Delete,
                                    Some(sel.id),
                                    Some(sel.collection_id),
                                ));
                            }
                            TreeKeyAction::Duplicate => {
                                on_event.call(SidebarEvent::new(
                                    SidebarAction::Duplicate,
                                    Some(sel.id),
                                    Some(sel.collection_id),
                                ));
                            }
                            TreeKeyAction::Copy => {
                                on_event.call(SidebarEvent::new(
                                    SidebarAction::Copy,
                                    Some(sel.id),
                                    Some(sel.collection_id),
                                ));
                            }
                            TreeKeyAction::Cut => {
                                on_event.call(SidebarEvent::new(
                                    SidebarAction::Cut,
                                    Some(sel.id),
                                    Some(sel.collection_id),
                                ));
                            }
                            TreeKeyAction::Paste => {
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
                            TreeKeyAction::Rename => {
                                let current = tree_row_name(&collections, sel.id);
                                renaming_id.set(Some(sel.id));
                                rename_draft.set(current);
                            }
                            TreeKeyAction::Open => {
                                if matches!(sel.kind, TreeRowKind::Request { .. }) {
                                    on_event.call(SidebarEvent::new(
                                        SidebarAction::SelectRequest,
                                        Some(sel.id),
                                        Some(sel.collection_id),
                                    ));
                                }
                            }
                        }
                        }
                    },
                    for row in rows {
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

                            let mut row_class = String::from("tree-row");
                            if row_active {
                                row_class.push_str(" active");
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
                                                if src != row_id {
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
                                                on_event.call(SidebarEvent {
                                                    action: SidebarAction::MoveItem,
                                                    id: Some(src),
                                                    collection_id: Some(col_id),
                                                    target_id: Some(row_id),
                                                    name: None,
                                                });
                                            }
                                        }
                                        dragging.set(None);
                                        drop_target.set(None);
                                    },
                                    oncontextmenu: move |evt| {
                                        evt.prevent_default();
                                        evt.stop_propagation();
                                        let coords = evt.client_coordinates();
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
                                        onclick: move |evt| {
                                            if is_renaming {
                                                evt.stop_propagation();
                                                return;
                                            }
                                            evt.stop_propagation();
                                            ctx_menu.set(None);
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
                                            } else if is_col {
                                                on_event.call(SidebarEvent::new(
                                                    SidebarAction::SelectCollection,
                                                    Some(row_id),
                                                    Some(col_id),
                                                ));
                                                let mut set = expanded();
                                                if set.contains(&row_id) {
                                                    set.remove(&row_id);
                                                } else {
                                                    set.insert(row_id);
                                                }
                                                expanded.set(set);
                                            } else if is_folder {
                                                on_event.call(SidebarEvent::new(
                                                    SidebarAction::SelectCollection,
                                                    Some(col_id),
                                                    Some(col_id),
                                                ));
                                                let mut set = expanded();
                                                if set.contains(&row_id) {
                                                    set.remove(&row_id);
                                                } else {
                                                    set.insert(row_id);
                                                }
                                                expanded.set(set);
                                            }
                                        },
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
                                            span { class: "tree-label", "{label}" }
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
                                                on_event.call(SidebarEvent::new(
                                                    SidebarAction::Delete,
                                                    Some(row_id),
                                                    Some(col_id),
                                                ));
                                            },
                                            span { class: "action-icon action-icon-delete", aria_hidden: "true" }
                                        }
                                    }
                                }
                            }
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
