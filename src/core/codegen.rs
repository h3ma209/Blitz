//! Client code snippets (cURL / fetch / Python requests).
//! Languages inspired by `postman-code-generators.js` (reports/11).

use super::request::{AuthType, BodyMode, RequestDraft};

pub fn snippet_languages() -> &'static [&'static str] {
    &[
        "cURL",
        "JavaScript fetch",
        "Python requests",
        "Java OkHttp",
        "Go net/http",
        "C# HttpClient",
    ]
}

pub fn generate_snippet(lang: &str, draft: &RequestDraft) -> String {
    match lang {
        "JavaScript fetch" => js_fetch(draft),
        "Python requests" => python_requests(draft),
        "Java OkHttp" => java_okhttp(draft),
        "Go net/http" => go_nethttp(draft),
        "C# HttpClient" => csharp_httpclient(draft),
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
        BodyMode::Binary => Some(format!("@{}", draft.binary_path)),
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

fn java_okhttp(draft: &RequestDraft) -> String {
    let mut hdrs = String::new();
    for (k, v) in enabled_headers(draft) {
        hdrs.push_str(&format!(
            "    .addHeader(\"{}\", \"{}\")\n",
            k,
            v.replace('"', "\\\"")
        ));
    }
    if let Some((k, v)) = auth_header(draft) {
        hdrs.push_str(&format!(
            "    .addHeader(\"{k}\", \"{}\")\n",
            v.replace('"', "\\\"")
        ));
    }
    let body = body_payload(draft)
        .map(|b| {
            format!(
                "RequestBody body = RequestBody.create(\"{}\", MediaType.parse(\"application/json\"));\n",
                b.replace('\\', "\\\\").replace('"', "\\\"")
            )
        })
        .unwrap_or_default();
    let body_arg = if body.is_empty() {
        "null"
    } else {
        "body"
    };
    format!(
        "OkHttpClient client = new OkHttpClient();\n{body}\
Request request = new Request.Builder()\n    .url(\"{}\")\n    .method(\"{}\", {body_arg})\n{hdrs}    .build();\n\
try (Response response = client.newCall(request).execute()) {{\n    System.out.println(response.body().string());\n}}",
        url_with_params(draft),
        draft.method.as_str(),
    )
}

fn go_nethttp(draft: &RequestDraft) -> String {
    let mut hdrs = String::new();
    for (k, v) in enabled_headers(draft) {
        hdrs.push_str(&format!(
            "\treq.Header.Set(\"{k}\", \"{}\")\n",
            v.replace('"', "\\\"")
        ));
    }
    if let Some((k, v)) = auth_header(draft) {
        hdrs.push_str(&format!(
            "\treq.Header.Set(\"{k}\", \"{}\")\n",
            v.replace('"', "\\\"")
        ));
    }
    let body = body_payload(draft)
        .map(|b| format!("strings.NewReader(`{b}`)"))
        .unwrap_or_else(|| "nil".into());
    format!(
        "package main\n\nimport (\n\t\"fmt\"\n\t\"io\"\n\t\"net/http\"\n\t\"strings\"\n)\n\nfunc main() {{\n\tbody := {body}\n\treq, _ := http.NewRequest(\"{}\", \"{}\", body)\n{hdrs}\tres, err := http.DefaultClient.Do(req)\n\tif err != nil {{ panic(err) }}\n\tdefer res.Body.Close()\n\tb, _ := io.ReadAll(res.Body)\n\tfmt.Println(res.Status, string(b))\n}}",
        draft.method.as_str(),
        url_with_params(draft),
    )
}

fn csharp_httpclient(draft: &RequestDraft) -> String {
    let mut hdrs = String::new();
    for (k, v) in enabled_headers(draft) {
        hdrs.push_str(&format!(
            "client.DefaultRequestHeaders.TryAddWithoutValidation(\"{k}\", \"{}\");\n",
            v.replace('"', "\\\"")
        ));
    }
    if let Some((k, v)) = auth_header(draft) {
        hdrs.push_str(&format!(
            "client.DefaultRequestHeaders.TryAddWithoutValidation(\"{k}\", \"{}\");\n",
            v.replace('"', "\\\"")
        ));
    }
    let body = body_payload(draft).unwrap_or_default();
    format!(
        "using var client = new HttpClient();\n{hdrs}var content = new StringContent(@\"{body}\", System.Text.Encoding.UTF8, \"application/json\");\nvar response = await client.{method}Async(\"{url}\"{content_arg});\nConsole.WriteLine(await response.Content.ReadAsStringAsync());",
        method = match draft.method {
            crate::core::request::HttpMethod::Post => "Post",
            crate::core::request::HttpMethod::Put => "Put",
            crate::core::request::HttpMethod::Delete => "Delete",
            _ => "Get",
        },
        url = url_with_params(draft),
        content_arg = if matches!(
            draft.method,
            crate::core::request::HttpMethod::Get | crate::core::request::HttpMethod::Head
        ) {
            ""
        } else {
            ", content"
        },
        body = body.replace('"', "\"\""),
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
