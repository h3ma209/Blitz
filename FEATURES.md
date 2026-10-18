# Blitz — Feature Catalog

**Product:** local-first API client (Rust + Dioxus desktop)  
**Not affiliated with Postman.** Guided by reverse-engineering notes under [`docs/`](docs/).  
**As of:** 18 October 2026 · crate `blitz` 0.1  
**Verify:** `cargo test` (unit + UI-flow smokes; keychain + network cases ignored)

This document lists what the **app actually does today**: shipped behavior, partial limits, and explicit non-goals.

---

## 1. Summary

Blitz is a single-window desktop workbench for building, sending, scripting, organizing, and locally mocking API traffic. Data lives on disk under the platform data directory (`…/blitz/`). Secrets can use the macOS Keychain. Optional **git cloud sync** pushes collections/envs to a GitHub remote (not Postman Cloud). No Agent Mode, Spec Hub, Flows, or billing.

| Area | Maturity |
|------|----------|
| HTTP request / response workbench | Complete for day-to-day REST/GraphQL-over-HTTP; binary body; Docs / Visualize / Timeline tabs |
| Multi-protocol | HTTP, WS, gRPC unary, MQTT, MCP, **Socket.IO**, **SSE**, **GraphQL-WS** |
| Variables, environments, vault | Complete (local scopes) + collection vars editor + env JSON impex |
| Auth | Bearer/Basic/API Key/Digest/OAuth2 + **OAuth1 / Hawk / AWS SigV4 / EdgeGrid**; NTLM Type1 partial |
| Scripts + Collection Runner + Perf VUs + CLI | Expanded `pm.*` / CryptoJS subset + `pm.visualizer`; `blitz run` CLI |
| Capture proxy | HTTP forward + CONNECT tunnel (no MITM decrypt) |
| Collections / history / trash | Tree search, sibling reorder, CRUD/DnD |
| Import / export / snippets / mock | v2.1 + HAR + OpenAPI YAML; Java/Go/C# snippets; mock scenarios |
| Git cloud sync | Optional: save → split tree → commit → push; envs = `env/<slug>` branches |
| AI / Spec Hub / Flows / SSO / Postman Cloud | Out of scope |

---

## 2. Workbench & navigation

### 2.1 Shell layout

- **Top bar:** brand, shortcut hint, **File** menu (collections / env / HAR / OpenAPI), Runner, Perf, Proxy, Cookies, Col vars, Settings, environment selector, variables eye (👁), code snippets (`</>`), Mock start/stop, Trash.
- **Left sidebar:** collections tree (search box) + **collapsible History** (▸/▾; default collapsed, `history_collapsed` in UiSettings); **drag right edge** to resize (180–480px, persisted). Tree starts **fully collapsed** (collections + folders); expand deliberately. Refolds after snapshot load, import, and cloud sync reload (session expand state not persisted).
- **Startup splash:** full-screen Blitz brand overlay until initial snapshot load finishes, then configurable min hold (`splash_min_ms`, default 80) + ~320ms fade (fixed overlay inside `.app-shell`, not a grid child).
- **Env switch overlay:** compact centered card (logo + “Switching environment…” + name) while git checkout / tree reload runs; hides on success or error (status message kept).
- **Center:** multi-tab request strip (hover ✕ closes tab; last tab resets to untitled), request builder, response pane, console — **drag handles** resize request/response split and console height (persisted).
- **Modals:** shared `ModalShell` (title + ✕, scrollable `.modal-body` ≤85vh) — settings, runner, vars, cookies, col vars, import/export, command palette, trash, snippets, perf; env manager overlay also has ✕.

### 2.2 Tabs & shortcuts

| Shortcut | Action |
|----------|--------|
| ⌘/Ctrl + Enter | Send (or **Cancel** while a request is in flight) |
| ⌘/Ctrl + S | Save request into collection |
| ⌘/Ctrl + T | New request tab |
| ⌘/Ctrl + W | Close current tab (neighbor or untitled if last) |
| ⌘/Ctrl + Shift + P | Command palette |
| ⌘/Ctrl + Alt + C | Toggle console |
| ⌘/Ctrl + / | Toggle `//` line comments in body / Scripts / Docs editors |
| ⌘/Ctrl + Shift + / | Toggle `/* … */` block comment around selection |

- **Multi-tab:** open several drafts; switch by tab; `+` opens a new untitled request.
- **Palette:** Send (Cancel while in flight), Save, New, Runner, Console, Settings.

### 2.3 Variables quick-look

Eye pane lists values in scope: local, environment, collection, global, plus vault key count.

---

## 3. Request builder

### 3.1 Basics

- **Protocol selector:** HTTP | WebSocket | gRPC | MQTT | MCP | Socket.IO | SSE | GraphQL-WS.
- Methods (HTTP): GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS.
- URL bar with `{{variable}}` substitution (HTTP); other protocols use URL as endpoint.
- HTTP tabs: **Params**, **Headers**, **Body**, **Auth**, **Scripts**, **Docs**, **Settings**.

### 3.1b Multi-protocol (local)

| Protocol | Actions | Notes / limits |
|----------|---------|----------------|
| WebSocket | Connect / Send / Disconnect | `tokio-tungstenite` |
| gRPC | Invoke unary | Struct JSON; reflection list stub (no .proto) |
| MQTT | Publish / Subscribe once | `rumqttc` |
| MCP | List tools / Call tool | STDIO JSON-RPC |
| Socket.IO | Connect / emit / listen | `rust_socketio` session |
| SSE | Listen (max events) | GET `text/event-stream` |
| GraphQL-WS | Subscribe | `graphql-transport-ws` / legacy start |

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
| binary | Local file path → raw bytes (`application/octet-stream`) |

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
| OAuth 1.0 | HMAC-SHA1 / HMAC-SHA256 / PLAINTEXT |
| Hawk | HMAC header (sha256/sha1) |
| AWS Signature | SigV4 (region + service) |
| Akamai EdgeGrid | EG1-HMAC-SHA256 |
| NTLM | **Partial** — Type1 negotiate only |

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
`{{var}}` / `{{vault:key}}` spans get orange accent highlight (unresolved → red) in URL, header/param values, raw body, and GraphQL editors.  
Hover / caret on `{{var}}` in URL (and header/param values) → popover with resolved value + in-place edit (updates owning scope). Dismisses ~200ms after pointer leaves token/input/popover (grace so user can enter popover); stays open while hovering popover or focusing the value input; Escape / Save / blur-after-leave still close.

### 5.2 Environments UI

- Defaults: **Local**, **Development**, **Production** (seeded; extras kept)  
- Vars with **initial** + **current** values  
- Top-right: env dropdown + **Sync** chip (hidden when **No Environment** or **Local**; first Sync enables cloud + canonical remote). `VariableContext.local` never synced.  
- File → Import / Export Postman environment JSON  
- Persisted in snapshot  
- **Script persistence:** `pm.environment.set` / `.unset` from pre-request or tests write through to the **active** environment’s `values` (current) and `persist()` after Send / Collection Runner. No active environment → sets ignored with console/status: `no active environment — pm.environment.set ignored` (not silently globalized).  

### 5.2b Collection variables

- **Col vars** toolbar modal: **Active** tab (editable key/value rows for current collection) + **All collections** accordion (browse/edit every collection)  
- Synced into `VariableContext.collection` when active collection changes or its rows are edited  
- Imported/exported with collection v2.1 `variable[]`  
- Persisted via snapshot on Done / normal save paths  
- `pm.collectionVariables.set` / `.unset` from scripts also write back to the active collection’s `variables` and persist  

**Initiate Envs / SNB:** open `1.Initiate Envs` → request **Local** or **SNB** → select an environment (e.g. Local) → **Send**. Prerequest/test scripts seed `endpoint_*` into that env; collection `variable[]` (`basic_auth_*`) must be imported (bundled export includes them). There is no separate “run scripts only” path — Send (or Collection Runner) runs them.

### 5.3 Vault

- File: `{data_dir}/blitz/vault.json`  
- Also mirrored via secrets service `Blitz (vault)` when using Keychain path  
- Syntax: `{{vault:key}}`  
- Scripts: `pm.vault.get/set/unset`  

### 5.4 OS secrets

- macOS: Keychain via `keyring` (service pattern `Blitz ({id})`, account `userKey` for crypto-style keys)  
- CI / fallback: `EVILPOSTMAN_SECRETS=memory`  

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
| `pm.variables` / `pm.environment` / `pm.collectionVariables` / `pm.globals` | `.get` / `.set` / `.unset` / `.has` / `.toObject` / `.clear` |
| `pm.variables.replaceIn(str)` | `{{var}}` from local → env → collection → globals → iterationData → vault |
| `pm.vault` | `.get` / `.set` / `.unset` |
| `pm.cookies` | `.get` / `.set` / `.clear` (jar sync on send path) |
| `pm.request` | `.url` (+ `.getPath()`), `.method`, `.headers.get/upsert/add/remove`, `.body.mode` / `.body.raw` — upserts applied to outgoing request |
| `pm.info` | `.eventName`, `.requestName`, `.iteration` (stubs) |
| `pm.iterationData` | Object from runner data row |
| `pm.execution.setNextRequest` / `skipRequest` | Runner control |
| `pm.response` | `.code`, `.status`, `.responseTime`, `.text()`, `.json()`, `.headers.get`, `.to.have.status`, `.to.have.jsonBody` |
| `pm.test` / `pm.expect` | equal/eql/include/be.ok/above/below/a(type)/have.status/have.property(+value)/not.* |
| `pm.sendRequest` | Sync nested HTTP via blocking reqwest + callback (pre + tests) |
| `pm.visualizer.set(template, data)` | → Visualize tab (simple `{{key}}` / `{{data}}` replacement) |
| `console.log` / `warn` / `error` / `info` | → Console panel |
| `CryptoJS.SHA256` / `HmacSHA256` | Host-backed Hex / Base64 (Hakki OTP signing) |
| `require` | Stub: `crypto-js` → CryptoJS; else `{}` + warn (no npm) |

### 6.4 Not supported (yet)

- Full Chai / chai-postman deep `jsonBody` paths / async tests  
- Arbitrary `require('lodash')` / package library  
- Exact Postman sandbox isolation (uniscope/uvm)  
- Full `pm.request.url` Url SDK / `pm.execution.runRequest` / cookie jar API object 

### 6.5 Test Results / Visualize / Timeline

Response pane: PASS/FAIL, Visualize HTML source, Timeline bars (DNS/connect/TLS/TTFB/download/total — approximated).

---

## 7. Send pipeline

On Send (simplified):

1. Inherited + request pre-request scripts (mutates shared `VariableContext`)  
2. Resolve `{{…}}` / vault / `secret://`  
3. Inherit auth → inject auth (or Digest challenge later)  
4. Attach cookies from jar  
5. Build body (raw / urlencoded / multipart / GraphQL)  
6. Apply proxy + TLS + timeout + optional PEM client cert  
7. HTTP via **reqwest** (rustls; auto `gzip`/`deflate`/`br` negotiate + decompress). Does **not** inject manual `Accept-Encoding` (that disables reqwest decompress). Safety net: if `Content-Encoding` / gzip magic still present, inflate before body text (`encoding: decompressed …` in pipeline log).  
8. Store Set-Cookie; run post scripts; update Test Results / Console / History  
9. Apply script env/collection maps back to active `Environment` / `Collection` + `persist()`

While sending, the primary button becomes **Cancel**. Cancel sets a cooperative abort flag (HTTP runs on a worker thread polled every 50ms); UI returns to idle immediately and ignores late results. Status shows `CANCELLED`.

---

## 8. Response viewer

| Tab / control | Behavior |
|---------------|----------|
| Status / time / size | Shown above body |
| Pretty | JSON pretty-print + syntax highlight (keys/strings/numbers/bools/null) when Content-Type or body is JSON; plain text stays mono |
| Raw | Exact body |
| Preview | HTML-oriented display (source when HTML-like) |
| Headers | Response headers list |
| Cookies | `Set-Cookie` headers from this response |
| Test Results | Script assertions |
| Visualize | `pm.visualizer` HTML or HTML response body |
| Timeline | Approximated phase bars |
| **Save example** | Stores response on the request for local mocks |

---

## 9. Collections & organization

- Tree: collections → folders → requests (expand/collapse via ▸/▾); **default all closed**  
- Kind icons: folder silhouette (amber), collection box (green); requests keep method badges (GET/POST…)  


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
- Drop onto a **request** → sibling reorder (`sort_key`)  
- Reject drop into self / descendant folder  
- Drop target highlighted in accent orange  
- Sidebar **search** filters tree by name (keeps ancestors)

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

### 12c. Capture proxy / interceptor limits

- **Proxy** top-bar: bind `127.0.0.1:ephemeral`  
- HTTP absolute-form forward + log  
- **CONNECT** = blind TLS tunnel (no decrypt; no local CA install)  
- Browser Interceptor bridge not shipped — point HTTP proxy here for plaintext only  

### 12d. CLI (Newman-like)

```
blitz run collection.json [--env environment.json]
blitz help
```

(No args → GUI.)

---

## 13. Local mock server

1. Send a request → **Save example**  
2. Click **Mock** → binds `http://127.0.0.1:{ephemeralPort}`  
3. Matches **method + path** (simple `:param` wildcards)  
4. Optional **scenarios:** `mock_match_header` (`Name: value`) and/or `mock_match_name` on draft filters examples  
5. **Stop mock** tears down the listener  

No cloud mock URL.

---

## 14. Import / export

| Format | Direction | Notes |
|--------|-----------|--------|
| Postman Collection **v2.1** JSON | Import + export | Folders, scripts, auth/bodies, **collection variables**, descriptions |
| Postman **environment** JSON | Import + export | File menu |
| **HAR** 1.2 | Import | One request per `log.entries` |
| **cURL** paste | Import | Detects `curl` → one-request collection |
| **OpenAPI 3** JSON **or YAML** | Import | One request per path+method; `servers[0].url` |

**File ▾:** Import Postman export / env / HAR / OpenAPI, paste, bundled samples, export collection / env.  

**Default seed:** `exported-postman-jsons/` (Hakki + Initiate Envs).

**Not imported:** gRPC, RAML/WSDL, collection v2.0, rich `$ref` graphs.

---

## 15. Code snippets

Languages (from `</>` modal):

- cURL  
- JavaScript `fetch`  
- Python `requests`  
- **Java OkHttp**  
- **Go net/http**  
- **C# HttpClient**  

Generated from the current draft (URL, method, headers, auth, body).

---

## 16. Cookies

- Persisted jar + **Cookies** toolbar manager (edit/delete/add/clear)  
- Set-Cookie on responses → jar  
- Cookie header attached on matching host/path  
- Response Cookies tab shows Set-Cookie for the last response  

---

## 17. App settings

- **Proxy:** env (`HTTP(S)_PROXY`), none, or custom `host:port`  
- **Client certificate:** PEM, or **PKCS#12** via `openssl pkcs12` conversion when openssl is on PATH  
- **Appearance** (Settings → Appearance; Reset restores defaults):  
  - UI / editor font sizes, sidebar width, request pane height, console height — CSS vars on `.app-shell` / `.workbench` (`--fs-base`, `--fs-editor`, `--sidebar-w`, `--request-h`, `--console-h`)  
  - **Density** comfortable / compact — scales `--space-*` (+ shorter top bar when compact)  
  - **Wrap editor text** — `white-space` on `.body-editor` / `.response-body`  
  - **Tab size** 2 / 4 — `--editor-tab-size`  
  - **Accent intensity** soft / normal / strong — `--accent-soft` / `--bg-active` (green theme kept)  
  - **Reduce motion** — kills animations / transitions on shell  
  - **Show status bar** — toggles workbench `.status-line`  
  - **Default response tab** Pretty / Raw / Headers  
  - **Splash min duration** — hold before startup fade (0–2000ms)  
- Workbench drag handles update `request_pane_height_px` / `console_height_px` (clamped); never mount drag UI as bare `.app-shell` grid children  
- Class tokens via `UiSettings::shell_class_tokens()` on `.app-shell`; persisted in snapshot  

---

## 18. Persistence

| Artifact | Location (typical) |
|----------|--------------------|
| Snapshot (collections, envs, history, cookies, settings, trash, selection) | `{data_dir}/blitz/snapshot.json` |
| Vault | `{data_dir}/blitz/vault.json` |
| Keychain secrets | macOS Keychain (when enabled) |

Demo collection/env seeded when snapshot empty.

---

## 18b. Git cloud sync (optional)

Top-nav **Cloud** menu + Settings → **Git cloud sync**. Parallel to local `snapshot.json` (local-first offline still works).

| Item | Behavior |
|------|----------|
| Canonical remote | `https://github.com/h3ma209/blitz-cloud.git` (overridable) |
| Clone path | `{data_dir}/blitz/cloud/` (or custom / Link folder) |
| On-disk layout | `.blitz/project.json`, `collections/<slug>/` (one `*.request.json` per request), `environments/*.environment.json` |
| UI | **Cloud** → Clone / Link folder / Status / Pull / Push / Open folder; **Sync** beside env (hidden if no env or **Local**) |
| Save | Local `snapshot.json`; if cloud on + linked clone + active env → export → **commit only** (never push) |
| Sync | Checkout `env/<slug>` → commit pending → `pull --rebase` → import → **push** (never force). Primary team share. |
| Pull / Push | Advanced Cloud-menu actions; no auto-pull interval |
| Environments | Defaults Local/Development/Production → branches `env/local` etc. |
| Commit message | Auto `{UTC} A file.json M other.json` (or `update N files`); Settings override if non-empty |
| Secrets / local vars | Redacted secrets in git; vault / cookies / history / tabs / **`VariableContext.local`** stay in local `snapshot.json` only |
| Auth | System `git` + SSH agent or `gh auth login` / credential helper — no force-push |
| Conflicts | Status message; resolve in clone then Sync / Pull again |

Design: [`docs/14-cloud-sync-re/git-team-sync-design.md`](docs/14-cloud-sync-re/git-team-sync-design.md).

---

## 19. Explicit non-goals (not in the app)

These appear in Postman docs / RE reports but are **not** implemented:

- Postman Cloud sync (Bifrost), workspaces, roles, SSO, SCIM, billing  
- Agent Mode / AI requests / Passport / Fabric  
- Spec Hub, Flows, cloud Monitors, API Catalog / Insights  
- Full conflict-resolution UI / PR create from app  
- Full gRPC `.proto` / tonic reflection + client/bidi streaming  
- HTTPS MITM with installed local CA / Browser Interceptor bridge  
- Package library / exact uvm sandbox / ASAP auth  
- Themes, bulk edit, scheduled runs  

---

## 20. Module map (for contributors)

| Module | Responsibility |
|--------|----------------|
| `src/ui/*` | Dioxus workbench |
| `src/core/request.rs` | Draft, protocol, body, auth, examples |
| `src/core/protocols/*` | WS / gRPC / MQTT / MCP / Socket.IO / SSE / GraphQL-WS |
| `src/core/capture_proxy.rs` | Local HTTP + CONNECT tunnel proxy |
| `src/core/cli_runner.rs` | `run` CLI |
| `src/core/auth_extra.rs` | OAuth1 / Hawk / AWS / EdgeGrid / NTLM |
| `src/core/pkcs12_util.rs` | PKCS#12 → PEM |
| `src/core/perf_runner.rs` | Concurrent VU runner |
| `src/core/collection.rs` | Tree, search filter, reorder, trash |
| `src/core/variables.rs` / `environment.rs` / `vault.rs` | Scopes + env JSON |
| `src/core/sandbox.rs` / `scripts.rs` | JS `pm` (+ visualizer) |
| `src/core/oauth.rs` / `digest.rs` | OAuth2 / Digest |
| `src/core/impex.rs` / `codegen.rs` / `mock_server.rs` | Import, snippets, mock |
| `src/runtime/pipeline.rs` | Send path |
| `src/runtime/runner.rs` | Collection runs |
| `src/secrets/*` | Keychain / memory |
| `src/storage/json_store.rs` | Snapshot I/O |
| `src/core/cloud_sync/` | Git tree export + shell `git` push / env branches |
| `docs/` | RE evidence (not runtime) |

---

## 21. Related docs

- [README.md](README.md) — build / quick try  
- [docs/README.md](docs/README.md) — RE index  
- [docs/11-docs-gap-refresh/gap-refresh.md](docs/11-docs-gap-refresh/gap-refresh.md) — docs × RE × app matrix  
- [docs/12-contents-grab/inventory.md](docs/12-contents-grab/inventory.md) — Contents / ASAR grab inventory  
- [docs/12-contents-grab/app-shipped.md](docs/12-contents-grab/app-shipped.md) — Phase B ship checklist  
- [docs/13-remaining-gaps/remaining-gaps.md](docs/13-remaining-gaps/remaining-gaps.md) — shipped vs deferred  

---

*End of feature catalog.*
