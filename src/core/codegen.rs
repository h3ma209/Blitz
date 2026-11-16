//! Client code snippets (cURL / fetch / Python requests).
//! Languages inspired by `postman-code-generators.js` (reports/11).

use super::request::{AuthType, BodyMode, HttpMethod, RequestDraft, RequestProtocol};

pub fn snippet_languages() -> &'static [&'static str] {
    &[
        "cURL",
        "Rust (reqwest)",
        "TypeScript (axios)",
        "JavaScript fetch",
        "Python (httpx)",
        "Python (requests)",
        "Go net/http",
        "Java OkHttp",
        "C# HttpClient",
    ]
}

pub fn generate_snippet(lang: &str, draft: &RequestDraft) -> String {
    generate_snippet_resolved(lang, draft, None)
}

pub fn generate_snippet_resolved(
    lang: &str,
    draft: &RequestDraft,
    ctx: Option<&super::variables::VariableContext>,
) -> String {
    let resolved_draft = if let Some(c) = ctx {
        resolve_draft_variables(draft, c)
    } else {
        draft.clone()
    };

    match lang {
        "Rust (reqwest)" => rust_reqwest(&resolved_draft),
        "TypeScript (axios)" => typescript_axios(&resolved_draft),
        "Python (httpx)" => python_httpx(&resolved_draft),
        "JavaScript fetch" => js_fetch(&resolved_draft),
        "Python (requests)" | "Python requests" => python_requests(&resolved_draft),
        "Java OkHttp" => java_okhttp(&resolved_draft),
        "Go net/http" => go_nethttp(&resolved_draft),
        "C# HttpClient" => csharp_httpclient(&resolved_draft),
        _ => curl(&resolved_draft),
    }
}

fn resolve_draft_variables(
    draft: &RequestDraft,
    ctx: &super::variables::VariableContext,
) -> RequestDraft {
    let mut d = draft.clone();
    for p in &mut d.path_params {
        p.key = ctx.resolve_string_non_vault(&p.key);
        p.value = ctx.resolve_string_non_vault(&p.value);
        if p.value.trim().is_empty() {
            if let Some(val) = ctx.path.get(p.key.trim()) {
                if !val.trim().is_empty() {
                    p.value = ctx.resolve_string_non_vault(val);
                }
            }
        }
    }
    d.url = ctx.resolve_string_non_vault(&d.url);
    d.url = crate::core::substitute_path_params(&d.url, &d.path_params);
    d.url = ctx.resolve_string_non_vault(&d.url);

    d.body = ctx.resolve_string_non_vault(&d.body);
    d.binary_path = ctx.resolve_string_non_vault(&d.binary_path);
    d.graphql_query = ctx.resolve_string_non_vault(&d.graphql_query);
    d.graphql_variables = ctx.resolve_string_non_vault(&d.graphql_variables);

    for h in &mut d.headers {
        h.key = ctx.resolve_string_non_vault(&h.key);
        h.value = ctx.resolve_string_non_vault(&h.value);
    }
    for p in &mut d.params {
        p.key = ctx.resolve_string_non_vault(&p.key);
        p.value = ctx.resolve_string_non_vault(&p.value);
    }
    for u in &mut d.body_urlencoded {
        u.key = ctx.resolve_string_non_vault(&u.key);
        u.value = ctx.resolve_string_non_vault(&u.value);
    }
    for f in &mut d.body_formdata {
        f.key = ctx.resolve_string_non_vault(&f.key);
        f.value = ctx.resolve_string_non_vault(&f.value);
    }

    d.auth.token = ctx.resolve_string_non_vault(&d.auth.token);
    d.auth.username = ctx.resolve_string_non_vault(&d.auth.username);
    d.auth.password = ctx.resolve_string_non_vault(&d.auth.password);
    d.auth.api_key_header = ctx.resolve_string_non_vault(&d.auth.api_key_header);

    d.auth.oauth2.auth_url = ctx.resolve_string_non_vault(&d.auth.oauth2.auth_url);
    d.auth.oauth2.token_url = ctx.resolve_string_non_vault(&d.auth.oauth2.token_url);
    d.auth.oauth2.client_id = ctx.resolve_string_non_vault(&d.auth.oauth2.client_id);
    d.auth.oauth2.client_secret = ctx.resolve_string_non_vault(&d.auth.oauth2.client_secret);
    d.auth.oauth2.scope = ctx.resolve_string_non_vault(&d.auth.oauth2.scope);
    d.auth.oauth2.redirect_uri = ctx.resolve_string_non_vault(&d.auth.oauth2.redirect_uri);
    d.auth.oauth2.access_token = ctx.resolve_string_non_vault(&d.auth.oauth2.access_token);
    d.auth.oauth2.refresh_token = ctx.resolve_string_non_vault(&d.auth.oauth2.refresh_token);

    d.auth.extra.key = ctx.resolve_string_non_vault(&d.auth.extra.key);
    d.auth.extra.secret = ctx.resolve_string_non_vault(&d.auth.extra.secret);
    d.auth.extra.token = ctx.resolve_string_non_vault(&d.auth.extra.token);
    d.auth.extra.token_secret = ctx.resolve_string_non_vault(&d.auth.extra.token_secret);
    d.auth.extra.signature_method = ctx.resolve_string_non_vault(&d.auth.extra.signature_method);
    d.auth.extra.region = ctx.resolve_string_non_vault(&d.auth.extra.region);
    d.auth.extra.service = ctx.resolve_string_non_vault(&d.auth.extra.service);
    d.auth.extra.algorithm = ctx.resolve_string_non_vault(&d.auth.extra.algorithm);
    d.auth.extra.ext = ctx.resolve_string_non_vault(&d.auth.extra.ext);

    d.ws_message = ctx.resolve_string_non_vault(&d.ws_message);
    d.grpc_method = ctx.resolve_string_non_vault(&d.grpc_method);
    d.mqtt_topic = ctx.resolve_string_non_vault(&d.mqtt_topic);
    d.mqtt_client_id = ctx.resolve_string_non_vault(&d.mqtt_client_id);
    d.mcp_command = ctx.resolve_string_non_vault(&d.mcp_command);
    d.mcp_args = ctx.resolve_string_non_vault(&d.mcp_args);
    d.mcp_tool_name = ctx.resolve_string_non_vault(&d.mcp_tool_name);
    d.mcp_tool_args_json = ctx.resolve_string_non_vault(&d.mcp_tool_args_json);
    d.socketio_event = ctx.resolve_string_non_vault(&d.socketio_event);
    d.graphql_ws_query = ctx.resolve_string_non_vault(&d.graphql_ws_query);

    d
}

fn enabled_headers(draft: &RequestDraft) -> Vec<(&str, &str)> {
    draft
        .headers
        .iter()
        .filter(|h| {
            h.enabled
                && !h.key.trim().is_empty()
                && !h.value.trim().eq_ignore_ascii_case("<calculated at runtime>")
        })
        .map(|h| (h.key.as_str(), h.value.as_str()))
        .collect()
}

fn url_with_params(draft: &RequestDraft) -> String {
    let mut url = crate::core::substitute_path_params(&draft.url, &draft.path_params);
    let enabled_params: Vec<_> = draft
        .params
        .iter()
        .filter(|p| p.enabled && !p.key.is_empty())
        .collect();
    if enabled_params.is_empty() {
        return url;
    }

    if let Some((base, existing_qs)) = url.split_once('?') {
        let existing_keys: Vec<&str> = existing_qs
            .split('&')
            .filter(|s| !s.is_empty())
            .map(|part| part.split_once('=').map(|(k, _)| k).unwrap_or(part))
            .collect();
        let all_in_draft = !existing_keys.is_empty()
            && existing_keys
                .iter()
                .all(|ek| draft.params.iter().any(|p| p.key == *ek));
        if all_in_draft {
            url = base.to_string();
        } else {
            let missing_qs: Vec<_> = enabled_params
                .into_iter()
                .filter(|p| !existing_keys.iter().any(|ek| *ek == p.key))
                .map(|p| format!("{}={}", enc(&p.key), enc(&p.value)))
                .collect();
            if !missing_qs.is_empty() {
                if !url.ends_with('?') && !url.ends_with('&') {
                    url.push('&');
                }
                url.push_str(&missing_qs.join("&"));
            }
            return url;
        }
    }

    let qs: Vec<_> = enabled_params
        .into_iter()
        .map(|p| format!("{}={}", enc(&p.key), enc(&p.value)))
        .collect();
    if !qs.is_empty() {
        if url.contains('?') {
            if !url.ends_with('?') && !url.ends_with('&') {
                url.push('&');
            }
        } else {
            url.push('?');
        }
        url.push_str(&qs.join("&"));
    }
    url
}

fn enc(s: &str) -> String {
    let mut out = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"{{") {
            if let Some(rel_end) = s[i + 2..].find("}}") {
                let end = i + 2 + rel_end + 2;
                out.push_str(&s[i..end]);
                i = end;
                continue;
            }
        }
        let b = bytes[i];
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
        i += 1;
    }
    out
}

fn body_payload(draft: &RequestDraft) -> Option<String> {
    match draft.body_mode {
        BodyMode::None => {
            if draft.protocol == RequestProtocol::Sse
                && draft.method != HttpMethod::Get
                && draft.method != HttpMethod::Head
                && !draft.body.trim().is_empty()
            {
                Some(draft.body.clone())
            } else {
                None
            }
        }
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
    use base64::Engine as _;

    let candidate: (String, String) = match draft.auth.auth_type {
        AuthType::Bearer => {
            if draft.auth.token.is_empty() {
                None
            } else {
                Some(("Authorization".into(), format!("Bearer {}", draft.auth.token)))
            }
        }
        AuthType::Oauth2 => {
            let tok = if !draft.auth.oauth2.access_token.is_empty() {
                &draft.auth.oauth2.access_token
            } else {
                &draft.auth.token
            };
            if tok.is_empty() {
                None
            } else {
                Some(("Authorization".into(), format!("Bearer {tok}")))
            }
        }
        AuthType::ApiKey => {
            if draft.auth.token.is_empty() {
                None
            } else {
                let h = if draft.auth.api_key_header.is_empty() {
                    "X-Api-Key"
                } else {
                    draft.auth.api_key_header.as_str()
                };
                Some((h.into(), draft.auth.token.clone()))
            }
        }
        AuthType::Basic => {
            let u = &draft.auth.username;
            let p = &draft.auth.password;
            if (u.is_empty() && p.is_empty()) || u.contains("{{") || p.contains("{{") {
                None
            } else {
                let encoded = base64::engine::general_purpose::STANDARD
                    .encode(format!("{u}:{p}").as_bytes());
                Some(("Authorization".into(), format!("Basic {encoded}")))
            }
        }
        _ => None,
    }?;

    let already_present = enabled_headers(draft)
        .iter()
        .any(|(hk, _)| hk.eq_ignore_ascii_case(&candidate.0));
    if already_present {
        None
    } else {
        Some(candidate)
    }
}

fn sh_escape(s: &str) -> String {
    s.replace('\'', "'\\''")
}

pub fn curl(draft: &RequestDraft) -> String {
    let mut parts = vec![format!("curl --request {} \\", draft.method.as_str())];
    parts.push(format!("  --url '{}' \\", sh_escape(&url_with_params(draft))));
    let hdrs = enabled_headers(draft);
    for (k, v) in &hdrs {
        parts.push(format!("  --header '{}: {}' \\", sh_escape(k), sh_escape(v)));
    }
    if let Some((k, v)) = auth_header(draft) {
        parts.push(format!("  --header '{}: {}' \\", sh_escape(&k), sh_escape(&v)));
    } else if draft.auth.auth_type == AuthType::Basic
        && (!draft.auth.username.is_empty() || !draft.auth.password.is_empty())
        && !hdrs
            .iter()
            .any(|(hk, _)| hk.eq_ignore_ascii_case("authorization"))
    {
        parts.push(format!(
            "  --user '{}:{}' \\",
            sh_escape(&draft.auth.username),
            sh_escape(&draft.auth.password)
        ));
    }

    if draft.body_mode == BodyMode::FormData {
        for f in draft
            .body_formdata
            .iter()
            .filter(|f| f.enabled && !f.key.trim().is_empty())
        {
            let prefix = if f.is_file { "@" } else { "" };
            parts.push(format!(
                "  --form '{}={}{}' \\",
                sh_escape(&f.key),
                prefix,
                sh_escape(&f.value)
            ));
        }
        if let Some(last) = parts.last_mut() {
            if last.ends_with(" \\") {
                *last = last.trim_end_matches(" \\").to_string();
            }
        }
    } else if let Some(body) = body_payload(draft) {
        let escaped = sh_escape(&body);
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

fn rust_reqwest(draft: &RequestDraft) -> String {
    let mut hdrs = String::new();
    for (k, v) in enabled_headers(draft) {
        hdrs.push_str(&format!(
            "        .header(\"{k}\", \"{}\")\n",
            v.replace('"', "\\\"")
        ));
    }
    if let Some((k, v)) = auth_header(draft) {
        hdrs.push_str(&format!(
            "        .header(\"{k}\", \"{}\")\n",
            v.replace('"', "\\\"")
        ));
    }
    let body = body_payload(draft)
        .map(|b| {
            format!(
                "        .body({})\n",
                serde_json::to_string(&b).unwrap_or_else(|_| "\"\"".into())
            )
        })
        .unwrap_or_default();
    let method = match draft.method {
        crate::core::request::HttpMethod::Post => "post",
        crate::core::request::HttpMethod::Put => "put",
        crate::core::request::HttpMethod::Delete => "delete",
        crate::core::request::HttpMethod::Patch => "patch",
        crate::core::request::HttpMethod::Head => "head",
        _ => "get",
    };
    format!(
        "// Cargo.toml: reqwest = {{ version = \"0.12\", features = [\"json\"] }}, tokio = {{ version = \"1\", features = [\"full\"] }}\n\
#[tokio::main]\n\
async fn main() -> Result<(), Box<dyn std::error::Error>> {{\n    \
let client = reqwest::Client::new();\n    \
let resp = client.{method}(\"{url}\")\n\
{hdrs}{body}\
        .send()\n\
        .await?;\n    \
println!(\"Status: {{}}\", resp.status());\n    \
let body = resp.text().await?;\n    \
println!(\"{{body}}\");\n    \
Ok(())\n\
}}",
        url = url_with_params(draft),
    )
}

fn typescript_axios(draft: &RequestDraft) -> String {
    let mut headers = String::from("{\n");
    for (k, v) in enabled_headers(draft) {
        headers.push_str(&format!("    \"{k}\": \"{}\",\n", v.replace('"', "\\\"")));
    }
    if let Some((k, v)) = auth_header(draft) {
        headers.push_str(&format!("    \"{k}\": \"{}\",\n", v.replace('"', "\\\"")));
    }
    headers.push_str("  }");
    let data_prop = body_payload(draft)
        .map(|b| format!(",\n  data: {}", serde_json::to_string(&b).unwrap_or_default()))
        .unwrap_or_default();
    format!(
        "import axios from 'axios';\n\n\
const config = {{\n  \
method: '{method}',\n  \
url: '{url}',\n  \
headers: {headers}{data_prop}\n\
}};\n\n\
axios.request(config)\n  \
.then((response) => {{\n    \
console.log(response.status, response.data);\n  \
}})\n  \
.catch((error) => {{\n    \
console.error(error);\n  \
}});\n",
        method = draft.method.as_str().to_lowercase(),
        url = url_with_params(draft),
    )
}

fn python_httpx(draft: &RequestDraft) -> String {
    let mut headers = String::from("{\n");
    for (k, v) in enabled_headers(draft) {
        headers.push_str(&format!("    \"{k}\": \"{}\",\n", v.replace('"', "\\\"")));
    }
    if let Some((k, v)) = auth_header(draft) {
        headers.push_str(&format!("    \"{k}\": \"{}\",\n", v.replace('"', "\\\"")));
    }
    headers.push_str("  }");
    let url = url_with_params(draft);
    let method = draft.method.as_str().to_lowercase();
    match body_payload(draft) {
        Some(b) if draft.body_mode == BodyMode::Raw || draft.body_mode == BodyMode::Graphql => {
            format!(
                "import asyncio\nimport httpx\n\n\
async def main():\n    \
headers = {headers}\n    \
content = {data}\n    \
async with httpx.AsyncClient() as client:\n        \
resp = await client.{method}(\"{url}\", headers=headers, content=content)\n        \
print(resp.status_code, resp.text)\n\n\
asyncio.run(main())\n",
                data = serde_json::to_string(&b).unwrap_or_default()
            )
        }
        _ => format!(
            "import asyncio\nimport httpx\n\n\
async def main():\n    \
headers = {headers}\n    \
async with httpx.AsyncClient() as client:\n        \
resp = await client.{method}(\"{url}\", headers=headers)\n        \
print(resp.status_code, resp.text)\n\n\
asyncio.run(main())\n"
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

    #[test]
    fn test_new_snippets_and_resolution() {
        let mut d = RequestDraft {
            url: "https://api.example.com/{{resource}}".into(),
            ..Default::default()
        };
        d.headers.push(crate::core::request::KeyValue {
            key: "X-Key".into(),
            value: "{{api_key}}".into(),
            enabled: true,
        });

        let mut vars = crate::core::variables::VariableContext::default();
        vars.local.insert("resource".into(), "users".into());
        vars.local.insert("api_key".into(), "secret123".into());

        let rust_code = generate_snippet_resolved("Rust (reqwest)", &d, Some(&vars));
        assert!(rust_code.contains("https://api.example.com/users"));
        assert!(rust_code.contains("secret123"));
        assert!(rust_code.contains("reqwest::Client"));

        let ts_code = generate_snippet_resolved("TypeScript (axios)", &d, Some(&vars));
        assert!(ts_code.contains("axios.request"));
        assert!(ts_code.contains("https://api.example.com/users"));

        let py_code = generate_snippet_resolved("Python (httpx)", &d, Some(&vars));
        assert!(py_code.contains("httpx.AsyncClient"));
    }

    #[test]
    fn curl_resolves_all_fields_except_vault() {
        let mut vars = crate::core::variables::VariableContext::default();
        vars.environment
            .insert("base".into(), "https://svc.test".into());
        vars.environment.insert("item_id".into(), "99".into());
        vars.environment.insert("q_val".into(), "hello".into());
        vars.environment.insert("hdr_val".into(), "abc-123".into());
        vars.environment.insert("tok".into(), "bearer-live-tok".into());
        vars.environment.insert("body_val".into(), "42".into());
        vars.vault.set("secret_api", "DO_NOT_COPY_SECRET");
        vars.vault.set("vault_tok", "DO_NOT_COPY_TOKEN");

        let mut d = RequestDraft {
            method: crate::core::request::HttpMethod::Post,
            url: "{{base}}/items/:id?q={{q_val}}&sec={{vault:secret_api}}".into(),
            path_params: vec![crate::core::request::KeyValue::new("id", "{{item_id}}")],
            params: vec![
                crate::core::request::KeyValue::new("q", "{{q_val}}"),
                crate::core::request::KeyValue::new("sec", "{{vault:secret_api}}"),
            ],
            headers: vec![
                crate::core::request::KeyValue::new("X-Env", "{{hdr_val}}"),
                crate::core::request::KeyValue::new("X-Vault", "{{vault:secret_api}}"),
            ],
            body_mode: BodyMode::Raw,
            body: r#"{"num": "{{body_val}}", "v": "{{vault:secret_api}}"}"#.into(),
            ..Default::default()
        };
        d.auth.auth_type = AuthType::Bearer;
        d.auth.token = "{{tok}}".into();

        let out = generate_snippet_resolved("cURL", &d, Some(&vars));
        assert!(
            out.contains("--url 'https://svc.test/items/99?q=hello&sec={{vault:secret_api}}'"),
            "unexpected curl url:\n{out}"
        );
        // Should not duplicate query params
        assert_eq!(out.matches("q=hello").count(), 1, "query param duplicated:\n{out}");
        assert!(out.contains("--header 'X-Env: abc-123'"));
        assert!(out.contains("--header 'X-Vault: {{vault:secret_api}}'"));
        assert!(out.contains("--header 'Authorization: Bearer bearer-live-tok'"));
        assert!(out.contains(r#"{"num": "42", "v": "{{vault:secret_api}}"}"#));
        assert!(!out.contains("DO_NOT_COPY_SECRET"));

        // Auth token using vault must NOT expand
        d.auth.token = "{{vault:vault_tok}}".into();
        let out_vault_auth = generate_snippet_resolved("cURL", &d, Some(&vars));
        assert!(out_vault_auth.contains("--header 'Authorization: Bearer {{vault:vault_tok}}'"));
        assert!(!out_vault_auth.contains("DO_NOT_COPY_TOKEN"));

        // Basic auth resolution vs Basic auth with vault password
        d.auth.auth_type = AuthType::Basic;
        d.auth.username = "{{hdr_val}}".into();
        d.auth.password = "pw123".into();
        let out_basic = generate_snippet_resolved("cURL", &d, Some(&vars));
        assert!(out_basic.contains("Authorization: Basic "));

        d.auth.password = "{{vault:secret_api}}".into();
        let out_basic_vault = generate_snippet_resolved("cURL", &d, Some(&vars));
        assert!(out_basic_vault.contains("--user 'abc-123:{{vault:secret_api}}'"));
        assert!(!out_basic_vault.contains("DO_NOT_COPY_SECRET"));

        // SSE POST with BodyMode::None still includes raw JSON body
        let sse_req = RequestDraft {
            protocol: RequestProtocol::Sse,
            method: HttpMethod::Post,
            url: "{{base}}/chat".into(),
            body_mode: BodyMode::None,
            body: r#"{"prompt":"{{q_val}}"}"#.into(),
            ..Default::default()
        };
        let out_sse = generate_snippet_resolved("cURL", &sse_req, Some(&vars));
        assert!(out_sse.contains(r#"--data '{"prompt":"hello"}'"#));
    }
}
