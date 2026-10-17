//! Minimal Postman-style JS sandbox via `boa_engine`.
//!
//! Supported `pm.*`:
//! - `pm.variables` / `pm.environment` / `pm.collectionVariables` / `pm.globals` — `.get` / `.set`
//! - `pm.response.code` / `.status` / `.text()` / `.json()`
//! - `pm.test(name, fn)` + `pm.expect(actual).to.eql(expected)` / `.to.equal` / `.to.be.ok` / `.to.have.status(n)`
//! - `pm.sendRequest(url|req, cb)` — nested HTTP via blocking reqwest (sync callback)
//! - `console.log(...)` → messages
//!
//! Unsupported (documented): full Chai, async tests, `require`, package library.

use boa_engine::{js_string, Context, JsArgs, JsResult, JsValue, NativeFunction, Source};
use serde_json::Value as JsonValue;

use crate::core::{TestResult, VariableContext};

#[derive(Debug, Default)]
pub struct SandboxOutcome {
    pub messages: Vec<String>,
    pub tests: Vec<TestResult>,
    pub error: Option<String>,
    pub execution: SandboxExecution,
    /// HTML from `pm.visualizer.set(template, data)`.
    pub visualizer_html: String,
}

/// Response snapshot injected into `pm.response`.
#[derive(Debug, Clone, Default)]
pub struct SandboxResponse {
    pub status: u16,
    pub status_text: String,
    pub body: String,
    pub headers: Vec<(String, String)>,
}

/// Extra host data for scripts (vault, cookies, request, iteration, execution).
#[derive(Debug, Clone, Default)]
pub struct SandboxExtras {
    pub vault: std::collections::HashMap<String, String>,
    pub cookies: std::collections::HashMap<String, String>,
    pub request_url: String,
    pub request_method: String,
    pub iteration_data_json: String,
}

/// Execution control written by `pm.execution`.
#[derive(Debug, Clone, Default)]
pub struct SandboxExecution {
    pub next_request: Option<String>,
    pub skip_request: bool,
}

/// Run script in boa with a minimal `pm` + `console` host.
pub fn run_js_sandbox(
    script: &str,
    phase: &str,
    ctx: &mut VariableContext,
    response: Option<&SandboxResponse>,
) -> SandboxOutcome {
    run_js_sandbox_ex(script, phase, ctx, response, &SandboxExtras::default())
}

pub fn run_js_sandbox_ex(
    script: &str,
    phase: &str,
    ctx: &mut VariableContext,
    response: Option<&SandboxResponse>,
    extras: &SandboxExtras,
) -> SandboxOutcome {
    let mut out = SandboxOutcome::default();
    if script.trim().is_empty() {
        out.messages.push(format!("{phase}: skipped (empty)"));
        return out;
    }

    let mut context = Context::default();

    if let Err(e) = context.register_global_callable(
        js_string!("__np_http"),
        1,
        NativeFunction::from_fn_ptr(np_http_native),
    ) {
        out.error = Some(format!("{phase}: register __np_http: {e}"));
        return out;
    }

    // Bridge state via JSON blob mutated by host helpers embedded as JS.
    let bridge = build_bridge_json(ctx, response, extras);
    let bridge_json = serde_json::to_string(&bridge).unwrap_or_else(|_| "{}".into());

    let prelude = format!(
        r#"
var __np = {bridge_json};
var console = {{
  log: function() {{
    var parts = [];
    for (var i = 0; i < arguments.length; i++) {{
      var a = arguments[i];
      parts.push(typeof a === "object" ? JSON.stringify(a) : String(a));
    }}
    __np.logs.push(parts.join(" "));
  }}
}};
function __scope(name) {{
  return {{
    get: function(k) {{
      var m = __np.scopes[name] || {{}};
      return m[k];
    }},
    set: function(k, v) {{
      if (!__np.scopes[name]) __np.scopes[name] = {{}};
      __np.scopes[name][k] = String(v);
    }},
    unset: function(k) {{
      if (__np.scopes[name]) delete __np.scopes[name][k];
    }}
  }};
}}
var __hdrMap = {{}};
if (__np.response && __np.response.headers) {{
  for (var i = 0; i < __np.response.headers.length; i++) {{
    var h = __np.response.headers[i];
    __hdrMap[String(h[0]).toLowerCase()] = h[1];
  }}
}}
var pm = {{
  variables: __scope("local"),
  environment: __scope("environment"),
  collectionVariables: __scope("collection"),
  globals: __scope("globals"),
  vault: {{
    get: function(k) {{ return (__np.vault && __np.vault[k]) || undefined; }},
    set: function(k, v) {{ if (!__np.vault) __np.vault = {{}}; __np.vault[k] = String(v); }},
    unset: function(k) {{ if (__np.vault) delete __np.vault[k]; }}
  }},
  cookies: {{
    get: function(k) {{ return (__np.cookies && __np.cookies[k]) || undefined; }}
  }},
  request: {{
    url: __np.requestUrl || "",
    method: __np.requestMethod || ""
  }},
  iterationData: (function() {{
    try {{ return __np.iterationData ? JSON.parse(__np.iterationData) : {{}}; }}
    catch (e) {{ return {{}}; }}
  }})(),
  execution: {{
    setNextRequest: function(name) {{ __np.execution.nextRequest = name === null ? null : String(name); }},
    skipRequest: function() {{ __np.execution.skipRequest = true; }}
  }},
  response: {{
    code: (__np.response && __np.response.status) || 0,
    status: (__np.response && __np.response.statusText) || "",
    headers: {{
      get: function(name) {{ return __hdrMap[String(name).toLowerCase()]; }}
    }},
    text: function() {{ return (__np.response && __np.response.body) || ""; }},
    json: function() {{
      var t = (__np.response && __np.response.body) || "";
      return t ? JSON.parse(t) : null;
    }},
    to: {{
      have: {{
        status: function(code) {{
          var c = (__np.response && __np.response.status) || 0;
          if (c !== code) throw new Error("expected status " + code + " got " + c);
        }}
      }}
    }}
  }},
  test: function(name, fn) {{
    var ok = true;
    var msg = "passed";
    try {{
      fn();
    }} catch (e) {{
      ok = false;
      msg = String(e && e.message ? e.message : e);
    }}
    __np.tests.push({{ name: String(name), passed: ok, message: msg }});
  }},
  sendRequest: function(req, cb) {{
    var url = typeof req === "string" ? req : (req && req.url) || "";
    var method = (typeof req === "object" && req && req.method) || "GET";
    var body = "";
    if (typeof req === "object" && req && req.body !== undefined && req.body !== null) {{
      body = typeof req.body === "string" ? req.body : JSON.stringify(req.body);
    }}
    var headers = [];
    if (typeof req === "object" && req && req.header) {{
      for (var i = 0; i < req.header.length; i++) {{
        var h = req.header[i];
        headers.push([String(h.key || h.key), String(h.value)]);
      }}
    }}
    var raw = __np_http(JSON.stringify({{ url: url, method: method, body: body, headers: headers }}));
    var parsed = {{}};
    try {{ parsed = JSON.parse(raw); }} catch (e) {{ parsed = {{ error: String(e) }}; }}
    var err = parsed.error || null;
    var res = parsed.response || null;
    if (typeof cb === "function") {{
      cb(err, res);
    }}
    return res;
  }},
  visualizer: {{
    set: function(template, data) {{
      var tpl = String(template || "");
      var html = tpl;
      if (data !== undefined && data !== null) {{
        try {{
          var json = typeof data === "string" ? data : JSON.stringify(data);
          html = tpl.split("{{{{data}}}}").join(json);
          // naive {{key}} replacement from object
          if (typeof data === "object") {{
            for (var k in data) {{
              if (Object.prototype.hasOwnProperty.call(data, k)) {{
                html = html.split("{{{{" + k + "}}}}").join(String(data[k]));
              }}
            }}
          }}
        }} catch (e) {{
          html = tpl + "<!-- visualizer data error -->";
        }}
      }}
      __np.visualizerHtml = html;
    }}
  }}
}};
pm.expect = function(actual) {{
  return {{
    to: {{
      eql: function(expected) {{
        var a = JSON.stringify(actual);
        var e = JSON.stringify(expected);
        if (a !== e) throw new Error("expected " + e + " but got " + a);
      }},
      equal: function(expected) {{
        if (actual !== expected) throw new Error("expected " + String(expected) + " but got " + String(actual));
      }},
      include: function(part) {{
        var s = typeof actual === "string" ? actual : JSON.stringify(actual);
        if (s.indexOf(String(part)) < 0) throw new Error("expected to include " + part);
      }},
      be: {{
        get ok() {{
          if (!actual) throw new Error("expected truthy");
          return true;
        }}
      }},
      have: {{
        status: function(code) {{
          var c = (__np.response && __np.response.status) || 0;
          if (c !== code) throw new Error("expected status " + code + " got " + c);
        }},
        property: function(name) {{
          if (actual == null || actual[name] === undefined) throw new Error("missing property " + name);
        }}
      }}
    }}
  }};
}};
"#
    );

    let full = format!("{prelude}\n{script}\n;JSON.stringify(__np);");

    let result = context.eval(Source::from_bytes(full.as_bytes()));
    match result {
        Ok(val) => {
            let json_str = js_to_string(&mut context, &val);
            apply_bridge(&json_str, ctx, &mut out, phase);
        }
        Err(e) => {
            out.error = Some(format!("{phase}: JS error: {e}"));
            out.messages
                .push(format!("{phase}: JS error: {e}"));
        }
    }

    out.messages
        .push(format!("{phase}: JS sandbox done (tests={})", out.tests.len()));
    out
}

fn np_http_native(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let spec = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let result = host_send_request(&spec);
    Ok(JsValue::from(js_string!(result.as_str())))
}

/// Blocking nested HTTP for `pm.sendRequest` (JSON in / JSON out).
fn host_send_request(spec_json: &str) -> String {
    #[derive(serde::Deserialize)]
    struct Spec {
        url: String,
        #[serde(default = "default_get")]
        method: String,
        #[serde(default)]
        body: String,
        #[serde(default)]
        headers: Vec<(String, String)>,
    }
    fn default_get() -> String {
        "GET".into()
    }

    let Ok(spec) = serde_json::from_str::<Spec>(spec_json) else {
        return r#"{"error":"bad sendRequest spec"}"#.into();
    };
    if spec.url.is_empty() {
        return r#"{"error":"sendRequest: empty url"}"#.into();
    }

    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return serde_json::json!({"error": e.to_string()}).to_string();
        }
    };

    let method = reqwest::Method::from_bytes(spec.method.as_bytes())
        .unwrap_or(reqwest::Method::GET);
    let mut builder = client.request(method, &spec.url);
    for (k, v) in &spec.headers {
        if !k.is_empty() {
            builder = builder.header(k, v);
        }
    }
    if !spec.body.is_empty() {
        builder = builder.body(spec.body.clone());
    }

    match builder.send() {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let status_text = resp
                .status()
                .canonical_reason()
                .unwrap_or("")
                .to_string();
            let mut headers = Vec::new();
            for (k, v) in resp.headers().iter() {
                if let Ok(val) = v.to_str() {
                    headers.push((k.to_string(), val.to_string()));
                }
            }
            let body = resp.text().unwrap_or_default();
            let code = body.clone();
            serde_json::json!({
                "error": null,
                "response": {
                    "code": status,
                    "status": status_text,
                    "headers": headers,
                    "text": body,
                    "json": serde_json::from_str::<JsonValue>(&code).ok(),
                }
            })
            .to_string()
        }
        Err(e) => serde_json::json!({"error": e.to_string(), "response": null}).to_string(),
    }
}

fn js_to_string(context: &mut Context, val: &JsValue) -> String {
    match val.to_string(context) {
        Ok(s) => s.to_std_string_escaped(),
        Err(_) => "{}".into(),
    }
}

fn build_bridge_json(
    ctx: &VariableContext,
    response: Option<&SandboxResponse>,
    extras: &SandboxExtras,
) -> JsonValue {
    let mut scopes = serde_json::Map::new();
    scopes.insert("local".into(), map_to_json(&ctx.local));
    scopes.insert("environment".into(), map_to_json(&ctx.environment));
    scopes.insert("collection".into(), map_to_json(&ctx.collection));
    scopes.insert("globals".into(), map_to_json(&ctx.global));

    let resp = match response {
        Some(r) => {
            let headers: Vec<_> = r
                .headers
                .iter()
                .map(|(k, v)| json_arr(k, v))
                .collect();
            serde_json::json!({
                "status": r.status,
                "statusText": r.status_text,
                "body": r.body,
                "headers": headers,
            })
        }
        None => JsonValue::Null,
    };

    serde_json::json!({
        "scopes": scopes,
        "logs": [],
        "tests": [],
        "response": resp,
        "vault": map_to_json(&extras.vault),
        "cookies": map_to_json(&extras.cookies),
        "requestUrl": extras.request_url,
        "requestMethod": extras.request_method,
        "iterationData": if extras.iteration_data_json.is_empty() {
            "{}".to_string()
        } else {
            extras.iteration_data_json.clone()
        },
        "execution": { "nextRequest": null, "skipRequest": false },
        "visualizerHtml": "",
    })
}

fn json_arr(k: &str, v: &str) -> JsonValue {
    JsonValue::Array(vec![JsonValue::String(k.into()), JsonValue::String(v.into())])
}

fn map_to_json(m: &std::collections::HashMap<String, String>) -> JsonValue {
    let mut obj = serde_json::Map::new();
    for (k, v) in m {
        obj.insert(k.clone(), JsonValue::String(v.clone()));
    }
    JsonValue::Object(obj)
}

fn apply_bridge(json_str: &str, ctx: &mut VariableContext, out: &mut SandboxOutcome, phase: &str) {
    let Ok(v) = serde_json::from_str::<JsonValue>(json_str) else {
        out.messages
            .push(format!("{phase}: bridge parse failed"));
        return;
    };

    if let Some(logs) = v.get("logs").and_then(|l| l.as_array()) {
        for log in logs {
            if let Some(s) = log.as_str() {
                out.messages.push(format!("{phase}/console: {s}"));
            }
        }
    }

    if let Some(tests) = v.get("tests").and_then(|t| t.as_array()) {
        for t in tests {
            let name = t
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unnamed")
                .to_string();
            let passed = t.get("passed").and_then(|p| p.as_bool()).unwrap_or(false);
            let message = t
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            out.tests.push(TestResult {
                name,
                passed,
                message,
            });
        }
    }

    if let Some(scopes) = v.get("scopes").and_then(|s| s.as_object()) {
        if let Some(m) = scopes.get("local").and_then(|x| x.as_object()) {
            ctx.local = json_obj_to_map(m);
        }
        if let Some(m) = scopes.get("environment").and_then(|x| x.as_object()) {
            ctx.environment = json_obj_to_map(m);
        }
        if let Some(m) = scopes.get("collection").and_then(|x| x.as_object()) {
            ctx.collection = json_obj_to_map(m);
        }
        if let Some(m) = scopes.get("globals").and_then(|x| x.as_object()) {
            ctx.global = json_obj_to_map(m);
        }
    }

    if let Some(vault) = v.get("vault").and_then(|x| x.as_object()) {
        for (k, val) in vault {
            let s = match val {
                JsonValue::String(s) => s.clone(),
                other => other.to_string(),
            };
            ctx.vault.set(k, &s);
        }
    }

    if let Some(exec) = v.get("execution") {
        // Only treat an explicit string as setNextRequest. JSON `null` means "unset"
        // (do not stop the runner). Empty string means stop (Postman-compatible).
        if let Some(nr) = exec.get("nextRequest") {
            if nr.is_null() {
                // leave next_request as None
            } else if let Some(s) = nr.as_str() {
                out.execution.next_request = Some(s.to_string());
            }
        }
        out.execution.skip_request = exec
            .get("skipRequest")
            .and_then(|b| b.as_bool())
            .unwrap_or(false);
    }

    if let Some(html) = v.get("visualizerHtml").and_then(|h| h.as_str()) {
        if !html.is_empty() {
            out.visualizer_html = html.to_string();
        }
    }
}

fn json_obj_to_map(m: &serde_json::Map<String, JsonValue>) -> std::collections::HashMap<String, String> {
    m.iter()
        .map(|(k, v)| {
            let s = match v {
                JsonValue::String(s) => s.clone(),
                other => other.to_string(),
            };
            (k.clone(), s)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_env_and_logs() {
        let mut ctx = VariableContext::default();
        let out = run_js_sandbox(
            r#"
            pm.environment.set("baseUrl", "http://localhost");
            console.log("hi");
            "#,
            "pre-request",
            &mut ctx,
            None,
        );
        assert_eq!(
            ctx.environment.get("baseUrl").map(String::as_str),
            Some("http://localhost")
        );
        assert!(out.messages.iter().any(|m| m.contains("hi")));
        assert!(out.error.is_none());
    }

    #[test]
    fn pm_send_request_bad_url() {
        let mut ctx = VariableContext::default();
        let out = run_js_sandbox(
            r#"
            var saw = false;
            pm.sendRequest("http://127.0.0.1:1/nope", function(err, res) {
              saw = err !== null;
            });
            if (!saw) throw new Error("expected error callback");
            "#,
            "pre-request",
            &mut ctx,
            None,
        );
        assert!(out.error.is_none(), "{:?}", out.error);
    }

    #[test]
    fn pm_test_pass_fail() {
        let mut ctx = VariableContext::default();
        let resp = SandboxResponse {
            status: 200,
            status_text: "OK".into(),
            body: r#"{"ok":true}"#.into(),
            headers: vec![],
        };
        let out = run_js_sandbox(
            r#"
            pm.test("status 200", function () {
              pm.expect(pm.response.code).to.equal(200);
            });
            pm.test("fail demo", function () {
              pm.expect(1).to.equal(2);
            });
            "#,
            "tests",
            &mut ctx,
            Some(&resp),
        );
        assert_eq!(out.tests.len(), 2);
        assert!(out.tests[0].passed);
        assert!(!out.tests[1].passed);
    }
}
