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
    AuthConfig, AuthType, BodyMode, FormDataField, HttpMethod, KeyValue, RequestDraft,
    SavedExample,
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

    let col_headers = extract_headers(root.get("header").or_else(|| root.get("headers")));
    let items_val = root.get("item").cloned().unwrap_or(Value::Array(vec![]));
    let items = import_items(&items_val, &col_headers)?;

    let mut col = Collection::new(name);
    col.description = description;
    col.items = items;
    col.auth = root
        .get("auth")
        .map(|a| import_auth(Some(a)))
        .unwrap_or_else(|| AuthConfig {
            auth_type: AuthType::NoAuth,
            ..Default::default()
        });
    if let Some(events) = root.get("event").and_then(|v| v.as_array()) {
        for ev in events {
            let listen = ev.get("listen").and_then(|v| v.as_str()).unwrap_or("");
            let exec = extract_script_exec(ev);
            match listen {
                "prerequest" => col.pre_request_script = exec,
                "test" => col.test_script = exec,
                _ => {}
            }
        }
    }
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

fn extract_headers(val: Option<&Value>) -> Vec<KeyValue> {
    let Some(val) = val else { return vec![]; };
    if let Some(arr) = val.as_array() {
        arr.iter()
            .filter_map(|h| {
                let key = h.get("key").and_then(|k| k.as_str())?.to_string();
                if key.is_empty() {
                    return None;
                }
                let value = h.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let disabled = h.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false);
                Some(KeyValue {
                    key,
                    value,
                    enabled: !disabled,
                })
            })
            .collect()
    } else if let Some(s) = val.as_str() {
        s.lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() {
                    return None;
                }
                let (k, v) = line.split_once(':')?;
                Some(KeyValue {
                    key: k.trim().to_string(),
                    value: v.trim().to_string(),
                    enabled: !k.trim().starts_with("//"),
                })
            })
            .collect()
    } else {
        vec![]
    }
}

fn extract_script_exec(ev: &Value) -> String {
    if let Some(lines) = ev.pointer("/script/exec").and_then(|v| v.as_array()) {
        lines.iter().filter_map(|l| l.as_str()).collect::<Vec<_>>().join("\n")
    } else if let Some(s) = ev.pointer("/script/exec").and_then(|v| v.as_str()) {
        s.to_string()
    } else if let Some(s) = ev.pointer("/script/src").and_then(|v| v.as_str()) {
        s.to_string()
    } else {
        String::new()
    }
}

fn import_items(val: &Value, inherited_headers: &[KeyValue]) -> Result<Vec<CollectionItem>, ImpexError> {
    let arr = val
        .as_array()
        .ok_or_else(|| ImpexError::Json("item must be array".into()))?;
    let mut out = Vec::new();
    for item in arr {
        if item.get("request").is_some() {
            out.push(CollectionItem::Request(import_request_with_inheritance(item, inherited_headers)?));
        } else if item.get("item").is_some() {
            let name = item
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Folder")
                .to_string();
            let mut folder = Folder::new(name);
            if let Some(id_str) = item.get("id").or_else(|| item.get("_postman_id")).and_then(|v| v.as_str()) {
                if let Ok(u) = Uuid::parse_str(id_str) {
                    folder.id = u;
                }
            }
            if let Some(desc) = item.get("description").and_then(|v| v.as_str()) {
                folder.description = desc.to_string();
            }
            folder.auth = import_auth(item.get("auth"));
            if let Some(events) = item.get("event").and_then(|v| v.as_array()) {
                for ev in events {
                    let listen = ev.get("listen").and_then(|v| v.as_str()).unwrap_or("");
                    let exec = extract_script_exec(ev);
                    match listen {
                        "prerequest" => folder.pre_request_script = exec,
                        "test" => folder.test_script = exec,
                        _ => {}
                    }
                }
            }
            let folder_headers = extract_headers(item.get("header").or_else(|| item.get("headers")));
            let mut combined = inherited_headers.to_vec();
            for fh in folder_headers {
                if !combined.iter().any(|h| h.key.eq_ignore_ascii_case(&fh.key)) {
                    combined.push(fh);
                }
            }
            folder.items = import_items(item.get("item").unwrap(), &combined)?;
            out.push(CollectionItem::Folder(folder));
        }
    }
    Ok(out)
}

fn import_request_with_inheritance(item: &Value, inherited_headers: &[KeyValue]) -> Result<RequestDraft, ImpexError> {
    let mut draft = import_request(item)?;
    for inh in inherited_headers {
        if !draft.headers.iter().any(|h| !h.key.is_empty() && h.key.eq_ignore_ascii_case(&inh.key)) {
            let insert_pos = if draft.headers.last().map(|h| h.key.is_empty()).unwrap_or(false) {
                draft.headers.len() - 1
            } else {
                draft.headers.len()
            };
            draft.headers.insert(insert_pos, inh.clone());
        }
    }
    Ok(draft)
}

fn import_request(item: &Value) -> Result<RequestDraft, ImpexError> {
    let req_val = item
        .get("request")
        .ok_or_else(|| ImpexError::Json("missing request".into()))?;

    let req_is_str = req_val.is_string();
    let empty_obj = serde_json::json!({});
    let req = if req_is_str { &empty_obj } else { req_val };

    let url_str = if req_is_str {
        req_val.as_str().unwrap_or("").to_string()
    } else {
        url_from_postman(req.get("url"))
    };

    let mut draft = RequestDraft {
        id: item
            .get("id")
            .or_else(|| item.get("_postman_id"))
            .or_else(|| item.get("_blitz_id"))
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
            .unwrap_or_else(Uuid::new_v4),
        name: item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Request")
            .to_string(),
        description: item
            .get("request")
            .and_then(|r| r.get("description"))
            .and_then(|d| d.as_str())
            .or_else(|| item.get("description").and_then(|d| d.as_str()))
            .unwrap_or("")
            .to_string(),
        method: parse_method(
            req.get("method")
                .and_then(|v| v.as_str())
                .unwrap_or("GET"),
        ),
        url: url_str,
        ..Default::default()
    };

    // Query parameters from Postman URL
    if let Some(query_arr) = req.pointer("/url/query").and_then(|v| v.as_array()) {
        let params: Vec<KeyValue> = query_arr
            .iter()
            .filter_map(|q| {
                let key = q.get("key")?.as_str()?.to_string();
                let value = q.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let disabled = q.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false);
                Some(KeyValue {
                    key,
                    value,
                    enabled: !disabled,
                })
            })
            .collect();
        if !params.is_empty() {
            draft.params = params;
            draft.params.push(KeyValue::new("", ""));
        }
    }

    // Path variables from Postman URL
    if let Some(var_arr) = req.pointer("/url/variable").and_then(|v| v.as_array()) {
        let path_params: Vec<KeyValue> = var_arr
            .iter()
            .filter_map(|q| {
                let key = q.get("key")?.as_str()?.to_string();
                let value = q.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let disabled = q.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false);
                Some(KeyValue {
                    key,
                    value,
                    enabled: !disabled,
                })
            })
            .collect();
        if !path_params.is_empty() {
            draft.path_params = path_params;
        }
    } else {
        draft.path_params = crate::core::sync_path_params(&draft.url, &[]);
    }

    let parsed_hdrs = extract_headers(
        req.get("header")
            .or_else(|| req.get("headers"))
            .or_else(|| item.get("header"))
            .or_else(|| item.get("headers")),
    );
    if !parsed_hdrs.is_empty() {
        draft.headers = parsed_hdrs;
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
                if let Some(arr) = body.get("formdata").and_then(|v| v.as_array()) {
                    draft.body_formdata = arr
                        .iter()
                        .filter_map(|f| {
                            let key = f.get("key")?.as_str()?.to_string();
                            let is_file = f.get("type").and_then(|v| v.as_str()) == Some("file");
                            let value = if is_file {
                                f.get("src").and_then(|v| v.as_str()).unwrap_or("").to_string()
                            } else {
                                f.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string()
                            };
                            let disabled = f.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false);
                            Some(FormDataField {
                                key,
                                value,
                                enabled: !disabled,
                                is_file,
                            })
                        })
                        .collect();
                    draft.body_formdata.push(FormDataField::default());
                }
            }
            "graphql" => {
                draft.body_mode = BodyMode::Graphql;
                if let Some(g) = body.get("graphql") {
                    draft.graphql_query = g.get("query").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    draft.graphql_variables = g.get("variables").and_then(|v| v.as_str()).unwrap_or("{}").to_string();
                }
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

    draft.auth = import_auth(req.get("auth").or_else(|| item.get("auth")));

    // events: prerequest / test
    let events_opt = item.get("event").or_else(|| req.get("event"));
    if let Some(events) = events_opt.and_then(|v| v.as_array()) {
        for ev in events {
            let listen = ev.get("listen").and_then(|v| v.as_str()).unwrap_or("");
            let exec = extract_script_exec(ev);
            match listen {
                "prerequest" => draft.pre_request_script = exec,
                "test" => draft.test_script = exec,
                _ => {}
            }
        }
    }

    // Protocol Profile Behavior
    if let Some(proto) = item.get("protocolProfileBehavior").or_else(|| req.get("protocolProfileBehavior")) {
        if let Some(fr) = proto.get("followRedirects").and_then(|v| v.as_bool()) {
            draft.settings.follow_redirects = fr;
        }
        if let Some(ssl) = proto.get("strictSSL").and_then(|v| v.as_bool()) {
            draft.settings.ssl_verify = ssl;
        }
        if let Some(to) = proto.get("timeout").and_then(|v| v.as_u64()) {
            draft.settings.timeout_ms = to;
        }
    }

    // Saved response examples (for mock / reference)
    if let Some(resps) = item.get("response").and_then(|v| v.as_array()) {
        draft.examples = resps
            .iter()
            .map(|r| {
                let name = r.get("name").and_then(|v| v.as_str()).unwrap_or("Response").to_string();
                let status = r.get("code").and_then(|v| v.as_u64()).unwrap_or(200) as u16;
                let status_text = r.get("status").and_then(|v| v.as_str()).unwrap_or("OK").to_string();
                let body = r.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let headers = extract_headers(r.get("header").or_else(|| r.get("headers")))
                    .into_iter()
                    .filter(|h| !h.key.is_empty())
                    .map(|h| (h.key, h.value))
                    .collect();
                SavedExample {
                    id: Uuid::new_v4(),
                    name,
                    status,
                    status_text,
                    headers,
                    body,
                }
            })
            .collect();
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
    let mut events = Vec::new();
    if !col.pre_request_script.is_empty() {
        events.push(json!({
            "listen": "prerequest",
            "script": {
                "type": "text/javascript",
                "exec": col.pre_request_script.lines().collect::<Vec<_>>(),
            }
        }));
    }
    if !col.test_script.is_empty() {
        events.push(json!({
            "listen": "test",
            "script": {
                "type": "text/javascript",
                "exec": col.test_script.lines().collect::<Vec<_>>(),
            }
        }));
    }
    let mut doc = json!({
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
    if !events.is_empty() {
        doc["event"] = Value::Array(events);
    }
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
            CollectionItem::Folder(f) => {
                let mut events = Vec::new();
                if !f.pre_request_script.is_empty() {
                    events.push(json!({
                        "listen": "prerequest",
                        "script": {
                            "type": "text/javascript",
                            "exec": f.pre_request_script.lines().collect::<Vec<_>>(),
                        }
                    }));
                }
                if !f.test_script.is_empty() {
                    events.push(json!({
                        "listen": "test",
                        "script": {
                            "type": "text/javascript",
                            "exec": f.test_script.lines().collect::<Vec<_>>(),
                        }
                    }));
                }
                let mut folder_json = json!({
                    "id": f.id.to_string(),
                    "_postman_id": f.id.to_string(),
                    "name": f.name,
                    "description": f.description,
                    "auth": export_auth(&f.auth),
                    "item": export_items(&f.items),
                });
                if !events.is_empty() {
                    folder_json["event"] = Value::Array(events);
                }
                folder_json
            }
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
            let fields: Vec<Value> = r
                .body_formdata
                .iter()
                .filter(|f| !f.key.is_empty())
                .map(|f| {
                    if f.is_file {
                        json!({
                            "key": f.key,
                            "type": "file",
                            "src": f.value,
                            "disabled": !f.enabled,
                        })
                    } else {
                        json!({
                            "key": f.key,
                            "type": "text",
                            "value": f.value,
                            "disabled": !f.enabled,
                        })
                    }
                })
                .collect();
            body = json!({ "mode": "formdata", "formdata": fields });
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

    let has_query = r.params.iter().any(|p| !p.key.is_empty());
    let has_path = r.path_params.iter().any(|p| !p.key.is_empty());
    let url_val = if has_query || has_path {
        let query_pairs: Vec<Value> = r
            .params
            .iter()
            .filter(|p| !p.key.is_empty())
            .map(|p| {
                json!({
                    "key": p.key,
                    "value": p.value,
                    "disabled": !p.enabled,
                })
            })
            .collect();
        let var_pairs: Vec<Value> = r
            .path_params
            .iter()
            .filter(|p| !p.key.is_empty())
            .map(|p| {
                json!({
                    "key": p.key,
                    "value": p.value,
                    "disabled": !p.enabled,
                })
            })
            .collect();
        let mut u = json!({
            "raw": r.url,
            "query": query_pairs,
        });
        if !var_pairs.is_empty() {
            u["variable"] = Value::Array(var_pairs);
        }
        u
    } else {
        json!(r.url)
    };

    let responses: Vec<Value> = r
        .examples
        .iter()
        .map(|ex| {
            let hdrs: Vec<Value> = ex
                .headers
                .iter()
                .map(|(k, v)| json!({ "key": k, "value": v }))
                .collect();
            json!({
                "name": ex.name,
                "status": ex.status_text,
                "code": ex.status,
                "header": hdrs,
                "body": ex.body,
            })
        })
        .collect();

    let mut item = json!({
        "id": r.id.to_string(),
        "_postman_id": r.id.to_string(),
        "name": r.name,
        "request": {
            "method": r.method.as_str(),
            "header": headers,
            "body": body,
            "url": url_val,
            "auth": export_auth(&r.auth),
            "description": r.description,
        },
        "event": events,
        "response": responses,
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

/// Parse a simple cURL command into a [`RequestDraft`].
pub fn import_curl(text: &str) -> Result<RequestDraft, ImpexError> {
    let mut draft = RequestDraft {
        name: "cURL import".into(),
        ..Default::default()
    };
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
            '\\' if in_double || !in_single => {
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
            let draft = RequestDraft {
                name: format!("{m} {path}"),
                method: parse_method(&m),
                url: format!("{base}{path}"),
                ..Default::default()
            };
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
        let mut draft = RequestDraft {
            name: req
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
                .unwrap_or_else(|| format!("entry-{i}")),
            method: parse_method(req.get("method").and_then(|m| m.as_str()).unwrap_or("GET")),
            url: req
                .get("url")
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string(),
            ..Default::default()
        };
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
        let req = RequestDraft {
            name: "Ping".into(),
            url: "https://example.com".into(),
            method: HttpMethod::Get,
            ..Default::default()
        };
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
        let mut req = RequestDraft {
            name: "List Products".into(),
            url: "https://api.store.com/v1/products?limit=10".into(),
            method: HttpMethod::Get,
            ..Default::default()
        };
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
        let req = RequestDraft {
            name: "GET /users".into(),
            method: HttpMethod::Get,
            url: "https://api.com/users".into(),
            test_script: "pm.test('status is 200', () => {});".into(),
            ..Default::default()
        };
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
        let mut r = RequestDraft {
            assertions: vec![
                Assertion::new(AssertionKind::StatusEquals { code: 201 }),
                Assertion::new(AssertionKind::JsonPathEquals {
                    path: "data.id".into(),
                    expected: "7".into(),
                }),
            ],
            ..Default::default()
        };
        r.assertions[1].enabled = false;
        let text = serde_json::to_string(&export_request_item(&r)).unwrap();
        let back = import_request_item(&text).unwrap();
        assert_eq!(back.assertions, r.assertions);

        // No assertions => no extension key written (clean Postman output).
        let plain = export_request_item(&RequestDraft::default());
        assert!(plain.get("x-blitz").is_none());
    }

    #[test]
    fn test_import_postman_comprehensive_features() {
        let sample = r#"{
            "info": {
                "name": "Comprehensive Collection",
                "description": "Full feature test",
                "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
            },
            "header": [
                { "key": "X-Col-Header", "value": "col-val" }
            ],
            "event": [
                {
                    "listen": "prerequest",
                    "script": { "exec": ["console.log('col prerequest');"] }
                },
                {
                    "listen": "test",
                    "script": { "exec": ["pm.test('col test', () => {});"] }
                }
            ],
            "item": [
                {
                    "name": "Admin Folder",
                    "description": "Admin APIs",
                    "header": [
                        { "key": "x-auth-host", "value": "snbhakkiportal.rcell.me" }
                    ],
                    "event": [
                        {
                            "listen": "prerequest",
                            "script": { "exec": ["console.log('folder prerequest');"] }
                        }
                    ],
                    "item": [
                        {
                            "name": "Login",
                            "request": {
                                "method": "POST",
                                "url": {
                                    "raw": "https://api.rcell.me/login?org=rcell",
                                    "query": [
                                        { "key": "org", "value": "rcell" }
                                    ]
                                },
                                "header": [
                                    { "key": "Content-Type", "value": "application/json" }
                                ],
                                "body": {
                                    "mode": "formdata",
                                    "formdata": [
                                        { "key": "username", "value": "admin", "type": "text" },
                                        { "key": "avatar", "src": "/path/to/img.png", "type": "file" }
                                    ]
                                },
                                "protocolProfileBehavior": {
                                    "followRedirects": false,
                                    "strictSSL": true
                                }
                            },
                            "response": [
                                {
                                    "name": "Success Example",
                                    "code": 200,
                                    "status": "OK",
                                    "body": "{\"token\":\"xyz\"}",
                                    "header": [
                                        { "key": "Content-Type", "value": "application/json" }
                                    ]
                                }
                            ]
                        }
                    ]
                }
            ]
        }"#;

        let col = import_collection_v21(sample).unwrap();
        assert_eq!(col.name, "Comprehensive Collection");
        assert_eq!(col.pre_request_script, "console.log('col prerequest');");
        assert_eq!(col.test_script, "pm.test('col test', () => {});");

        assert_eq!(col.items.len(), 1);
        let folder = match &col.items[0] {
            CollectionItem::Folder(f) => f,
            _ => panic!("expected folder"),
        };
        assert_eq!(folder.name, "Admin Folder");
        assert_eq!(folder.description, "Admin APIs");
        assert_eq!(folder.pre_request_script, "console.log('folder prerequest');");

        assert_eq!(folder.items.len(), 1);
        let req = match &folder.items[0] {
            CollectionItem::Request(r) => r,
            _ => panic!("expected request"),
        };
        assert_eq!(req.name, "Login");
        // Verify folder header inherited into request!
        assert!(req.headers.iter().any(|h| h.key.eq_ignore_ascii_case("x-auth-host") && h.value == "snbhakkiportal.rcell.me"));
        // Verify collection header inherited into request!
        assert!(req.headers.iter().any(|h| h.key.eq_ignore_ascii_case("X-Col-Header") && h.value == "col-val"));
        // Verify request's own header
        assert!(req.headers.iter().any(|h| h.key.eq_ignore_ascii_case("Content-Type") && h.value == "application/json"));

        // Verify query params imported
        assert!(req.params.iter().any(|p| p.key == "org" && p.value == "rcell"));

        // Verify form-data imported
        assert_eq!(req.body_mode, BodyMode::FormData);
        assert!(req.body_formdata.iter().any(|f| f.key == "username" && f.value == "admin" && !f.is_file));
        assert!(req.body_formdata.iter().any(|f| f.key == "avatar" && f.value == "/path/to/img.png" && f.is_file));

        // Verify protocol profile settings imported
        assert!(!req.settings.follow_redirects);
        assert!(req.settings.ssl_verify);

        // Verify saved response examples imported
        assert_eq!(req.examples.len(), 1);
        assert_eq!(req.examples[0].status, 200);
        assert_eq!(req.examples[0].body, "{\"token\":\"xyz\"}");
    }
}
