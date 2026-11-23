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
use md5::Md5;
use serde_json::Value as JsonValue;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};

use crate::core::{TestResult, VariableContext};

type HmacSha1 = Hmac<Sha1>;
type HmacSha256 = Hmac<Sha256>;
type HmacSha512 = Hmac<Sha512>;

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
    /// Headers removed via `blitz.request.headers.remove` / `.clear` (pre-request).
    pub request_header_removals: Vec<String>,
    /// Overridden request URL from `blitz.request.url.update(...)` or assignment.
    pub request_url_override: Option<String>,
    /// Overridden raw request body from `blitz.request.body.update(...)`.
    pub request_body_raw_override: Option<String>,
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
#[cfg(test)]
pub fn run_js_sandbox(
    script: &str,
    phase: &str,
    ctx: &mut VariableContext,
    response: Option<&SandboxResponse>,
) -> SandboxOutcome {
    run_js_sandbox_ex(script, phase, ctx, response, &SandboxExtras::default())
}

/// Normalize smart/curly quotes (common on macOS) to standard ASCII quotes.
pub fn normalize_smart_quotes(code: &str) -> String {
    code.replace(['“', '”', '„', '«', '»'], "\"")
        .replace(['‘', '’', '‚', '‹', '›'], "'")
}

// shortcut: leaked realm per thread. Dropping Boa Context underflows boa_gc, and Context is !Send so it cannot move to a pool. Implicit globals (`x = 1`) still leak across scripts on that thread.
thread_local! {
    static JS_REALM: *mut Context = {
        let mut context = Context::default();
        context.runtime_limits_mut().set_loop_iteration_limit(2_000_000);
        register_host_fns(&mut context).expect("sandbox host fns");
        Box::into_raw(Box::new(context))
    };
}

fn register_host_fns(context: &mut Context) -> Result<(), String> {
    for (name, arity, f) in [
        (
            "__np_http",
            1usize,
            np_http_native as fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>,
        ),
        ("__np_sha256_hex", 1, np_sha256_hex_native),
        ("__np_hmac_sha256_b64", 2, np_hmac_sha256_b64_native),
        ("__np_hash", 2, np_hash_native),
        ("__np_hmac", 3, np_hmac_native),
        ("__np_hex_to_b64", 1, np_hex_to_b64_native),
        ("__np_b64_to_hex", 1, np_b64_to_hex_native),
        ("__np_btoa", 1, np_btoa_native),
        ("__np_atob", 1, np_atob_native),
        ("__np_utf8_to_hex", 1, np_utf8_to_hex_native),
        ("__np_hex_to_utf8", 1, np_hex_to_utf8_native),
    ] {
        context
            .register_global_callable(js_string!(name), arity, NativeFunction::from_fn_ptr(f))
            .map_err(|e| format!("register {name}: {e}"))?;
    }
    Ok(())
}

pub fn run_js_sandbox_ex(
    script: &str,
    phase: &str,
    ctx: &mut VariableContext,
    response: Option<&SandboxResponse>,
    extras: &SandboxExtras,
) -> SandboxOutcome {
    let mut out = SandboxOutcome::default();
    let sanitized_script = normalize_smart_quotes(script);
    if sanitized_script.trim().is_empty() {
        out.messages.push(format!("{phase}: skipped (empty)"));
        return out;
    }

    let bridge = build_bridge_json(ctx, response, extras);
    let bridge_json = serde_json::to_string(&bridge).unwrap_or_else(|_| "{}".into());

    let prelude = format!(
        r##"
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
function __np_dynamicVar(key) {{
  key = String(key || "").toLowerCase();
  switch (key) {{
    case "$guid":
    case "$randomuuid":
      return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, function(c) {{
        var r = Math.random() * 16 | 0;
        return (c === 'x' ? r : (r & 0x3 | 0x8)).toString(16);
      }});
    case "$timestamp":
      return String(Math.floor(Date.now() / 1000));
    case "$timestampms":
      return String(Date.now());
    case "$isotimestamp":
      return new Date().toISOString();
    case "$date":
      return new Date().toISOString().slice(0, 10);
    case "$randomint":
      return String(Math.floor(Math.random() * 1000) + 1);
    case "$randomemail":
      return "user_" + Math.random().toString(36).substring(2, 10) + "@example.com";
    case "$randomfirstname":
      var fnames = ["Alice", "Bob", "Carol", "David", "Emma", "Frank", "Grace", "Henry"];
      return fnames[Math.floor(Math.random() * fnames.length)];
    case "$randomlastname":
      var lnames = ["Smith", "Johnson", "Williams", "Brown", "Jones", "Garcia", "Miller"];
      return lnames[Math.floor(Math.random() * lnames.length)];
    case "$randomfullname":
      return __np_dynamicVar("$randomfirstname") + " " + __np_dynamicVar("$randomlastname");
    case "$randomcity":
      var cities = ["New York", "London", "Tokyo", "Paris", "Berlin", "Sydney", "San Francisco"];
      return cities[Math.floor(Math.random() * cities.length)];
    case "$randomcountry":
      var countries = ["United States", "United Kingdom", "Germany", "Japan", "France", "Canada"];
      return countries[Math.floor(Math.random() * countries.length)];
    case "$randomip":
    case "$randomipv4":
      return Math.floor(Math.random() * 255 + 1) + "." + Math.floor(Math.random() * 255) + "." + Math.floor(Math.random() * 255) + "." + Math.floor(Math.random() * 255 + 1);
    case "$randomboolean":
      return Math.random() < 0.5 ? "true" : "false";
    case "$randomprice":
      return (Math.random() * 1000 + 1).toFixed(2);
    case "$randomalphanumeric":
      return Math.random().toString(36).substring(2, 12);
    case "$randomword":
      var words = ["lorem", "payload", "service", "cluster", "gateway", "token", "request"];
      return words[Math.floor(Math.random() * words.length)];
    case "$randomphonenumber":
      return "+1-555-" + Math.floor(1000 + Math.random() * 9000);
    case "$randomdomain":
      return "svc-" + Math.random().toString(36).substring(2, 8) + ".example.com";
    case "$randomuseragent":
      return "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
    case "$randomslug":
      return __np_dynamicVar("$randomword") + "-" + __np_dynamicVar("$randomword") + "-" + Math.floor(100 + Math.random() * 900);
    case "$randomcolor":
    case "$randomhexcolor":
      return '#' + ("000000" + Math.floor(Math.random() * 16777215).toString(16)).slice(-6);
    default:
      return undefined;
  }}
}}
function __np_lookupVar(key) {{
  key = String(key).trim();
  if (key.charAt(0) === '$') {{
    var d = __np_dynamicVar(key);
    if (d !== undefined) return d;
  }}
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
function __reqHdrHas(name) {{
  return __reqHdrGet(name) !== undefined;
}}
function __reqHdrUpsert(kv) {{
  if (!kv) return;
  var key = String(kv.key || kv.name || "");
  var value = kv.value === undefined || kv.value === null ? "" : String(kv.value);
  if (!key) return;
  var lower = key.toLowerCase();
  var found = false;
  for (var i = 0; i < __reqHdrs.length; i++) {{
    if (String(__reqHdrs[i].key).toLowerCase() === lower) {{
      __reqHdrs[i].key = key;
      __reqHdrs[i].value = value;
      found = true;
      break;
    }}
  }}
  if (!found) __reqHdrs.push({{ key: key, value: value }});
  __np.requestHeaderRemovals = (__np.requestHeaderRemovals || []).filter(function(r) {{
    return String(r).toLowerCase() !== lower;
  }});
  var replaced = false;
  for (var j = 0; j < __np.requestHeaderUpserts.length; j++) {{
    if (String(__np.requestHeaderUpserts[j][0]).toLowerCase() === lower) {{
      __np.requestHeaderUpserts[j] = [key, value];
      replaced = true;
      break;
    }}
  }}
  if (!replaced) __np.requestHeaderUpserts.push([key, value]);
}}
function __reqHdrRemove(name) {{
  var lower = String(name || "").toLowerCase();
  if (!lower) return;
  __reqHdrs = __reqHdrs.filter(function(h) {{ return String(h.key).toLowerCase() !== lower; }});
  __np.requestHeaderUpserts = __np.requestHeaderUpserts.filter(function(u) {{
    return String(u[0]).toLowerCase() !== lower;
  }});
  if (!__np.requestHeaderRemovals) __np.requestHeaderRemovals = [];
  if (__np.requestHeaderRemovals.indexOf(lower) < 0) {{
    __np.requestHeaderRemovals.push(lower);
  }}
}}
function __reqHdrClear() {{
  var copy = __reqHdrs.slice();
  for (var i = 0; i < copy.length; i++) __reqHdrRemove(copy[i].key);
}}
var __hdrMap = {{}};
var __respHdrs = [];
if (__np.response && __np.response.headers) {{
  for (var i = 0; i < __np.response.headers.length; i++) {{
    var h = __np.response.headers[i];
    __hdrMap[String(h[0]).toLowerCase()] = h[1];
    __respHdrs.push({{ key: String(h[0]), value: String(h[1]) }});
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
function __urlHost(urlStr) {{
  urlStr = String(urlStr || "");
  var m = urlStr.match(/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\/([^\/?#]+)/);
  return m ? m[1] : "";
}}
function __urlQuery(urlStr) {{
  urlStr = String(urlStr || "");
  var q = urlStr.indexOf("?");
  if (q < 0) return "";
  var h = urlStr.indexOf("#", q);
  return h >= 0 ? urlStr.slice(q + 1, h) : urlStr.slice(q + 1);
}}
function __urlPathWithQuery(urlStr) {{
  var p = __urlPath(urlStr);
  var q = __urlQuery(urlStr);
  return q ? (p + "?" + q) : p;
}}
function __np_deepEqual(a, b) {{
  if (a === b) return true;
  if (a == null || b == null) return a === b;
  if (typeof a !== typeof b) return false;
  if (typeof a !== "object") return Number.isNaN(a) && Number.isNaN(b);
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a)) {{
    if (a.length !== b.length) return false;
    for (var i = 0; i < a.length; i++) {{
      if (!__np_deepEqual(a[i], b[i])) return false;
    }}
    return true;
  }}
  var ka = Object.keys(a);
  var kb = Object.keys(b);
  if (ka.length !== kb.length) return false;
  for (var j = 0; j < ka.length; j++) {{
    var k = ka[j];
    if (!Object.prototype.hasOwnProperty.call(b, k)) return false;
    if (!__np_deepEqual(a[k], b[k])) return false;
  }}
  return true;
}}
function __np_typeOf(v) {{
  if (v === null) return "null";
  if (Array.isArray(v)) return "array";
  return typeof v;
}}
function __np_getNested(obj, path) {{
  var parts = String(path).replace(/\[(\d+)\]/g, ".$1").split(".");
  var cur = obj;
  for (var i = 0; i < parts.length; i++) {{
    if (cur == null) return {{ found: false, value: undefined }};
    var p = parts[i];
    if (!(p in Object(cur))) return {{ found: false, value: undefined }};
    cur = cur[p];
  }}
  return {{ found: true, value: cur }};
}}
function __np_makeExpect(actual, flags) {{
  flags = flags || {{ neg: false, deep: false, nested: false, allKeys: true, anyKeys: false }};
  function fail(msg) {{ throw new Error(msg); }}
  function check(cond, posMsg, negMsg) {{
    if (flags.neg) {{
      if (cond) fail(negMsg || ("not " + posMsg));
    }} else {{
      if (!cond) fail(posMsg);
    }}
  }}
  function withFlag(patch) {{
    var nf = {{
      neg: patch.neg !== undefined ? patch.neg : flags.neg,
      deep: patch.deep !== undefined ? patch.deep : flags.deep,
      nested: patch.nested !== undefined ? patch.nested : flags.nested,
      allKeys: patch.allKeys !== undefined ? patch.allKeys : flags.allKeys,
      anyKeys: patch.anyKeys !== undefined ? patch.anyKeys : flags.anyKeys
    }};
    return __np_makeExpect(actual, nf);
  }}
  var api = {{
    get to() {{ return api; }},
    get be() {{ return api; }},
    get have() {{ return api; }},
    get and() {{ return __np_makeExpect(actual, {{ neg: false, deep: false, nested: false, allKeys: true, anyKeys: false }}); }},
    get is() {{ return api; }},
    get that() {{ return api; }},
    get which() {{ return api; }},
    get with() {{ return api; }},
    get at() {{ return api; }},
    get of() {{ return api; }},
    get same() {{ return api; }},
    get but() {{ return api; }},
    get does() {{ return api; }},
    get still() {{ return api; }},
    get not() {{ return withFlag({{ neg: !flags.neg }}); }},
    get deep() {{ return withFlag({{ deep: true }}); }},
    get nested() {{ return withFlag({{ nested: true }}); }},
    get all() {{ return withFlag({{ allKeys: true, anyKeys: false }}); }},
    get any() {{ return withFlag({{ allKeys: false, anyKeys: true }}); }},
    eql: function(expected) {{
      var eq = __np_deepEqual(actual, expected);
      check(eq, "expected " + JSON.stringify(expected) + " but got " + JSON.stringify(actual), "expected not eql " + JSON.stringify(expected));
      return api;
    }},
    eqls: function(expected) {{ return api.eql(expected); }},
    equal: function(expected) {{
      var eq = flags.deep ? __np_deepEqual(actual, expected) : (actual === expected);
      check(eq, "expected " + String(expected) + " but got " + String(actual), "expected not equal " + String(expected));
      return api;
    }},
    equals: function(expected) {{ return api.equal(expected); }},
    eq: function(expected) {{ return api.equal(expected); }},
    include: function(part) {{
      var ok = false;
      if (Array.isArray(actual)) {{
        for (var i = 0; i < actual.length; i++) {{
          if (flags.deep ? __np_deepEqual(actual[i], part) : (actual[i] === part || JSON.stringify(actual[i]) === JSON.stringify(part))) {{ ok = true; break; }}
        }}
      }} else if (typeof actual === "string") {{
        ok = actual.indexOf(String(part)) >= 0;
      }} else if (actual && typeof actual === "object" && part && typeof part === "object") {{
        ok = true;
        var pk = Object.keys(part);
        for (var j = 0; j < pk.length; j++) {{
          if (!__np_deepEqual(actual[pk[j]], part[pk[j]])) {{ ok = false; break; }}
        }}
      }} else {{
        ok = JSON.stringify(actual).indexOf(String(part)) >= 0;
      }}
      check(ok, "expected to include " + part, "expected not to include " + part);
      return api;
    }},
    includes: function(p) {{ return api.include(p); }},
    contain: function(p) {{ return api.include(p); }},
    contains: function(p) {{ return api.include(p); }},
    match: function(re) {{
      var r = re instanceof RegExp ? re : new RegExp(String(re));
      var ok = r.test(String(actual));
      check(ok, "expected " + actual + " to match " + r, "expected " + actual + " not to match " + r);
      return api;
    }},
    matches: function(re) {{ return api.match(re); }},
    get ok() {{
      check(Boolean(actual), "expected truthy", "expected falsy");
      return api;
    }},
    get true() {{
      check(actual === true, "expected true but got " + actual, "expected not true");
      return api;
    }},
    get false() {{
      check(actual === false, "expected false but got " + actual, "expected not false");
      return api;
    }},
    get null() {{
      check(actual === null, "expected null but got " + actual, "expected not null");
      return api;
    }},
    get undefined() {{
      check(actual === undefined, "expected undefined but got " + actual, "expected not undefined");
      return api;
    }},
    get nan() {{
      check(typeof actual === "number" && Number.isNaN(actual), "expected NaN", "expected not NaN");
      return api;
    }},
    get empty() {{
      var isEmpty = false;
      if (actual == null) isEmpty = true;
      else if (typeof actual === "string" || Array.isArray(actual)) isEmpty = actual.length === 0;
      else if (typeof actual === "object") isEmpty = Object.keys(actual).length === 0;
      check(isEmpty, "expected empty", "expected not empty");
      return api;
    }},
    above: function(n) {{
      check(Number(actual) > Number(n), "expected " + actual + " above " + n, "expected " + actual + " not above " + n);
      return api;
    }},
    greaterThan: function(n) {{ return api.above(n); }},
    gt: function(n) {{ return api.above(n); }},
    below: function(n) {{
      check(Number(actual) < Number(n), "expected " + actual + " below " + n, "expected " + actual + " not below " + n);
      return api;
    }},
    lessThan: function(n) {{ return api.below(n); }},
    lt: function(n) {{ return api.below(n); }},
    least: function(n) {{
      check(Number(actual) >= Number(n), "expected " + actual + " >= " + n, "expected " + actual + " < " + n);
      return api;
    }},
    gte: function(n) {{ return api.least(n); }},
    most: function(n) {{
      check(Number(actual) <= Number(n), "expected " + actual + " <= " + n, "expected " + actual + " > " + n);
      return api;
    }},
    lte: function(n) {{ return api.most(n); }},
    within: function(min, max) {{
      var v = Number(actual);
      check(v >= Number(min) && v <= Number(max), "expected " + actual + " within [" + min + "," + max + "]", "expected " + actual + " not within [" + min + "," + max + "]");
      return api;
    }},
    oneOf: function(list) {{
      var ok = Array.isArray(list) && list.indexOf(actual) >= 0;
      check(ok, "expected " + actual + " to be one of " + JSON.stringify(list), "expected " + actual + " not to be one of " + JSON.stringify(list));
      return api;
    }},
    a: function(type) {{
      type = String(type).toLowerCase();
      var t = __np_typeOf(actual);
      check(t === type, "expected type " + type + " but got " + t, "expected type not " + type);
      return api;
    }},
    an: function(type) {{ return api.a(type); }},
    status: function(code) {{
      var c = (__np.response && __np.response.status) || 0;
      check(c === code, "expected status " + code + " got " + c, "expected status not " + code);
      return api;
    }},
    property: function(name, val) {{
      var hasVal = arguments.length > 1;
      var found = false;
      var propVal = undefined;
      if (actual != null) {{
        if (flags.nested) {{
          var r = __np_getNested(actual, name);
          found = r.found;
          propVal = r.value;
        }} else if (actual[name] !== undefined || Object.prototype.hasOwnProperty.call(Object(actual), name)) {{
          found = true;
          propVal = actual[name];
        }}
      }}
      if (flags.neg) {{
        if (hasVal) {{
          if (found && __np_deepEqual(propVal, val)) fail("expected property " + name + " not to equal " + val);
        }} else {{
          if (found) fail("expected not to have property " + name);
        }}
      }} else {{
        if (!found) fail("missing property " + name);
        if (hasVal && !(flags.deep ? __np_deepEqual(propVal, val) : (propVal === val))) {{
          fail("property " + name + " expected " + val + " got " + propVal);
        }}
      }}
      return api;
    }},
    ownProperty: function(name) {{
      var found = actual != null && Object.prototype.hasOwnProperty.call(Object(actual), name);
      check(found, "missing own property " + name, "expected not to have own property " + name);
      return api;
    }},
    lengthOf: function(n) {{
      var len = actual != null && actual.length !== undefined ? actual.length : -1;
      check(len === Number(n), "expected length " + n + " but got " + len, "expected length not " + n);
      return api;
    }},
    length: function(n) {{ return api.lengthOf(n); }},
    keys: function() {{
      var expected = arguments.length === 1 && Array.isArray(arguments[0]) ? arguments[0] : Array.prototype.slice.call(arguments);
      var objKeys = actual && typeof actual === "object" ? Object.keys(actual) : [];
      var ok = false;
      if (flags.anyKeys) {{
        for (var i = 0; i < expected.length; i++) {{
          if (objKeys.indexOf(String(expected[i])) >= 0) {{ ok = true; break; }}
        }}
      }} else {{
        ok = expected.length === objKeys.length;
        if (ok) {{
          for (var j = 0; j < expected.length; j++) {{
            if (objKeys.indexOf(String(expected[j])) < 0) {{ ok = false; break; }}
          }}
        }}
      }}
      check(ok, "expected keys " + JSON.stringify(expected) + " got " + JSON.stringify(objKeys), "expected not to have keys " + JSON.stringify(expected));
      return api;
    }},
    key: function() {{ return api.keys.apply(api, arguments); }},
    jsonBody: function() {{
      var t = (__np.response && __np.response.body) || "";
      var ok = true;
      try {{ JSON.parse(t); }} catch (e) {{ ok = false; }}
      check(ok, "expected JSON body", "expected non-JSON body");
      return api;
    }},
    throw: function(expected) {{
      var threw = false;
      var errMsg = "";
      try {{
        if (typeof actual === "function") actual();
      }} catch (e) {{
        threw = true;
        errMsg = String(e && e.message ? e.message : e);
      }}
      if (flags.neg) {{
        if (threw) fail("expected not to throw but threw: " + errMsg);
      }} else {{
        if (!threw) fail("expected function to throw");
        if (expected !== undefined) {{
          if (expected instanceof RegExp && !expected.test(errMsg)) fail("expected error matching " + expected + " got " + errMsg);
          else if (typeof expected === "string" && errMsg.indexOf(expected) < 0) fail("expected error containing " + expected + " got " + errMsg);
        }}
      }}
      return api;
    }},
    throws: function(e) {{ return api.throw(e); }}
  }};
  return api;
}}
function __expect(actual) {{
  return __np_makeExpect(actual);
}}
function __np_wordStr(w) {{
  if (w == null) return "";
  if (typeof w === "object") {{
    if (w.__utf8 !== undefined) return String(w.__utf8);
    if (w.__hex !== undefined) return __np_hex_to_utf8(String(w.__hex));
  }}
  return String(w);
}}
function __np_makeHashObj(hexStr) {{
  var hex = String(hexStr || "");
  return {{
    __hex: hex,
    toString: function(enc) {{
      if (enc === CryptoJS.enc.Base64) return __np_hex_to_b64(hex);
      if (enc === CryptoJS.enc.Utf8) return __np_hex_to_utf8(hex);
      return hex;
    }}
  }};
}}
var CryptoJS = {{
  enc: {{
    Hex: {{
      parse: function(hexStr) {{ return __np_makeHashObj(String(hexStr || "")); }},
      stringify: function(w) {{
        if (w && w.__hex !== undefined) return String(w.__hex);
        if (w && w.__utf8 !== undefined) return __np_utf8_to_hex(String(w.__utf8));
        return String(w == null ? "" : w);
      }}
    }},
    Base64: {{
      parse: function(b64Str) {{ return __np_makeHashObj(__np_b64_to_hex(String(b64Str || ""))); }},
      stringify: function(w) {{
        if (w && w.__hex !== undefined) return __np_hex_to_b64(String(w.__hex));
        if (w && w.__utf8 !== undefined) return __np_btoa(String(w.__utf8));
        return __np_btoa(String(w == null ? "" : w));
      }}
    }},
    Utf8: {{
      parse: function(str) {{
        var s = String(str == null ? "" : str);
        return {{
          __utf8: s,
          __hex: __np_utf8_to_hex(s),
          toString: function(enc) {{
            if (enc === CryptoJS.enc.Base64) return __np_btoa(s);
            if (enc === CryptoJS.enc.Hex) return __np_utf8_to_hex(s);
            return s;
          }}
        }};
      }},
      stringify: function(w) {{ return __np_wordStr(w); }}
    }}
  }},
  MD5: function(msg) {{ return __np_makeHashObj(__np_hash("md5", __np_wordStr(msg))); }},
  SHA1: function(msg) {{ return __np_makeHashObj(__np_hash("sha1", __np_wordStr(msg))); }},
  SHA256: function(msg) {{ return __np_makeHashObj(__np_hash("sha256", __np_wordStr(msg))); }},
  SHA512: function(msg) {{ return __np_makeHashObj(__np_hash("sha512", __np_wordStr(msg))); }},
  HmacSHA1: function(msg, key) {{ return __np_makeHashObj(__np_hmac("sha1", __np_wordStr(msg), __np_wordStr(key))); }},
  HmacSHA256: function(msg, key) {{ return __np_makeHashObj(__np_hmac("sha256", __np_wordStr(msg), __np_wordStr(key))); }},
  HmacSHA512: function(msg, key) {{ return __np_makeHashObj(__np_hmac("sha512", __np_wordStr(msg), __np_wordStr(key))); }}
}};
if (typeof btoa === "undefined") {{ var btoa = function(s) {{ return __np_btoa(String(s == null ? "" : s)); }}; }}
if (typeof atob === "undefined") {{ var atob = function(s) {{ return __np_atob(String(s == null ? "" : s)); }}; }}
if (typeof URLSearchParams === "undefined") {{
  var URLSearchParams = function(init) {{
    this._pairs = [];
    if (typeof init === "string") {{
      var s = init.charAt(0) === "?" ? init.slice(1) : init;
      if (s) {{
        var parts = s.split("&");
        for (var i = 0; i < parts.length; i++) {{
          var eq = parts[i].indexOf("=");
          var k = eq >= 0 ? decodeURIComponent(parts[i].slice(0, eq).replace(/\+/g, " ")) : decodeURIComponent(parts[i]);
          var v = eq >= 0 ? decodeURIComponent(parts[i].slice(eq + 1).replace(/\+/g, " ")) : "";
          this._pairs.push([k, v]);
        }}
      }}
    }} else if (init && typeof init === "object") {{
      var ks = Object.keys(init);
      for (var j = 0; j < ks.length; j++) this._pairs.push([ks[j], String(init[ks[j]])]);
    }}
  }};
  URLSearchParams.prototype.append = function(k, v) {{ this._pairs.push([String(k), String(v)]); }};
  URLSearchParams.prototype.set = function(k, v) {{
    k = String(k); v = String(v);
    var out = []; var set = false;
    for (var i = 0; i < this._pairs.length; i++) {{
      if (this._pairs[i][0] === k) {{ if (!set) {{ out.push([k, v]); set = true; }} }}
      else out.push(this._pairs[i]);
    }}
    if (!set) out.push([k, v]);
    this._pairs = out;
  }};
  URLSearchParams.prototype.get = function(k) {{
    k = String(k);
    for (var i = 0; i < this._pairs.length; i++) if (this._pairs[i][0] === k) return this._pairs[i][1];
    return null;
  }};
  URLSearchParams.prototype.has = function(k) {{ return this.get(k) !== null; }};
  URLSearchParams.prototype.toString = function() {{
    var out = [];
    for (var i = 0; i < this._pairs.length; i++) {{
      out.push(encodeURIComponent(this._pairs[i][0]) + "=" + encodeURIComponent(this._pairs[i][1]));
    }}
    return out.join("&");
  }};
}}
var __np_lodash = {{
  isEmpty: function(v) {{
    if (v == null) return true;
    if (typeof v === "string" || Array.isArray(v)) return v.length === 0;
    if (typeof v === "object") return Object.keys(v).length === 0;
    return false;
  }},
  get: function(obj, path, defVal) {{
    var r = __np_getNested(obj, path);
    return r.found && r.value !== undefined ? r.value : defVal;
  }},
  has: function(obj, path) {{ return __np_getNested(obj, path).found; }},
  uniq: function(arr) {{
    if (!Array.isArray(arr)) return [];
    var out = [];
    for (var i = 0; i < arr.length; i++) if (out.indexOf(arr[i]) < 0) out.push(arr[i]);
    return out;
  }},
  flatten: function(arr) {{
    if (!Array.isArray(arr)) return [];
    var out = [];
    for (var i = 0; i < arr.length; i++) {{
      if (Array.isArray(arr[i])) out = out.concat(arr[i]);
      else out.push(arr[i]);
    }}
    return out;
  }},
  chunk: function(arr, size) {{
    if (!Array.isArray(arr) || !size || size < 1) return [];
    var out = [];
    for (var i = 0; i < arr.length; i += size) out.push(arr.slice(i, i + size));
    return out;
  }},
  pick: function(obj, keys) {{
    var out = {{}};
    if (!obj || typeof obj !== "object") return out;
    var ks = Array.isArray(keys) ? keys : Array.prototype.slice.call(arguments, 1);
    for (var i = 0; i < ks.length; i++) if (ks[i] in obj) out[ks[i]] = obj[ks[i]];
    return out;
  }},
  omit: function(obj, keys) {{
    var out = {{}};
    if (!obj || typeof obj !== "object") return out;
    var skip = Array.isArray(keys) ? keys : Array.prototype.slice.call(arguments, 1);
    var ks = Object.keys(obj);
    for (var i = 0; i < ks.length; i++) if (skip.indexOf(ks[i]) < 0) out[ks[i]] = obj[ks[i]];
    return out;
  }},
  cloneDeep: function(v) {{ return v === undefined ? undefined : JSON.parse(JSON.stringify(v)); }},
  isEqual: function(a, b) {{ return __np_deepEqual(a, b); }},
  keys: function(o) {{ return o && typeof o === "object" ? Object.keys(o) : []; }},
  values: function(o) {{ return o && typeof o === "object" ? Object.keys(o).map(function(k) {{ return o[k]; }}) : []; }},
  includes: function(col, val) {{
    if (typeof col === "string") return col.indexOf(String(val)) >= 0;
    if (Array.isArray(col)) return col.indexOf(val) >= 0;
    return false;
  }},
  map: function(arr, fn) {{ return Array.isArray(arr) ? arr.map(typeof fn === "function" ? fn : function(x) {{ return x && x[fn]; }}) : []; }},
  filter: function(arr, fn) {{ return Array.isArray(arr) ? arr.filter(typeof fn === "function" ? fn : function(x) {{ return Boolean(x && x[fn]); }}) : []; }},
  find: function(arr, fn) {{
    if (!Array.isArray(arr)) return undefined;
    var pred = typeof fn === "function" ? fn : function(x) {{ return Boolean(x && x[fn]); }};
    for (var i = 0; i < arr.length; i++) if (pred(arr[i], i, arr)) return arr[i];
    return undefined;
  }},
  some: function(arr, fn) {{ return Array.isArray(arr) ? arr.some(fn) : false; }},
  every: function(arr, fn) {{ return Array.isArray(arr) ? arr.every(fn) : false; }},
  random: function(min, max) {{
    if (max === undefined) {{ max = min || 1; min = 0; }}
    return Math.floor(Math.random() * (max - min + 1)) + min;
  }},
  range: function(start, end, step) {{
    if (end === undefined) {{ end = start; start = 0; }}
    step = step || 1;
    var out = [];
    for (var i = start; step > 0 ? i < end : i > end; i += step) out.push(i);
    return out;
  }}
}};
function require(id) {{
  id = String(id || "");
  if (id === "crypto-js" || id === "crypto") return CryptoJS;
  if (id === "lodash" || id === "_") return __np_lodash;
  console.warn('require("' + id + '") unsupported — stub empty object');
  return {{}};
}}
var pm = {{
  variables: (function() {{
    var s = __scope("local");
    s.get = function(k) {{
      var v = __np_lookupVar(k);
      return v !== undefined ? v : undefined;
    }};
    s.has = function(k) {{
      return __np_lookupVar(k) !== undefined;
    }};
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
      var o = new String(u);
      o.getPath = function() {{ return __urlPath(__np.requestUrlOverride !== null ? __np.requestUrlOverride : u); }};
      o.getHost = function() {{ return __urlHost(__np.requestUrlOverride !== null ? __np.requestUrlOverride : u); }};
      o.getQueryString = function() {{ return __urlQuery(__np.requestUrlOverride !== null ? __np.requestUrlOverride : u); }};
      o.getPathWithQuery = function() {{ return __urlPathWithQuery(__np.requestUrlOverride !== null ? __np.requestUrlOverride : u); }};
      o.update = function(nextUrl) {{
        var s = String(nextUrl || "");
        __np.requestUrlOverride = s;
        return s;
      }};
      o.addQueryParams = function(params) {{
        var cur = __np.requestUrlOverride !== null ? __np.requestUrlOverride : u;
        var sep = cur.indexOf("?") >= 0 ? "&" : "?";
        var pairs = [];
        if (typeof params === "string") {{
          pairs.push(params.replace(/^\?/, ""));
        }} else if (Array.isArray(params)) {{
          for (var i = 0; i < params.length; i++) {{
            var p = params[i];
            if (p && p.key) pairs.push(encodeURIComponent(p.key) + "=" + encodeURIComponent(p.value == null ? "" : p.value));
          }}
        }} else if (params && typeof params === "object") {{
          var ks = Object.keys(params);
          for (var j = 0; j < ks.length; j++) {{
            pairs.push(encodeURIComponent(ks[j]) + "=" + encodeURIComponent(params[ks[j]] == null ? "" : params[ks[j]]));
          }}
        }}
        var next = pairs.length ? (cur + sep + pairs.join("&")) : cur;
        __np.requestUrlOverride = next;
        return next;
      }};
      return o;
    }})(),
    method: String(__np.requestMethod || ""),
    headers: {{
      get: __reqHdrGet,
      has: __reqHdrHas,
      upsert: __reqHdrUpsert,
      add: __reqHdrUpsert,
      remove: __reqHdrRemove,
      clear: __reqHdrClear,
      toObject: function() {{
        var out = {{}};
        for (var i = 0; i < __reqHdrs.length; i++) out[__reqHdrs[i].key] = __reqHdrs[i].value;
        return out;
      }},
      each: function(fn) {{
        if (typeof fn === "function") for (var i = 0; i < __reqHdrs.length; i++) fn(__reqHdrs[i]);
      }},
      all: function() {{ return __reqHdrs.slice(); }}
    }},
    body: {{
      mode: String(__np.requestBodyMode || "none"),
      raw: String(__np.requestBodyRaw || ""),
      update: function(nextBody) {{
        var s = typeof nextBody === "string" ? nextBody : JSON.stringify(nextBody);
        __np.requestBodyRawOverride = s;
        pm.request.body.raw = s;
        return s;
      }}
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
  response: (function() {{
    var code = (__np.response && __np.response.status) || 0;
    var statusText = (__np.response && __np.response.statusText) || "";
    var bodyStr = (__np.response && __np.response.body) || "";
    var respApi = {{
      code: code,
      status: statusText,
      responseTime: (__np.response && __np.response.responseTime) || 0,
      responseSize: bodyStr.length,
      reason: function() {{ return statusText; }},
      headers: {{
        get: function(name) {{ return __hdrMap[String(name).toLowerCase()]; }},
        has: function(name) {{ return Object.prototype.hasOwnProperty.call(__hdrMap, String(name).toLowerCase()); }},
        toObject: function() {{
          var out = {{}};
          for (var i = 0; i < __respHdrs.length; i++) out[__respHdrs[i].key] = __respHdrs[i].value;
          return out;
        }},
        each: function(fn) {{
          if (typeof fn === "function") for (var i = 0; i < __respHdrs.length; i++) fn(__respHdrs[i]);
        }},
        all: function() {{ return __respHdrs.slice(); }}
      }},
      text: function() {{ return bodyStr; }},
      json: function() {{
        return bodyStr ? JSON.parse(bodyStr) : null;
      }},
      to: {{
        have: {{
          status: function(expected) {{
            if (typeof expected === "number") {{
              if (code !== expected) throw new Error("expected status " + expected + " got " + code);
            }} else if (typeof expected === "string") {{
              if (statusText.toLowerCase() !== expected.toLowerCase()) throw new Error("expected status " + expected + " got " + statusText);
            }}
            return respApi;
          }},
          header: function(name, val) {{
            var key = String(name).toLowerCase();
            if (!Object.prototype.hasOwnProperty.call(__hdrMap, key)) throw new Error("missing response header " + name);
            if (arguments.length > 1 && __hdrMap[key] !== String(val)) {{
              throw new Error("header " + name + " expected " + val + " got " + __hdrMap[key]);
            }}
            return respApi;
          }},
          body: function(expected) {{
            if (arguments.length === 0) {{
              if (!bodyStr) throw new Error("expected non-empty response body");
            }} else if (expected instanceof RegExp) {{
              if (!expected.test(bodyStr)) throw new Error("expected body to match " + expected);
            }} else {{
              if (bodyStr !== String(expected)) throw new Error("expected body " + expected + " got " + bodyStr);
            }}
            return respApi;
          }},
          jsonBody: function(expected) {{
            var parsed;
            try {{ parsed = JSON.parse(bodyStr); }} catch (e) {{ throw new Error("expected JSON body"); }}
            if (arguments.length > 0 && !__np_deepEqual(parsed, expected)) {{
              throw new Error("expected JSON body " + JSON.stringify(expected) + " got " + JSON.stringify(parsed));
            }}
            return respApi;
          }}
        }},
        be: {{
          get ok() {{ if (code < 200 || code >= 300) throw new Error("expected 2xx status got " + code); return respApi; }},
          get success() {{ if (code < 200 || code >= 300) throw new Error("expected 2xx status got " + code); return respApi; }},
          get redirection() {{ if (code < 300 || code >= 400) throw new Error("expected 3xx status got " + code); return respApi; }},
          get clientError() {{ if (code < 400 || code >= 500) throw new Error("expected 4xx status got " + code); return respApi; }},
          get serverError() {{ if (code < 500 || code >= 600) throw new Error("expected 5xx status got " + code); return respApi; }},
          get error() {{ if (code < 400 || code >= 600) throw new Error("expected 4xx/5xx status got " + code); return respApi; }},
          get json() {{
            try {{ JSON.parse(bodyStr); }} catch (e) {{ throw new Error("expected JSON response body"); }}
            return respApi;
          }},
          get accepted() {{ if (code !== 202) throw new Error("expected 202 got " + code); return respApi; }},
          get badRequest() {{ if (code !== 400) throw new Error("expected 400 got " + code); return respApi; }},
          get unauthorized() {{ if (code !== 401) throw new Error("expected 401 got " + code); return respApi; }},
          get forbidden() {{ if (code !== 403) throw new Error("expected 403 got " + code); return respApi; }},
          get notFound() {{ if (code !== 404) throw new Error("expected 404 got " + code); return respApi; }},
          get rateLimited() {{ if (code !== 429) throw new Error("expected 429 got " + code); return respApi; }}
        }}
      }}
    }};
    return respApi;
  }})(),
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
var bru = pm;
var bruno = pm;
var insomnia = pm;
pm.setEnvVar = function(k, v) {{ pm.environment.set(k, v); }};
pm.getEnvVar = function(k) {{ return pm.environment.get(k); }};
pm.setVar = function(k, v) {{ pm.variables.set(k, v); }};
pm.getVar = function(k) {{ return pm.variables.get(k); }};
pm.setGlobalEnvVar = function(k, v) {{ pm.globals.set(k, v); }};
pm.getGlobalEnvVar = function(k) {{ return pm.globals.get(k); }};
pm.setNextRequest = function(name) {{ pm.execution.setNextRequest(name); }};
pm.expect = __expect;
blitz.expect = __expect;
blitz.expect = __expect;
// Coerce request.url in string contexts (boa may not use Object string wrapper)
pm.request.toString = function() {{ return String(__np.requestUrl || ""); }};
blitz.request.toString = pm.request.toString;
blitz.request.toString = pm.request.toString;
"##
    );

    // IIFE keeps `var` / `function` off the reused realm. Newline before close so a trailing `//` comment cannot swallow it.
    // Strip heavy read-only fields before serializing and null out `__np` inside an IIFE so multi-MB response/request payloads are never pinned in JS_REALM.
    let full = format!(
        "{prelude}\n(function(){{\n{sanitized_script}\n}})();\n(function(){{ if (__np) {{ delete __np.response; delete __np.requestBodyRaw; delete __np.requestHeaders; delete __np.iterationData; }} var out = JSON.stringify(__np); __np = null; return out; }})();"
    );

    let eval_result = JS_REALM.with(|ptr| {
        // Exclusive for this call: host callbacks do not re-enter the sandbox.
        let context = unsafe { &mut **ptr };
        let res = match context.eval(Source::from_bytes(full.as_bytes())) {
            Ok(val) => Ok(js_to_string(context, &val)),
            Err(e) => {
                let _ = context.eval(Source::from_bytes(b"__np = null;"));
                Err(format!("{phase}: JS error: {e}"))
            }
        };
        res
    });
    match eval_result {
        Ok(json_str) => apply_bridge(&json_str, ctx, &mut out, phase),
        Err(e) => {
            out.error = Some(e.clone());
            out.messages.push(e);
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

fn np_hash_native(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let alg = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped()
        .to_ascii_lowercase();
    let msg = args
        .get_or_undefined(1)
        .to_string(context)?
        .to_std_string_escaped();
    let hex_out = match alg.as_str() {
        "md5" => hex::encode(Md5::digest(msg.as_bytes())),
        "sha1" => hex::encode(Sha1::digest(msg.as_bytes())),
        "sha512" => hex::encode(Sha512::digest(msg.as_bytes())),
        _ => hex::encode(Sha256::digest(msg.as_bytes())),
    };
    Ok(JsValue::from(js_string!(hex_out.as_str())))
}

fn np_hmac_native(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let alg = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped()
        .to_ascii_lowercase();
    let msg = args
        .get_or_undefined(1)
        .to_string(context)?
        .to_std_string_escaped();
    let key = args
        .get_or_undefined(2)
        .to_string(context)?
        .to_std_string_escaped();
    let hex_out = match alg.as_str() {
        "sha1" => {
            let mut mac = HmacSha1::new_from_slice(key.as_bytes())
                .unwrap_or_else(|_| HmacSha1::new_from_slice(b"").expect("hmac empty key"));
            mac.update(msg.as_bytes());
            hex::encode(mac.finalize().into_bytes())
        }
        "sha512" => {
            let mut mac = HmacSha512::new_from_slice(key.as_bytes())
                .unwrap_or_else(|_| HmacSha512::new_from_slice(b"").expect("hmac empty key"));
            mac.update(msg.as_bytes());
            hex::encode(mac.finalize().into_bytes())
        }
        _ => {
            let mut mac = HmacSha256::new_from_slice(key.as_bytes())
                .unwrap_or_else(|_| HmacSha256::new_from_slice(b"").expect("hmac empty key"));
            mac.update(msg.as_bytes());
            hex::encode(mac.finalize().into_bytes())
        }
    };
    Ok(JsValue::from(js_string!(hex_out.as_str())))
}

fn np_hex_to_b64_native(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    use base64::Engine;
    let hex_str = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let bytes = hex::decode(hex_str.trim()).unwrap_or_default();
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(JsValue::from(js_string!(b64.as_str())))
}

fn np_b64_to_hex_native(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    use base64::Engine;
    let b64 = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(b64.trim()))
        .unwrap_or_default();
    let hex_str = hex::encode(bytes);
    Ok(JsValue::from(js_string!(hex_str.as_str())))
}

fn np_btoa_native(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    use base64::Engine;
    let s = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let b64 = base64::engine::general_purpose::STANDARD.encode(s.as_bytes());
    Ok(JsValue::from(js_string!(b64.as_str())))
}

fn np_atob_native(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    use base64::Engine;
    let b64 = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let trimmed = b64.trim();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(trimmed))
        .unwrap_or_default();
    let s = String::from_utf8_lossy(&bytes).into_owned();
    Ok(JsValue::from(js_string!(s.as_str())))
}

fn np_utf8_to_hex_native(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let s = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let h = hex::encode(s.as_bytes());
    Ok(JsValue::from(js_string!(h.as_str())))
}

fn np_hex_to_utf8_native(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let h = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let bytes = hex::decode(h.trim()).unwrap_or_default();
    let s = String::from_utf8_lossy(&bytes).into_owned();
    Ok(JsValue::from(js_string!(s.as_str())))
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
            let mut body = resp.text().unwrap_or_default();
            const MAX_SCRIPT_HTTP_BYTES: usize = 2 * 1024 * 1024;
            if body.len() > MAX_SCRIPT_HTTP_BYTES {
                let end = (0..=MAX_SCRIPT_HTTP_BYTES)
                    .rev()
                    .find(|&i| body.is_char_boundary(i))
                    .unwrap_or(0);
                body.truncate(end);
            }
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
        "requestUrlOverride": JsonValue::Null,
        "requestMethod": extras.request_method,
        "requestHeaders": request_headers,
        "requestHeaderUpserts": [],
        "requestHeaderRemovals": [],
        "requestBodyMode": extras.request_body_mode,
        "requestBodyRaw": extras.request_body_raw,
        "requestBodyRawOverride": JsonValue::Null,
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

    if let Some(removals) = v.get("requestHeaderRemovals").and_then(|a| a.as_array()) {
        for r in removals {
            if let Some(s) = r.as_str() {
                if !s.is_empty() {
                    out.request_header_removals.push(s.to_string());
                }
            }
        }
    }

    if let Some(url_ov) = v.get("requestUrlOverride").and_then(|u| u.as_str()) {
        if !url_ov.is_empty() {
            out.request_url_override = Some(url_ov.to_string());
        }
    }

    if let Some(body_ov) = v.get("requestBodyRawOverride").and_then(|b| b.as_str()) {
        out.request_body_raw_override = Some(body_ov.to_string());
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

/// Remove headers requested via `blitz.request.headers.remove` / `.clear`.
pub fn apply_request_header_removals(
    draft: &mut crate::core::RequestDraft,
    removals: &[String],
) {
    if removals.is_empty() {
        return;
    }
    draft.headers.retain(|h| {
        !removals
            .iter()
            .any(|rem| h.key.eq_ignore_ascii_case(rem))
    });
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
    fn reused_realm_hides_previous_var() {
        let mut ctx = VariableContext::default();
        let first = run_js_sandbox_ex(
            "var leaked = 'secret'; blitz.environment.set('k', leaked);",
            "a",
            &mut ctx,
            None,
            &SandboxExtras::default(),
        );
        assert!(first.error.is_none(), "{:?}", first.error);
        let second = run_js_sandbox_ex(
            "blitz.environment.set('k2', typeof leaked);",
            "b",
            &mut ctx,
            None,
            &SandboxExtras::default(),
        );
        assert!(second.error.is_none(), "{:?}", second.error);
        assert_eq!(ctx.environment.get("k2").map(String::as_str), Some("undefined"));
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

    #[test]
    fn dynamic_faker_variables_in_sandbox() {
        let mut ctx = VariableContext::default();
        let out = run_js_sandbox(
            r#"
            var g = pm.variables.get("$guid");
            if (!g || g.length !== 36) throw new Error("guid invalid: " + g);

            var replaced = pm.variables.replaceIn("hello {{$randomEmail}} - {{$timestamp}}");
            if (replaced.indexOf("@example.com") === -1) throw new Error("replaceIn email failed: " + replaced);
            "#,
            "pre-request",
            &mut ctx,
            None,
        );
        assert!(out.error.is_none(), "{:?}", out.error);
    }

    #[test]
    fn advanced_crypto_encoding_and_url_search_params() {
        let mut ctx = VariableContext::default();
        let out = run_js_sandbox(
            r#"
            const hmacHex = CryptoJS.HmacSHA256("msg", "key").toString(CryptoJS.enc.Hex);
            const hmacB64 = CryptoJS.HmacSHA256("msg", "key").toString(CryptoJS.enc.Base64);
            if (hmacHex.length !== 64) throw new Error("hmacHex len " + hmacHex.length);
            if (hmacHex === hmacB64) throw new Error("hex and b64 should differ");
            const sha512 = CryptoJS.SHA512("abc").toString(CryptoJS.enc.Hex);
            if (sha512.length !== 128) throw new Error("sha512 len " + sha512.length);
            const md5 = CryptoJS.MD5("abc").toString(CryptoJS.enc.Hex);
            if (md5 !== "900150983cd24fb0d6963f7d28e17f72") throw new Error("md5 mismatch " + md5);
            const sha1 = CryptoJS.SHA1("abc").toString(CryptoJS.enc.Hex);
            if (sha1 !== "a9993e364706816aba3e25717850c26c9cd0d89d") throw new Error("sha1 mismatch " + sha1);
            const basic = CryptoJS.enc.Base64.stringify(CryptoJS.enc.Utf8.parse("user:pass"));
            if (basic !== "dXNlcjpwYXNz") throw new Error("basic mismatch " + basic);
            if (btoa("user:pass") !== "dXNlcjpwYXNz") throw new Error("btoa mismatch");
            if (atob("dXNlcjpwYXNz") !== "user:pass") throw new Error("atob mismatch");
            const usp = new URLSearchParams({ a: "1", b: "hello world" });
            if (usp.get("b") !== "hello world") throw new Error("usp get");
            if (usp.toString().indexOf("a=1") < 0) throw new Error("usp str " + usp.toString());
            "#,
            "pre-request",
            &mut ctx,
            None,
        );
        assert!(out.error.is_none(), "{:?}", out.error);
    }

    #[test]
    fn advanced_request_mutation_headers_url_and_body() {
        let mut ctx = VariableContext::default();
        let extras = SandboxExtras {
            request_url: "https://api.example.com/v1/orders?page=1".into(),
            request_method: "POST".into(),
            request_headers: vec![
                ("X-Keep".into(), "1".into()),
                ("X-Remove".into(), "drop".into()),
            ],
            request_body_mode: "raw".into(),
            request_body_raw: r#"{"old":true}"#.into(),
            ..Default::default()
        };
        let out = run_js_sandbox_ex(
            r#"
            if (pm.request.url.getHost() !== "api.example.com") throw new Error("host");
            if (pm.request.url.getQueryString() !== "page=1") throw new Error("qs");
            if (!pm.request.headers.has("x-remove")) throw new Error("has");
            pm.request.headers.remove("X-Remove");
            pm.request.headers.upsert({ key: "X-Added", value: "yes" });
            if (pm.request.headers.has("X-Remove")) throw new Error("still has");
            pm.request.url.addQueryParams({ sort: "desc" });
            pm.request.body.update({ fresh: 42 });
            "#,
            "pre-request",
            &mut ctx,
            None,
            &extras,
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert!(out
            .request_header_removals
            .iter()
            .any(|h| h.eq_ignore_ascii_case("x-remove")));
        assert!(out
            .request_header_upserts
            .iter()
            .any(|(k, v)| k == "X-Added" && v == "yes"));
        assert_eq!(
            out.request_url_override.as_deref(),
            Some("https://api.example.com/v1/orders?page=1&sort=desc")
        );
        assert_eq!(
            out.request_body_raw_override.as_deref(),
            Some(r#"{"fresh":42}"#)
        );
    }

    #[test]
    fn advanced_chai_bdd_response_and_lodash_tests() {
        let mut ctx = VariableContext::default();
        let resp = SandboxResponse {
            status: 201,
            status_text: "Created".into(),
            body: r#"{"user":{"name":"Alice","roles":["admin","dev"],"active":true,"score":85,"notes":null},"tags":["api","v2"]}"#.into(),
            headers: vec![
                ("Content-Type".into(), "application/json".into()),
                ("X-Request-Id".into(), "req-99".into()),
            ],
            response_time: 18,
        };
        let out = run_js_sandbox(
            r#"
            const _ = require("lodash");
            pm.test("response status & headers", function () {
                pm.response.to.be.ok;
                pm.response.to.be.success;
                pm.response.to.be.json;
                pm.response.to.have.status(201);
                pm.response.to.have.header("x-request-id", "req-99");
                pm.response.to.have.body(/Alice/);
                pm.expect(pm.response.reason()).to.equal("Created");
                pm.expect(pm.response.headers.has("content-type")).to.be.true;
            });
            pm.test("chai BDD chains & matchers", function () {
                const d = pm.response.json();
                pm.expect(d.user.active).to.be.true;
                pm.expect(d.user.notes).to.be.null;
                pm.expect(d.user.missing).to.be.undefined;
                pm.expect(d.user.name).to.be.a("string").and.not.be.empty.and.match(/^Al/);
                pm.expect(d.user.score).to.be.at.least(80).and.be.at.most(90).and.be.within(0, 100);
                pm.expect(d.user.roles).to.be.an("array").and.have.lengthOf(2).and.include("admin");
                pm.expect(d).to.have.nested.property("user.roles[0]", "admin");
                pm.expect(d.user.roles[0]).to.be.oneOf(["admin", "user"]);
                pm.expect({ a: 1, b: 2 }).to.have.all.keys("a", "b");
                pm.expect({ a: 1, b: 2 }).to.have.any.keys("b", "c");
                pm.expect({ a: [1, 2] }).to.deep.equal({ a: [1, 2] });
                pm.expect(function() { throw new Error("bad input"); }).to.throw(/bad/);
            });
            pm.test("lodash helpers", function () {
                const d = pm.response.json();
                pm.expect(_.get(d, "user.roles[1]")).to.equal("dev");
                pm.expect(_.uniq([1, 1, 2, 3])).to.eql([1, 2, 3]);
                pm.expect(_.chunk([1, 2, 3, 4], 2)).to.eql([[1, 2], [3, 4]]);
                pm.expect(_.pick(d.user, ["name", "active"])).to.eql({ name: "Alice", active: true });
                pm.expect(_.isEmpty({})).to.be.true;
            });
            "#,
            "tests",
            &mut ctx,
            Some(&resp),
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.tests.len(), 3);
        for t in &out.tests {
            assert!(t.passed, "test `{}` failed: {}", t.name, t.message);
        }
    }

    #[test]
    fn infinite_loop_is_aborted_safely() {
        let mut ctx = VariableContext::default();
        let out = run_js_sandbox(
            "while (true) {}",
            "pre-request",
            &mut ctx,
            None,
        );
        assert!(out.error.is_some(), "expected loop limit error");
    }

    #[test]
    fn bruno_and_insomnia_runtime_aliases_and_normalized_scripts_execute() {
        let mut ctx = VariableContext::default();
        // 1. Raw un-normalized bru/bruno/insomnia script executes via runtime aliases
        let raw_script = r#"
            bru.setEnvVar("env_k", "ev1");
            bruno.setVar("loc_k", "lv1");
            bru.setGlobalEnvVar("glob_k", "gv1");
            if (bru.getEnvVar("env_k") !== "ev1") throw new Error("getEnvVar");
            if (bruno.getVar("loc_k") !== "lv1") throw new Error("getVar");
            if (bru.getGlobalEnvVar("glob_k") !== "gv1") throw new Error("getGlobalEnvVar");
            insomnia.environment.set("ins_k", "iv1");
            bru.setNextRequest("Step2");
        "#;
        let out1 = run_js_sandbox(raw_script, "pre-request", &mut ctx, None);
        assert!(out1.error.is_none(), "{:?}", out1.error);
        assert_eq!(ctx.environment.get("env_k").map(String::as_str), Some("ev1"));
        assert_eq!(ctx.local.get("loc_k").map(String::as_str), Some("lv1"));
        assert_eq!(ctx.global.get("glob_k").map(String::as_str), Some("gv1"));
        assert_eq!(ctx.environment.get("ins_k").map(String::as_str), Some("iv1"));
        assert_eq!(out1.execution.next_request.as_deref(), Some("Step2"));

        // 2. Normalized script (pm./bru./insomnia. -> blitz.) executes identically
        let mut ctx2 = VariableContext::default();
        let normalized = crate::core::normalize_script_to_blitz(raw_script);
        assert!(!normalized.contains("bru."));
        assert!(!normalized.contains("bruno."));
        assert!(!normalized.contains("insomnia."));
        let out2 = run_js_sandbox(&normalized, "pre-request", &mut ctx2, None);
        assert!(out2.error.is_none(), "{:?}", out2.error);
        assert_eq!(ctx2.environment.get("env_k").map(String::as_str), Some("ev1"));
        assert_eq!(ctx2.local.get("loc_k").map(String::as_str), Some("lv1"));
        assert_eq!(ctx2.global.get("glob_k").map(String::as_str), Some("gv1"));
        assert_eq!(ctx2.environment.get("ins_k").map(String::as_str), Some("iv1"));
        assert_eq!(out2.execution.next_request.as_deref(), Some("Step2"));
    }
}
