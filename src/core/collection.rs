//! Collection / folder model (postman-collection–inspired; reports/04).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use super::request::{AuthConfig, AuthType, BodyMode, FormDataField, HttpMethod, KeyValue, RequestDraft};

/// Per-environment override for a request.
/// Allows different auth, URL, headers, body per environment without duplicating collections.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestEnvOverride {
    /// Override the request URL for this environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Override the HTTP method for this environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<HttpMethod>,
    /// Override request headers for this environment (replaces base headers).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<KeyValue>,
    /// Override request body mode for this environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_mode: Option<BodyMode>,
    /// Override raw body content.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body_raw: String,
    /// Override body content type (for raw mode).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body_content_type: String,
    /// Override URL-encoded body.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body_urlencoded: Vec<KeyValue>,
    /// Override form-data fields.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body_formdata: Vec<FormDataField>,
    /// Override GraphQL query.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub graphql_query: String,
    /// Override GraphQL variables.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub graphql_variables: String,
    /// Override binary file path.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub binary_path: String,
    /// Override auth configuration for this environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,
    /// Additional collection variables for this environment (merged into vars.collection).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<KeyValue>,
    /// Override pre-request script for this environment.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pre_request_script: String,
    /// Override test script for this environment.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub test_script: String,
}

/// Per-environment overrides for the entire collection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionEnvOverrides {
    /// Override collection-level variables per environment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<KeyValue>,
    /// Override collection-level auth per environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,
    /// Per-request overrides keyed by request UUID.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub requests: HashMap<Uuid, RequestEnvOverride>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    /// Collection-level auth (used when request auth is Inherit).
    #[serde(default)]
    pub auth: AuthConfig,
    pub items: Vec<CollectionItem>,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub pre_request_script: String,
    #[serde(default)]
    pub test_script: String,
    /// Collection variables (synced → VariableContext.collection when active).
    #[serde(default)]
    pub variables: Vec<KeyValue>,
    /// Per-environment overrides (optional, for team sync without git branch switching).
    #[serde(default, skip_serializing_if = "CollectionEnvOverrides::is_empty")]
    pub environment_overrides: CollectionEnvOverrides,
}

impl CollectionEnvOverrides {
    pub fn is_empty(&self) -> bool {
        self.variables.is_empty()
            && self.auth.is_none()
            && self.requests.is_empty()
    }
}

impl Default for Collection {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: "My Collection".into(),
            description: String::new(),
            auth: AuthConfig {
                auth_type: AuthType::NoAuth,
                ..Default::default()
            },
            items: vec![],
            favorite: false,
            pre_request_script: String::new(),
            test_script: String::new(),
            variables: vec![],
            environment_overrides: CollectionEnvOverrides::default(),
        }
    }
}

impl Collection {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: String::new(),
            auth: AuthConfig {
                auth_type: AuthType::NoAuth,
                ..Default::default()
            },
            items: vec![],
            favorite: false,
            pre_request_script: String::new(),
            test_script: String::new(),
            variables: vec![],
            environment_overrides: CollectionEnvOverrides::default(),
        }
    }

    /// Flat map of enabled collection variables.
    pub fn variables_map(&self) -> std::collections::HashMap<String, String> {
        self.variables
            .iter()
            .filter(|v| v.enabled && !v.key.is_empty())
            .map(|v| (v.key.clone(), v.value.clone()))
            .collect()
    }

    pub fn duplicate(&self) -> Self {
        let mut copy = self.clone();
        copy.id = Uuid::new_v4();
        copy.name = format!("{} Copy", self.name);
        copy.favorite = false;
        remint_tree_ids(&mut copy.items);
        copy
    }

    pub fn find_request(&self, id: Uuid) -> Option<&RequestDraft> {
        find_request_in_items(&self.items, id)
    }

    pub fn find_request_mut(&mut self, id: Uuid) -> Option<&mut RequestDraft> {
        find_request_in_items_mut(&mut self.items, id)
    }

    /// Replace request with matching id anywhere in the tree. Returns true if found.
    pub fn update_request(&mut self, draft: RequestDraft) -> bool {
        if let Some(existing) = self.find_request_mut(draft.id) {
            *existing = draft;
            true
        } else {
            false
        }
    }

    /// Insert request at collection root.
    pub fn add_request(&mut self, draft: RequestDraft) {
        self.items.push(CollectionItem::Request(draft));
    }

    pub fn add_folder(&mut self, folder: Folder) {
        self.items.push(CollectionItem::Folder(folder));
    }

    /// Remove a folder or request by id (recursive). Returns true if removed.
    pub fn remove_by_id(&mut self, id: Uuid) -> bool {
        remove_item_by_id(&mut self.items, id)
    }

    /// Rename collection itself, or a folder/request inside it.
    pub fn rename(&mut self, id: Uuid, name: impl Into<String>) -> bool {
        let name = name.into();
        if self.id == id {
            self.name = name;
            return true;
        }
        rename_item_in_items(&mut self.items, id, name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum CollectionItem {
    Folder(Folder),
    Request(RequestDraft),
}

impl CollectionItem {
    pub fn id(&self) -> Uuid {
        match self {
            Self::Folder(f) => f.id,
            Self::Request(r) => r.id,
        }
    }

}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Folder-level auth (used when child request auth is Inherit).
    #[serde(default)]
    pub auth: AuthConfig,
    pub items: Vec<CollectionItem>,
    #[serde(default)]
    pub pre_request_script: String,
    #[serde(default)]
    pub test_script: String,
    #[serde(default)]
    pub sort_key: i64,
}

impl Folder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: String::new(),
            auth: AuthConfig {
                auth_type: AuthType::Inherit,
                ..Default::default()
            },
            items: vec![],
            pre_request_script: String::new(),
            test_script: String::new(),
            sort_key: 0,
        }
    }

    pub fn add_request(&mut self, draft: RequestDraft) {
        self.items.push(CollectionItem::Request(draft));
    }

    #[cfg(test)]
    pub fn add_folder(&mut self, folder: Folder) {
        self.items.push(CollectionItem::Folder(folder));
    }
}

/// Flat row for sidebar rendering (collections → folders → requests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeRowKind {
    Collection,
    Folder,
    Request { method: HttpMethod },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub kind: TreeRowKind,
    pub id: Uuid,
    pub collection_id: Uuid,
    /// Parent collection or folder id; `None` for collection rows.
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub depth: u8,
    /// Collection favorite flag (false for folders/requests).
    pub favorite: bool,
    /// Lower-cased blob of every searchable string on this row (name + URL +
    /// method + params + headers + body + scripts + …). Populated at flatten
    /// time so the sidebar search box can match request *content*, not just
    /// the request's display name.
    pub search_text: String,
}

/// Sort collections: favorites first, then name; within a level: sort_key then folders first then name.
pub fn sort_collections(collections: &mut [Collection]) {
    collections.sort_by(|a, b| {
        b.favorite
            .cmp(&a.favorite)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    for col in collections.iter_mut() {
        sort_items_folders_first(&mut col.items);
    }
}

fn item_sort_key(item: &CollectionItem) -> i64 {
    match item {
        CollectionItem::Folder(f) => f.sort_key,
        CollectionItem::Request(r) => r.sort_key,
    }
}

fn sort_items_folders_first(items: &mut [CollectionItem]) {
    items.sort_by(|a, b| {
        item_sort_key(a)
            .cmp(&item_sort_key(b))
            .then_with(|| match (a, b) {
                (CollectionItem::Folder(_), CollectionItem::Request(_)) => std::cmp::Ordering::Less,
                (CollectionItem::Request(_), CollectionItem::Folder(_)) => {
                    std::cmp::Ordering::Greater
                }
                (CollectionItem::Folder(fa), CollectionItem::Folder(fb)) => {
                    fa.name.to_lowercase().cmp(&fb.name.to_lowercase())
                }
                (CollectionItem::Request(ra), CollectionItem::Request(rb)) => {
                    ra.name.to_lowercase().cmp(&rb.name.to_lowercase())
                }
            })
    });
    for item in items.iter_mut() {
        if let CollectionItem::Folder(f) = item {
            sort_items_folders_first(&mut f.items);
        }
    }
}

/// Reorder `item_id` to sit immediately before `before_id` under the same parent.
/// If `before_id` is None, move to end of sibling list. Returns true on success.
pub fn reorder_before(
    collections: &mut [Collection],
    item_id: Uuid,
    before_id: Option<Uuid>,
) -> bool {
    if before_id == Some(item_id) {
        return false;
    }
    let Some(item) = take_item(collections, item_id) else {
        return false;
    };
    // Find sibling list containing before_id (or original parent if None → end of that list hard).
    // Simpler: find parent of before_id, or if None use parent we had — we already took item.
    // Insert into target list.
    if let Some(before) = before_id {
        if let Some(siblings) = find_sibling_list_mut(collections, before) {
            if let Some(pos) = siblings.iter().position(|i| i.id() == before) {
                siblings.insert(pos, item);
                rekey_sort(siblings);
                return true;
            }
        }
        // before not found — put back at collection root
        if let Some(col) = collections.first_mut() {
            col.items.push(item);
            rekey_sort(&mut col.items);
            return true;
        }
        return false;
    }
    // before_id None: append to first collection (caller should pass parent via before of last sibling)
    if let Some(col) = collections.first_mut() {
        col.items.push(item);
        rekey_sort(&mut col.items);
        true
    } else {
        false
    }
}

fn rekey_sort(items: &mut [CollectionItem]) {
    for (i, item) in items.iter_mut().enumerate() {
        let key = i as i64;
        match item {
            CollectionItem::Folder(f) => f.sort_key = key,
            CollectionItem::Request(r) => r.sort_key = key,
        }
    }
}

fn find_sibling_list_mut(
    collections: &mut [Collection],
    item_id: Uuid,
) -> Option<&mut Vec<CollectionItem>> {
    for col in collections.iter_mut() {
        if col.items.iter().any(|i| i.id() == item_id) {
            return Some(&mut col.items);
        }
        if let Some(list) = find_sibling_list_in_items(&mut col.items, item_id) {
            return Some(list);
        }
    }
    None
}

fn find_sibling_list_in_items(
    items: &mut Vec<CollectionItem>,
    item_id: Uuid,
) -> Option<&mut Vec<CollectionItem>> {
    let mut path = Vec::new();
    if !sibling_path(items, item_id, &mut path) {
        return None;
    }
    let mut cur: &mut Vec<CollectionItem> = items;
    for (depth, &idx) in path.iter().enumerate() {
        if depth + 1 == path.len() {
            // last index is the folder whose `.items` contains the target
            let CollectionItem::Folder(f) = &mut cur[idx] else {
                return None;
            };
            return Some(&mut f.items);
        }
        let CollectionItem::Folder(f) = &mut cur[idx] else {
            return None;
        };
        cur = &mut f.items;
    }
    None
}

/// Path of folder indices from `items` root to the folder that directly contains `item_id`.
fn sibling_path(items: &[CollectionItem], item_id: Uuid, path: &mut Vec<usize>) -> bool {
    for (i, item) in items.iter().enumerate() {
        if let CollectionItem::Folder(f) = item {
            if f.items.iter().any(|x| x.id() == item_id) {
                path.push(i);
                return true;
            }
            path.push(i);
            if sibling_path(&f.items, item_id, path) {
                return true;
            }
            path.pop();
        }
    }
    false
}

/// Filter tree rows whose name or content contains `query` (case-insensitive),
/// keeping ancestors. Requests match against their full draft (URL, method,
/// params, headers, body, scripts, …) so the sidebar search box isn't limited
/// to the request's display name.
pub fn filter_tree_rows(rows: Vec<TreeRow>, query: &str) -> Vec<TreeRow> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return rows;
    }
    let match_ids: std::collections::HashSet<Uuid> = rows
        .iter()
        .filter(|r| r.search_text.contains(&q))
        .map(|r| r.id)
        .collect();
    // Keep matching rows + all ancestors
    let mut keep = match_ids.clone();
    for r in &rows {
        if match_ids.contains(&r.id) {
            let mut pid = r.parent_id;
            while let Some(p) = pid {
                keep.insert(p);
                pid = rows.iter().find(|x| x.id == p).and_then(|x| x.parent_id);
            }
            keep.insert(r.collection_id);
        }
    }
    rows.into_iter().filter(|r| keep.contains(&r.id)).collect()
}

/// Ids of collection/folder nodes that can expand/collapse in the sidebar tree.
pub fn collapsible_tree_ids(collections: &[Collection]) -> std::collections::HashSet<Uuid> {
    let mut out = std::collections::HashSet::new();
    for col in collections {
        out.insert(col.id);
        collect_folder_ids(&col.items, &mut out);
    }
    out
}

fn collect_folder_ids(items: &[CollectionItem], out: &mut std::collections::HashSet<Uuid>) {
    for item in items {
        if let CollectionItem::Folder(folder) = item {
            out.insert(folder.id);
            collect_folder_ids(&folder.items, out);
        }
    }
}

/// Collapse every collapsible id except those in `expanded` (empty `expanded` ⇒ all folded).
pub fn collapsed_except_expanded(
    collections: &[Collection],
    expanded: &std::collections::HashSet<Uuid>,
) -> std::collections::HashSet<Uuid> {
    collapsible_tree_ids(collections)
        .into_iter()
        .filter(|id| !expanded.contains(id))
        .collect()
}

/// Walk all collections into indented rows for the sidebar tree.
pub fn flatten_collections(collections: &[Collection]) -> Vec<TreeRow> {
    flatten_collections_filtered(collections, &std::collections::HashSet::new())
}

/// Like [`flatten_collections`], but skip children of ids in `collapsed`.
pub fn flatten_collections_filtered(
    collections: &[Collection],
    collapsed: &std::collections::HashSet<Uuid>,
) -> Vec<TreeRow> {
    let mut out = Vec::new();
    for col in collections {
        out.push(TreeRow {
            kind: TreeRowKind::Collection,
            id: col.id,
            collection_id: col.id,
            parent_id: None,
            name: col.name.clone(),
            depth: 0,
            favorite: col.favorite,
            search_text: col.name.to_ascii_lowercase(),
        });
        if !collapsed.contains(&col.id) {
            flatten_items(&col.items, col.id, col.id, 1, collapsed, &mut out);
        }
    }
    out
}

fn remint_tree_ids(items: &mut [CollectionItem]) {
    for item in items.iter_mut() {
        match item {
            CollectionItem::Folder(f) => {
                f.id = Uuid::new_v4();
                remint_tree_ids(&mut f.items);
            }
            CollectionItem::Request(r) => {
                r.id = Uuid::new_v4();
                for ex in r.examples.iter_mut() {
                    ex.id = Uuid::new_v4();
                }
            }
        }
    }
}

/// Soft-delete: move collection into trash vec.
pub fn trash_collection(
    collections: &mut Vec<Collection>,
    trash: &mut Vec<Collection>,
    id: Uuid,
) -> bool {
    if let Some(pos) = collections.iter().position(|c| c.id == id) {
        trash.push(collections.remove(pos));
        return true;
    }
    false
}

pub fn restore_from_trash(
    collections: &mut Vec<Collection>,
    trash: &mut Vec<Collection>,
    id: Uuid,
) -> bool {
    if let Some(pos) = trash.iter().position(|c| c.id == id) {
        collections.push(trash.remove(pos));
        sort_collections(collections);
        return true;
    }
    false
}

/// Permanently remove a collection from trash.
pub fn purge_from_trash(trash: &mut Vec<Collection>, id: Uuid) -> bool {
    if let Some(pos) = trash.iter().position(|c| c.id == id) {
        trash.remove(pos);
        return true;
    }
    false
}

/// Insert a request under a collection or folder (`parent_id`).
pub fn add_request_under(
    collections: &mut [Collection],
    parent_id: Uuid,
    draft: RequestDraft,
) -> bool {
    try_insert_item_under(collections, parent_id, CollectionItem::Request(draft)).is_ok()
}

/// Insert a folder under a collection or folder (`parent_id`).
pub fn add_folder_under(collections: &mut [Collection], parent_id: Uuid, folder: Folder) -> bool {
    try_insert_item_under(collections, parent_id, CollectionItem::Folder(folder)).is_ok()
}

/// Reparent a folder or request onto a collection or folder target.
/// Rejects: missing ids, drop on request, drop folder into self/descendant.
pub fn move_item_to(
    collections: &mut [Collection],
    item_id: Uuid,
    target_id: Uuid,
) -> bool {
    if item_id == target_id {
        return false;
    }
    if collections.iter().any(|c| c.id == item_id) {
        return false; // collections move via trash/restore only
    }
    if !is_valid_drop_target(collections, target_id) {
        return false;
    }
    if item_contains_id(collections, item_id, target_id) {
        return false;
    }
    // Already directly under target → no-op success
    if parent_of(collections, item_id) == Some(target_id) {
        return true;
    }

    let Some(item) = take_item(collections, item_id) else {
        return false;
    };
    match try_insert_item_under(collections, target_id, item) {
        Ok(()) => true,
        Err(orphaned) => {
            // Target vanished between checks — attach to first collection root.
            if let Some(col) = collections.first_mut() {
                col.items.push(orphaned);
            }
            false
        }
    }
}

/// Duplicate collection, folder, or request in place. Returns new id.
pub fn duplicate_across(collections: &mut Vec<Collection>, id: Uuid) -> Option<Uuid> {
    if let Some(pos) = collections.iter().position(|c| c.id == id) {
        let copy = collections[pos].duplicate();
        let new_id = copy.id;
        collections.insert(pos + 1, copy);
        return Some(new_id);
    }
    for col in collections.iter_mut() {
        if let Some(new_id) = duplicate_item_in_items(&mut col.items, id) {
            return Some(new_id);
        }
    }
    None
}

/// True when `needle` is `folder_id` itself or nested under that folder.
pub fn item_contains_id(collections: &[Collection], folder_id: Uuid, needle: Uuid) -> bool {
    for col in collections {
        if let Some(items) = find_folder_items(&col.items, folder_id) {
            if folder_id == needle {
                return true;
            }
            return contains_id(items, needle);
        }
    }
    false
}

/// Represents an ancestor node in the collection tree (Folder or Collection)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AncestorAuthOrigin {
    pub id: Uuid,
    pub name: String,
    pub is_collection: bool,
    pub auth: AuthConfig,
}

/// Resolves the effective inherited auth by traversing ancestors from immediate parent up to collection.
/// Returns `Some((effective_auth, origin))` or `None` if all ancestors are Inherit or NoAuth.
pub fn resolve_ancestor_auth(
    collections: &[Collection],
    request_id: Uuid,
) -> Option<(AuthConfig, AncestorAuthOrigin)> {
    for col in collections {
        if col.find_request(request_id).is_some() {
            let folders = folder_path_to_request(col, request_id);
            // Walk from deepest folder upwards to root folder
            for f in folders.iter().rev() {
                if f.auth.auth_type != AuthType::Inherit && f.auth.auth_type != AuthType::NoAuth {
                    return Some((
                        f.auth.clone(),
                        AncestorAuthOrigin {
                            id: f.id,
                            name: f.name.clone(),
                            is_collection: false,
                            auth: f.auth.clone(),
                        },
                    ));
                }
            }
            // If no folder configured auth, check collection
            if col.auth.auth_type != AuthType::Inherit && col.auth.auth_type != AuthType::NoAuth {
                return Some((
                    col.auth.clone(),
                    AncestorAuthOrigin {
                        id: col.id,
                        name: col.name.clone(),
                        is_collection: true,
                        auth: col.auth.clone(),
                    },
                ));
            }
            return None;
        }
    }
    None
}

/// Returns all ancestor IDs (collection ID and all containing folder IDs) for a given request.
pub fn ancestor_ids_for_request(
    collections: &[Collection],
    request_id: Uuid,
) -> Vec<Uuid> {
    for col in collections {
        let path = folder_path_to_request(col, request_id);
        // If request was found under this collection:
        // folder_path_to_request returns empty vec for root-level requests in collection too,
        // so check if the request is actually in this collection.
        if col.find_request(request_id).is_some() {
            let mut ids = Vec::with_capacity(path.len() + 1);
            ids.push(col.id);
            for f in path {
                ids.push(f.id);
            }
            return ids;
        }
    }
    Vec::new()
}

/// Path of folders from collection root to a request (for script inheritance).
pub fn folder_path_to_request(col: &Collection, request_id: Uuid) -> Vec<Folder> {
    let mut path = Vec::new();
    fn walk(
        items: &[CollectionItem],
        request_id: Uuid,
        path: &mut Vec<Folder>,
    ) -> bool {
        for item in items {
            match item {
                CollectionItem::Request(r) if r.id == request_id => return true,
                CollectionItem::Folder(f) => {
                    path.push(f.clone());
                    if walk(&f.items, request_id, path) {
                        return true;
                    }
                    path.pop();
                }
                _ => {}
            }
        }
        false
    }
    walk(&col.items, request_id, &mut path);
    path
}

fn flatten_items(
    items: &[CollectionItem],
    collection_id: Uuid,
    parent_id: Uuid,
    depth: u8,
    collapsed: &std::collections::HashSet<Uuid>,
    out: &mut Vec<TreeRow>,
) {
    for item in items {
        match item {
            CollectionItem::Folder(folder) => {
                out.push(TreeRow {
                    kind: TreeRowKind::Folder,
                    id: folder.id,
                    collection_id,
                    parent_id: Some(parent_id),
                    name: folder.name.clone(),
                    depth,
                    favorite: false,
                    search_text: folder.name.to_ascii_lowercase(),
                });
                if !collapsed.contains(&folder.id) {
                    flatten_items(
                        &folder.items,
                        collection_id,
                        folder.id,
                        depth.saturating_add(1),
                        collapsed,
                        out,
                    );
                }
            }
            CollectionItem::Request(req) => {
                out.push(TreeRow {
                    kind: TreeRowKind::Request { method: req.method },
                    id: req.id,
                    collection_id,
                    parent_id: Some(parent_id),
                    name: req.name.clone(),
                    depth,
                    favorite: false,
                    search_text: req.search_text(),
                });
            }
        }
    }
}

fn is_valid_drop_target(collections: &[Collection], target_id: Uuid) -> bool {
    if collections.iter().any(|c| c.id == target_id) {
        return true;
    }
    for col in collections {
        if find_folder_items(&col.items, target_id).is_some() {
            return true;
        }
    }
    false
}

fn parent_of(collections: &[Collection], item_id: Uuid) -> Option<Uuid> {
    for col in collections {
        if col.items.iter().any(|i| i.id() == item_id) {
            return Some(col.id);
        }
        if let Some(p) = parent_of_in_items(&col.items, item_id) {
            return Some(p);
        }
    }
    None
}

fn parent_of_in_items(items: &[CollectionItem], item_id: Uuid) -> Option<Uuid> {
    for item in items {
        if let CollectionItem::Folder(f) = item {
            if f.items.iter().any(|i| i.id() == item_id) {
                return Some(f.id);
            }
            if let Some(p) = parent_of_in_items(&f.items, item_id) {
                return Some(p);
            }
        }
    }
    None
}

fn contains_id(items: &[CollectionItem], needle: Uuid) -> bool {
    for item in items {
        if item.id() == needle {
            return true;
        }
        if let CollectionItem::Folder(f) = item {
            if contains_id(&f.items, needle) {
                return true;
            }
        }
    }
    false
}

fn find_folder_items(items: &[CollectionItem], folder_id: Uuid) -> Option<&[CollectionItem]> {
    for item in items {
        if let CollectionItem::Folder(f) = item {
            if f.id == folder_id {
                return Some(&f.items);
            }
            if let Some(found) = find_folder_items(&f.items, folder_id) {
                return Some(found);
            }
        }
    }
    None
}

fn take_item(collections: &mut [Collection], id: Uuid) -> Option<CollectionItem> {
    for col in collections.iter_mut() {
        if let Some(item) = take_item_from_items(&mut col.items, id) {
            return Some(item);
        }
    }
    None
}

fn take_item_from_items(items: &mut Vec<CollectionItem>, id: Uuid) -> Option<CollectionItem> {
    if let Some(pos) = items.iter().position(|i| i.id() == id) {
        return Some(items.remove(pos));
    }
    for item in items.iter_mut() {
        if let CollectionItem::Folder(f) = item {
            if let Some(taken) = take_item_from_items(&mut f.items, id) {
                return Some(taken);
            }
        }
    }
    None
}

#[allow(clippy::result_large_err)]
fn try_insert_item_under(
    collections: &mut [Collection],
    parent_id: Uuid,
    item: CollectionItem,
) -> Result<(), CollectionItem> {
    let mut pending = Some(item);
    for col in collections.iter_mut() {
        if col.id == parent_id {
            if let Some(it) = pending.take() {
                col.items.push(it);
                return Ok(());
            }
        }
        if let Some(it) = pending.take() {
            match try_insert_under_items(&mut col.items, parent_id, it) {
                Ok(()) => return Ok(()),
                Err(back) => pending = Some(back),
            }
        }
    }
    Err(pending.expect("item not consumed"))
}

#[allow(clippy::result_large_err)]
fn try_insert_under_items(
    items: &mut [CollectionItem],
    parent_id: Uuid,
    item: CollectionItem,
) -> Result<(), CollectionItem> {
    for existing in items.iter_mut() {
        if let CollectionItem::Folder(f) = existing {
            if f.id == parent_id {
                f.items.push(item);
                return Ok(());
            }
        }
    }
    let mut pending = Some(item);
    for existing in items.iter_mut() {
        if let CollectionItem::Folder(f) = existing {
            if let Some(it) = pending.take() {
                match try_insert_under_items(&mut f.items, parent_id, it) {
                    Ok(()) => return Ok(()),
                    Err(back) => pending = Some(back),
                }
            }
        }
    }
    Err(pending.expect("item not consumed"))
}

fn duplicate_item_in_items(items: &mut Vec<CollectionItem>, id: Uuid) -> Option<Uuid> {
    if let Some(pos) = items.iter().position(|i| i.id() == id) {
        let mut copy = items[pos].clone();
        let new_id = remint_item_ids(&mut copy);
        match &mut copy {
            CollectionItem::Folder(f) => f.name = format!("{} Copy", f.name),
            CollectionItem::Request(r) => r.name = format!("{} Copy", r.name),
        }
        items.insert(pos + 1, copy);
        return Some(new_id);
    }
    for item in items.iter_mut() {
        if let CollectionItem::Folder(f) = item {
            if let Some(new_id) = duplicate_item_in_items(&mut f.items, id) {
                return Some(new_id);
            }
        }
    }
    None
}

fn remint_item_ids(item: &mut CollectionItem) -> Uuid {
    match item {
        CollectionItem::Folder(f) => {
            f.id = Uuid::new_v4();
            remint_tree_ids(&mut f.items);
            f.id
        }
        CollectionItem::Request(r) => {
            r.id = Uuid::new_v4();
            for ex in r.examples.iter_mut() {
                ex.id = Uuid::new_v4();
            }
            r.id
        }
    }
}

/// Find a request across collections. Returns `(collection_id, draft)`.
pub fn find_request_across(
    collections: &[Collection],
    request_id: Uuid,
) -> Option<(Uuid, &RequestDraft)> {
    for col in collections {
        if let Some(req) = col.find_request(request_id) {
            return Some((col.id, req));
        }
    }
    None
}

/// Tab / chrome label: `request name | parent folder name` (parent = direct folder or collection root).
pub fn request_tab_display_name(collections: &[Collection], request_id: Uuid) -> String {
    let rows = flatten_collections(collections);
    let Some(req) = rows.iter().find(|r| r.id == request_id) else {
        return find_request_across(collections, request_id)
            .map(|(_, d)| d.name.clone())
            .unwrap_or_else(|| "Request".into());
    };
    let parent_label = req
        .parent_id
        .and_then(|pid| rows.iter().find(|r| r.id == pid).map(|r| r.name.clone()));
    match parent_label {
        Some(parent) => format!("{} | {}", req.name, parent),
        None => req.name.clone(),
    }
}

/// Whether deleting `id` should show a confirmation dialog (empty folders skip confirm).
pub fn delete_needs_confirmation(collections: &[Collection], id: Uuid) -> bool {
    if collections.iter().any(|c| c.id == id) {
        return true;
    }
    if let Some(count) = folder_child_count(collections, id) {
        return count > 0;
    }
    find_request_across(collections, id).is_some()
}

/// Number of direct children under a folder, if `folder_id` is a folder.
pub fn folder_child_count(collections: &[Collection], folder_id: Uuid) -> Option<usize> {
    for col in collections {
        if let Some(items) = find_folder_items(&col.items, folder_id) {
            return Some(items.len());
        }
    }
    None
}

/// Deep-clone a folder or request with fresh ids (for cut/copy/paste).
pub fn snapshot_tree_item(collections: &[Collection], id: Uuid) -> Option<CollectionItem> {
    for col in collections {
        if let Some(item) = find_item_in_items(&col.items, id) {
            let mut copy = item.clone();
            remint_item_ids(&mut copy);
            return Some(copy);
        }
    }
    None
}

/// Assign fresh ids before inserting a clipboard duplicate (safe for repeated paste).
pub fn remint_pasted_item(item: &mut CollectionItem) -> Uuid {
    remint_item_ids(item)
}

/// Insert a tree item under a collection or folder parent. Returns false if parent missing.
pub fn insert_item_under(
    collections: &mut [Collection],
    parent_id: Uuid,
    item: CollectionItem,
) -> bool {
    try_insert_item_under(collections, parent_id, item).is_ok()
}

/// Parent collection/folder id to insert clipboard items under for the given tree row.
pub fn paste_target_parent_id(collections: &[Collection], item_id: Uuid) -> Option<Uuid> {
    if collections.iter().any(|c| c.id == item_id) {
        return Some(item_id);
    }
    let rows = flatten_collections(collections);
    rows.iter().find(|r| r.id == item_id).and_then(|r| match r.kind {
        TreeRowKind::Collection | TreeRowKind::Folder => Some(r.id),
        TreeRowKind::Request { .. } => r.parent_id,
    })
}

fn find_item_in_items(items: &[CollectionItem], id: Uuid) -> Option<&CollectionItem> {
    for item in items {
        if item.id() == id {
            return Some(item);
        }
        if let CollectionItem::Folder(f) = item {
            if let Some(found) = find_item_in_items(&f.items, id) {
                return Some(found);
            }
        }
    }
    None
}

/// Update existing request by id, or append to `collection_id` root if not found.
/// Returns the collection id that owns the request after the write.
pub fn save_request_to_collections(
    collections: &mut Vec<Collection>,
    collection_id: Option<Uuid>,
    draft: RequestDraft,
) -> Uuid {
    for col in collections.iter_mut() {
        if col.update_request(draft.clone()) {
            return col.id;
        }
    }

    let target_idx = collection_id
        .and_then(|id| collections.iter().position(|c| c.id == id))
        .or_else(|| (!collections.is_empty()).then_some(0));

    if let Some(idx) = target_idx {
        let id = collections[idx].id;
        collections[idx].add_request(draft);
        id
    } else {
        let mut col = Collection::new("My Collection");
        let id = col.id;
        col.add_request(draft);
        collections.push(col);
        id
    }
}

pub fn remove_across(collections: &mut Vec<Collection>, id: Uuid) -> bool {
    if let Some(pos) = collections.iter().position(|c| c.id == id) {
        collections.remove(pos);
        return true;
    }
    for col in collections.iter_mut() {
        if col.remove_by_id(id) {
            return true;
        }
    }
    false
}

pub fn rename_across(collections: &mut [Collection], id: Uuid, name: String) -> bool {
    for col in collections.iter_mut() {
        if col.rename(id, name.clone()) {
            return true;
        }
    }
    false
}

/// Seed used when snapshot has no collections yet.
pub fn demo_collections() -> Vec<Collection> {
    let mut mine = Collection::new("My Collection");
    let get = RequestDraft {
        name: "httpbin GET".into(),
        url: "{{baseUrl}}/get".into(),
        ..Default::default()
    };
    mine.add_request(get);

    let mut folder = Folder::new("Examples");
    let mut post = RequestDraft {
        name: "httpbin POST".into(),
        method: HttpMethod::Post,
        url: "{{baseUrl}}/post".into(),
        body_mode: super::request::BodyMode::Raw,
        body: r#"{"hello":"world"}"#.into(),
        ..Default::default()
    };
    post.headers.push(KeyValue::new("Content-Type", "application/json"));
    folder.add_request(post);
    mine.add_folder(folder);

    vec![mine]
}

fn find_request_in_items(items: &[CollectionItem], id: Uuid) -> Option<&RequestDraft> {
    for item in items {
        match item {
            CollectionItem::Request(r) if r.id == id => return Some(r),
            CollectionItem::Folder(f) => {
                if let Some(r) = find_request_in_items(&f.items, id) {
                    return Some(r);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_request_in_items_mut(items: &mut [CollectionItem], id: Uuid) -> Option<&mut RequestDraft> {
    for item in items.iter_mut() {
        match item {
            CollectionItem::Request(r) if r.id == id => return Some(r),
            CollectionItem::Folder(f) => {
                if let Some(r) = find_request_in_items_mut(&mut f.items, id) {
                    return Some(r);
                }
            }
            _ => {}
        }
    }
    None
}

fn remove_item_by_id(items: &mut Vec<CollectionItem>, id: Uuid) -> bool {
    if let Some(pos) = items.iter().position(|i| i.id() == id) {
        items.remove(pos);
        return true;
    }
    for item in items.iter_mut() {
        if let CollectionItem::Folder(f) = item {
            if remove_item_by_id(&mut f.items, id) {
                return true;
            }
        }
    }
    false
}

#[allow(clippy::collapsible_match)]
fn rename_item_in_items(items: &mut [CollectionItem], id: Uuid, name: String) -> bool {
    for item in items.iter_mut() {
        match item {
            CollectionItem::Folder(f) if f.id == id => {
                f.name = name;
                return true;
            }
            CollectionItem::Request(r) if r.id == id => {
                r.name = name;
                return true;
            }
            CollectionItem::Folder(f) => {
                if rename_item_in_items(&mut f.items, id, name.clone()) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_nested_tree() {
        let cols = demo_collections();
        let rows = flatten_collections(&cols);
        assert!(rows.len() >= 4);
        assert!(matches!(rows[0].kind, TreeRowKind::Collection));
        assert!(rows.iter().any(|r| matches!(r.kind, TreeRowKind::Folder)));
        assert!(rows
            .iter()
            .any(|r| matches!(r.kind, TreeRowKind::Request { .. })));
    }

    #[test]
    fn update_nested_request() {
        let mut cols = demo_collections();
        let (col_id, req) = find_request_across(&cols, {
            let rows = flatten_collections(&cols);
            rows.iter()
                .find(|r| matches!(r.kind, TreeRowKind::Request { .. }))
                .unwrap()
                .id
        })
        .map(|(c, r)| (c, r.clone()))
        .unwrap();

        let mut updated = req;
        updated.url = "https://example.com".into();
        let owner = save_request_to_collections(&mut cols, Some(col_id), updated.clone());
        assert_eq!(owner, col_id);
        let found = find_request_across(&cols, updated.id).unwrap().1;
        assert_eq!(found.url, "https://example.com");
    }

    #[test]
    fn save_new_creates_collection_when_empty() {
        let mut cols = Vec::new();
        let draft = RequestDraft::default();
        let id = draft.id;
        save_request_to_collections(&mut cols, None, draft);
        assert_eq!(cols.len(), 1);
        assert!(cols[0].find_request(id).is_some());
    }

    #[test]
    fn remove_folder_and_rename() {
        let mut cols = demo_collections();
        let folder_id = flatten_collections(&cols)
            .into_iter()
            .find(|r| matches!(r.kind, TreeRowKind::Folder))
            .unwrap()
            .id;
        assert!(rename_across(&mut cols, folder_id, "Renamed".into()));
        assert!(remove_across(&mut cols, folder_id));
        assert!(!flatten_collections(&cols)
            .iter()
            .any(|r| r.id == folder_id));
    }

    #[test]
    fn rename_sets_explicit_name_without_duplicating() {
        let mut cols = demo_collections();
        let before = flatten_collections(&cols).len();
        let req_id = flatten_collections(&cols)
            .into_iter()
            .find(|r| matches!(r.kind, TreeRowKind::Request { .. }))
            .unwrap()
            .id;

        assert!(rename_across(&mut cols, req_id, "Alpha".into()));
        assert_eq!(flatten_collections(&cols).len(), before);
        let row = flatten_collections(&cols)
            .into_iter()
            .find(|r| r.id == req_id)
            .unwrap();
        assert_eq!(row.name, "Alpha");

        // Second rename replaces name; still one item (no numbered suffix / duplicate).
        assert!(rename_across(&mut cols, req_id, "Beta".into()));
        assert_eq!(flatten_collections(&cols).len(), before);
        let row = flatten_collections(&cols)
            .into_iter()
            .find(|r| r.id == req_id)
            .unwrap();
        assert_eq!(row.name, "Beta");
        assert!(!flatten_collections(&cols)
            .iter()
            .any(|r| r.name.contains(" (2)") || r.name.ends_with(" Copy") && r.id == req_id));

        let col_id = cols[0].id;
        assert!(rename_across(&mut cols, col_id, "My Renamed Col".into()));
        assert_eq!(cols[0].name, "My Renamed Col");
        assert_eq!(flatten_collections(&cols).len(), before);
    }

    #[test]
    fn add_under_nested_folder() {
        let mut cols = demo_collections();
        let folder_id = flatten_collections(&cols)
            .into_iter()
            .find(|r| matches!(r.kind, TreeRowKind::Folder))
            .unwrap()
            .id;
        let mut parent = Folder::new("Nested");
        let nested_id = parent.id;
        parent.add_folder(Folder::new("ViaApi"));
        assert!(add_folder_under(&mut cols, folder_id, parent));
        let req = RequestDraft {
            name: "Deep".into(),
            ..Default::default()
        };
        let req_id = req.id;
        assert!(add_request_under(&mut cols, nested_id, req));
        let row = flatten_collections(&cols)
            .into_iter()
            .find(|r| r.id == req_id)
            .unwrap();
        assert_eq!(row.parent_id, Some(nested_id));
        assert!(row.depth >= 3);
    }

    #[test]
    fn move_request_to_folder_and_back() {
        let mut cols = demo_collections();
        let col_id = cols[0].id;
        let folder_id = flatten_collections(&cols)
            .into_iter()
            .find(|r| matches!(r.kind, TreeRowKind::Folder))
            .unwrap()
            .id;
        let root_req = flatten_collections(&cols)
            .into_iter()
            .find(|r| matches!(r.kind, TreeRowKind::Request { .. }) && r.parent_id == Some(col_id))
            .unwrap()
            .id;
        assert!(move_item_to(&mut cols, root_req, folder_id));
        let row = flatten_collections(&cols)
            .into_iter()
            .find(|r| r.id == root_req)
            .unwrap();
        assert_eq!(row.parent_id, Some(folder_id));
        assert!(move_item_to(&mut cols, root_req, col_id));
        let row = flatten_collections(&cols)
            .into_iter()
            .find(|r| r.id == root_req)
            .unwrap();
        assert_eq!(row.parent_id, Some(col_id));
    }

    #[test]
    fn move_folder_rejects_self_and_descendant() {
        let mut cols = demo_collections();
        let folder_id = flatten_collections(&cols)
            .into_iter()
            .find(|r| matches!(r.kind, TreeRowKind::Folder))
            .unwrap()
            .id;
        let nested = Folder::new("Child");
        let child_id = nested.id;
        assert!(add_folder_under(&mut cols, folder_id, nested));
        assert!(!move_item_to(&mut cols, folder_id, folder_id));
        assert!(!move_item_to(&mut cols, folder_id, child_id));
        // sibling-style: move child to collection root OK
        let col_id = cols[0].id;
        assert!(move_item_to(&mut cols, child_id, col_id));
    }

    #[test]
    fn duplicate_folder_and_request() {
        let mut cols = demo_collections();
        let folder_id = flatten_collections(&cols)
            .into_iter()
            .find(|r| matches!(r.kind, TreeRowKind::Folder))
            .unwrap()
            .id;
        let before = flatten_collections(&cols).len();
        let new_folder = duplicate_across(&mut cols, folder_id).unwrap();
        assert_ne!(new_folder, folder_id);
        assert!(flatten_collections(&cols).len() > before);

        let req_id = flatten_collections(&cols)
            .into_iter()
            .find(|r| matches!(r.kind, TreeRowKind::Request { .. }))
            .unwrap()
            .id;
        let new_req = duplicate_across(&mut cols, req_id).unwrap();
        assert_ne!(new_req, req_id);
    }

    #[test]
    fn trash_restore_purge() {
        let mut cols = demo_collections();
        let mut trash = Vec::new();
        let id = cols[0].id;
        assert!(trash_collection(&mut cols, &mut trash, id));
        assert!(cols.is_empty());
        assert_eq!(trash.len(), 1);
        assert!(restore_from_trash(&mut cols, &mut trash, id));
        assert_eq!(cols.len(), 1);
        assert!(trash.is_empty());
        assert!(trash_collection(&mut cols, &mut trash, id));
        assert!(purge_from_trash(&mut trash, id));
        assert!(trash.is_empty());
    }

    #[test]
    fn flatten_respects_collapsed() {
        let cols = demo_collections();
        let folder_id = flatten_collections(&cols)
            .into_iter()
            .find(|r| matches!(r.kind, TreeRowKind::Folder))
            .unwrap()
            .id;
        let mut collapsed = std::collections::HashSet::new();
        collapsed.insert(folder_id);
        let full = flatten_collections(&cols);
        let filtered = flatten_collections_filtered(&cols, &collapsed);
        assert!(filtered.len() < full.len());
        assert!(!filtered
            .iter()
            .any(|r| r.parent_id == Some(folder_id)));
    }

    #[test]
    fn default_tree_all_collapsed_via_empty_expanded() {
        let cols = demo_collections();
        let ids = collapsible_tree_ids(&cols);
        assert!(!ids.is_empty());
        let collapsed = collapsed_except_expanded(&cols, &std::collections::HashSet::new());
        assert_eq!(collapsed, ids);
        let folded = flatten_collections_filtered(&cols, &collapsed);
        // Only collection roots — no nested folders/requests.
        assert!(folded.iter().all(|r| matches!(r.kind, TreeRowKind::Collection)));
        assert_eq!(folded.len(), cols.len());
    }

    #[test]
    fn delete_confirmation_rules() {
        let mut cols = demo_collections();
        let empty_folder = Folder::new("Empty");
        let empty_id = empty_folder.id;
        let col_id = cols[0].id;
        assert!(add_folder_under(&mut cols, col_id, empty_folder));
        assert!(!delete_needs_confirmation(&cols, empty_id));

        let mut nested = Folder::new("Has child");
        let nested_id = nested.id;
        nested.add_request(RequestDraft::default());
        assert!(add_folder_under(&mut cols, col_id, nested));
        assert!(delete_needs_confirmation(&cols, nested_id));

        let req_id = flatten_collections(&cols)
            .into_iter()
            .find(|r| matches!(r.kind, TreeRowKind::Request { .. }))
            .unwrap()
            .id;
        assert!(delete_needs_confirmation(&cols, req_id));
        assert!(delete_needs_confirmation(&cols, col_id));
    }

    #[test]
    fn request_tab_title_includes_parent() {
        let mut cols = demo_collections();
        let col_id = cols[0].id;
        let folder = Folder::new("Parent Folder");
        let folder_id = folder.id;
        assert!(add_folder_under(&mut cols, col_id, folder));
        let req = RequestDraft {
            name: "My Req".into(),
            ..Default::default()
        };
        let req_id = req.id;
        assert!(add_request_under(&mut cols, folder_id, req));
        let title = request_tab_display_name(&cols, req_id);
        assert_eq!(title, "My Req | Parent Folder");
    }

    #[test]
    fn test_ancestor_ids_for_request() {
        let mut cols = demo_collections();
        let col_id = cols[0].id;
        let folder = Folder::new("Folder A");
        let folder_id = folder.id;
        assert!(add_folder_under(&mut cols, col_id, folder));
        let req = RequestDraft::default();
        let req_id = req.id;
        assert!(add_request_under(&mut cols, folder_id, req));

        let ancestors = ancestor_ids_for_request(&cols, req_id);
        assert_eq!(ancestors, vec![col_id, folder_id]);
    }

    #[test]
    fn test_resolve_ancestor_auth_multilevel() {
        let mut cols = demo_collections();
        let col_id = cols[0].id;
        cols[0].auth = AuthConfig {
            auth_type: AuthType::Bearer,
            token: "col-bearer-token".into(),
            ..Default::default()
        };

        // Create outer folder inheriting from collection
        let mut outer = Folder::new("Outer Folder");
        outer.auth.auth_type = AuthType::Inherit;
        let outer_id = outer.id;
        assert!(add_folder_under(&mut cols, col_id, outer));

        // Create inner folder with ApiKey override
        let mut inner = Folder::new("Inner Folder");
        inner.auth = AuthConfig {
            auth_type: AuthType::ApiKey,
            api_key_header: "X-Inner-Key".into(),
            token: "inner-secret".into(),
            ..Default::default()
        };
        let inner_id = inner.id;
        assert!(add_folder_under(&mut cols, outer_id, inner));

        // Create request in inner folder inheriting auth
        let mut req = RequestDraft::default();
        req.auth.auth_type = AuthType::Inherit;
        let req_id = req.id;
        assert!(add_request_under(&mut cols, inner_id, req));

        // Should resolve to Inner Folder auth
        let resolved = resolve_ancestor_auth(&cols, req_id);
        assert!(resolved.is_some());
        let (auth, origin) = resolved.unwrap();
        assert_eq!(origin.id, inner_id);
        assert_eq!(origin.name, "Inner Folder");
        assert!(!origin.is_collection);
        assert_eq!(auth.auth_type, AuthType::ApiKey);
        assert_eq!(auth.api_key_header, "X-Inner-Key");
        assert_eq!(auth.token, "inner-secret");

        // Now test request directly in outer folder inheriting auth -> should resolve to collection
        let mut req_outer = RequestDraft::default();
        req_outer.auth.auth_type = AuthType::Inherit;
        let req_outer_id = req_outer.id;
        assert!(add_request_under(&mut cols, outer_id, req_outer));

        let resolved_outer = resolve_ancestor_auth(&cols, req_outer_id);
        assert!(resolved_outer.is_some());
        let (auth_col, origin_col) = resolved_outer.unwrap();
        assert_eq!(origin_col.id, col_id);
        assert!(origin_col.is_collection);
        assert_eq!(auth_col.auth_type, AuthType::Bearer);
        assert_eq!(auth_col.token, "col-bearer-token");
    }

    #[test]
    fn favorite_name_not_prefixed_in_tree_row() {
        let mut cols = demo_collections();
        cols[0].favorite = true;
        let row = &flatten_collections(&cols)[0];
        assert!(row.favorite);
        assert!(!row.name.starts_with('★'));
        assert_eq!(row.name, cols[0].name);
    }
}
