//! Client code snippets (cURL / fetch / Python requests).
//! Languages inspired by `postman-code-generators.js` (reports/11).

use super::request::{AuthType, BodyMode, RequestDraft};

pub fn snippet_languages() -> &'static [&'static str] {
    &["cURL", "JavaScript fetch", "Python requests"]
}

pub fn generate_snippet(lang: &str, draft: &RequestDraft) -> String {
    match lang {
        "JavaScript fetch" => js_fetch(draft),
        "Python requests" => python_requests(draft),
        _ => curl(draft),
    }
}

fn enabled_headers(draft: &RequestDraft) -> Vec<(&str, &str)> {
    draft
        .headers
        .iter()
        .filter(|h| h.enabled && !h.key.is_empty())
        .map(|h| (h.key.as_str(), h.value.as_str()))
        .collect()
}

fn url_with_params(draft: &RequestDraft) -> String {
    let mut url = draft.url.clone();
    let qs: Vec<_> = draft
        .params
        .iter()
        .filter(|p| p.enabled && !p.key.is_empty())
        .map(|p| format!("{}={}", enc(&p.key), enc(&p.value)))
        .collect();
    if !qs.is_empty() {
        if url.contains('?') {
            url.push('&');
        } else {
            url.push('?');
        }
        url.push_str(&qs.join("&"));
    }
    url
}

fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn body_payload(draft: &RequestDraft) -> Option<String> {
    match draft.body_mode {
        BodyMode::None => None,
        BodyMode::Raw => Some(draft.body.clone()),
        BodyMode::Graphql => {
            let vars = if draft.graphql_variables.trim().is_empty() {
                "{}".into()
            } else {
                draft.graphql_variables.clone()
            };
            Some(format!(
                "{{\"query\":{},\"variables\":{}}}",
                serde_json::to_string(&draft.graphql_query).unwrap_or_else(|_| "\"\"".into()),
                vars
            ))
        }
        BodyMode::Urlencoded => {
            let s: String = draft
                .body_urlencoded
                .iter()
                .filter(|p| p.enabled && !p.key.is_empty())
                .map(|p| format!("{}={}", enc(&p.key), enc(&p.value)))
                .collect::<Vec<_>>()
                .join("&");
            Some(s)
        }
        BodyMode::FormData => Some("(multipart form-data — see UI)".into()),
    }
}

fn auth_header(draft: &RequestDraft) -> Option<(String, String)> {
    match draft.auth.auth_type {
        AuthType::Bearer => {
            if draft.auth.token.is_empty() {
                None
            } else {
                Some(("Authorization".into(), format!("Bearer {}", draft.auth.token)))
            }
        }
        AuthType::Oauth2 => {
            if draft.auth.oauth2.access_token.is_empty() {
                None
            } else {
                Some((
                    "Authorization".into(),
                    format!("Bearer {}", draft.auth.oauth2.access_token),
                ))
            }
        }
        AuthType::ApiKey => {
            let h = if draft.auth.api_key_header.is_empty() {
                "X-Api-Key"
            } else {
                draft.auth.api_key_header.as_str()
            };
            Some((h.into(), draft.auth.token.clone()))
        }
        _ => None,
    }
}

pub fn curl(draft: &RequestDraft) -> String {
    let mut parts = vec![format!("curl --request {} \\", draft.method.as_str())];
    parts.push(format!("  --url '{}' \\", url_with_params(draft)));
    for (k, v) in enabled_headers(draft) {
        parts.push(format!("  --header '{k}: {v}' \\"));
    }
    if let Some((k, v)) = auth_header(draft) {
        parts.push(format!("  --header '{k}: {v}' \\"));
    }
    if let Some(body) = body_payload(draft) {
        let escaped = body.replace('\'', "'\\''");
        parts.push(format!("  --data '{escaped}'"));
    } else if let Some(last) = parts.last_mut() {
        if last.ends_with(" \\") {
            *last = last.trim_end_matches(" \\").to_string();
        }
    }
    parts.join("\n")
}

fn js_fetch(draft: &RequestDraft) -> String {
    let mut headers = String::from("{\n");
    for (k, v) in enabled_headers(draft) {
        headers.push_str(&format!("    \"{k}\": \"{}\",\n", v.replace('"', "\\\"")));
    }
    if let Some((k, v)) = auth_header(draft) {
        headers.push_str(&format!("    \"{k}\": \"{}\",\n", v.replace('"', "\\\"")));
    }
    headers.push_str("  }");
    let body = body_payload(draft)
        .map(|b| format!(",\n  body: {}", serde_json::to_string(&b).unwrap_or_default()))
        .unwrap_or_default();
    format!(
        "fetch(\"{}\", {{\n  method: \"{}\",\n  headers: {headers}{body}\n}});",
        url_with_params(draft),
        draft.method.as_str()
    )
}

fn python_requests(draft: &RequestDraft) -> String {
    let mut headers = String::from("{\n");
    for (k, v) in enabled_headers(draft) {
        headers.push_str(&format!("  \"{k}\": \"{}\",\n", v.replace('"', "\\\"")));
    }
    if let Some((k, v)) = auth_header(draft) {
        headers.push_str(&format!("  \"{k}\": \"{}\",\n", v.replace('"', "\\\"")));
    }
    headers.push('}');
    let url = url_with_params(draft);
    let method = draft.method.as_str().to_lowercase();
    match body_payload(draft) {
        Some(b) if draft.body_mode == BodyMode::Raw || draft.body_mode == BodyMode::Graphql => {
            format!(
                "import requests\n\nheaders = {headers}\ndata = {data}\nresp = requests.{method}(\"{url}\", headers=headers, data=data)\nprint(resp.status_code, resp.text)",
                data = serde_json::to_string(&b).unwrap_or_default()
            )
        }
        _ => format!(
            "import requests\n\nheaders = {headers}\nresp = requests.{method}(\"{url}\", headers=headers)\nprint(resp.status_code, resp.text)"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curl_contains_method() {
        let d = RequestDraft::default();
        let s = curl(&d);
        assert!(s.contains("GET"));
        assert!(s.contains("curl"));
    }
}
