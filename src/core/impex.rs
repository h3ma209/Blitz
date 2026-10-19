//! Postman Collection v2.1 / env / HAR / OpenAPI import / export (best-effort subset).
//!
//! **Supported:** info.name, item[] (folder/request), request.method/url/header/body/auth,
//! event scripts (prerequest / test), collection `variable`, Postman env JSON, HAR entries,
//! OpenAPI 3 JSON + YAML happy-path.
//! **Not supported:** proto/gRPC full, RAML/WSDL, rich description HTML.

use serde_json::{json, Value};
use uuid::Uuid;

use super::collection::{Collection, CollectionItem, Folder};
use super::request::{
    AuthConfig, AuthType, BodyMode, HttpMethod, KeyValue, RequestDraft,
};

#[derive(Debug, Clone, thiserror::Error)]
pub enum ImpexError {
    #[error("json: {0}")]
    Json(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
}

/// Import a Postman Collection v2.1 JSON document into our [`Collection`].
pub fn import_collection_v21(text: &str) -> Result<Collection, ImpexError> {
    let root: Value =
        serde_json::from_str(text).map_err(|e| ImpexError::Json(e.to_string()))?;
    let schema = root
        .pointer("/info/schema")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !schema.is_empty() && !schema.contains("v2.1.0") && !schema.contains("collection/v2") {
        // Soft warning — still try.
    }

    let name = root
        .pointer("/info/name")
        .and_then(|v| v.as_str())
        .unwrap_or("Imported")
        .to_string();
    let description = root
        .pointer("/info/description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let items_val = root.get("item").cloned().unwrap_or(Value::Array(vec![]));
    let items = import_items(&items_val)?;

    let mut col = Collection::new(name);
    col.description = description;
    col.items = items;
    col.auth = import_auth(root.get("auth"));
    if let Some(vars) = root.get("variable").and_then(|v| v.as_array()) {
        col.variables = vars
            .iter()
            .filter_map(|v| {
                let key = v.get("key")?.as_str()?.to_string();
                let value = v
                    .get("value")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let disabled = v.get("disabled").and_then(|d| d.as_bool()).unwrap_or(false);
                Some(KeyValue {
                    key,
                    value,
                    enabled: !disabled,
                })
            })
            .collect();
    }
    Ok(col)
}

fn import_items(val: &Value) -> Result<Vec<CollectionItem>, ImpexError> {
    let arr = val
        .as_array()
        .ok_or_else(|| ImpexError::Json("item must be array".into()))?;
    let mut out = Vec::new();
    for item in arr {
        if item.get("request").is_some() {
            out.push(CollectionItem::Request(import_request(item)?));
        } else if item.get("item").is_some() {
            let name = item
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Folder")
                .to_string();
            let mut folder = Folder::new(name);
            folder.items = import_items(item.get("item").unwrap())?;
            out.push(CollectionItem::Folder(folder));
        }
    }
    Ok(out)
}

fn import_request(item: &Value) -> Result<RequestDraft, ImpexError> {
    let mut draft = RequestDraft::default();
    draft.id = item
        .get("id")
        .or_else(|| item.get("_postman_id"))
        .or_else(|| item.get("_blitz_id"))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_else(Uuid::new_v4);
    draft.name = item
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Request")
        .to_string();
    draft.description = item
        .get("request")
        .and_then(|r| r.get("description"))
        .and_then(|d| d.as_str())
        .or_else(|| item.get("description").and_then(|d| d.as_str()))
        .unwrap_or("")
        .to_string();

    let req = item
        .get("request")
        .ok_or_else(|| ImpexError::Json("missing request".into()))?;

    draft.method = parse_method(
        req.get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET"),
    );
    draft.url = url_from_postman(req.get("url"));

    if let Some(headers) = req.get("header").and_then(|v| v.as_array()) {
        draft.headers = headers
            .iter()
            .filter_map(|h| {
                let key = h.get("key")?.as_str()?.to_string();
                let value = h.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let disabled = h.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false);
                Some(KeyValue {
                    key,
                    value,
                    enabled: !disabled,
                })
            })
            .collect();
        draft.headers.push(KeyValue::new("", ""));
    }

    if let Some(body) = req.get("body") {
        let mode = body.get("mode").and_then(|v| v.as_str()).unwrap_or("raw");
        match mode {
            "raw" => {
                draft.body_mode = BodyMode::Raw;
                draft.body = body
                    .get("raw")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                // Postman UI: body.options.raw.language === "json" ⇒ Content-Type
                let lang = body
                    .pointer("/options/raw/language")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if lang.eq_ignore_ascii_case("json")
                    && !draft
                        .headers
                        .iter()
                        .any(|h| h.key.eq_ignore_ascii_case("Content-Type") && !h.key.is_empty())
                {
                    draft
                        .headers
                        .push(KeyValue::new("Content-Type", "application/json"));
                }
            }
            "urlencoded" => {
                draft.body_mode = BodyMode::Urlencoded;
                if let Some(arr) = body.get("urlencoded").and_then(|v| v.as_array()) {
                    draft.body_urlencoded = arr
                        .iter()
                        .filter_map(|h| {
                            Some(KeyValue {
                                key: h.get("key")?.as_str()?.to_string(),
                                value: h
                                    .get("value")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                enabled: !h.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false),
                            })
                        })
                        .collect();
                    draft.body_urlencoded.push(KeyValue::new("", ""));
                }
            }
            "formdata" => {
                draft.body_mode = BodyMode::FormData;
                draft.body = String::new();
            }
            "file" | "binary" => {
                draft.body_mode = BodyMode::Binary;
                draft.binary_path = body
                    .get("file")
                    .or_else(|| body.get("src"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
            }
            _ => {
                draft.body_mode = BodyMode::None;
            }
        }
    }

    draft.auth = import_auth(req.get("auth"));

    // events: prerequest / test
    if let Some(events) = item.get("event").and_then(|v| v.as_array()) {
        for ev in events {
            let listen = ev.get("listen").and_then(|v| v.as_str()).unwrap_or("");
            let exec = ev
                .pointer("/script/exec")
                .and_then(|v| v.as_array())
                .map(|lines| {
                    lines
                        .iter()
                        .filter_map(|l| l.as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            match listen {
                "prerequest" => draft.pre_request_script = exec,
                "test" => draft.test_script = exec,
                _ => {}
            }
        }
    }

    if let Some(arr) = item.pointer("/x-blitz/assertions") {
        if let Ok(list) = serde_json::from_value::<Vec<super::assertions::Assertion>>(arr.clone()) {
            draft.assertions = list;
        }
    }

    Ok(draft)
}

fn import_auth(auth: Option<&Value>) -> AuthConfig {
    let Some(auth) = auth else {
        return AuthConfig {
            auth_type: AuthType::Inherit,
            ..Default::default()
        };
    };
    let ty = auth.get("type").and_then(|v| v.as_str()).unwrap_or("noauth");
    let mut cfg = AuthConfig::default();
    match ty {
        "bearer" => {
            cfg.auth_type = AuthType::Bearer;
            cfg.token = auth_kv(auth, "bearer", "token");
        }
        "basic" => {
            cfg.auth_type = AuthType::Basic;
            cfg.username = auth_kv(auth, "basic", "username");
            cfg.password = auth_kv(auth, "basic", "password");
        }
        "apikey" => {
            cfg.auth_type = AuthType::ApiKey;
            cfg.token = auth_kv(auth, "apikey", "value");
            cfg.api_key_header = auth_kv(auth, "apikey", "key");
        }
        "digest" => {
            cfg.auth_type = AuthType::Digest;
            cfg.username = auth_kv(auth, "digest", "username");
            cfg.password = auth_kv(auth, "digest", "password");
        }
        "noauth" => cfg.auth_type = AuthType::NoAuth,
        _ => cfg.auth_type = AuthType::Inherit,
    }
    cfg
}

fn auth_kv(auth: &Value, section: &str, key: &str) -> String {
    auth.get(section)
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .find(|e| e.get("key").and_then(|k| k.as_str()) == Some(key))
        .and_then(|e| e.get("value"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn url_from_postman(url: Option<&Value>) -> String {
    let Some(url) = url else {
        return String::new();
    };
    if let Some(s) = url.as_str() {
        return s.to_string();
    }
    if let Some(raw) = url.get("raw").and_then(|v| v.as_str()) {
        return raw.to_string();
    }
    // host + path fallback
    let protocol = url
        .get("protocol")
        .and_then(|v| v.as_str())
        .unwrap_or("https");
    let host = url
        .get("host")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(".")
        })
        .unwrap_or_default();
    let path = url
        .get("path")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    if host.is_empty() {
        String::new()
    } else if path.is_empty() {
        format!("{protocol}://{host}")
    } else {
        format!("{protocol}://{host}/{path}")
    }
}

fn parse_method(s: &str) -> HttpMethod {
    match s.to_uppercase().as_str() {
        "POST" => HttpMethod::Post,
        "PUT" => HttpMethod::Put,
        "PATCH" => HttpMethod::Patch,
        "DELETE" => HttpMethod::Delete,
        "HEAD" => HttpMethod::Head,
        "OPTIONS" => HttpMethod::Options,
        _ => HttpMethod::Get,
    }
}

/// Export a [`Collection`] as Postman Collection v2.1 JSON text.
pub fn export_collection_v21(col: &Collection) -> Result<String, ImpexError> {
    let variables: Vec<Value> = col
        .variables
        .iter()
        .filter(|v| !v.key.is_empty())
        .map(|v| {
            json!({
                "key": v.key,
                "value": v.value,
                "disabled": !v.enabled,
            })
        })
        .collect();
    let doc = json!({
        "info": {
            "name": col.name,
            "description": col.description,
            "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json",
            "_postman_id": col.id.to_string(),
        },
        "auth": export_auth(&col.auth),
        "variable": variables,
        "item": export_items(&col.items),
    });
    serde_json::to_string_pretty(&doc).map_err(|e| ImpexError::Json(e.to_string()))
}

/// Export a [`Collection`] as an OpenAPI 3.0.3 JSON document.
pub fn export_openapi3(col: &Collection) -> Result<String, ImpexError> {
    use std::collections::BTreeMap;

    let mut paths: BTreeMap<String, Value> = BTreeMap::new();

    fn collect_requests<'a>(items: &'a [CollectionItem], tag: Option<&str>, out: &mut Vec<(&'a RequestDraft, Option<String>)>) {
        for item in items {
            match item {
                CollectionItem::Folder(f) => {
                    let next_tag = tag.unwrap_or(&f.name);
                    collect_requests(&f.items, Some(next_tag), out);
                }
                CollectionItem::Request(r) => {
                    out.push((r, tag.map(|s| s.to_string())));
                }
            }
        }
    }

    let mut all_requests = Vec::new();
    collect_requests(&col.items, None, &mut all_requests);

    for (r, tag) in all_requests {
        let mut path_str = r.url.clone();
        if let Some(idx) = path_str.find("://") {
            let rest = &path_str[idx + 3..];
            if let Some(slash_idx) = rest.find('/') {
                path_str = rest[slash_idx..].to_string();
            } else {
                path_str = "/".to_string();
            }
        }
        if let Some(q_idx) = path_str.find('?') {
            path_str = path_str[..q_idx].to_string();
        }
        if !path_str.starts_with('/') {
            path_str = format!("/{}", path_str);
        }

        let method_str = r.method.as_str().to_lowercase();
        let mut operation = json!({
            "summary": r.name,
            "description": r.description,
            "responses": {
                "200": {
                    "description": "Successful response"
                }
            }
        });

        if let Some(t) = tag {
            operation["tags"] = json!([t]);
        }

        let mut parameters = Vec::new();
        for p in &r.params {
            if !p.key.is_empty() && p.enabled {
                parameters.push(json!({
                    "name": p.key,
                    "in": "query",
                    "required": false,
                    "schema": { "type": "string" },
                    "example": p.value
                }));
            }
        }
        for h in &r.headers {
            if !h.key.is_empty() && h.enabled && !h.key.eq_ignore_ascii_case("content-type") {
                parameters.push(json!({
                    "name": h.key,
                    "in": "header",
                    "required": false,
                    "schema": { "type": "string" },
                    "example": h.value
                }));
            }
        }
        if !parameters.is_empty() {
            operation["parameters"] = Value::Array(parameters);
        }

        if r.body_mode == BodyMode::Raw && !r.body.is_empty() {
            operation["requestBody"] = json!({
                "content": {
                    "application/json": {
                        "schema": { "type": "object" },
                        "example": serde_json::from_str::<Value>(&r.body).unwrap_or(Value::String(r.body.clone()))
                    }
                }
            });
        }

        let path_entry = paths.entry(path_str).or_insert_with(|| json!({}));
        if let Some(obj) = path_entry.as_object_mut() {
            obj.insert(method_str, operation);
        }
    }

    let openapi_doc = json!({
        "openapi": "3.0.3",
        "info": {
            "title": col.name,
            "description": if col.description.is_empty() { "Exported from Blitz API Client" } else { &col.description },
            "version": "1.0.0"
        },
        "paths": paths
    });

    serde_json::to_string_pretty(&openapi_doc).map_err(|e| ImpexError::Json(e.to_string()))
}

fn export_items(items: &[CollectionItem]) -> Vec<Value> {
    items
        .iter()
        .map(|item| match item {
            CollectionItem::Folder(f) => json!({
                "name": f.name,
                "item": export_items(&f.items),
            }),
            CollectionItem::Request(r) => export_request(r),
        })
        .collect()
}

fn export_request(r: &RequestDraft) -> Value {
    let mut body = json!({ "mode": "raw", "raw": "" });
    match r.body_mode {
        BodyMode::None => {
            body = json!({ "mode": "raw", "raw": "" });
        }
        BodyMode::Raw => {
            body = json!({ "mode": "raw", "raw": r.body });
        }
        BodyMode::Urlencoded => {
            let pairs: Vec<Value> = r
                .body_urlencoded
                .iter()
                .filter(|kv| !kv.key.is_empty())
                .map(|kv| {
                    json!({
                        "key": kv.key,
                        "value": kv.value,
                        "disabled": !kv.enabled,
                    })
                })
                .collect();
            body = json!({ "mode": "urlencoded", "urlencoded": pairs });
        }
        BodyMode::FormData => {
            body = json!({ "mode": "formdata", "formdata": [] });
        }
        BodyMode::Graphql => {
            body = json!({
                "mode": "graphql",
                "graphql": {
                    "query": r.graphql_query,
                    "variables": r.graphql_variables,
                }
            });
        }
        BodyMode::Binary => {
            body = json!({
                "mode": "file",
                "src": r.binary_path,
            });
        }
    }

    let headers: Vec<Value> = r
        .headers
        .iter()
        .filter(|h| !h.key.is_empty())
        .map(|h| {
            json!({
                "key": h.key,
                "value": h.value,
                "disabled": !h.enabled,
            })
        })
        .collect();

    let mut events = Vec::new();
    if !r.pre_request_script.is_empty() {
        events.push(json!({
            "listen": "prerequest",
            "script": {
                "type": "text/javascript",
                "exec": r.pre_request_script.lines().collect::<Vec<_>>(),
            }
        }));
    }
    if !r.test_script.is_empty() {
        events.push(json!({
            "listen": "test",
            "script": {
                "type": "text/javascript",
                "exec": r.test_script.lines().collect::<Vec<_>>(),
            }
        }));
    }

    let mut item = json!({
        "id": r.id.to_string(),
        "_postman_id": r.id.to_string(),
        "name": r.name,
        "request": {
            "method": r.method.as_str(),
            "header": headers,
            "body": body,
            "url": r.url,
            "auth": export_auth(&r.auth),
            "description": r.description,
        },
        "event": events,
    });
    // Blitz extension (ignored by Postman): no-code assertions survive Git sync.
    if !r.assertions.is_empty() {
        if let Ok(v) = serde_json::to_value(&r.assertions) {
            item["x-blitz"] = json!({ "assertions": v });
        }
    }
    item
}

/// Export a single request as a Postman v2.1 `item` object (for split tree files).
pub fn export_request_item(r: &RequestDraft) -> Value {
    export_request(r)
}

/// Import a single request item JSON (one-file-per-request on disk).
pub fn import_request_item(text: &str) -> Result<RequestDraft, ImpexError> {
    let item: Value =
        serde_json::from_str(text).map_err(|e| ImpexError::Json(e.to_string()))?;
    import_request(&item)
}

fn export_auth(auth: &AuthConfig) -> Value {
    match auth.auth_type {
        AuthType::NoAuth => json!({ "type": "noauth" }),
        AuthType::Inherit => Value::Null,
        AuthType::Bearer => json!({
            "type": "bearer",
            "bearer": [{ "key": "token", "value": auth.token, "type": "string" }]
        }),
        AuthType::Basic => json!({
            "type": "basic",
            "basic": [
                { "key": "username", "value": auth.username, "type": "string" },
                { "key": "password", "value": auth.password, "type": "string" },
            ]
        }),
        AuthType::ApiKey => json!({
            "type": "apikey",
            "apikey": [
                { "key": "key", "value": auth.api_key_header, "type": "string" },
                { "key": "value", "value": auth.token, "type": "string" },
                { "key": "in", "value": "header", "type": "string" },
            ]
        }),
        AuthType::Digest => json!({
            "type": "digest",
            "digest": [
                { "key": "username", "value": auth.username, "type": "string" },
                { "key": "password", "value": auth.password, "type": "string" },
            ]
        }),
        AuthType::Oauth2 => json!({
            "type": "oauth2",
            "oauth2": [
                { "key": "accessToken", "value": auth.oauth2.access_token, "type": "string" },
                { "key": "tokenUrl", "value": auth.oauth2.token_url, "type": "string" },
            ]
        }),
        AuthType::Oauth1 => json!({ "type": "oauth1" }),
        AuthType::Hawk => json!({ "type": "hawk" }),
        AuthType::Ntlm => json!({ "type": "ntlm" }),
        AuthType::AwsSigV4 => json!({ "type": "awsv4" }),
        AuthType::EdgeGrid => json!({ "type": "edgegrid" }),
    }
}

/// Limitations blurb for UI / README.
pub fn limitations_blurb() -> &'static str {
    "Import/export: Postman Collection v2.1 + env JSON + HAR + cURL + OpenAPI 3 JSON/YAML. \
     HTTP requests, folders, bodies, scripts, collection vars."
}

/// Default sidebar seed on first launch / empty snapshot (built-in demo collections).
pub fn bundled_postman_collections() -> Vec<Collection> {
    super::collection::demo_collections()
}

/// Import one or more Postman Collection v2.1 JSON documents (file contents).
pub fn import_postman_export_texts(texts: &[String]) -> Result<Vec<Collection>, ImpexError> {
    let mut out = Vec::new();
    for text in texts {
        out.push(import_collection_v21(text)?);
    }
    if out.is_empty() {
        return Err(ImpexError::Unsupported("no collections in selection".into()));
    }
    Ok(out)
}

/// Parse a simple cURL command into a [`RequestDraft`].
pub fn import_curl(text: &str) -> Result<RequestDraft, ImpexError> {
    let mut draft = RequestDraft::default();
    draft.name = "cURL import".into();
    let t = text.trim();
    if !t.to_ascii_lowercase().contains("curl") {
        return Err(ImpexError::Unsupported("not a curl command".into()));
    }
    // URL: first http(s) token or -X after
    let tokens = shellish_tokens(t);
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        if tok == "-X" || tok == "--request" {
            if let Some(m) = tokens.get(i + 1) {
                draft.method = parse_method(m);
                i += 2;
                continue;
            }
        }
        if tok == "-H" || tok == "--header" {
            if let Some(h) = tokens.get(i + 1) {
                if let Some((k, v)) = h.split_once(':') {
                    draft.headers.push(KeyValue::new(k.trim(), v.trim()));
                }
                i += 2;
                continue;
            }
        }
        if tok == "-d" || tok == "--data" || tok == "--data-raw" || tok == "--data-binary" {
            if let Some(b) = tokens.get(i + 1) {
                draft.body_mode = BodyMode::Raw;
                draft.body = b.clone();
                draft.method = if matches!(draft.method, HttpMethod::Get) {
                    HttpMethod::Post
                } else {
                    draft.method
                };
                i += 2;
                continue;
            }
        }
        if tok == "-u" || tok == "--user" {
            if let Some(u) = tokens.get(i + 1) {
                let (user, pass) = u.split_once(':').unwrap_or((u.as_str(), ""));
                draft.auth.auth_type = AuthType::Basic;
                draft.auth.username = user.into();
                draft.auth.password = pass.into();
                i += 2;
                continue;
            }
        }
        if tok.starts_with("http://") || tok.starts_with("https://") {
            draft.url = tok.clone();
        }
        i += 1;
    }
    if draft.url.is_empty() {
        return Err(ImpexError::Unsupported("curl: no URL found".into()));
    }
    Ok(draft)
}

fn shellish_tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' if in_double || (!in_single && !in_double) => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ' ' | '\t' | '\n' if !in_single && !in_double => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Parse OpenAPI root from JSON or YAML text.
fn parse_openapi_root(text: &str) -> Result<Value, ImpexError> {
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') {
        serde_json::from_str(text).map_err(|e| ImpexError::Json(format!("OpenAPI JSON: {e}")))
    } else {
        serde_yaml::from_str(text).map_err(|e| ImpexError::Json(format!("OpenAPI YAML: {e}")))
    }
}

/// OpenAPI 3 JSON or YAML → collection with one request per path+method.
pub fn import_openapi3(text: &str) -> Result<Collection, ImpexError> {
    let root = parse_openapi_root(text)?;
    let openapi = root.get("openapi").and_then(|v| v.as_str()).unwrap_or("");
    if !openapi.starts_with('3') && root.get("swagger").is_none() {
        // still try if paths exist
        if root.get("paths").is_none() {
            return Err(ImpexError::Unsupported("not OpenAPI 3 JSON/YAML".into()));
        }
    }
    let title = root
        .pointer("/info/title")
        .and_then(|v| v.as_str())
        .unwrap_or("OpenAPI import")
        .to_string();
    let base = root
        .get("servers")
        .and_then(|s| s.as_array())
        .and_then(|a| a.first())
        .and_then(|s| s.get("url"))
        .and_then(|u| u.as_str())
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string();

    let mut col = Collection::new(title);
    let paths = root
        .get("paths")
        .and_then(|p| p.as_object())
        .ok_or_else(|| ImpexError::Json("missing paths".into()))?;
    for (path, item) in paths {
        let Some(obj) = item.as_object() else {
            continue;
        };
        for (method, _op) in obj {
            let m = method.to_ascii_uppercase();
            if !matches!(
                m.as_str(),
                "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
            ) {
                continue;
            }
            let mut draft = RequestDraft::default();
            draft.name = format!("{m} {path}");
            draft.method = parse_method(&m);
            draft.url = format!("{base}{path}");
            col.add_request(draft);
        }
    }
    Ok(col)
}

/// Smart non-destructive OpenAPI 3 reconciliation.
/// Merges new endpoints from an updated OpenAPI spec into an existing collection,
/// preserving user tests, pre-request scripts, auth settings, and custom headers.
/// Returns (number of new endpoints added, number of existing preserved).
pub fn reconcile_openapi3_collection(
    existing: &mut Collection,
    new_spec_text: &str,
) -> Result<(usize, usize), ImpexError> {
    let fresh = import_openapi3(new_spec_text)?;
    let mut added = 0;
    let mut preserved = 0;

    for item in fresh.items {
        if let CollectionItem::Request(new_req) = item {
            // Match existing by Method + URL Path
            let found = existing.items.iter_mut().find_map(|it| match it {
                CollectionItem::Request(r) if r.method == new_req.method && r.url == new_req.url => {
                    Some(r)
                }
                _ => None,
            });

            if let Some(existing_req) = found {
                preserved += 1;
                // Preserve scripts, auth, and mock headers, but update description if empty
                if existing_req.description.is_empty() && !new_req.description.is_empty() {
                    existing_req.description = new_req.description;
                }
            } else {
                existing.add_request(new_req);
                added += 1;
            }
        }
    }

    Ok((added, preserved))
}

/// Import HAR 1.2 log → collection (one request per entry).
pub fn import_har(text: &str) -> Result<Collection, ImpexError> {
    let root: Value =
        serde_json::from_str(text).map_err(|e| ImpexError::Json(format!("HAR: {e}")))?;
    let entries = root
        .pointer("/log/entries")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ImpexError::Unsupported("not a HAR log (missing log.entries)".into()))?;
    let mut col = Collection::new("HAR import");
    for (i, entry) in entries.iter().enumerate() {
        let req = entry.get("request").ok_or_else(|| {
            ImpexError::Json(format!("entry {i}: missing request"))
        })?;
        let mut draft = RequestDraft::default();
        draft.name = req
            .get("url")
            .and_then(|u| u.as_str())
            .map(|u| {
                u.split('?')
                    .next()
                    .unwrap_or(u)
                    .rsplit('/')
                    .next()
                    .unwrap_or(u)
                    .to_string()
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("entry-{i}"));
        draft.method = parse_method(req.get("method").and_then(|m| m.as_str()).unwrap_or("GET"));
        draft.url = req
            .get("url")
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string();
        if let Some(headers) = req.get("headers").and_then(|h| h.as_array()) {
            draft.headers = headers
                .iter()
                .filter_map(|h| {
                    Some(KeyValue::new(
                        h.get("name")?.as_str()?,
                        h.get("value").and_then(|v| v.as_str()).unwrap_or(""),
                    ))
                })
                .collect();
            draft.headers.push(KeyValue::new("", ""));
        }
        if let Some(post) = req.get("postData") {
            let mime = post
                .get("mimeType")
                .and_then(|m| m.as_str())
                .unwrap_or("");
            let text_body = post
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            if !text_body.is_empty() {
                draft.body_mode = BodyMode::Raw;
                draft.body = text_body;
                if mime.contains("json")
                    && !draft
                        .headers
                        .iter()
                        .any(|h| h.key.eq_ignore_ascii_case("Content-Type"))
                {
                    draft
                        .headers
                        .push(KeyValue::new("Content-Type", "application/json"));
                }
            }
        }
        col.add_request(draft);
    }
    Ok(col)
}

#[cfg(test)]
mod import_extra_tests {
    use super::*;

    #[test]
    fn curl_get() {
        let d = import_curl("curl -X GET https://example.com/a -H 'Accept: application/json'").unwrap();
        assert_eq!(d.url, "https://example.com/a");
        assert_eq!(d.method, HttpMethod::Get);
        assert!(d.headers.iter().any(|h| h.key == "Accept"));
    }

    #[test]
    fn openapi_paths() {
        let spec = r#"{
          "openapi":"3.0.0",
          "info":{"title":"Demo"},
          "servers":[{"url":"https://api.example.com"}],
          "paths":{"/pets":{"get":{},"post":{}}}
        }"#;
        let col = import_openapi3(spec).unwrap();
        assert_eq!(col.name, "Demo");
        assert_eq!(col.items.len(), 2);
    }

    #[test]
    fn openapi_yaml() {
        let spec = r#"
openapi: "3.0.0"
info:
  title: YamlDemo
servers:
  - url: https://api.example.com
paths:
  /pets:
    get: {}
"#;
        let col = import_openapi3(spec).unwrap();
        assert_eq!(col.name, "YamlDemo");
        assert_eq!(col.items.len(), 1);
    }

    #[test]
    fn har_entries() {
        let har = r#"{
          "log": {
            "entries": [{
              "request": {
                "method": "GET",
                "url": "https://example.com/api/hello",
                "headers": [{"name":"Accept","value":"*/*"}]
              }
            }]
          }
        }"#;
        let col = import_har(har).unwrap();
        assert_eq!(col.items.len(), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_minimal() {
        let mut col = Collection::new("Roundtrip");
        let mut req = RequestDraft::default();
        req.name = "Ping".into();
        req.url = "https://example.com".into();
        req.method = HttpMethod::Get;
        col.add_request(req);

        let json = export_collection_v21(&col).unwrap();
        let imported = import_collection_v21(&json).unwrap();
        assert_eq!(imported.name, "Roundtrip");
        assert_eq!(imported.items.len(), 1);
        match &imported.items[0] {
            CollectionItem::Request(r) => {
                assert_eq!(r.name, "Ping");
                assert_eq!(r.url, "https://example.com");
            }
            _ => panic!("expected request"),
        }
    }

    #[test]
    fn import_sample_v21() {
        let sample = r#"{
            "info": { "name": "Sample", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
            "item": [{
                "name": "Hello",
                "request": {
                    "method": "POST",
                    "header": [{ "key": "Content-Type", "value": "application/json" }],
                    "body": { "mode": "raw", "raw": "{\"a\":1}" },
                    "url": "https://httpbin.org/post",
                    "auth": { "type": "bearer", "bearer": [{ "key": "token", "value": "tok", "type": "string" }] }
                },
                "event": [{
                    "listen": "prerequest",
                    "script": { "exec": ["console.log(1)"] }
                }]
            }]
        }"#;
        let col = import_collection_v21(sample).unwrap();
        assert_eq!(col.name, "Sample");
        let CollectionItem::Request(r) = &col.items[0] else {
            panic!("request");
        };
        assert_eq!(r.method, HttpMethod::Post);
        assert_eq!(r.body, r#"{"a":1}"#);
        assert_eq!(r.auth.auth_type, AuthType::Bearer);
        assert_eq!(r.auth.token, "tok");
        assert!(r.pre_request_script.contains("console.log"));
    }

    #[test]
    fn import_bundled_collections_sanity() {
        let cols = bundled_postman_collections();
        assert!(!cols.is_empty(), "expected seed collections");
        for c in &cols {
            assert!(!c.items.is_empty(), "{} should have items", c.name);
        }
    }

    #[test]
    fn test_export_openapi3() {
        let mut col = Collection::new("Store API");
        let mut req = RequestDraft::default();
        req.name = "List Products".into();
        req.url = "https://api.store.com/v1/products?limit=10".into();
        req.method = HttpMethod::Get;
        req.params.push(KeyValue::new("limit", "10"));
        col.items.push(CollectionItem::Request(req));

        let openapi_json = export_openapi3(&col).expect("export openapi3");
        assert!(openapi_json.contains("\"openapi\": \"3.0.3\""));
        assert!(openapi_json.contains("/v1/products"));
        assert!(openapi_json.contains("List Products"));
    }

    #[test]
    fn import_collection_variable_array() {
        let sample = r#"{
            "info": { "name": "Vars", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
            "item": [{
                "name": "Ping",
                "request": { "method": "GET", "header": [], "url": "https://example.com" }
            }],
            "variable": [
                { "key": "basic_auth_020j", "value": "Basic abc" },
                { "key": "basic_auth_14fb", "value": "Basic def", "disabled": false },
                { "key": "skip_me", "value": "x", "disabled": true }
            ]
        }"#;
        let col = import_collection_v21(sample).unwrap();
        let map = col.variables_map();
        assert_eq!(map.get("basic_auth_020j").map(String::as_str), Some("Basic abc"));
        assert_eq!(map.get("basic_auth_14fb").map(String::as_str), Some("Basic def"));
        assert!(!map.contains_key("skip_me"));
    }

    #[test]
    fn test_reconcile_openapi3_preserves_custom_tests() {
        let mut col = Collection::new("API");
        let mut req = RequestDraft::default();
        req.name = "GET /users".into();
        req.method = HttpMethod::Get;
        req.url = "https://api.com/users".into();
        req.test_script = "pm.test('status is 200', () => {});".into();
        col.items.push(CollectionItem::Request(req));

        let updated_spec = r#"{
            "openapi": "3.0.0",
            "info": { "title": "API", "version": "1.0" },
            "servers": [{ "url": "https://api.com" }],
            "paths": {
                "/users": {
                    "get": { "description": "Returns user list" }
                },
                "/orders": {
                    "post": { "description": "Create order" }
                }
            }
        }"#;

        let (added, preserved) = reconcile_openapi3_collection(&mut col, updated_spec).unwrap();
        assert_eq!(added, 1, "should add /orders");
        assert_eq!(preserved, 1, "should preserve /users");

        // Verify custom test script was not wiped
        let user_req = col.items.iter().find_map(|it| match it {
            CollectionItem::Request(r) if r.url == "https://api.com/users" => Some(r),
            _ => None,
        }).expect("users endpoint must exist");
        assert_eq!(user_req.test_script, "pm.test('status is 200', () => {});");
    }

    #[test]
    fn assertions_roundtrip_through_request_item() {
        use crate::core::{Assertion, AssertionKind};
        let mut r = RequestDraft::default();
        r.assertions = vec![
            Assertion::new(AssertionKind::StatusEquals { code: 201 }),
            Assertion::new(AssertionKind::JsonPathEquals {
                path: "data.id".into(),
                expected: "7".into(),
            }),
        ];
        r.assertions[1].enabled = false;
        let text = serde_json::to_string(&export_request_item(&r)).unwrap();
        let back = import_request_item(&text).unwrap();
        assert_eq!(back.assertions, r.assertions);

        // No assertions => no extension key written (clean Postman output).
        let plain = export_request_item(&RequestDraft::default());
        assert!(plain.get("x-blitz").is_none());
    }
}
