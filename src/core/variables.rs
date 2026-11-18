//! Variable scopes: global / collection / environment / local / vault.
//! Resolution order matches Postman product-docs convention (reports/04, 07).
//! `{{vault:key}}` resolved via vault map (file / SecretsStore) — see `vault.rs`.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;
use uuid::Uuid;

use super::collection::Collection;
use super::environment::{EnvVar, Environment};
use super::request::KeyValue;
use super::vault::{resolve_vault_key, VaultStore};
use crate::secrets::SecretsStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableScope {
    Global,
    Collection,
    Environment,
    Local,
}

/// Scope shown in the variable hover popover (includes vault + unresolved).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VarResolveScope {
    Path,
    Local,
    Environment,
    Collection,
    Global,
    Vault,
    Dynamic,
    Unresolved,
}

impl VarResolveScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Local => "local",
            Self::Environment => "environment",
            Self::Collection => "collection",
            Self::Global => "global",
            Self::Vault => "vault",
            Self::Dynamic => "dynamic",
            Self::Unresolved => "unresolved",
        }
    }
}

/// Byte span of a `{{…}}` token inside text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarSpan {
    pub start: usize,
    pub end: usize,
    /// Trimmed inner content, e.g. `baseUrl` or `vault:token`.
    pub raw: String,
}

/// Resolved hover target for editing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarHoverInfo {
    /// Display name (vault key without `vault:` prefix).
    pub name: String,
    /// Full inner raw (`vault:key` or plain name).
    pub raw: String,
    pub scope: VarResolveScope,
    pub value: String,
}

impl VarHoverInfo {
    pub fn is_unresolved(&self) -> bool {
        self.scope == VarResolveScope::Unresolved
    }
}

fn var_token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\{\{\s*([^}]+?)\s*\}\}").unwrap())
}

/// Clamp index within `[0, text.len()]` and snap downwards to nearest valid UTF-8 char boundary.
pub fn snap_to_char_boundary(text: &str, mut index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// Convert a UTF-16 code unit offset (e.g. from DOM `selectionStart`) to UTF-8 byte offset.
#[cfg(test)]
pub fn utf16_offset_to_byte_offset(text: &str, utf16_offset: usize) -> usize {
    let mut u16_count = 0;
    for (byte_idx, ch) in text.char_indices() {
        if u16_count >= utf16_offset {
            return byte_idx;
        }
        u16_count += ch.len_utf16();
    }
    text.len()
}

/// All non-empty `{{…}}` spans in `text` (left → right).
pub fn iter_var_spans(text: &str) -> impl Iterator<Item = VarSpan> + '_ {
    var_token_re().captures_iter(text).filter_map(|caps| {
        let m = caps.get(0)?;
        let raw = caps.get(1)?.as_str().trim().to_string();
        if raw.is_empty() {
            return None;
        }
        Some(VarSpan {
            start: m.start(),
            end: m.end(),
            raw,
        })
    })
}

/// All variable spans in `text`, including `{{…}}` and path variables `/:paramName`.
pub fn iter_all_var_spans(text: &str) -> Vec<VarSpan> {
    let mut spans: Vec<VarSpan> = iter_var_spans(text).collect();

    let path_end = text.find('?').unwrap_or(text.len());
    let path_text = &text[..path_end];

    static PATH_VAR_RE: OnceLock<Regex> = OnceLock::new();
    let re = PATH_VAR_RE.get_or_init(|| {
        Regex::new(r"/(?P<token>:(?P<name>[a-zA-Z_][a-zA-Z0-9_]*))").unwrap()
    });

    for cap in re.captures_iter(path_text) {
        if let (Some(token_m), Some(name_m)) = (cap.name("token"), cap.name("name")) {
            let start = token_m.start();
            let end = token_m.end();
            if !spans.iter().any(|s| (start >= s.start && start < s.end) || (end > s.start && end <= s.end)) {
                spans.push(VarSpan {
                    start,
                    end,
                    raw: name_m.as_str().to_string(),
                });
            }
        }
    }

    spans.sort_by_key(|s| s.start);
    spans
}

/// Find `{{name}}`, `{{vault:key}}`, or `:pathVar` span containing byte `index`.
/// Index may equal `end` (caret after closing `}}` or param name).
pub fn find_var_at(text: &str, index: usize) -> Option<VarSpan> {
    let index = snap_to_char_boundary(text, index);
    iter_all_var_spans(text).into_iter().find(|span| index >= span.start && index <= span.end)
}

/// Caret is inside an incomplete `{{…}}` token → `(byte index of `{`, partial name)`.
/// Safely handles UTF-8 character boundaries if caret is byte or char offset.
pub fn var_insert_context(text: &str, caret: usize) -> Option<(usize, String)> {
    let caret = snap_to_char_boundary(text, caret);
    let before = &text[..caret];
    let open = before.rfind("{{")?;
    let tail = &before[open + 2..];
    if tail.contains("}}") {
        return None;
    }
    Some((open, tail.trim_start().to_string()))
}

/// Replace incomplete `{{…}}` at `open` through `caret` with `{{insert_name}}`.
/// Strips existing closing `}}` or `}` if present right after the replaced token.
pub fn apply_var_insert(text: &str, caret: usize, open: usize, insert_name: &str) -> (String, usize) {
    let open = snap_to_char_boundary(text, open);
    let caret = snap_to_char_boundary(text, caret.max(open));
    let rest = &text[caret..];
    let replace_end = if let Some(close_rel) = rest.find("}}") {
        let candidate = caret + close_rel + 2;
        let between = &rest[..close_rel];
        if !between.contains("{{") && !between.contains('\n') {
            candidate
        } else {
            caret
        }
    } else if rest.starts_with('}') {
        caret + 1
    } else {
        caret
    };
    let token = format!("{{{{{}}}}}", insert_name.trim());
    let mut out = String::with_capacity(text.len() + token.len());
    out.push_str(&text[..open]);
    out.push_str(&token);
    out.push_str(&text[replace_end..]);
    let new_caret = open + token.len();
    (out, new_caret)
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// HTML for a highlight mirror: plain text escaped, `{{…}}` wrapped in
/// `<span class="var-token">` (adds ` unresolved` when `vars` marks it unresolved),
/// and `:pathVar` wrapped in `<span class="var-token path-var-token">`.
pub fn render_var_highlight_html(text: &str, vars: Option<&VariableContext>) -> String {
    let mut html = String::with_capacity(text.len().saturating_mul(2));
    let mut cursor = 0;
    for span in iter_all_var_spans(text) {
        if span.start > cursor {
            html.push_str(&escape_html(&text[cursor..span.start]));
        }
        let token = &text[span.start..span.end];
        let is_path_var = token.starts_with(':');
        let (class, title) = match vars {
            Some(v) => {
                let info = v.inspect_var(&span.raw);
                if info.scope == VarResolveScope::Path {
                    let val = if info.value.is_empty() { "(empty)" } else { &info.value };
                    ("var-token path-var-token", format!("{}: {} (path)", span.raw, val))
                } else if is_path_var {
                    ("var-token path-var-token", format!("{}: (path)", span.raw))
                } else if info.is_unresolved() {
                    ("var-token unresolved", format!("{}: (unresolved)", span.raw))
                } else {
                    ("var-token", format!("{}: {} ({})", span.raw, info.value, info.scope.label()))
                }
            }
            None => {
                if is_path_var {
                    ("var-token path-var-token", format!("{}: (path)", span.raw))
                } else {
                    ("var-token", span.raw.clone())
                }
            }
        };
        html.push_str("<span class=\"");
        html.push_str(class);
        html.push_str("\" title=\"");
        html.push_str(&escape_html(&title));
        html.push_str("\">");
        html.push_str(&escape_html(token));
        html.push_str("</span>");
        cursor = span.end;
    }
    if cursor < text.len() {
        html.push_str(&escape_html(&text[cursor..]));
    }
    // Trailing newline keeps last empty line visible in multiline mirrors.
    if text.ends_with('\n') {
        html.push('\n');
    }
    html
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariableContext {
    pub global: HashMap<String, String>,
    pub collection: HashMap<String, String>,
    pub environment: HashMap<String, String>,
    /// Script / request-local — persisted in app snapshot only; **never** written to git cloud tree.
    pub local: HashMap<String, String>,
    /// Request URL path variables (e.g. `:cabinetID` -> `"123"`).
    #[serde(default)]
    pub path: HashMap<String, String>,
    /// In-memory vault keys (loaded from `vault.json` at startup).
    #[serde(default, skip)]
    pub vault: VaultStore,
}

/// One row in the `{{` variable insertion dropdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableSuggestion {
    /// Display name (vault keys omit the `vault:` prefix in the label).
    pub label: String,
    /// Inner token inserted inside `{{…}}` (e.g. `vault:api_key`).
    pub insert: String,
    pub scope: VarResolveScope,
}

impl VariableContext {
    /// Suggestions for variable insertion UI, filtered by partial name after `{{`.
    ///
    /// @param filter Partial variable name (case-insensitive prefix match); empty shows all.
    /// @returns Sorted list: environment, collection, global, local, then vault keys.
    pub fn insert_suggestions(&self, filter: &str) -> Vec<VariableSuggestion> {
        let needle = filter.trim().to_lowercase();
        let mut out: Vec<VariableSuggestion> = Vec::new();

        let mut push_scope = |scope: VarResolveScope, map: &HashMap<String, String>| {
            for key in map.keys() {
                if key.is_empty() {
                    continue;
                }
                if !needle.is_empty() && !key.to_lowercase().starts_with(&needle) {
                    continue;
                }
                out.push(VariableSuggestion {
                    label: key.clone(),
                    insert: key.clone(),
                    scope,
                });
            }
        };

        push_scope(VarResolveScope::Environment, &self.environment);
        push_scope(VarResolveScope::Collection, &self.collection);
        push_scope(VarResolveScope::Global, &self.global);
        push_scope(VarResolveScope::Local, &self.local);

        for key in self.vault.entries.keys() {
            if key.is_empty() {
                continue;
            }
            let insert = format!("vault:{key}");
            if !needle.is_empty()
                && !key.to_lowercase().starts_with(&needle)
                && !insert.to_lowercase().starts_with(&needle)
            {
                continue;
            }
            out.push(VariableSuggestion {
                label: format!("vault:{key}"),
                insert,
                scope: VarResolveScope::Vault,
            });
        }

        let dynamic_names = [
            "$guid",
            "$randomUUID",
            "$timestamp",
            "$timestampMs",
            "$isoTimestamp",
            "$date",
            "$randomInt",
            "$randomEmail",
            "$randomFullName",
            "$randomFirstName",
            "$randomLastName",
            "$randomCity",
            "$randomCountry",
            "$randomIP",
            "$randomDomain",
            "$randomUserAgent",
            "$randomSlug",
            "$randomBoolean",
            "$randomPrice",
            "$randomAlphaNumeric",
            "$randomPhoneNumber",
            "$randomWord",
            "$randomColor",
            "$randomHexColor",
        ];
        for d in dynamic_names {
            if needle.is_empty() || d.to_lowercase().starts_with(&needle) {
                out.push(VariableSuggestion {
                    label: d.to_string(),
                    insert: d.to_string(),
                    scope: VarResolveScope::Dynamic,
                });
            }
        }

        out.sort_by_key(|a| a.label.to_lowercase());
        out
    }

    pub fn demo() -> Self {
        let mut ctx = Self::default();
        ctx.environment
            .insert("baseUrl".into(), "https://httpbin.org".into());
        ctx.global
            .insert("userAgent".into(), "Blitz/0.1".into());
        ctx
    }

    /// Resolve `{{name}}` with precedence: path > local > environment > collection > global.
    /// Prefers non-empty values so empty placeholder keys imported in an environment do not
    /// shadow populated values in collection or global scopes.
    pub fn get(&self, name: &str) -> Option<&str> {
        let scopes = [
            &self.path,
            &self.local,
            &self.environment,
            &self.collection,
            &self.global,
        ];
        scopes
            .iter()
            .filter_map(|m| m.get(name))
            .find(|v| !v.is_empty())
            .or_else(|| scopes.iter().find_map(|m| m.get(name)))
            .map(|s| s.as_str())
    }

    /// Owning scope for a plain (non-vault) name, following resolve precedence.
    /// Prefers non-empty values first so empty placeholder rows don't mask real values.
    pub fn scope_of(&self, name: &str) -> Option<VariableScope> {
        let scopes = [
            (VariableScope::Local, &self.local),
            (VariableScope::Environment, &self.environment),
            (VariableScope::Collection, &self.collection),
            (VariableScope::Global, &self.global),
        ];
        scopes
            .iter()
            .find(|(_, m)| m.get(name).is_some_and(|v| !v.is_empty()))
            .or_else(|| scopes.iter().find(|(_, m)| m.contains_key(name)))
            .map(|(scope, _)| *scope)
    }

    /// Inspect `{{…}}` or `:param` inner raw for hover UI (value + owning scope).
    pub fn inspect_var(&self, raw: &str) -> VarHoverInfo {
        let raw = raw.trim();
        let stripped_name = raw.strip_prefix(':').unwrap_or(raw);
        if let Some(val) = self.path.get(stripped_name) {
            return VarHoverInfo {
                name: stripped_name.to_string(),
                raw: raw.to_string(),
                scope: VarResolveScope::Path,
                value: val.clone(),
            };
        }
        if raw.starts_with(':') {
            return VarHoverInfo {
                name: stripped_name.to_string(),
                raw: raw.to_string(),
                scope: VarResolveScope::Path,
                value: String::new(),
            };
        }
        if raw.starts_with('$') {
            if let Some(dyn_val) = resolve_dynamic_var(raw) {
                return VarHoverInfo {
                    name: raw.to_string(),
                    raw: raw.to_string(),
                    scope: VarResolveScope::Dynamic,
                    value: dyn_val,
                };
            }
        }
        if let Some(key) = raw.strip_prefix("vault:") {
            let key = key.trim();
            let value = self.vault.get(key).unwrap_or("").to_string();
            let scope = if self.vault.get(key).is_some() {
                VarResolveScope::Vault
            } else {
                VarResolveScope::Unresolved
            };
            return VarHoverInfo {
                name: key.to_string(),
                raw: format!("vault:{key}"),
                scope,
                value,
            };
        }

        match self.scope_of(raw) {
            Some(VariableScope::Local) => VarHoverInfo {
                name: raw.to_string(),
                raw: raw.to_string(),
                scope: VarResolveScope::Local,
                value: self.local.get(raw).cloned().unwrap_or_default(),
            },
            Some(VariableScope::Environment) => VarHoverInfo {
                name: raw.to_string(),
                raw: raw.to_string(),
                scope: VarResolveScope::Environment,
                value: self.environment.get(raw).cloned().unwrap_or_default(),
            },
            Some(VariableScope::Collection) => VarHoverInfo {
                name: raw.to_string(),
                raw: raw.to_string(),
                scope: VarResolveScope::Collection,
                value: self.collection.get(raw).cloned().unwrap_or_default(),
            },
            Some(VariableScope::Global) => VarHoverInfo {
                name: raw.to_string(),
                raw: raw.to_string(),
                scope: VarResolveScope::Global,
                value: self.global.get(raw).cloned().unwrap_or_default(),
            },
            None => VarHoverInfo {
                name: raw.to_string(),
                raw: raw.to_string(),
                scope: VarResolveScope::Unresolved,
                value: String::new(),
            },
        }
    }

    /// Update the map for `scope` (vault uses `name` as key).
    pub fn apply_edit(&mut self, scope: VarResolveScope, name: &str, value: &str) {
        match scope {
            VarResolveScope::Local => {
                self.local.insert(name.to_string(), value.to_string());
            }
            VarResolveScope::Environment => {
                self.environment
                    .insert(name.to_string(), value.to_string());
            }
            VarResolveScope::Collection => {
                self.collection
                    .insert(name.to_string(), value.to_string());
            }
            VarResolveScope::Global => {
                self.global.insert(name.to_string(), value.to_string());
            }
            VarResolveScope::Path => {
                self.path.insert(name.to_string(), value.to_string());
            }
            VarResolveScope::Vault => {
                self.vault.set(name, value);
            }
            VarResolveScope::Unresolved | VarResolveScope::Dynamic => {}
        }
    }

    pub const MAX_RESOLVE_DEPTH: usize = 10;

    pub fn resolve_string(&self, input: &str) -> String {
        self.resolve_string_with_secrets(input, None)
    }

    /// Like [`resolve_string`], but `{{vault:key}}` may fall back to SecretsStore.
    /// Supports nested recursive variable expansion up to [`Self::MAX_RESOLVE_DEPTH`]
    /// iterations with cycle protection.
    pub fn resolve_string_with_secrets(
        &self,
        input: &str,
        secrets: Option<&dyn SecretsStore>,
    ) -> String {
        let re = var_token_re();
        let mut current = input.to_string();

        for _ in 0..Self::MAX_RESOLVE_DEPTH {
            if !current.contains("{{") {
                break;
            }

            let mut replaced_any = false;
            let next = re.replace_all(&current, |caps: &regex::Captures| {
                let raw = caps[1].trim();
                replaced_any = true;
                if let Some(key) = raw.strip_prefix("vault:") {
                    resolve_vault_key(&self.vault, secrets, key.trim()).unwrap_or_default()
                } else if raw.starts_with('$') {
                    if let Some(dyn_val) = resolve_dynamic_var(raw) {
                        dyn_val
                    } else {
                        self.get(raw).unwrap_or("").to_string()
                    }
                } else {
                    self.get(raw).unwrap_or("").to_string()
                }
            }).into_owned();

            if next == current || !replaced_any {
                break;
            }
            current = next;
        }

        current
    }

    /// Returns `true` when `raw` inside `{{…}}` refers to a vault variable (`{{vault:key}}`,
    /// `{{$vault:key}}`, or a key resolved exclusively from `self.vault`).
    pub fn is_vault_var_token(&self, raw: &str) -> bool {
        let trimmed = raw.trim();
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("vault:") || lower.starts_with("$vault:") {
            return true;
        }
        let has_non_empty_regular = self.get(trimmed).is_some_and(|v| !v.is_empty());
        !has_non_empty_regular && self.vault.get(trimmed).is_some()
    }

    /// Resolve all non-vault `{{…}}` variables recursively (path/local/env/collection/global
    /// and dynamic `$` variables), leaving vault tokens (`{{vault:…}}`) unexpanded so secrets
    /// are never copied into cURL or code snippets.
    pub fn resolve_string_non_vault(&self, input: &str) -> String {
        let re = var_token_re();
        let mut current = input.to_string();

        for _ in 0..Self::MAX_RESOLVE_DEPTH {
            if !current.contains("{{") {
                break;
            }

            let mut replaced_any = false;
            let next = re
                .replace_all(&current, |caps: &regex::Captures| {
                    let full = &caps[0];
                    let raw = caps[1].trim();
                    if self.is_vault_var_token(raw) {
                        return full.to_string();
                    }
                    if raw.starts_with('$') {
                        if let Some(dyn_val) = resolve_dynamic_var(raw) {
                            replaced_any = true;
                            return dyn_val;
                        }
                    }
                    if let Some(val) = self.get(raw) {
                        replaced_any = true;
                        val.to_string()
                    } else {
                        full.to_string()
                    }
                })
                .into_owned();

            if next == current || !replaced_any {
                break;
            }
            current = next;
        }

        current
    }
}

/// Resolve Postman-compatible dynamic / faker pseudo-variables (e.g. `{{$guid}}`, `{{$timestamp}}`).
pub fn resolve_dynamic_var(name: &str) -> Option<String> {
    let key = name.trim().to_lowercase();
    match key.as_str() {
        "$guid" | "$randomuuid" => Some(Uuid::new_v4().to_string()),
        "$timestamp" => Some(chrono::Utc::now().timestamp().to_string()),
        "$timestampms" => Some(chrono::Utc::now().timestamp_millis().to_string()),
        "$isotimestamp" => Some(chrono::Utc::now().to_rfc3339()),
        "$date" => Some(chrono::Utc::now().format("%Y-%m-%d").to_string()),
        "$randomint" => {
            let n = (Uuid::new_v4().as_u128() % 1000) + 1;
            Some(n.to_string())
        }
        "$randomemail" => {
            let u = Uuid::new_v4().to_string();
            Some(format!("user_{}@example.com", &u[..8]))
        }
        "$randomfirstname" => {
            let list = ["Alice", "Bob", "Carol", "David", "Emma", "Frank", "Grace", "Henry", "Ivy", "Jack"];
            let idx = (Uuid::new_v4().as_u128() as usize) % list.len();
            Some(list[idx].to_string())
        }
        "$randomlastname" => {
            let list = ["Smith", "Johnson", "Williams", "Brown", "Jones", "Garcia", "Miller", "Davis", "Taylor", "Wilson"];
            let idx = (Uuid::new_v4().as_u128() as usize) % list.len();
            Some(list[idx].to_string())
        }
        "$randomfullname" => {
            let first = resolve_dynamic_var("$randomfirstname")?;
            let last = resolve_dynamic_var("$randomlastname")?;
            Some(format!("{first} {last}"))
        }
        "$randomcity" => {
            let list = ["New York", "London", "Tokyo", "Paris", "Berlin", "Sydney", "San Francisco", "Toronto", "Singapore", "Amsterdam"];
            let idx = (Uuid::new_v4().as_u128() as usize) % list.len();
            Some(list[idx].to_string())
        }
        "$randomcountry" => {
            let list = ["United States", "United Kingdom", "Germany", "Japan", "France", "Canada", "Australia", "Netherlands", "Sweden", "Singapore"];
            let idx = (Uuid::new_v4().as_u128() as usize) % list.len();
            Some(list[idx].to_string())
        }
        "$randomip" | "$randomipv4" => {
            let u = Uuid::new_v4().as_u128();
            Some(format!("{}.{}.{}.{}", (u & 0xff).max(1), ((u >> 8) & 0xff), ((u >> 16) & 0xff), ((u >> 24) & 0xff).max(1)))
        }
        "$randomboolean" => {
            let b = (Uuid::new_v4().as_u128() & 1) == 0;
            Some(b.to_string())
        }
        "$randomprice" => {
            let n = (Uuid::new_v4().as_u128() % 9900) + 100;
            Some(format!("{}.{:02}", n / 100, n % 100))
        }
        "$randomalphanumeric" => {
            let s = Uuid::new_v4().to_string().replace('-', "");
            Some(s[..10].to_string())
        }
        "$randomphonenumber" => {
            let n = (Uuid::new_v4().as_u128() % 9000) + 1000;
            Some(format!("+1-555-{:04}", n))
        }
        "$randomword" => {
            let list = ["lorem", "payload", "service", "cluster", "gateway", "token", "request", "client", "response", "metric", "pipeline"];
            let idx = (Uuid::new_v4().as_u128() as usize) % list.len();
            Some(list[idx].to_string())
        }
        "$randomdomain" => {
            let s = Uuid::new_v4().to_string();
            Some(format!("svc-{}.example.com", &s[..6]))
        }
        "$randomuseragent" => {
            let list = [
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
                "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0",
                "Blitz-Client/0.2 (+https://github.com/h3ma209/Blitz)",
            ];
            let idx = (Uuid::new_v4().as_u128() as usize) % list.len();
            Some(list[idx].to_string())
        }
        "$randomslug" => {
            let w1 = resolve_dynamic_var("$randomword")?;
            let w2 = resolve_dynamic_var("$randomword")?;
            let n = (Uuid::new_v4().as_u128() % 900) + 100;
            Some(format!("{w1}-{w2}-{n}"))
        }
        "$randomcolor" | "$randomhexcolor" => {
            let u = Uuid::new_v4().as_u128();
            Some(format!("#{:06x}", u & 0xffffff))
        }
        _ => None,
    }
}

/// Set/update an environment variable's **current** value (creates row if missing).
pub fn apply_env_var_edit(env: &mut Environment, key: &str, value: &str) {
    if let Some(v) = env.values.iter_mut().find(|v| v.key == key) {
        v.current_value = value.to_string();
        v.enabled = true;
    } else {
        env.values.push(EnvVar::new(key, value));
    }
}

/// Write a full environment scope map (from scripts) onto [`Environment::values`].
///
/// - Keys present → upsert current (and create if missing)
/// - Enabled keys missing from map → removed (`blitz.environment.unset` / `.clear`)
/// - Disabled rows are preserved even if absent from the map
pub fn apply_environment_map(env: &mut Environment, map: &HashMap<String, String>) {
    for (k, v) in map {
        if k.is_empty() {
            continue;
        }
        apply_env_var_edit(env, k, v);
    }
    env.values.retain(|ev| {
        ev.key.is_empty() || !ev.enabled || map.contains_key(&ev.key)
    });
}

/// Set/update a collection variable (creates row if missing).
pub fn apply_collection_var_edit(col: &mut Collection, key: &str, value: &str) {
    if let Some(v) = col.variables.iter_mut().find(|v| v.key == key) {
        v.value = value.to_string();
        v.enabled = true;
    } else {
        col.variables.push(KeyValue::new(key, value));
    }
}

/// Write a full collection scope map (from scripts) onto [`Collection::variables`].
pub fn apply_collection_map(col: &mut Collection, map: &HashMap<String, String>) {
    for (k, v) in map {
        if k.is_empty() {
            continue;
        }
        apply_collection_var_edit(col, k, v);
    }
    col.variables.retain(|kv| {
        kv.key.is_empty() || !kv.enabled || map.contains_key(&kv.key)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::vault::VAULT_ACCOUNT_ID;
    use crate::secrets::{service_name, InMemorySecrets};

    #[test]
    fn resolve_env_var() {
        let ctx = VariableContext::demo();
        assert_eq!(
            ctx.resolve_string("{{baseUrl}}/get"),
            "https://httpbin.org/get"
        );
    }

    #[test]
    fn local_overrides_env() {
        let mut ctx = VariableContext::demo();
        ctx.local
            .insert("baseUrl".into(), "http://localhost".into());
        assert_eq!(ctx.resolve_string("{{baseUrl}}"), "http://localhost");
    }

    #[test]
    fn vault_prefix_from_store() {
        let mut ctx = VariableContext::default();
        ctx.vault.set("token", "v-123");
        assert_eq!(ctx.resolve_string("Bearer {{vault:token}}"), "Bearer v-123");
    }

    #[test]
    fn vault_falls_back_to_secrets() {
        let ctx = VariableContext::default();
        let store = InMemorySecrets::default();
        store
            .set(&service_name(VAULT_ACCOUNT_ID), "k", "from-store")
            .unwrap();
        assert_eq!(
            ctx.resolve_string_with_secrets("{{vault:k}}", Some(&store)),
            "from-store"
        );
    }

    #[test]
    fn find_var_at_mid_token() {
        let text = "https://{{endpoint_hakki_user}}/v1";
        let start = text.find("{{").unwrap();
        let span = find_var_at(text, start + 4).unwrap();
        assert_eq!(span.raw, "endpoint_hakki_user");
        assert_eq!(&text[span.start..span.end], "{{endpoint_hakki_user}}");
    }

    #[test]
    fn iter_var_spans_and_highlight_html() {
        let text = "https://{{baseUrl}}/<x>&{{missing}}";
        let spans: Vec<_> = iter_var_spans(text).collect();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].raw, "baseUrl");
        assert_eq!(spans[1].raw, "missing");

        let ctx = VariableContext::demo();
        let html = render_var_highlight_html(text, Some(&ctx));
        assert!(html.contains("<span class=\"var-token\" title=\"baseUrl: https://httpbin.org (environment)\">{{baseUrl}}</span>"));
        assert!(html.contains("<span class=\"var-token unresolved\" title=\"missing: (unresolved)\">{{missing}}</span>"));
        assert!(html.contains("&lt;x&gt;&amp;"));
        assert!(!html.contains("<x>"));
    }

    #[test]
    fn find_var_at_caret_at_end_of_token() {
        let text = "x{{baseUrl}}y";
        let end = text.find("}}").unwrap() + 2;
        let span = find_var_at(text, end).unwrap();
        assert_eq!(span.raw, "baseUrl");
        assert!(find_var_at(text, 0).is_none());
        assert!(find_var_at(text, text.len()).is_none());
    }

    #[test]
    fn find_var_at_vault_and_spaces() {
        let text = "Bearer {{ vault: api_token }}";
        let idx = text.find("vault").unwrap();
        let span = find_var_at(text, idx).unwrap();
        assert_eq!(span.raw, "vault: api_token");
    }

    #[test]
    fn inspect_var_scope_precedence_and_unresolved() {
        let mut ctx = VariableContext::demo();
        ctx.collection
            .insert("baseUrl".into(), "https://collection".into());
        ctx.local
            .insert("baseUrl".into(), "http://localhost".into());
        let info = ctx.inspect_var("baseUrl");
        assert_eq!(info.scope, VarResolveScope::Local);
        assert_eq!(info.value, "http://localhost");

        let miss = ctx.inspect_var("nope");
        assert_eq!(miss.scope, VarResolveScope::Unresolved);
        assert!(miss.value.is_empty());

        ctx.vault.set("tok", "secret");
        let v = ctx.inspect_var("vault:tok");
        assert_eq!(v.scope, VarResolveScope::Vault);
        assert_eq!(v.name, "tok");
        assert_eq!(v.value, "secret");
    }

    #[test]
    fn apply_edit_helpers_update_owners() {
        let mut ctx = VariableContext::demo();
        ctx.apply_edit(VarResolveScope::Environment, "baseUrl", "https://x.test");
        assert_eq!(ctx.environment.get("baseUrl").unwrap(), "https://x.test");

        let mut env = Environment::new("Dev");
        env.values.push(EnvVar::new("baseUrl", "old"));
        apply_env_var_edit(&mut env, "baseUrl", "new");
        assert_eq!(env.values[0].current_value, "new");
        apply_env_var_edit(&mut env, "fresh", "v");
        assert!(env.values.iter().any(|v| v.key == "fresh" && v.effective() == "v"));

        let mut col = Collection::new("C");
        apply_collection_var_edit(&mut col, "path", "/api");
        assert_eq!(col.variables_map().get("path").unwrap(), "/api");
        apply_collection_var_edit(&mut col, "path", "/v2");
        assert_eq!(col.variables_map().get("path").unwrap(), "/v2");

        ctx.apply_edit(VarResolveScope::Vault, "k", "vv");
        assert_eq!(ctx.vault.get("k"), Some("vv"));
    }

    #[test]
    fn apply_environment_map_persists_script_sets() {
        let mut env = Environment::new("Local");
        env.values.push(EnvVar::new("baseUrl", "http://old"));
        let mut map = HashMap::new();
        map.insert("baseUrl".into(), "http://x".into());
        map.insert("endpoint_rcell_auth".into(), "http://x".into());
        apply_environment_map(&mut env, &map);
        assert_eq!(
            env.values
                .iter()
                .find(|v| v.key == "endpoint_rcell_auth")
                .map(|v| v.effective()),
            Some("http://x")
        );
        assert_eq!(
            env.values
                .iter()
                .find(|v| v.key == "baseUrl")
                .map(|v| v.current_value.as_str()),
            Some("http://x")
        );
        // unset: key missing from map → enabled row removed
        let mut only_one = HashMap::new();
        only_one.insert("endpoint_rcell_auth".into(), "http://x".into());
        apply_environment_map(&mut env, &only_one);
        assert!(!env.values.iter().any(|v| v.key == "baseUrl"));
        assert!(env.values.iter().any(|v| v.key == "endpoint_rcell_auth"));
    }

    #[test]
    fn apply_collection_map_persists_script_sets() {
        let mut col = Collection::new("C");
        apply_collection_var_edit(&mut col, "basic_auth_020j", "Basic xxx");
        let mut map = HashMap::new();
        map.insert("basic_auth_020j".into(), "Basic yyy".into());
        map.insert("token".into(), "t1".into());
        apply_collection_map(&mut col, &map);
        assert_eq!(col.variables_map().get("basic_auth_020j").unwrap(), "Basic yyy");
        assert_eq!(col.variables_map().get("token").unwrap(), "t1");
    }

    #[test]
    fn unicode_char_boundary_safety_in_var_insert() {
        // Test curly quotes `{“"}` where byte 3 falls inside the 3-byte character '“'
        let text = "{“\"}";
        // '“' occupies bytes 1..4. Byte 3 is inside '“'.
        let ctx = var_insert_context(text, 3);
        assert_eq!(ctx, None);

        // Caret inside incomplete curly quote {{“foo
        let text2 = "{{“foo";
        // '“' is 3 bytes (bytes 2..5). Byte 3 or 4 are non-boundary indices.
        let ctx2 = var_insert_context(text2, 3);
        assert!(ctx2.is_some());
        let (open, name) = ctx2.unwrap();
        assert_eq!(open, 0);
        assert_eq!(name, ""); // snap clamped to before '“'

        let ctx3 = var_insert_context(text2, text2.len());
        assert_eq!(ctx3, Some((0, "“foo".to_string())));

        // Safe apply_var_insert with mid-char caret
        let (out, new_caret) = apply_var_insert(text, 3, 0, "my_var");
        assert_eq!(out, "{{my_var}}“\"}");
        assert_eq!(new_caret, 10);

        // No double closing braces: {{foo}} -> {{my_var}} (NOT {{my_var}}}})
        // "https://api.com/" is 16 bytes. "{{" is at 16..18. "foo" is at 18..21. "}}" is at 21..23.
        let (out, new_caret) = apply_var_insert("https://api.com/{{foo}}", 21, 16, "my_var");
        assert_eq!(out, "https://api.com/{{my_var}}");
        assert_eq!(new_caret, 26);

        // Incomplete single closing brace: {{foo} -> {{my_var}}
        let (out, new_caret) = apply_var_insert("{{foo}", 5, 0, "my_var");
        assert_eq!(out, "{{my_var}}");
        assert_eq!(new_caret, 10);

        // No closing brace: {{foo -> {{my_var}}
        let (out, new_caret) = apply_var_insert("{{foo", 5, 0, "my_var");
        assert_eq!(out, "{{my_var}}");
        assert_eq!(new_caret, 10);

        // Emoji 4-byte boundaries: {{🔥
        let text_emoji = "{{🔥";
        // 🔥 is bytes 2..6.
        let ctx_emoji = var_insert_context(text_emoji, 4);
        assert!(ctx_emoji.is_some());

        // UTF-16 to byte offset conversion
        let s = "a“b🔥c";
        // 'a' = 1 byte (idx 0), '“' = 3 bytes (idx 1..4), 'b' = 1 byte (idx 4), '🔥' = 4 bytes / 2 UTF-16 units (idx 5..9), 'c' = 1 byte (idx 9)
        assert_eq!(utf16_offset_to_byte_offset(s, 0), 0);
        assert_eq!(utf16_offset_to_byte_offset(s, 1), 1); // before '“'
        assert_eq!(utf16_offset_to_byte_offset(s, 2), 4); // after '“', before 'b'
        assert_eq!(utf16_offset_to_byte_offset(s, 3), 5); // after 'b', before '🔥'
        assert_eq!(utf16_offset_to_byte_offset(s, 5), 9); // after '🔥' (2 u16 units), before 'c'
        assert_eq!(utf16_offset_to_byte_offset(s, 6), 10); // after 'c'
    }

    #[test]
    fn test_dynamic_faker_variables() {
        let ctx = VariableContext::default();

        // 1. {{$guid}}
        let guid = ctx.resolve_string("{{$guid}}");
        assert_eq!(guid.len(), 36);
        assert_eq!(guid.chars().filter(|c| *c == '-').count(), 4);

        // 2. {{$timestamp}}
        let ts = ctx.resolve_string("{{$timestamp}}");
        assert!(ts.parse::<u64>().is_ok());

        // 3. {{$randomInt}}
        let ri = ctx.resolve_string("{{$randomInt}}");
        let n: u32 = ri.parse().expect("random int integer");
        assert!((1..=1000).contains(&n));

        // 4. {{$randomEmail}}
        let em = ctx.resolve_string("{{$randomEmail}}");
        assert!(em.contains("@example.com"));

        // 5. New dynamic variables ($date, $timestampMs, $randomDomain, $randomSlug, $randomUserAgent, $randomHexColor)
        let dt = ctx.resolve_string("{{$date}}");
        assert_eq!(dt.len(), 10);
        assert!(ctx.resolve_string("{{$timestampMs}}").parse::<u64>().is_ok());
        assert!(ctx.resolve_string("{{$randomDomain}}").ends_with(".example.com"));
        assert!(ctx.resolve_string("{{$randomSlug}}").contains('-'));
        assert!(ctx.resolve_string("{{$randomUserAgent}}").contains("Mozilla/") || ctx.resolve_string("{{$randomUserAgent}}").contains("Blitz-Client/"));
        assert_eq!(ctx.resolve_string("{{$randomHexColor}}").len(), 7);

        // 6. Autocomplete suggestions include dynamic variables
        let suggs = ctx.insert_suggestions("$");
        assert!(suggs.iter().any(|s| s.label == "$guid" && s.scope == VarResolveScope::Dynamic));
        assert!(suggs.iter().any(|s| s.label == "$randomEmail" && s.scope == VarResolveScope::Dynamic));
        assert!(suggs.iter().any(|s| s.label == "$date" && s.scope == VarResolveScope::Dynamic));
    }

    #[test]
    fn test_nested_variable_resolution_and_cycle_guard() {
        let mut ctx = VariableContext::default();
        ctx.environment.insert("subdomain".into(), "api".into());
        ctx.environment.insert("host".into(), "{{subdomain}}.service.local".into());
        ctx.environment.insert("baseUrl".into(), "https://{{host}}:8443".into());

        // Multi-level nested resolution:
        assert_eq!(
            ctx.resolve_string("{{baseUrl}}/v1/users"),
            "https://api.service.local:8443/v1/users"
        );

        // Circular references terminate safely without hanging:
        ctx.environment.insert("cycle_a".into(), "{{cycle_b}}".into());
        ctx.environment.insert("cycle_b".into(), "{{cycle_a}}".into());
        let out = ctx.resolve_string("ping {{cycle_a}}");
        assert!(out == "ping {{cycle_a}}" || out == "ping {{cycle_b}}");
    }

    #[test]
    fn empty_env_var_falls_back_to_collection_and_global() {
        let mut ctx = VariableContext::default();
        ctx.environment.insert("share_basic_auth".into(), String::new());
        ctx.environment.insert("only_empty".into(), String::new());
        ctx.collection
            .insert("share_basic_auth".into(), "col-secret".into());
        ctx.global.insert("global_fallback".into(), "g-val".into());
        ctx.environment
            .insert("global_fallback".into(), String::new());

        assert_eq!(ctx.get("share_basic_auth"), Some("col-secret"));
        assert_eq!(ctx.scope_of("share_basic_auth"), Some(VariableScope::Collection));
        assert_eq!(ctx.get("global_fallback"), Some("g-val"));
        assert_eq!(ctx.scope_of("global_fallback"), Some(VariableScope::Global));
        assert_eq!(ctx.get("only_empty"), Some(""));
        assert_eq!(ctx.scope_of("only_empty"), Some(VariableScope::Environment));
    }

    #[test]
    fn resolve_string_non_vault_resolves_everything_except_vault() {
        let mut ctx = VariableContext::default();
        ctx.environment.insert("host".into(), "api.test.local".into());
        ctx.collection.insert("tenant".into(), "acme".into());
        ctx.collection
            .insert("nested_vault_ref".into(), "Bearer {{vault:jwt}}".into());
        ctx.vault.set("jwt", "TOP_SECRET_JWT");
        ctx.vault.set("plain_vault_only", "TOP_SECRET_PLAIN");

        let input = "https://{{host}}/{{tenant}} auth={{nested_vault_ref}} direct={{vault:jwt}} bare={{plain_vault_only}} missing={{unset_var}}";
        let resolved = ctx.resolve_string_non_vault(input);

        assert!(resolved.contains("https://api.test.local/acme"));
        assert!(resolved.contains("auth=Bearer {{vault:jwt}}"));
        assert!(resolved.contains("direct={{vault:jwt}}"));
        assert!(resolved.contains("bare={{plain_vault_only}}"));
        assert!(resolved.contains("missing={{unset_var}}"));
        assert!(!resolved.contains("TOP_SECRET_JWT"));
        assert!(!resolved.contains("TOP_SECRET_PLAIN"));
    }
}
