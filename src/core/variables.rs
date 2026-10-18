//! Variable scopes: global / collection / environment / local / vault.
//! Resolution order matches Postman product-docs convention (reports/04, 07).
//! `{{vault:key}}` resolved via vault map (file / SecretsStore) — see `vault.rs`.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

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
    Local,
    Environment,
    Collection,
    Global,
    Vault,
    Unresolved,
}

impl VarResolveScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Environment => "environment",
            Self::Collection => "collection",
            Self::Global => "global",
            Self::Vault => "vault",
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

/// Find `{{name}}` / `{{vault:key}}` span containing byte `index`.
/// Index may equal `end` (caret after closing `}}`).
pub fn find_var_at(text: &str, index: usize) -> Option<VarSpan> {
    let index = index.min(text.len());
    for span in iter_var_spans(text) {
        if index >= span.start && index <= span.end {
            return Some(span);
        }
    }
    None
}

/// Caret is inside an incomplete `{{…}}` token → `(byte index of `{`, partial name)`.
pub fn var_insert_context(text: &str, caret: usize) -> Option<(usize, String)> {
    let caret = caret.min(text.len());
    let before = &text[..caret];
    let open = before.rfind("{{")?;
    let tail = &before[open + 2..];
    if tail.contains("}}") {
        return None;
    }
    Some((open, tail.trim_start().to_string()))
}

/// Replace incomplete `{{…}}` at `open` through `caret` with `{{insert_name}}`.
pub fn apply_var_insert(text: &str, caret: usize, open: usize, insert_name: &str) -> (String, usize) {
    let token = format!("{{{{{}}}}}", insert_name.trim());
    let mut out = String::with_capacity(text.len() + token.len());
    out.push_str(&text[..open]);
    out.push_str(&token);
    out.push_str(&text[caret..]);
    let new_caret = open + token.len();
    (out, new_caret)
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
/// `<span class="var-token">` (adds ` unresolved` when `vars` marks it unresolved).
pub fn render_var_highlight_html(text: &str, vars: Option<&VariableContext>) -> String {
    let mut html = String::with_capacity(text.len().saturating_mul(2));
    let mut cursor = 0;
    for span in iter_var_spans(text) {
        if span.start > cursor {
            html.push_str(&escape_html(&text[cursor..span.start]));
        }
        let token = &text[span.start..span.end];
        let unresolved = vars
            .map(|v| v.inspect_var(&span.raw).is_unresolved())
            .unwrap_or(false);
        let class = if unresolved {
            "var-token unresolved"
        } else {
            "var-token"
        };
        html.push_str("<span class=\"");
        html.push_str(class);
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

        out.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
        out
    }

    pub fn demo() -> Self {
        let mut ctx = Self::default();
        ctx.environment
            .insert("baseUrl".into(), "https://httpbin.org".into());
        ctx.global
            .insert("userAgent".into(), "EvilPostman/0.1".into());
        ctx
    }

    /// Resolve `{{name}}` with precedence: local > environment > collection > global.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.local
            .get(name)
            .or_else(|| self.environment.get(name))
            .or_else(|| self.collection.get(name))
            .or_else(|| self.global.get(name))
            .map(|s| s.as_str())
    }

    /// Owning scope for a plain (non-vault) name, following resolve precedence.
    pub fn scope_of(&self, name: &str) -> Option<VariableScope> {
        if self.local.contains_key(name) {
            Some(VariableScope::Local)
        } else if self.environment.contains_key(name) {
            Some(VariableScope::Environment)
        } else if self.collection.contains_key(name) {
            Some(VariableScope::Collection)
        } else if self.global.contains_key(name) {
            Some(VariableScope::Global)
        } else {
            None
        }
    }

    /// Inspect `{{…}}` inner raw for hover UI (value + owning scope).
    pub fn inspect_var(&self, raw: &str) -> VarHoverInfo {
        let raw = raw.trim();
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
            VarResolveScope::Vault => {
                self.vault.set(name, value);
            }
            VarResolveScope::Unresolved => {}
        }
    }

    pub fn resolve_string(&self, input: &str) -> String {
        self.resolve_string_with_secrets(input, None)
    }

    /// Like [`resolve_string`], but `{{vault:key}}` may fall back to SecretsStore.
    pub fn resolve_string_with_secrets(
        &self,
        input: &str,
        secrets: Option<&dyn SecretsStore>,
    ) -> String {
        let re = var_token_re();

        re.replace_all(input, |caps: &regex::Captures| {
            let raw = caps[1].trim();
            if let Some(key) = raw.strip_prefix("vault:") {
                resolve_vault_key(&self.vault, secrets, key.trim()).unwrap_or_default()
            } else {
                self.get(raw).unwrap_or("").to_string()
            }
        })
        .into_owned()
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
/// - Enabled keys missing from map → removed (`epm.environment.unset` / `.clear`)
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
        assert!(html.contains("<span class=\"var-token\">{{baseUrl}}</span>"));
        assert!(html.contains("<span class=\"var-token unresolved\">{{missing}}</span>"));
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
}
