# 10 — RE complete (local-client architecture)

**Pass date:** 2026-10-17  
**Target:** `/Applications/Postman.app` · ASAR SHA-256 `30abb7db2af633bf854f32b60fde0a1dc3e8241140cc8f9e033ee6c1091564f4`  
**ASAR `package.json` version:** `12.32.4` (**observed**); userData `settings.json` also records `downloadedVersion` / `lastKnownVersion` 12.32.x  
**Constraint:** Native = **Ghidra only** (Hopper not used). This pass = offline ASAR extract + REA JS analysis + userData layout. No new `.node` Ghidra.  
**Scratch:** `.rea-scratch/re-complete-extract/` (`npx @electron/asar extract`), `mined.json`, `deep.json`.

Closes P0/P1 RE holes from [09 re-followups](../09-docs-gap-matrix/re-followups.md) / [09 next-actions](../09-coverage-gap/next-actions.md) for **local client recreation**. Does **not** implement Rust app.

---

## REA / tool call log

| Call | Result | Note |
|------|--------|------|
| `npx @electron/asar extract` → `.rea-scratch/re-complete-extract/app` | **ok** | Full offline tree |
| `analyze_javascript_application` (package/sandbox/runner/process-manager) | **ok** (summary) | Parent `ev_1928090e472b3d7f11485511165bfd8a5525e92bdf672d5e651b9e9b63b65c16`; view `ev_83bf3a53940a46f8c383ad77804fc098343d3055980bf32ffdcd7dc9b1b958ad` |
| `analyze_javascript_application` (proxy/perftest/webProxy) | **ok** (summary, semantic truncated) | Parent `ev_f6e411837034f68b751a4997db2feb6f489aa5e6f497cb99a912dadba3fa918c`; view `ev_f5596f19ce230732fdecff6b12d550134346b55618e45071204694d2d9d9283b` |
| Same with wrong `source_paths` (`js/services/…`) | **failed** `invalid_request` | Paths must match ASAR inventory; retried with webpack names |
| Offline Python string mining on extract | **ok** | `mined.json` / `deep.json` |
| `list_electron_targets` / live CDP | **not run** | No live Postman process (`pgrep` empty); `DevToolsActivePort` empty; Singleton* look stale |
| Ghidra on new native | **not run** | No new native crypto gap beyond keytar (`08`) |
| Hopper | **not used** | Forbidden |
| Prior ASAR offset mapper | **failed** (still) | See `09` — not retried |

---

## 1. `postman-sandbox` / `epm.*` + isolation (uvm / uniscope)

### Host stack (**observed**)

| Layer | Role |
|-------|------|
| `html/tester_sandbox.html` + `js/tester/Evaluator.js` | Legacy webview shell; Evaluator only blocks `window.open` + echo `postMessage` |
| `postman-runtime` → `require("postman-sandbox")` | Real script host (`createContext`, event execution) |
| Bundled **`uvm`** | Bridge: `__uvm_emit` / `__uvm_dispatch` / `__uvm_on` / `__uvm_setTimeout`; `bridge.dispatch('exit')` |
| Bundled **`uniscope`** | Scope isolation; `uniscope/lib/allowed-globals` + extras `require`, `eval`, `console`, `bridge`, `__uvm_emit`, `__uvm_setTimeout`; deletes other globals |
| Sandbox module allowlist | `postman-sandbox`, `uniscope`, `uvm`, `postman-collection`, `postman-request`, `chai-postman` (**observed** package-gate object) |

**uniscope create options (**observed** in execution-worker sandbox bootstrap):**

```js
o.create({ eval: true, ignore: ["require"], block: ["bridge"] })
```

Non-legacy markers: `isNonLegacySandbox`, `getNonLegacyCodeMarker` (**observed**).

Vault / datasets APIs wired inside sandbox:

```js
{ Vault, getVaultInterface } = require("./vault")
getVaultInterface: e => ({ get, set, unset })  // host bridge
{ Datasets, getDatasetsInterface } = require("./datasets")
```

Synced VariableScope names after script (**observed**): `_variables`, `environment`, `collectionVariables`, `globals`.

### `epm.*` method table recovered from ASAR strings (**observed**, not full decompile)

Host-process APIs (`epm.logger`, `epm.sdk.IPC`, `epm.eventBus`) ≠ sandbox `pm` — listed separately.

| Area | Methods / properties (**observed** string hits) |
|------|--------------------------------------------------|
| Environment | `epm.environment.get` / `set` / `unset` / `clear` |
| Globals | `epm.globals.get` / `set` / `unset` / `clear` |
| Local vars | `epm.variables.get` / `set` / `unset` |
| Collection vars | context includes `collectionVariables` VariableScope; `epm.collectionVariables.*` not densely string-hit this pass (API still **inferred** as VariableScope twin of env/globals) |
| Cookies | `epm.cookies` / `epm.cookies.get` (+ CookieStore bridge ops below) |
| Request | `epm.request` |
| Response | `epm.response.code` / `headers` / `headers.get` / `text` / `responseTime` / `to` |
| Test / expect | `epm.test`, `epm.expect` |
| Execution | `epm.execution.setNextRequest` / `skipRequest` / `runRequest` |
| Info / data | `epm.info.iteration`, `epm.iterationData`, `epm.datasets` |
| Send | `epm.sendRequest` (prefix) |
| Vault | `getVaultInterface` → get/set/unset (exposed as vault API in sandbox module) |

**CookieStore bridge ops (**observed**):** `findCookie`, `findCookies`, `putCookie`, `updateCookie`, `removeCookie`, `removeCookies` + `allowProgrammaticAccess` gate.

**Still incomplete:** full `epm.response.json` / `epm.collectionVariables.set` surface needs unminified `postman-sandbox` sources or denser AST of `./pmapi` factory — string mining is upper bound for literals only.

---

## 2. `{{vault:}}` / VariableScope vault + `~/Postman/files`

### Substitution (**observed**)

AgentWorker constructs vault VariableScope:

```text
prefix: "vault:"
_allowScriptAccess: (url) => <gated by script.allowVaultInScript + collection id>
values[]: { enabled, disabled, type, key, value, _domains: domains, shouldMaskInConsole, source? }
```

→ `{{vault:<key>}}` via prefix (**inferred** from VariableScope semantics + prior passport `{{vault:"+ref+"}}` literal).

Domain filter: `__hasDomainPatterns` → `_.domainPatterns`; `__getMatchingVariables(url)` filters before `replaceSubstitutions` (**observed**).

### Files root (**observed**)

```js
initializeDefaultWorkingDirectory:
  path = join(app.homeDir(), "Postman", "files")
fileResolver default workingDir =
  join(homeDir(), "Postman", "files")
```

userData `storage/settings.json`: `"createdDefaultWorkingDir": true` (**observed**).  
On this machine `~/Postman/files` exists but is **empty** — no vault ciphertext samples on disk.

### Encryption (**observed** / **unknown**)

| Backend | Status |
|---------|--------|
| CryptoService → keytar (`account=userKey`, `service=${app.getName()} (${id})`) | **observed** (`05`/`08`, Ghidra) |
| Azure Key Vault / 1Password / AES deps | **observed** deps + config; runtime selection not fully mapped |
| Encryption of files under `~/Postman/files` | **unknown** — directory empty; no file-format crypto observed this pass. **No new Ghidra** (no new native crypto binary identified). |
| Vault secret values in VariableScope | In-memory / IPC cache; at-rest format for vault blob store **not recovered** |

---

## 3. Proxy + certificates

### On-disk layout under `app.getPath("userData")` (**observed** code)

```text
{userData}/proxy/
  certificates/cert.json          # hostname → {certFile,keyFile} map
  postman-proxy-ca.crt
  postman-proxy-ca.key
  postman-proxy-ca.p12            # deleted/regenerated with pair
```

`generateRootCAForProxy(userDataPath)` creates dirs + empty `cert.json`, ensures root CA.  
**This userData has no `proxy/` tree yet** — proxy MITM never materialized locally (**observed** absence).

### `ProxyConstants` (**observed**)

| Key | Value |
|-----|-------|
| `CA_SUFFIX_NAME` | `postman-proxy-ca` |
| `KEYBITSIZE` | 2048 |
| `HASH` | `sha256` |
| `DAYSTOEXPIRY` | 500 |
| `COMMON_NAME` | channel → `Postman Proxy CA` (prod) / Dev/Test/Beta/Stage/Canary variants |
| Org fields | US / CA / San Francisco / Postman Inc. |

### Runtime apply (**observed**)

1. **Capture / MITM proxy process:** `ProxyNodeProcess` → `http-proxy` `createProxyServer({ secure:false, ssl:{ key, cert } })` using root CA from `generateRootCAForProxy`; header constant `x-postman-captr`.
2. **Requester settings → run options (scratchpad):**  
   `epm.settings`: `useSystemProxy`, `useProxyEnvironmentVariables`, `SSLCertVerify`, `isCACertEnabled`, `CACertPath`, …  
   → `proxyOptions: { useSystemProxy, ignoreProxyEnvironmentVariables, proxyConfigList }`  
   → runtime `useSystemProxy` wires `systemProxy = (url, cb) => proxy.resolve(url)`.
3. **HTTP options:** `strictSSL`, `followRedirects`, `disableCookies` on requester/runtime (**observed**).
4. **Perf executor:** `GET_SYSTEM_PROXY` / `SYSTEM_PROXY_RECEIVED` via `process-manager` (**observed**).

---

## 4. Collection Runner vs performance runner

| Path | Modules / events | Role |
|------|------------------|------|
| **Collection Runner UI** | `html/runner.html` title “Collection Runner Webview”; scripts `js/preload.js`, `js/vendor-ui.js`, `js/runner.js` | **observed** HTML |
| | `js/runner.js`, `js/vendor-ui.js`, `js/runner.css` | **absent** from ASAR extract — same CDN-missing pattern as `requester.js` (**observed**) |
| **Collection Run engine** | `postman-runtime` Run / partitions / `cookieJar` / `vaultSecrets` / nested requests | **observed** strings — sequential/collection execution |
| **Performance runner** | `process-manager.js` forks `execution-handler.js`; `perftest.js`, `perftestExecute.js`, `ThreadGroupExecutable.js`, `main-performance-test-ipc-handlers-*.min.js` | **observed** |
| Perf events | `ExecutionManagerEvents`: `START/STOP/PAUSE/RESUME`, `ADD_USERS`/`REMOVE_USERS`, `GET_SYSTEM_PROXY`, `GET_OAUTH_REFRESH_TOKEN`, `ASSERTION_METRICS_DATA`, `SCRIPT_ERROR_DATA`, …; load profiles `fixed`/`ramp-up`/`spike`/`peak` + VU transitions | **observed** |

**Verdict:** Collection Runner = remote/webview UI + `postman-runtime` Run. Performance = separate child-process VU executor (`process-manager` ↔ handler). Not the same module graph.

---

## 5. Protocol clients (high-level entrypoints)

| Package / ASAR chunk | Size (sample) | Surface (**observed** verbs / names) |
|----------------------|---------------|--------------------------------------|
| `@postman/runtime_grpc-client` | ~22 KB chunks | `Client`; `invoke`, `stream`, `write` |
| `@postman/runtime_grpc-utils` + `@postman/grpc-js` / `grpc-reflection-js` | vendor chunks | Reflection / transport deps |
| `@postman/runtime_mqtt-client` | ~328 KB | `MqttClient` / `Client`; `connect`, `subscribe`, `unsubscribe`, `publish`, `disconnect`, … |
| `@postman/runtime_websocket-client` | node_modules chunks | `connect`, `send`, `close` |
| `postman-ws` | ~40 KB | `send`, `stream`, `write`, `connect`, `close` |
| GraphQL | `graphql-ws` dep; scratchpad `ECWebsocketWorkbench.js`, graphql workers | UI/workbench local; execute path uses runtime API client strings (`graphql.execute` in scratchpad) |

**Recreation note:** treat each `@postman/runtime_*-client` as protocol adapter beside HTTP `postman-request`; do not assume shared cookie/vault wiring without further traces.

---

## 6. Cookie jar → send path

### Runtime wiring (**observed**)

1. Requester option map includes `disableCookies:"disableCookies"`.
2. When `!disableCookies`: `h.jar = t.cookieJar || true` (postman-request jar).
3. `jar.getCookieString(uri)` → `cookiesFromJar` before send; response `Set-Cookie` → `jar.setCookie` unless `_disableCookies`.
4. Runner: `perPartitionCookieJar = options.requester.perPartitionCookieJar && !options.requester.cookieJar` (mutual exclusion with shared jar).
5. Send / nested request: `requester.create({ …, cookieJar: run.getCookieJarFor(coords) })`.
6. Sandbox CookieStore ops domain-gated via `allowProgrammaticAccess`.

### Electron partition jars (**observed** code + userData)

```js
// partitionUtils
persist prefix: "persist:postman_user_cookies"
getCookiePartitionId(userKey) => "persist:postman_user_cookies" + (default ? "" : `_${userKey}`)
```

On disk under `~/Library/Application Support/Postman/Partitions/`:

- `postman_user_cookies_<userPartitionUuid>/Cookies` (+ journal)
- `postman_user_cookies_undefined/Cookies`
- Also Chromium `Cookies` under UUID partitions

`storage/userPartitionData.json` maps partition UUIDs → user/scratchPad namespaces (**observed** structure; **do not** treat auth tokens in that file as RE targets).

Electron fuse `EnableCookieEncryption` = disabled (**observed** prior) — separate from app jar.

---

## Final RE state (local-client)

| Topic | Status after this pass |
|-------|------------------------|
| Sandbox / `epm.*` | **observed** isolation + solid method list; not exhaustive SDK |
| Vault `{{vault:}}` + files root | **observed**; at-rest vault file crypto **unknown** |
| Proxy / CA paths + settings keys | **observed** |
| Runner vs perf | **observed** separation; Runner UI JS missing locally |
| Protocol clients | **observed** chunk roles (high-level) |
| Cookie jar / partitions | **observed** attach + disk layout |
| Send click → HTTP live path | **still open** (no CDP) |
| Collection v3 YAML | **still missing** in desktop ASAR |
| VariableScope precedence algorithm | **partial** (classes observed; not decompiled) |
| History store | **still missing** |

---

## Remaining unknowns (honest)

1. Live CDP Send / Runner click (`list_electron_targets` / `capture_electron_scenario`) — Postman not running this pass.
2. Where CDN hosts `js/runner.js` / `vendor-ui.js` (same class as requester).
3. Exhaustive `epm.*` / `epm.collectionVariables.*` / `epm.response.json` from unminified sandbox.
4. Vault secret blob at-rest format (empty `~/Postman/files`; no native vault `.node` opened).
5. Full `replaceSubstitutions` precedence decompile.
6. postman-cli collection YAML (not extracted this pass).
7. CryptoService `id` → user/team GUID mapping.
8. Whether Electron partition Cookies DB is synced with `@postman/tough-cookie` jar or parallel store (**inferred** dual-layer; not proven).

---

## Patches to earlier reports

- `03` — cookie `getCookieJarFor` / partition + disableCookies apply notes.
- `04` — runner.js absent; sandbox uniscope/uvm detail; protocol verbs; cookie partitions.
- `05` — vault files init + proxy CA constants / settings keys.
- `08` — pointer to this report; runner CDN-missing confirmed.
- `09` gap-matrix / re-followups — status bumps where closed.
- `README.md` — index `10`.
