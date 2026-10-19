//! Postman-style JS sandbox via `boa_engine`.
//!
//! Supported `blitz.*` (also aliased as `pm.*` and `blitz.*`):
//! - Scopes: `blitz.variables` / `environment` / `collectionVariables` / `globals`
//!   — `.get` / `.set` / `.unset` / `.has` / `.toObject` / `.clear`
//! - `blitz.variables.replaceIn(str)` — `{{var}}` substitution
//! - `blitz.vault` — `.get` / `.set` / `.unset`
//! - `blitz.cookies` — `.get` / `.set` / `.clear`
//! - `blitz.request` — `.url` (string + `.getPath()`), `.method`, `.headers.get/upsert/add/remove`,
//!   `.body.mode` / `.body.raw`
//! - `blitz.response` — `.code` / `.status` / `.responseTime` / `.text()` / `.json()` /
//!   `.headers.get` / `.to.have.status` / `.to.have.jsonBody`
//! - `blitz.test` / `blitz.expect` — equal/eql/include/be.ok/above/below/a(type)/
//!   have.status/have.property (+ optional value) / not.*
//! - `blitz.sendRequest` — sync nested HTTP + callback
//! - `blitz.execution.setNextRequest` / `skipRequest`
//! - `blitz.info` — eventName / requestName / iteration stubs
//! - `blitz.visualizer.set` — simple `{{key}}` / `{{data}}`
//! - `console.log/warn/error/info` → console panel
//! - `CryptoJS.SHA256` / `HmacSHA256` (+ Hex / Base64) — host-backed subset
//! - `require` — stub; `crypto-js` → CryptoJS; else empty object + warn
//!
//! Unsupported: full Chai, async/await tests, npm packages, uniscope/uvm isolation.

use boa_engine::{js_string, Context, JsArgs, JsResult, JsValue, NativeFunction, Source};
use hmac::{Hmac, Mac};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

use crate::core::{TestResult, VariableContext};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Default)]
pub struct SandboxOutcome {
    pub messages: Vec<String>,
    pub tests: Vec<TestResult>,
    pub error: Option<String>,
    pub execution: SandboxExecution,
    /// HTML from `blitz.visualizer.set(template, data)`.
    pub visualizer_html: String,
    /// Headers upserted via `blitz.request.headers.upsert` / `.add` (pre-request).
    pub request_header_upserts: Vec<(String, String)>,
    /// Cookie map after script when `blitz.cookies.set/clear` ran.
    pub cookies: Option<std::collections::HashMap<String, String>>,
}

/// Response snapshot injected into `blitz.response`.
#[derive(Debug, Clone, Default)]
pub struct SandboxResponse {
    pub status: u16,
    pub status_text: String,
    pub body: String,
    pub headers: Vec<(String, String)>,
    /// Milliseconds (Postman / blitz `response.responseTime`).
    pub response_time: u64,
}

/// Extra host data for scripts (vault, cookies, request, iteration, execution).
#[derive(Debug, Clone, Default)]
pub struct SandboxExtras {
    pub vault: std::collections::HashMap<String, String>,
    pub cookies: std::collections::HashMap<String, String>,
    pub request_url: String,
    pub request_method: String,
    /// Request headers visible to scripts (`[{key,value}, …]`).
    pub request_headers: Vec<(String, String)>,
    /// `raw` | `urlencoded` | `formdata` | `graphql` | `binary` | `none`
    pub request_body_mode: String,
    pub request_body_raw: String,
    pub iteration_data_json: String,
    pub request_name: String,
    pub event_name: String,
    pub iteration: u32,
}

/// Execution control written by `blitz.execution`.
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

    for (name, arity, f) in [
        (
            "__np_http",
            1usize,
            np_http_native as fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>,
        ),
        ("__np_sha256_hex", 1, np_sha256_hex_native),
        ("__np_hmac_sha256_b64", 2, np_hmac_sha256_b64_native),
    ] {
        if let Err(e) = context.register_global_callable(
            js_string!(name),
            arity,
            NativeFunction::from_fn_ptr(f),
        ) {
            out.error = Some(format!("{phase}: register {name}: {e}"));
            return out;
        }
    }

    let bridge = build_bridge_json(ctx, response, extras);
    let bridge_json = serde_json::to_string(&bridge).unwrap_or_else(|_| "{}".into());

    let prelude = format!(
        r#"
var __np = {bridge_json};
var console = {{
  log: function() {{ __np.logs.push(__np_fmtArgs(arguments)); }},
  warn: function() {{ __np.logs.push("[warn] " + __np_fmtArgs(arguments)); }},
  error: function() {{ __np.logs.push("[error] " + __np_fmtArgs(arguments)); }},
  info: function() {{ __np.logs.push("[info] " + __np_fmtArgs(arguments)); }}
}};
function __np_fmtArgs(args) {{
  var parts = [];
  for (var i = 0; i < args.length; i++) {{
    var a = args[i];
    parts.push(typeof a === "object" && a !== null ? JSON.stringify(a) : String(a));
  }}
  return parts.join(" ");
}}
function __scope(name) {{
  return {{
    get: function(k) {{
      var m = __np.scopes[name] || {{}};
      return m[k];
    }},
    set: function(k, v) {{
      if (!__np.scopes[name]) __np.scopes[name] = {{}};
      __np.scopes[name][k] = (v === undefined || v === null) ? "" : String(v);
    }},
    unset: function(k) {{
      if (__np.scopes[name]) delete __np.scopes[name][k];
    }},
    has: function(k) {{
      var m = __np.scopes[name] || {{}};
      return Object.prototype.hasOwnProperty.call(m, k);
    }},
    toObject: function() {{
      var m = __np.scopes[name] || {{}};
      var out = {{}};
      for (var k in m) {{
        if (Object.prototype.hasOwnProperty.call(m, k)) out[k] = m[k];
      }}
      return out;
    }},
    clear: function() {{
      __np.scopes[name] = {{}};
    }}
  }};
}}
function __np_lookupVar(key) {{
  key = String(key).trim();
  var order = ["local", "environment", "collection", "globals"];
  for (var i = 0; i < order.length; i++) {{
    var m = __np.scopes[order[i]] || {{}};
    if (Object.prototype.hasOwnProperty.call(m, key)) return m[key];
  }}
  try {{
    var data = __np.iterationData ? JSON.parse(__np.iterationData) : {{}};
    if (data && data[key] !== undefined && data[key] !== null) return String(data[key]);
  }} catch (e) {{}}
  if (__np.vault && Object.prototype.hasOwnProperty.call(__np.vault, key)) return __np.vault[key];
  return undefined;
}}
var __reqHdrs = [];
if (__np.requestHeaders) {{
  for (var hi = 0; hi < __np.requestHeaders.length; hi++) {{
    var rh = __np.requestHeaders[hi];
    __reqHdrs.push({{ key: String(rh[0]), value: String(rh[1]) }});
  }}
}}
function __reqHdrGet(name) {{
  name = String(name).toLowerCase();
  for (var i = 0; i < __reqHdrs.length; i++) {{
    if (String(__reqHdrs[i].key).toLowerCase() === name) return __reqHdrs[i].value;
  }}
  return undefined;
}}
function __reqHdrUpsert(kv) {{
  if (!kv) return;
  var key = String(kv.key || kv.name || "");
  var value = kv.value === undefined || kv.value === null ? "" : String(kv.value);
  if (!key) return;
  var found = false;
  for (var i = 0; i < __reqHdrs.length; i++) {{
    if (String(__reqHdrs[i].key).toLowerCase() === key.toLowerCase()) {{
      __reqHdrs[i].key = key;
      __reqHdrs[i].value = value;
      found = true;
      break;
    }}
  }}
  if (!found) __reqHdrs.push({{ key: key, value: value }});
  __np.requestHeaderUpserts.push([key, value]);
}}
var __hdrMap = {{}};
if (__np.response && __np.response.headers) {{
  for (var i = 0; i < __np.response.headers.length; i++) {{
    var h = __np.response.headers[i];
    __hdrMap[String(h[0]).toLowerCase()] = h[1];
  }}
}}
function __urlPath(urlStr) {{
  urlStr = String(urlStr || "");
  try {{
    var m = urlStr.match(/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\/[^\/]+(\/[^?#]*)/);
    if (m) return m[1] || "/";
    if (urlStr.charAt(0) === "/") {{
      var q = urlStr.indexOf("?");
      return q >= 0 ? urlStr.slice(0, q) : urlStr;
    }}
  }} catch (e) {{}}
  return "/";
}}
function __expect(actual) {{
  function fail(msg) {{ throw new Error(msg); }}
  var api = {{
    to: {{
      eql: function(expected) {{
        var a = JSON.stringify(actual);
        var e = JSON.stringify(expected);
        if (a !== e) fail("expected " + e + " but got " + a);
        return api;
      }},
      equal: function(expected) {{
        if (actual !== expected) fail("expected " + String(expected) + " but got " + String(actual));
        return api;
      }},
      include: function(part) {{
        if (Array.isArray(actual)) {{
          var ok = false;
          for (var i = 0; i < actual.length; i++) {{
            if (actual[i] === part || JSON.stringify(actual[i]) === JSON.stringify(part)) {{ ok = true; break; }}
          }}
          if (!ok) fail("expected array to include " + part);
        }} else {{
          var s = typeof actual === "string" ? actual : JSON.stringify(actual);
          if (s.indexOf(String(part)) < 0) fail("expected to include " + part);
        }}
        return api;
      }},
      be: {{
        get ok() {{
          if (!actual) fail("expected truthy");
          return true;
        }},
        above: function(n) {{
          if (!(Number(actual) > Number(n))) fail("expected " + actual + " above " + n);
          return api;
        }},
        below: function(n) {{
          if (!(Number(actual) < Number(n))) fail("expected " + actual + " below " + n);
          return api;
        }},
        a: function(type) {{
          type = String(type).toLowerCase();
          var t = Array.isArray(actual) ? "array" : (actual === null ? "null" : typeof actual);
          if (t !== type) fail("expected type " + type + " but got " + t);
          return api;
        }},
        an: function(type) {{ return api.to.be.a(type); }}
      }},
      have: {{
        status: function(code) {{
          var c = (__np.response && __np.response.status) || 0;
          if (c !== code) fail("expected status " + code + " got " + c);
          return api;
        }},
        property: function(name, val) {{
          if (actual == null || actual[name] === undefined) fail("missing property " + name);
          if (arguments.length > 1 && actual[name] !== val) {{
            fail("property " + name + " expected " + val + " got " + actual[name]);
          }}
          return api;
        }},
        jsonBody: function() {{
          var t = (__np.response && __np.response.body) || "";
          try {{ JSON.parse(t); }} catch (e) {{ fail("expected JSON body"); }}
          return api;
        }}
      }},
      not: {{
        eql: function(expected) {{
          if (JSON.stringify(actual) === JSON.stringify(expected)) fail("expected not eql");
          return api;
        }},
        equal: function(expected) {{
          if (actual === expected) fail("expected not equal");
          return api;
        }},
        include: function(part) {{
          var s = typeof actual === "string" ? actual : JSON.stringify(actual);
          if (s.indexOf(String(part)) >= 0) fail("expected not to include " + part);
          return api;
        }}
      }}
    }}
  }};
  return api;
}}
var CryptoJS = {{
  enc: {{
    Hex: {{ stringify: function(w) {{ return String(w); }} }},
    Base64: {{ stringify: function(w) {{ return String(w); }} }}
  }},
  SHA256: function(msg) {{
    var hex = __np_sha256_hex(String(msg == null ? "" : msg));
    return {{ toString: function(enc) {{ return hex; }} }};
  }},
  HmacSHA256: function(msg, key) {{
    var b64 = __np_hmac_sha256_b64(String(msg == null ? "" : msg), String(key == null ? "" : key));
    return {{ toString: function(enc) {{ return b64; }} }};
  }}
}};
function require(id) {{
  id = String(id || "");
  if (id === "crypto-js" || id === "crypto") return CryptoJS;
  console.warn('require("' + id + '") unsupported — stub empty object');
  return {{}};
}}
var pm = {{
  variables: (function() {{
    var s = __scope("local");
    s.replaceIn = function(str) {{
      var out = String(str);
      var i = 0;
      var result = "";
      while (i < out.length) {{
        var open = out.indexOf("{{{{", i);
        if (open < 0) {{ result += out.slice(i); break; }}
        var close = out.indexOf("}}}}", open + 2);
        if (close < 0) {{ result += out.slice(i); break; }}
        result += out.slice(i, open);
        var key = out.slice(open + 2, close).replace(/^\\s+|\\s+$/g, "");
        var v = __np_lookupVar(key);
        result += (v === undefined) ? out.slice(open, close + 2) : String(v);
        i = close + 2;
      }}
      return result;
    }};
    return s;
  }})(),
  environment: __scope("environment"),
  collectionVariables: __scope("collection"),
  globals: __scope("globals"),
  vault: {{
    get: function(k) {{ return (__np.vault && Object.prototype.hasOwnProperty.call(__np.vault, k)) ? __np.vault[k] : undefined; }},
    set: function(k, v) {{ if (!__np.vault) __np.vault = {{}}; __np.vault[k] = String(v); }},
    unset: function(k) {{ if (__np.vault) delete __np.vault[k]; }}
  }},
  cookies: {{
    get: function(k) {{ return (__np.cookies && Object.prototype.hasOwnProperty.call(__np.cookies, k)) ? __np.cookies[k] : undefined; }},
    set: function(k, v) {{
      if (!__np.cookies) __np.cookies = {{}};
      __np.cookies[String(k)] = String(v);
      __np.cookiesDirty = true;
    }},
    clear: function() {{
      __np.cookies = {{}};
      __np.cookiesDirty = true;
    }}
  }},
  info: {{
    eventName: __np.eventName || "",
    requestName: __np.requestName || "",
    iteration: __np.iteration || 0
  }},
  request: {{
    url: (function() {{
      var u = String(__np.requestUrl || "");
      // String object so `.getPath` works; coerces in many string contexts.
      var o = new String(u);
      o.getPath = function() {{ return __urlPath(u); }};
      return o;
    }})(),
    method: String(__np.requestMethod || ""),
    headers: {{
      get: __reqHdrGet,
      upsert: __reqHdrUpsert,
      add: __reqHdrUpsert,
      remove: function(name) {{
        name = String(name).toLowerCase();
        __reqHdrs = __reqHdrs.filter(function(h) {{ return String(h.key).toLowerCase() !== name; }});
      }}
    }},
    body: {{
      mode: String(__np.requestBodyMode || "none"),
      raw: String(__np.requestBodyRaw || "")
    }}
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
    responseTime: (__np.response && __np.response.responseTime) || 0,
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
        }},
        jsonBody: function() {{
          var t = (__np.response && __np.response.body) || "";
          try {{ JSON.parse(t); }} catch (e) {{ throw new Error("expected JSON body"); }}
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
        headers.push([String(h.key || h.name || ""), String(h.value)]);
      }}
    }}
    var raw = __np_http(JSON.stringify({{ url: url, method: method, body: body, headers: headers }}));
    var parsed = {{}};
    try {{ parsed = JSON.parse(raw); }} catch (e) {{ parsed = {{ error: String(e) }}; }}
    var err = parsed.error || null;
    var res = parsed.response || null;
    if (res) {{
      var bodyText = typeof res.text === "string" ? res.text : "";
      var cachedJson = res.json;
      res.text = function() {{ return bodyText; }};
      res.json = function() {{
        if (cachedJson !== undefined && cachedJson !== null) return cachedJson;
        try {{ return JSON.parse(bodyText); }} catch (e) {{ return null; }}
      }};
    }}
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
var blitz = pm;
var epm = pm;
pm.expect = __expect;
blitz.expect = __expect;
blitz.expect = __expect;
// Coerce request.url in string contexts (boa may not use Object string wrapper)
pm.request.toString = function() {{ return String(__np.requestUrl || ""); }};
blitz.request.toString = pm.request.toString;
blitz.request.toString = pm.request.toString;
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
            out.messages.push(format!("{phase}: JS error: {e}"));
        }
    }

    out.messages.push(format!(
        "{phase}: JS sandbox done (tests={})",
        out.tests.len()
    ));
    out
}

fn np_http_native(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let spec = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let result = host_send_request(&spec);
    Ok(JsValue::from(js_string!(result.as_str())))
}

fn np_sha256_hex_native(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let msg = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let digest = Sha256::digest(msg.as_bytes());
    Ok(JsValue::from(js_string!(hex::encode(digest).as_str())))
}

fn np_hmac_sha256_b64_native(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let msg = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let key = args
        .get_or_undefined(1)
        .to_string(context)?
        .to_std_string_escaped();
    let mut mac = HmacSha256::new_from_slice(key.as_bytes())
        .unwrap_or_else(|_| HmacSha256::new_from_slice(b"").expect("hmac empty key"));
    mac.update(msg.as_bytes());
    let result = mac.finalize().into_bytes();
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(result);
    Ok(JsValue::from(js_string!(b64.as_str())))
}

/// Blocking nested HTTP for `blitz.sendRequest` (JSON in / JSON out).
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

    let method =
        reqwest::Method::from_bytes(spec.method.as_bytes()).unwrap_or(reqwest::Method::GET);
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
            let status_text = resp.status().canonical_reason().unwrap_or("").to_string();
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
            let headers: Vec<_> = r.headers.iter().map(|(k, v)| json_arr(k, v)).collect();
            serde_json::json!({
                "status": r.status,
                "statusText": r.status_text,
                "body": r.body,
                "headers": headers,
                "responseTime": r.response_time,
            })
        }
        None => JsonValue::Null,
    };

    let request_headers: Vec<_> = extras
        .request_headers
        .iter()
        .map(|(k, v)| json_arr(k, v))
        .collect();

    serde_json::json!({
        "scopes": scopes,
        "logs": [],
        "tests": [],
        "response": resp,
        "vault": map_to_json(&extras.vault),
        "cookies": map_to_json(&extras.cookies),
        "cookiesDirty": false,
        "requestUrl": extras.request_url,
        "requestMethod": extras.request_method,
        "requestHeaders": request_headers,
        "requestHeaderUpserts": [],
        "requestBodyMode": extras.request_body_mode,
        "requestBodyRaw": extras.request_body_raw,
        "requestName": extras.request_name,
        "eventName": extras.event_name,
        "iteration": extras.iteration,
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
    JsonValue::Array(vec![
        JsonValue::String(k.into()),
        JsonValue::String(v.into()),
    ])
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
        out.messages.push(format!("{phase}: bridge parse failed"));
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

    if let Some(upserts) = v.get("requestHeaderUpserts").and_then(|a| a.as_array()) {
        for u in upserts {
            if let Some(arr) = u.as_array() {
                if arr.len() >= 2 {
                    let k = arr[0].as_str().unwrap_or("").to_string();
                    let val = match &arr[1] {
                        JsonValue::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    if !k.is_empty() {
                        out.request_header_upserts.push((k, val));
                    }
                }
            }
        }
    }

    if v.get("cookiesDirty")
        .and_then(|b| b.as_bool())
        .unwrap_or(false)
    {
        if let Some(cookies) = v.get("cookies").and_then(|c| c.as_object()) {
            out.cookies = Some(json_obj_to_map(cookies));
        } else {
            out.cookies = Some(std::collections::HashMap::new());
        }
    }
}

fn json_obj_to_map(
    m: &serde_json::Map<String, JsonValue>,
) -> std::collections::HashMap<String, String> {
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

/// Apply script header upserts onto a draft (case-insensitive key replace).
pub fn apply_request_header_upserts(
    draft: &mut crate::core::RequestDraft,
    upserts: &[(String, String)],
) {
    use crate::core::KeyValue;
    for (key, value) in upserts {
        if key.is_empty() {
            continue;
        }
        if let Some(slot) = draft
            .headers
            .iter_mut()
            .find(|h| h.key.eq_ignore_ascii_case(key))
        {
            slot.key = key.clone();
            slot.value = value.clone();
            slot.enabled = true;
        } else {
            draft
                .headers
                .push(KeyValue::new(key.clone(), value.clone()));
        }
    }
}

/// Map draft body mode to Postman-ish script string.
pub fn body_mode_label(mode: crate::core::BodyMode) -> &'static str {
    use crate::core::BodyMode;
    match mode {
        BodyMode::None => "none",
        BodyMode::Raw => "raw",
        BodyMode::Urlencoded => "urlencoded",
        BodyMode::FormData => "formdata",
        BodyMode::Graphql => "graphql",
        BodyMode::Binary => "file",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_env_and_logs() {
        let mut ctx = VariableContext::default();
        let out = run_js_sandbox(
            r#"
            blitz.environment.set("baseUrl", "http://localhost");
            console.log("hi");
            console.warn("w");
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
        assert!(out.messages.iter().any(|m| m.contains("[warn]")));
        assert!(out.error.is_none());
    }

    #[test]
    fn pm_send_request_bad_url() {
        let mut ctx = VariableContext::default();
        let out = run_js_sandbox(
            r#"
            var saw = false;
            blitz.sendRequest("http://127.0.0.1:1/nope", function(err, res) {
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
            response_time: 12,
        };
        let out = run_js_sandbox(
            r#"
            blitz.test("status 200", function () {
              blitz.expect(blitz.response.code).to.equal(200);
            });
            blitz.test("fail demo", function () {
              blitz.expect(1).to.equal(2);
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

    #[test]
    fn hakki_login_test_saves_token() {
        let mut ctx = VariableContext::default();
        let resp = SandboxResponse {
            status: 200,
            status_text: "OK".into(),
            body: r#"{"data":{"token":"tok-abc"}}"#.into(),
            headers: vec![],
            response_time: 5,
        };
        let out = run_js_sandbox(
            r#"
            blitz.test("Barez", function () {
                var jsonData = blitz.response.json();
                var token = jsonData["data"]["token"];
                blitz.collectionVariables.set("token_hakki", token);
                blitz.environment.set("token_hakki", token);
            });
            "#,
            "tests",
            &mut ctx,
            Some(&resp),
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.tests.len(), 1);
        assert!(out.tests[0].passed, "{}", out.tests[0].message);
        assert_eq!(
            ctx.collection.get("token_hakki").map(String::as_str),
            Some("tok-abc")
        );
        assert_eq!(
            ctx.environment.get("token_hakki").map(String::as_str),
            Some("tok-abc")
        );
    }

    #[test]
    fn expect_matchers_and_response_helpers() {
        let mut ctx = VariableContext::default();
        let resp = SandboxResponse {
            status: 201,
            status_text: "Created".into(),
            body: r#"{"data":{"api_secret":"s","key_prefix":"k","n":5},"arr":[1,2]}"#.into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            response_time: 42,
        };
        let out = run_js_sandbox(
            r#"
            blitz.test("vendor", function () {
              blitz.response.to.have.status(201);
              blitz.response.to.have.jsonBody();
              var res = blitz.response.json();
              blitz.expect(res.data).to.have.property("api_secret");
              blitz.expect(res.data).to.have.property("key_prefix", "k");
              blitz.expect(res.data.n).to.be.above(3);
              blitz.expect(res.data.n).to.be.below(10);
              blitz.expect(res.data.api_secret).to.be.a("string");
              blitz.expect(res.arr).to.include(2);
              blitz.expect(blitz.response.responseTime).to.be.above(0);
              blitz.expect(blitz.response.headers.get("content-type")).to.include("json");
              blitz.expect(1).to.eql(1);
              blitz.expect("x").to.not.equal("y");
            });
            "#,
            "tests",
            &mut ctx,
            Some(&resp),
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.tests.len(), 1);
        assert!(out.tests[0].passed, "{}", out.tests[0].message);
    }

    #[test]
    fn scope_has_to_object_replace_in() {
        let mut ctx = VariableContext::default();
        ctx.environment.insert("host".into(), "example.com".into());
        let out = run_js_sandbox(
            r#"
            blitz.variables.set("x", "1");
            if (!blitz.variables.has("x")) throw new Error("has");
            if (!blitz.environment.has("host")) throw new Error("env has");
            var o = blitz.environment.toObject();
            if (o.host !== "example.com") throw new Error("toObject");
            var s = blitz.variables.replaceIn("https://{{host}}/{{x}}");
            if (s !== "https://example.com/1") throw new Error("replaceIn " + s);
            blitz.globals.clear();
            "#,
            "pre-request",
            &mut ctx,
            None,
        );
        assert!(out.error.is_none(), "{:?}", out.error);
    }

    #[test]
    fn request_url_path_headers_upsert_crypto() {
        let mut ctx = VariableContext::default();
        ctx.environment.insert("api_secret".into(), "sekrit".into());
        ctx.environment.insert("key_prefix".into(), "pref_".into());
        let extras = SandboxExtras {
            request_url: "https://api.example.com/api/hakkiotp/v1/otp".into(),
            request_method: "POST".into(),
            request_headers: vec![("Accept".into(), "application/json".into())],
            request_body_mode: "raw".into(),
            request_body_raw: r#"{"phone":"{{phone}}"}"#.into(),
            event_name: "prerequest".into(),
            request_name: "Create OTP".into(),
            ..Default::default()
        };
        ctx.environment.insert("phone".into(), "100".into());
        let out = run_js_sandbox_ex(
            r#"
            const secret = blitz.environment.get("api_secret");
            const keyPrefix = blitz.environment.get("key_prefix");
            const method = blitz.request.method.toUpperCase();
            let path = blitz.request.url.getPath();
            const timestamp = "2026-01-01T00:00:00.000Z";
            let body = "";
            if (blitz.request.body && blitz.request.body.mode === "raw" && blitz.request.body.raw) {
                body = blitz.variables.replaceIn(blitz.request.body.raw);
            }
            const bodyHash = CryptoJS.SHA256(body).toString(CryptoJS.enc.Hex);
            const baseString = method + "\n" + path + "\n" + timestamp + "\n" + bodyHash;
            const signature = CryptoJS.HmacSHA256(baseString, secret).toString(CryptoJS.enc.Base64);
            blitz.request.headers.upsert({ key: "X-Api-Key", value: keyPrefix });
            blitz.request.headers.upsert({ key: "X-Api-Timestamp", value: timestamp });
            blitz.request.headers.upsert({ key: "X-Api-Signature", value: signature });
            if (blitz.info.eventName !== "prerequest") throw new Error("info");
            if (path !== "/api/hakkiotp/v1/otp") throw new Error("path " + path);
            if (body !== '{"phone":"100"}') throw new Error("body " + body);
            if (!signature || signature.length < 10) throw new Error("sig");
            "#,
            "pre-request",
            &mut ctx,
            None,
            &extras,
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert!(
            out.request_header_upserts
                .iter()
                .any(|(k, v)| k == "X-Api-Key" && v == "pref_"),
            "{:?}",
            out.request_header_upserts
        );
        assert!(
            out.request_header_upserts
                .iter()
                .any(|(k, _)| k == "X-Api-Signature"),
            "{:?}",
            out.request_header_upserts
        );
    }

    #[test]
    fn cookies_set_clear_and_pm_info() {
        let mut ctx = VariableContext::default();
        let extras = SandboxExtras {
            cookies: {
                let mut m = std::collections::HashMap::new();
                m.insert("a".into(), "1".into());
                m
            },
            ..Default::default()
        };
        let out = run_js_sandbox_ex(
            r#"
            if (blitz.cookies.get("a") !== "1") throw new Error("get");
            blitz.cookies.set("b", "2");
            blitz.cookies.clear();
            blitz.cookies.set("c", "3");
            "#,
            "pre-request",
            &mut ctx,
            None,
            &extras,
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        let c = out.cookies.expect("cookies");
        assert_eq!(c.get("c").map(String::as_str), Some("3"));
        assert!(!c.contains_key("a"));
        assert!(!c.contains_key("b"));
    }

    #[test]
    fn alias_parity_blitz_pm_epm() {
        let mut ctx = VariableContext::default();
        let out = run_js_sandbox(
            r#"
            blitz.environment.set("k1", "v1");
            pm.environment.set("k2", "v2");
            blitz.environment.set("k3", "v3");
            if (blitz !== pm || pm !== epm) throw new Error("aliases not identical");
            "#,
            "pre-request",
            &mut ctx,
            None,
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(ctx.environment.get("k1").map(String::as_str), Some("v1"));
        assert_eq!(ctx.environment.get("k2").map(String::as_str), Some("v2"));
        assert_eq!(ctx.environment.get("k3").map(String::as_str), Some("v3"));
    }

    #[test]
    fn require_crypto_js_and_stub() {
        let mut ctx = VariableContext::default();
        let out = run_js_sandbox(
            r#"
            var C = require("crypto-js");
            var h = C.SHA256("hi").toString(C.enc.Hex);
            if (h.length !== 64) throw new Error("sha");
            var empty = require("lodash");
            if (typeof empty !== "object") throw new Error("stub");
            "#,
            "pre-request",
            &mut ctx,
            None,
        );
        assert!(out.error.is_none(), "{:?}", out.error);
    }
}
