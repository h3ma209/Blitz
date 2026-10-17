# notPostman — Feature Catalog

**Product:** local-first API client (Rust + Dioxus desktop)  
**Not affiliated with Postman.** Guided by reverse-engineering notes under [`reports/`](reports/).  
**As of:** 17 October 2026 · crate `not_postman` 0.1  
**Verify:** `cargo test` (unit + UI-flow smokes; keychain + network cases ignored)

This document lists what the **app actually does today**: shipped behavior, partial limits, and explicit non-goals.

---

## 1. Summary

notPostman is a single-window desktop workbench for building, sending, scripting, organizing, and locally mocking API traffic. Data lives on disk under the platform data directory (`…/notpostman/`). Secrets can use the macOS Keychain. There is **no** Postman Cloud account, sync, Agent Mode, Spec Hub, Flows, or billing.

| Area | Maturity |
|------|----------|
| HTTP request / response workbench | Complete for day-to-day REST/GraphQL-over-HTTP; Postman Runtime default headers + JSON comment strip |
| WebSocket / gRPC unary / MQTT / MCP STDIO | Minimal local clients (protocol selector) |
| Variables, environments, vault | Complete (local scopes) |
| Auth (incl. OAuth2 + Digest) | Complete for common types |
| Scripts + Collection Runner + Perf VUs | Complete / minimal for focused `pm` + VUs |
| Capture proxy | Lightweight HTTP absolute-form logger |
| Collections / history / trash | Complete for local tree ops |
| Import / export / snippets / mock | Complete for happy-path use |
| Cloud / AI / Socket.IO / Interceptor | Out of scope |

---

## 2. Workbench & navigation

### 2.1 Shell layout

- **Top bar:** brand, shortcut hint, **File** menu (Import from Postman export / paste / bundled samples / export), Runner, Perf, Proxy, Settings, environment selector, variables eye (👁), code snippets (`</>`), Mock start/stop, Trash.
- **Left sidebar:** collections tree + history.
- **Center:** multi-tab request strip, request builder, response pane, console.
- **Modals:** settings, runner, env manager, import/export, command palette, trash, snippets.

### 2.2 Tabs & shortcuts

| Shortcut | Action |
|----------|--------|
| ⌘/Ctrl + Enter | Send |
| ⌘/Ctrl + S | Save request into collection |
| ⌘/Ctrl + T | New request tab |
| ⌘/Ctrl + W | Close / reset current tab |
| ⌘/Ctrl + Shift + P | Command palette |
| ⌘/Ctrl + Alt + C | Toggle console |

- **Multi-tab:** open several drafts; switch by tab; `+` opens a new untitled request.
- **Palette:** Send, Save, New, Runner, Console, Settings.

### 2.3 Variables quick-look

Eye pane lists values in scope: local, environment, collection, global, plus vault key count.

---

## 3. Request builder

### 3.1 Basics

- **Protocol selector:** HTTP | WebSocket | gRPC | MQTT | MCP (draft fields persisted in snapshot).
- Methods (HTTP): GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS.
- URL bar with `{{variable}}` substitution (HTTP); WS/MQTT/gRPC use URL as endpoint.
- HTTP tabs: **Params**, **Headers**, **Body**, **Auth**, **Scripts**, **Settings**.

### 3.1b Multi-protocol (local)

| Protocol | Actions | Notes / limits |
|----------|---------|----------------|
| WebSocket | Connect / Send message / Disconnect | `tokio-tungstenite`; event log in protocol pane |
| gRPC | Invoke unary | JSON → `google.protobuf.Struct` over HTTP/2; server must accept Struct (or raw b64 fallback) |
| MQTT | Publish / Subscribe once | `rumqttc`; `mqtt://host:port` |
| MCP | List tools / Call tool | STDIO JSON-RPC; command + args |

### 3.2 Params & headers

- Key/value rows with enable/disable.
- Query params appended to URL on send.
- Headers merged into the outgoing request (Content-Type auto-set for some body modes).

### 3.3 Body modes

| Mode | Behavior |
|------|----------|
| none | No body |
| raw | Free text (JSON/text/etc.) |
| x-www-form-urlencoded | Key/value → encoded body |
| form-data | Multipart; text fields, or **file** when “File?” + local path |
| GraphQL | Query + variables JSON → `{"query","variables"}` over HTTP (forces POST if method was GET) |

### 3.4 Per-request settings

- Timeout (ms; 0 = none)
- Follow redirects on/off
- SSL certificate verify on/off (insecure when off)

---

## 4. Authorization

| Type | Notes |
|------|--------|
| No Auth | — |
| Inherit | Uses collection-level auth when set |
| Bearer | `Authorization: Bearer …` |
| Basic | Username/password → Basic header |
| API Key | Custom header name (default `X-Api-Key`) |
| Digest | RFC 2617 MD5 / `qop=auth` after 401 challenge |
| OAuth 2.0 | **Client credentials** or **authorization code** (loopback redirect, “Get New Access Token”); token injected as Bearer |

Secret references in auth fields:

- `secret://{id}` / `secret://{id}/{account}` → Keychain / memory secrets store  
- `{{vault:key}}` → local vault file / vault keychain service  

---

## 5. Variables, environments, vault

### 5.1 Scopes (narrow wins)

1. Local (scripts / runner iteration data)  
2. Environment (active env **current** values)  
3. Collection  
4. Global  

Substitution: `{{name}}` in URL, params, headers, body, auth fields.

### 5.2 Environments UI

- List / create environments  
- Vars with **initial** + **current** values  
- Activate from top-right selector  
- Persisted in snapshot  

### 5.3 Vault

- File: `{data_dir}/notpostman/vault.json`  
- Also mirrored via secrets service `notPostman (vault)` when using Keychain path  
- Syntax: `{{vault:key}}`  
- Scripts: `pm.vault.get/set/unset`  

### 5.4 OS secrets

- macOS: Keychain via `keyring` (service pattern `notPostman ({id})`, account `userKey` for crypto-style keys)  
- CI / fallback: `NOTPOSTMAN_SECRETS=memory`  

---

## 6. Scripts & testing

### 6.1 Where scripts live

- Per-request: pre-request + post-response (Tests)  
- Per-collection and per-folder: pre + post (runner / inherited send path)  
- **Order (pre):** collection → folder(s) → request  
- **Order (post):** request → folder(s) reverse → collection  

### 6.2 Engine

JavaScript via **`boa_engine`** (not full Postman sandbox / uvm).

### 6.3 Supported `pm` surface

| API | Support |
|-----|---------|
| `pm.variables` / `pm.environment` / `pm.collectionVariables` / `pm.globals` | `.get` / `.set` / `.unset` |
| `pm.vault` | `.get` / `.set` / `.unset` |
| `pm.cookies` | `.get` (name → value) |
| `pm.request` | `.url`, `.method` (read) |
| `pm.iterationData` | Object from runner data row |
| `pm.execution.setNextRequest` / `skipRequest` | Runner control |
| `pm.response` | `.code`, `.status`, `.text()`, `.json()`, `.headers.get`, `.to.have.status` |
| `pm.test` / `pm.expect` | equal/eql/include/be.ok/have.status/have.property |
| `pm.sendRequest` | Sync nested HTTP via blocking reqwest + callback |
| `console.log` | → Console panel |

### 6.4 Not supported (yet)

- Full Chai / async tests / `require` / package library  
- Exact Postman sandbox isolation (uniscope/uvm)  

### 6.5 Test Results

Response pane shows PASS/FAIL per `pm.test`. Runner aggregates pass/fail per request × iteration.

---

## 7. Send pipeline

On Send (simplified):

1. Inherited + request pre-request scripts  
2. Resolve `{{…}}` / vault / `secret://`  
3. Inherit auth → inject auth (or Digest challenge later)  
4. Attach cookies from jar  
5. Build body (raw / urlencoded / multipart / GraphQL)  
6. Apply proxy + TLS + timeout + optional PEM client cert  
7. HTTP via **reqwest** (rustls)  
8. Store Set-Cookie; run post scripts; update Test Results / Console / History  

---

## 8. Response viewer

| Tab / control | Behavior |
|---------------|----------|
| Status / time / size | Shown above body |
| Pretty | JSON pretty-print when possible |
| Raw | Exact body |
| Preview | HTML-oriented display (source when HTML-like) |
| Headers | Response headers list |
| Cookies | `Set-Cookie` headers from this response |
| Test Results | Script assertions |
| **Save example** | Stores response on the request for local mocks |

---

## 9. Collections & organization

- Tree: collections → folders → requests (expand/collapse via ▸/▾)  
- Create collection / folder / request (folder or collection as parent)  
- Rename, delete, **Duplicate** (collection / folder / request)  
- **Favorite** (★), **Sort** (favorites first, folders before requests)  
- Save current draft into active collection (updates in-tree request)  
- Collection-level auth + scripts  
- Folder-level scripts  

### Context menus (right-click)

| Target | Items |
|--------|--------|
| Empty sidebar / list | New Collection |
| Collection | New Request, New Folder, Rename, Duplicate, Favorite, Export, Delete |
| Folder | New Request, New Folder, Rename, Duplicate, Delete |
| Request | Open, Rename, Duplicate, Copy ID, Delete |

Dismiss: click outside or Escape. Hover row icons still available as shortcuts.

### Drag and drop

- Drag **request** or **folder** onto a **collection** or **folder** → reparent (persisted)  
- Reject drop into self / descendant folder  
- Drop target highlighted in accent orange  
- Sibling reorder not modeled (no sort_key); reparent-only  

### Trash

- Deleting a **collection** soft-moves it to Trash (persisted)  
- Restore or **Delete forever** from Trash modal  
- Nested request/folder delete is hard-remove from tree  

---

## 10. History

- Every send logged (method, URL, status, time)  
- Click to reopen draft  
- Save history item into a collection  

---

## 11. Console

- Footer / toggle panel  
- Pipeline stages, `console.log` from scripts, errors  
- Clear  

---

## 12. Collection Runner

- Pick collection  
- **Iterations**, **delay (ms)**, optional **data file** (CSV with header row, or JSON array of objects)  
- Data row → `pm.iterationData` + local vars  
- Honors `setNextRequest` / `skipRequest`  
- Results table: iteration, pass/fail, method, name, status, time  
- Shared environment + cookie jar across the run  

### 12b. Performance (minimal)

- **Perf** modal: N concurrent VUs × duration (seconds) against active collection  
- Reuses HTTP send pipeline; shows total requests, errors, avg/min/max latency  

### 12c. Capture proxy

- **Proxy** top-bar: bind `127.0.0.1:ephemeral` HTTP forward proxy  
- Logs absolute-form requests into history on stop  
- **No** CONNECT / TLS MITM (browser Interceptor not shipped)  

---

## 13. Local mock server

1. Send a request → **Save example**  
2. Click **Mock** → binds `http://127.0.0.1:{ephemeralPort}`  
3. Matches **method + path** (simple `:param` wildcards) to first saved example  
4. **Stop mock** tears down the listener  

No cloud mock URL; no scenario headers UI yet.

---

## 14. Import / export

| Format | Direction | Notes |
|--------|-----------|--------|
| Postman Collection **v2.1** JSON | Import + export | HTTP subset (folders, scripts, common auth/bodies) |
| **cURL** paste | Import | Detects `curl` → one-request collection |
| **OpenAPI 3** JSON | Import | One request per path+method; uses `servers[0].url` when present |

**File ▾** menu: *Import from Postman export…* (native multi-file picker), *Paste Postman / cURL / OpenAPI…*, *Load bundled sample exports*, *Export active collection…*. Sidebar ↓ / ↑ still work.  

**Default seed:** first launch / empty snapshot loads collections from `exported-postman-jsons/` (Hakki + Initiate Envs), embedded via `bundled_postman_collections()`.

**Not imported:** gRPC, full OAuth token dances from file, rich docs, collection v3 YAML.

---

## 15. Code snippets

Languages (from `</>` modal):

- cURL  
- JavaScript `fetch`  
- Python `requests`  

Generated from the current draft (URL, method, headers, auth, body).

---

## 16. Cookies

- In-memory / persisted jar  
- Set-Cookie on responses → jar  
- Cookie header attached on later requests to matching host/path  
- Response Cookies tab shows Set-Cookie for the last response  

---

## 17. App settings

- **Proxy:** env (`HTTP(S)_PROXY`), none, or custom `host:port`  
- **Client certificate:** PEM identity path (cert+key); PKCS#12 paths get a clear “convert to PEM” error  
- Persisted in snapshot  

---

## 18. Persistence

| Artifact | Location (typical) |
|----------|--------------------|
| Snapshot (collections, envs, history, cookies, settings, trash, selection) | `{data_dir}/notpostman/snapshot.json` |
| Vault | `{data_dir}/notpostman/vault.json` |
| Keychain secrets | macOS Keychain (when enabled) |

Demo collection/env seeded when snapshot empty.

---

## 19. Explicit non-goals (not in the app)

These appear in Postman docs / RE reports but are **not** implemented:

- Postman Cloud sync, workspaces, roles, SSO, SCIM, billing  
- Agent Mode / AI requests / Passport / Fabric  
- Spec Hub, Flows, cloud Monitors, API Catalog / Insights  
- Native Git (Local/Cloud views)  
- Socket.IO client / GraphQL subscriptions / full protoset gRPC reflection UI  
- Browser Interceptor / CONNECT TLS MITM  
- Newman / Postman CLI clone  
- Package library / exact uvm sandbox  

---

## 20. Module map (for contributors)

| Module | Responsibility |
|--------|----------------|
| `src/ui/*` | Dioxus workbench |
| `src/core/request.rs` | Draft, protocol, body, auth, examples |
| `src/core/protocols/*` | WS / gRPC unary / MQTT / MCP |
| `src/core/capture_proxy.rs` | Local HTTP capture proxy |
| `src/core/perf_runner.rs` | Concurrent VU runner |
| `src/core/collection.rs` | Tree, favorite, trash helpers |
| `src/core/variables.rs` / `environment.rs` / `vault.rs` | Scopes |
| `src/core/sandbox.rs` / `scripts.rs` | JS `pm` (+ sendRequest) |
| `src/core/oauth.rs` / `digest.rs` | Auth helpers |
| `src/core/impex.rs` / `codegen.rs` / `mock_server.rs` | Import, snippets, mock |
| `src/runtime/pipeline.rs` | Send path |
| `src/runtime/runner.rs` | Collection runs |
| `src/secrets/*` | Keychain / memory |
| `src/storage/json_store.rs` | Snapshot I/O |
| `reports/` | RE evidence (not runtime) |

---

## 21. Related docs

- [README.md](README.md) — build / quick try  
- [reports/README.md](reports/README.md) — RE index  
- [reports/11-docs-gap-refresh/gap-refresh.md](reports/11-docs-gap-refresh/gap-refresh.md) — docs × RE × app matrix  
- [reports/12-contents-grab/inventory.md](reports/12-contents-grab/inventory.md) — Contents / ASAR grab inventory  
- [reports/12-contents-grab/app-shipped.md](reports/12-contents-grab/app-shipped.md) — Phase B ship checklist  

---

*End of feature catalog.*
