//! Collection health lint — unresolved variables, weak auth, duplicate names.

use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use super::collection::{Collection, CollectionItem};
use super::request::{AuthType, RequestDraft};
use super::variables::{iter_var_spans, VariableContext, VarResolveScope};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintIssue {
    pub collection_id: Uuid,
    pub collection_name: String,
    pub request_id: Option<Uuid>,
    pub request_name: Option<String>,
    pub field: String,
    pub message: String,
    pub severity: LintSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LintSeverity {
    Warning,
    Error,
}

pub fn lint_collections(collections: &[Collection], vars: &VariableContext) -> Vec<LintIssue> {
    let mut issues = Vec::new();
    let mut names_seen: HashMap<Uuid, HashSet<String>> = HashMap::new();

    for col in collections {
        let col_id = col.id;
        let col_name = col.name.clone();
        lint_auth(
            &mut issues,
            col_id,
            &col_name,
            None,
            None,
            "collection auth",
            &col.auth,
        );
        walk_items(
            col,
            &col.items,
            &mut issues,
            &mut names_seen,
            vars,
        );
    }
    issues.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then(a.collection_name.cmp(&b.collection_name))
            .then(a.request_name.cmp(&b.request_name))
    });
    issues
}

fn walk_items(
    col: &Collection,
    items: &[CollectionItem],
    issues: &mut Vec<LintIssue>,
    names_seen: &mut HashMap<Uuid, HashSet<String>>,
    vars: &VariableContext,
) {
    for item in items {
        match item {
            CollectionItem::Folder(f) => {
                let key = f.name.to_ascii_lowercase();
                let dup = {
                    let set = names_seen.entry(col.id).or_default();
                    !set.insert(key)
                };
                if dup {
                    issues.push(LintIssue {
                        collection_id: col.id,
                        collection_name: col.name.clone(),
                        request_id: None,
                        request_name: None,
                        field: "folder name".into(),
                        message: format!("duplicate folder name «{}»", f.name),
                        severity: LintSeverity::Warning,
                    });
                }
                lint_auth(
                    issues,
                    col.id,
                    &col.name,
                    None,
                    Some(f.name.clone()),
                    "folder auth",
                    &f.auth,
                );
                walk_items(col, &f.items, issues, names_seen, vars);
            }
            CollectionItem::Request(req) => lint_request(col, req, issues, names_seen, vars),
        }
    }
}

fn lint_request(
    col: &Collection,
    req: &RequestDraft,
    issues: &mut Vec<LintIssue>,
    names_seen: &mut HashMap<Uuid, HashSet<String>>,
    vars: &VariableContext,
) {
    let key = req.name.to_ascii_lowercase();
    let dup = if key.is_empty() {
        false
    } else {
        let set = names_seen.entry(col.id).or_default();
        !set.insert(key)
    };
    if dup {
        issues.push(LintIssue {
            collection_id: col.id,
            collection_name: col.name.clone(),
            request_id: Some(req.id),
            request_name: Some(req.name.clone()),
            field: "request name".into(),
            message: format!("duplicate request name «{}»", req.name),
            severity: LintSeverity::Warning,
        });
    }
    lint_auth(
        issues,
        col.id,
        &col.name,
        Some(req.id),
        Some(req.name.clone()),
        "request auth",
        &req.auth,
    );
    lint_text(issues, col, req, "url", &req.url, vars);
    for (i, h) in req.headers.iter().enumerate() {
        if h.enabled {
            lint_text(issues, col, req, &format!("header[{i}] value"), &h.value, vars);
            lint_text(issues, col, req, &format!("header[{i}] key"), &h.key, vars);
        }
    }
    if !req.body.is_empty() {
        lint_text(issues, col, req, "body", &req.body, vars);
    }
}

fn lint_auth(
    issues: &mut Vec<LintIssue>,
    collection_id: Uuid,
    collection_name: &str,
    request_id: Option<Uuid>,
    request_name: Option<String>,
    field: &str,
    auth: &super::request::AuthConfig,
) {
    if auth.auth_type == AuthType::Inherit {
        return;
    }
    let empty = match auth.auth_type {
        AuthType::NoAuth | AuthType::Inherit => false,
        AuthType::Bearer => auth.token.trim().is_empty(),
        AuthType::Basic => auth.username.trim().is_empty() || auth.password.trim().is_empty(),
        AuthType::ApiKey => auth.api_key_header.trim().is_empty() || auth.token.trim().is_empty(),
        AuthType::Digest | AuthType::Oauth2 | AuthType::Oauth1 | AuthType::Hawk
        | AuthType::Ntlm | AuthType::AwsSigV4 | AuthType::EdgeGrid => {
            auth.username.trim().is_empty() && auth.token.trim().is_empty()
        }
    };
    if empty {
        issues.push(LintIssue {
            collection_id,
            collection_name: collection_name.to_string(),
            request_id,
            request_name,
            field: field.to_string(),
            message: format!("{:?} selected but credentials empty", auth.auth_type),
            severity: LintSeverity::Error,
        });
    }
}

fn lint_text(
    issues: &mut Vec<LintIssue>,
    col: &Collection,
    req: &RequestDraft,
    field: &str,
    text: &str,
    vars: &VariableContext,
) {
    for span in iter_var_spans(text) {
        let info = vars.inspect_var(&span.raw);
        if info.scope == VarResolveScope::Unresolved {
            issues.push(LintIssue {
                collection_id: col.id,
                collection_name: col.name.clone(),
                request_id: Some(req.id),
                request_name: Some(req.name.clone()),
                field: field.to_string(),
                message: format!("unresolved {{{{{}}}}}", span.raw),
                severity: LintSeverity::Error,
            });
        }
    }
}

pub fn lint_error_count(issues: &[LintIssue]) -> usize {
    issues
        .iter()
        .filter(|i| i.severity == LintSeverity::Error)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{add_request_under, demo_collections, RequestDraft, VariableContext};

    #[test]
    fn lint_flags_unresolved_url_var() {
        let mut cols = demo_collections();
        let col_id = cols[0].id;
        let mut req = RequestDraft::default();
        req.name = "Users".into();
        req.url = "https://{{missing}}/users".into();
        assert!(add_request_under(&mut cols, col_id, req));
        let issues = lint_collections(&cols, &VariableContext::default());
        assert!(issues.iter().any(|i| i.message.contains("missing")));
    }

    #[test]
    fn lint_flags_duplicate_request_names() {
        let mut cols = demo_collections();
        let col_id = cols[0].id;
        let mut a = RequestDraft::default();
        a.name = "Ping".into();
        let mut b = RequestDraft::default();
        b.name = "Ping".into();
        assert!(add_request_under(&mut cols, col_id, a));
        assert!(add_request_under(&mut cols, col_id, b));
        let issues = lint_collections(&cols, &VariableContext::default());
        assert!(issues
            .iter()
            .any(|i| i.field == "request name" && i.message.contains("Ping")));
    }
}
