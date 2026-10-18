# 09 — RE follow-ups (P0/P1 local-client architecture)

**Pass date:** 2026-10-17  
**Target:** `/Applications/Postman.app` · ASAR SHA-256 `30abb7db2af633bf854f32b60fde0a1dc3e8241140cc8f9e033ee6c1091564f4`  
**Constraint:** Native = **Ghidra only** (Hopper not used). This pass = JS/ASAR; no new native addon opened (keytar already in `08`).  
**Scratch:** `.rea-scratch/gap-extract/` (extracted members for offline string mining).

---

## REA / tool call log

| Call | Result | Evidence / note |
|------|--------|-----------------|
| `analyze_javascript_application` focused paths (`tester_sandbox`, `Evaluator`, `runner.html`, `proxyCertificateService`, `IPCAgent`, `package.json`) | **ok** (partial graph) | Parent `ev_d64c427521e0895d16eddf2c117fc50be4ebf2900c07230bc42697b9c4dd62d8`; summary view `ev_76791f522ca5de28cb6f07767d44788c99216ed57c55face46ca5a1bf1ebfef7`. 434 files unselected. Inline script in `tester_sandbox.html` **not** analyzed by REA (limitation text). |
| `trace_application_feature` seed `vault-ipc` on that graph | **no-match** | `ev_36c48debc699fabe857e5f0e3d3c4042549ab86ae12c1f88b7dcb6e663d0a1f0` — not absence of feature; seed not in selected graph as string entity. Vault facts recovered via ASAR byte search instead. |
| Full `postman-runtime-*.min.js` via REA semantic analyze | **not attempted** | ~4.7 MB; prior passes heap/budget fail (`03`). Used offline ASAR/extract string mining (**observed** bytes). |
| ASAR member-offset mapper (custom Python) | **failed** | Header pickle/JSON decode error on this ASAR layout; did not attribute file-offsets to member paths. |
| Ghidra on new `.node` | **not run** | No P0/P1 native gap beyond existing keytar (`08`). |
| Hopper | **not used** | Forbidden. |

---

## 1. Variable scopes + vault refs

### Variable scopes (**observed**)

From `postman-runtime` / sandbox bootstrap string list:

```
_variables, environment, collectionVariables, globals, cookies, data, request, response
```

Plus run-state / resolve pipeline also carries **`vaultSecrets`** as a `VariableScope`.

Collection model: `VariableScope`, `Property.replaceSubstitutions` / `replaceIn` (**observed** in `vendors-node_modules_postman-collection_*.min.js` and runtime).

**Inference for recreation:** resolve `{{name}}` across scopes; vault uses **prefixed** keys (below). App already approximates local>env>collection>global without vault prefix (**app-implemented** `core/variables.rs`).

### Vault syntax (**observed**)

1. **VariableScope prefix** (runner setup chunk in ASAR):

```text
homedir() + "Postman" + "files"   → working files root (path join observed)
vaultSecrets VariableScope:
  id: <vaultId>
  prefix: "vault:"
  _allowScriptAccess: () => false
  values[].{enabled, type, key, value, _domains: domains, shouldMaskInConsole}
```

→ Substitutions of form **`{{vault:<key>}}`** via prefix mechanism (**inferred** from `prefix:"vault:"` + Postman VariableScope semantics; literal `{{vault:` also appears for passport refs below).

2. **Passport / Secured Access Proxy** (**observed** ASAR text):

```js
resolvedValue: "{{vault:" + secretReference + "}}"
allowedInScript: true  // when passport.secretReference present
```

Error string: `Secret reference not found for passport secret "…"`. Ensure Secured Access Proxy enabled…

3. **Sandbox API** (**observed** in runtime sandbox modules):

```js
getVaultInterface: e => ({ get, set, unset })  // bridges to host
```

Runtime event context includes `vault:f` alongside `datasets`, `globals`, `environment`, `collectionVariables`.

4. **URL-scoped vault resolution** (**observed**):

`vaultSecrets.__getMatchingVariables(url)` filters by `_.domainPatterns` then `Property.replaceSubstitutions`.

5. **IPC / crypto backends** (prior **observed**, still valid):

- Channels: `vault-ipc`, `vault-ipc-command`, `vault-ipc-event`; commands `init` / `login` / `clearCache` / `updateCacheTtl` / `reportVaultScriptAccess`; methods `saveVaultUserKey` / `retrieveVaultUserKey` → CryptoService keytar.
- Azure Key Vault keys in `config.json` (`05`).
- OS keychain via keytar (**Ghidra**, `08`).

**notPostman gap:** `secret://{id}` only — no `{{vault:}}` prefix, no domainPatterns, no `~/Postman/files` layout.

---

## 2. Script sandbox / `epm.*` surface

### Host documents (**observed**)

| Artifact | Role |
|----------|------|
| `html/tester_sandbox.html` | Loads `../js/tester/Evaluator.js`; declares legacy globals `responseBody`, `responseHeaders`, `responseTime`, `responseCode`, `request`, `iteration`, `data`, `postman`, `environments`, `globals`; `evaluator = new Evaluator()` |
| `js/tester/Evaluator.js` (437 B) | Blocks `window.open`; `postMessage` echo `sandboxEchoText` → `sandboxEchoResponse` only |

Real script host is **`postman-sandbox`** required from `postman-runtime` (`./node_modules/postman-sandbox/index.js`) + **uvm** / **uniscope** (**observed** dependency allowlist strings). Non-legacy sandbox markers: `isNonLegacySandbox`, `getNonLegacyCodeMarker`.

### `epm.*` surface recovered from runtime strings (**observed**, not exhaustive decompile)

| Area | Methods / properties seen |
|------|---------------------------|
| Environment | `epm.environment.get/set/unset/clear` |
| Globals | `epm.globals.get/set/unset/clear` |
| Collection vars | `epm.collectionVariables.get` (snippet catalog) |
| Cookies | `epm.cookies.get` |
| Request / response | `epm.request`, `epm.response.code/headers/headers.get/text/responseTime` |
| Test | `epm.test` |
| Execution | `epm.execution.setNextRequest`, `skipRequest`, `runRequest` |
| Info / data | `epm.info.iteration`, `epm.iterationData` |
| Send | `epm.sendRequest.` (prefix hit) |
| Vault | `getVaultInterface` → get/set/unset (exposed as vault API in sandbox module) |

Sandbox context array includes `cookies` and syncs mutated scopes after script (**observed** `environment`/`globals`/`collectionVariables` VariableScope clone pattern).

**App:** scripts stored, `run_*_stub` — no VM (**app-implemented** gap).

---

## 3. Collection Runner / execution pipeline

| Finding | Label |
|---------|-------|
| `html/runner.html` title `Collection Runner Webview`; loads `js/vendor-ui.js` + `js/runner.js` | **observed** |
| `js/runner.js` presence in ASAR list | **not verified this pass** (HTML references it; may be absent like `requester.js`) |
| Runtime runner: `postman-runtime/lib/runner/run.js`, partitions, `cookieJar`, `vaultSecrets`, nested requests | **observed** strings |
| Perf / VU path: `process-manager.js`, `perftest*`, `ThreadGroupExecutable`, `GET_SYSTEM_PROXY`, OAuth refresh events | **observed** (`03`) — **separate** from Collection Runner webview |

**Inference:** Collection Runner UI shell + `postman-runtime` Run; performance tests = child-process executor. Boundary clearer than `04` alone, still no live CDP of Run click.

---

## 4. Protocol clients beyond HTTP

**package.json deps (**observed**):** `postman-runtime`, `postman-request`, `postman-ws`, `mqtt`, `@postman/grpc-js`, `@postman/grpc-reflection-js`, `graphql-ws`, `ws`, …

**ASAR vendor / node_modules chunks (**observed** list):**

- `@postman/runtime_grpc-client` + `runtime_grpc-utils`
- `@postman/runtime_mqtt-client` (+ mqtt-utils)
- `@postman/runtime_websocket-client`
- `postman-ws`
- Scratchpad: `graphql-to-postman.js`, `graphql.worker.js`, `ECWebsocketWorkbench.js`

MQTT extract sample (~328 KB) contains many `connect`/`subscribe`/`publish` strings — client bundle present, not wired into notPostman.

---

## 5. Proxy / certificate handling

### `services/proxyCertificateService.js` (**observed** extract)

- Uses `@postman/pem` + `node-forge`; `generateRootCa` / `getCertInfo`.
- Layout under user data:

```text
{userData}/proxy/
  certificates/
    cert.json
  postman-proxy-ca.crt   # validation path
```

- Log / error strings: `Error generating rootCA certificate for HTTPS proxy`, `Postman Proxy CA` (env variants: Test/Beta/Stage/Canary).

### `ProxyNodeProcess.js` (**observed**)

- Embeds `http-proxy` (web + WS incoming passes).
- Requires `proxyCertificateService`, `electronProxy`, `ProxyConstants`.
- Starts listen proxy; HTTPS MITM with generated root CA; header `x-postman-captr` seen in constants vicinity.

**App:** no proxy/cert settings.

---

## 6. Import/export / collection schema

| Finding | Label |
|---------|-------|
| Schema URL `https://schema.getpostman.com/json/collection/v2.1.0/collection.json` | **observed** in `postman-collection` vendor chunk |
| Literal `importCollection` (≥15 ASAR hits) | **observed** |
| `exportCollection` | **0** hits this pass |
| `.postman_collection` | **observed** |
| `schema.getpostman.com/.../collection/v3` / `collection/v3` | **0** hits — **missing** as desktop schema |
| `*-pme_v3.worker.min.js` (yaml/json/html/ts/xml/editor) | **observed** — Monaco **PME v3 editor** workers, **not** collection-format v3 |

**Recreation:** implement Collection **v2.1** JSON first. Treat “v3 YAML” as docs/Native Git story until a schema/parser member is found (CLI may still differ — `Resources/data/postman-cli` not re-walked this pass).

---

## 7. Cookie jar

| Finding | Label |
|---------|-------|
| Dep `@postman/tough-cookie` ^4.1.3-postman.1 | **observed** `package.json` |
| Runtime: `cookieJar`, `disableCookies`, `perPartitionCookieJar`, `getCookieJarFor`, `resetCookieJar` | **observed** |
| Request path: `jar.getCookieString` / `setCookie` on response `Set-Cookie` | **observed** (postman-request fork patterns) |
| Sandbox CookieStore: `findCookie`/`putCookie`/… + `allowProgrammaticAccess` gate | **observed** |
| Electron fuse `EnableCookieEncryption` = disabled | **observed** prior (`05`) — ≠ app cookie jar |

**App:** no jar.

---

## Patches applied to earlier reports

- `04-subsystems` — vault prefix, VariableScope list, pm/sandbox, cookies, protocols, collection schema, runner boundary.
- `03-request-lifecycle` — cookieJar + vaultSecrets in pipeline notes.
- `05-auth-credentials` — `{{vault:}}` / files path / script access flag.
- `08-deeper-re` — pointer to this follow-up + ASAR digest note.
- `README.md` — index entry for `09-docs-gap-matrix`.

---

## Still open (honest)

**Superseded for closed items by [10 re-complete](../10-re-complete/re-complete.md)** (sandbox isolation, vault files init, proxy constants/settings, runner.js absent, protocol verbs, cookie send/partitions).

1. Live CDP Send / Runner click — Postman not running in 10 pass (`list_electron_targets` not usable).
2. Exhaustive `epm.*` from unminified `postman-sandbox` (string table improved in 10, still incomplete).
3. Full VariableScope precedence algorithm decompile.
4. postman-cli collection YAML — not extracted.
5. Vault at-rest file crypto under `~/Postman/files` (dir empty; no new Ghidra target).
6. Native dataset engine — P2; Ghidra later if in scope.
