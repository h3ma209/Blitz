# 09 — Coverage gap matrix (Postman Docs v12 map vs RE vs notPostman)

**Purpose:** Feature checklist only — docs map used for completeness, not as copyrighted text.  
**Sources:** `reports/01`–`08`, `README.md`, `src/{core,runtime,ui,storage,secrets}`.  
**Date:** 2026-10-17 · Target Postman Desktop **12.32.3** · notPostman **0.1**.

### Status keys

| Column | Values |
|--------|--------|
| **RE** | `covered` = architecture / modules / deps observed enough to recreate; `partial` = paths/deps seen, semantics incomplete; `missing` = docs feature, RE never looked or no hit; `N/A` = cloud-only / SaaS / out of local-first scope |
| **App** | `implemented` = works end-to-end; `stub` = UI/model/log only; `missing` = absent |
| **Pri** | `P0` core HTTP client · `P1` parity essentials · `P2` advanced · `P3` enterprise/cloud/AI — defer |

### High-level counts (feature rows below)

| RE status | Count (approx) | App status | Count (approx) |
|-----------|----------------|------------|----------------|
| covered | ~12 | implemented | ~10 |
| partial | ~28 | stub | ~12 |
| missing | ~22 | missing | ~40 |
| N/A | ~18 | — | — |

Rows are checklist items (not unique product modules). Many N/A rows are intentional deferrals.

---

## A. Get started / onboarding

| Feature area | RE | App | Pri | Notes |
|--------------|----|-----|-----|-------|
| Install / first-run / account signup | N/A | missing | P3 | Cloud identity URLs in `config.json` (`reports/04`). Local app: no onboarding. |
| Scratchpad / offline mode | partial | missing | P2 | `html/scratchpad.html`, `desktop-offline.html` (`reports/01`, `02`). App is always local-first; no scratchpad shell. |
| Learning / in-app tours | N/A | missing | P3 | Not RE’d; cloud/UI CDN. |

---

## B. Use Postman — requests (HTTP)

| Feature area | RE | App | Pri | Notes |
|--------------|----|-----|-----|-------|
| Method + URL + Send | partial | implemented | P0 | RE: pipeline inferred (`reports/03`, `07`); live Send→HTTP unrecovered (`08`). App: `src/ui/request_builder.rs` + `runtime/pipeline.rs`. |
| Query params | missing | implemented | P0 | RE never isolated UI/runtime param model. App: `RequestDraft.params`, `build_url`. |
| Headers | missing | implemented | P0 | Same. App: `headers` + `build_headers`. |
| Body: raw | missing | implemented | P0 | App: `BodyMode::Raw`. RE: inside `postman-runtime` (not decompiled). |
| Body: form-data / urlencoded | missing | stub | P1 | Comment in `core/request.rs` (`BodyMode`); UI none. |
| Body: binary / GraphQL body mode | missing | missing | P1–P2 | GraphQL also under protocols. |
| Cookies / cookie jar | missing | missing | P1 | RE never looked. Electron cookie fuse off (`05`) ≠ jar UX. |
| Redirects / timeout / SSL verify settings | partial | missing | P1 | Inferred in `07` settings blob; `process-manager` proxy events (`03`). App: fixed 30s timeout, no UI. |
| Request history | missing | stub | P1 | Sidebar “History — not wired” (`ui/sidebar.rs`). RE never recovered history store. |
| Multi-tab requests | missing | stub | P1 | Workbench `+ (stub)` tab (`ui/workbench.rs`). RE: remote `requester.js` (`02`, `08`). |
| Keyboard shortcuts | missing | missing | P2 | RE never looked (remote UI). |
| Code snippet generators | missing | missing | P2 | Docs Publish/CLI adjacent; RE never looked. |

---

## C. Use Postman — protocols (non-HTTP)

| Feature area | RE | App | Pri | Notes |
|--------------|----|-----|-----|-------|
| GraphQL | partial | missing | P1 | `package.json` stacks + body mode; RE no client walk (`01`). |
| gRPC | partial | missing | P2 | Dep `@postman/grpc-js` (`01`). No RE of stubs/streaming. |
| WebSocket | partial | missing | P2 | Deps `ws`, `postman-ws` (`01`). |
| MQTT | partial | missing | P2 | Dep `mqtt` (`01`). |
| MCP (Model Context Protocol) | partial | missing | P3 | `mcp-server-*.min.js`, `toolset-mcp-server/`, `@modelcontextprotocol/sdk` (`01`, `04`). |
| Socket.IO / SSE (if in map) | missing | missing | P2 | RE never looked beyond package hints. |

---

## D. Use Postman — collections & variables

| Feature area | RE | App | Pri | Notes |
|--------------|----|-----|-----|-------|
| Collections / folders / requests tree | partial | implemented | P0 | RE: `postman-collection` dep + scratchpad `CollectionSidebar` (`04`). App: `core/collection.rs` + sidebar CRUD. |
| Collection format import/export (v2.1 JSON) | missing | missing | P1 | Schema inferred only (`04`). No importer. |
| Collection format v3 YAML | missing | missing | P1 | **RE never looked**; docs imply — high-value RE target. |
| Environments (switch / edit) | partial | stub | P0–P1 | App: env name selector + single `VariableContext` (`env_selector`, `storage`). No multi-env maps. RE: storage unknown (`04`). |
| `{{var}}` resolution (local > env > collection > global) | partial | implemented | P0 | RE: not recovered as named fn (`03`); convention in `07`. App: `core/variables.rs` + tests. |
| Dynamic / data variables | missing | missing | P2 | Datasets engine archive in bundle (`01`); RE shallow (`04`). |

---

## E. Use Postman — Native Git / Agent / Context / Vault / capture

| Feature area | RE | App | Pri | Notes |
|--------------|----|-----|-----|-------|
| Native Git | partial | missing | P2 | `main-git-ipc-handlers`, `services_git_GitResolver` (`01`, `04`). |
| Agent Mode (AI) | partial | missing | P3 | Large `AgentWorker.js` (~10.8 MB); heap exhausted (`01`, `03`). Defer. |
| Context Graph | missing | missing | P3 | RE never looked; likely cloud/UI. |
| Postman Vault (`{{vault:}}`) | partial | stub | P1 | Azure Vault config + `VaultIntegrationService` / `VaultScriptService` in IPCAgent chunk (`05`, `08`). App: `secret://` only (`secrets/`). |
| 1Password / Azure vault backends | partial | missing | P2–P3 | Deps observed (`05`); not wired in app. |
| OS keychain (CryptoService pattern) | covered | implemented | P0 | Ghidra keytar + CryptoService (`05`, `08`). App: `secrets/keychain.rs` + `secret://`. |
| Capture / interceptor / proxy traffic | partial | missing | P2 | Interceptor bridge, Chrome ext id (`04`); `ProxyNodeProcess` (`03`). |
| Webhook tunnel / capture replay | partial | missing | P2 | `main-webhooks-tunnel-forwarding` + tunnel-liaison URL (`01`, `04`). |

---

## F. Test APIs

| Feature area | RE | App | Pri | Notes |
|--------------|----|-----|-----|-------|
| Pre-request scripts | partial | stub | P1 | Sandbox HTML + runtime inferred (`03`, `08`). App: script fields, not executed (`pipeline.rs`). |
| Test scripts / assertions | partial | stub | P1 | Same. |
| Sandbox `pm.*` API surface | missing | missing | P1 | Explicit gap (`03`, `04`, `08`). **Top RE target.** |
| Collection Runner | partial | missing | P1 | `html/runner.html` (`01`, `04`); boundary vs perf unclear. |
| Datasets / data files | partial | missing | P2 | Dataset daemon IPC + DuckDB + `postman-dataset-engine-*.tar.br` (`01`, `04`). |
| Performance / load tests | partial | missing | P2 | `perftest*`, `ThreadGroupExecutable`, `process-manager`, perf IPC (`01`, `03`). |
| Monitors (scheduled cloud) | N/A | missing | P3 | Cloud product; no local RE focus. |
| Visualizer | partial | missing | P2 | `visualizer_view_preload.js` (`01`). |

---

## G. Design APIs

| Feature area | RE | App | Pri | Notes |
|--------------|----|-----|-----|-------|
| Mock servers | partial | missing | P2 | `mock-server-runtime/`, simulation-http worker (`04`). |
| Spec Hub / OpenAPI sync | missing | missing | P3 | RE never looked; largely cloud. |
| Simulations | partial | missing | P2–P3 | Same mock-server tree; analysis truncated (`04`). |

---

## H. Collaborate

| Feature area | RE | App | Pri | Notes |
|--------------|----|-----|-----|-------|
| Workspaces / team sync | N/A | missing | P3 | Identity/update URLs observed (`04`); sync protocol unknown. Out of local-first scope. |
| Pull requests / comments | N/A | missing | P3 | Cloud + Native Git adjacent; not RE’d for product PR UX. |
| Private API Network / roles | N/A | missing | P3 | Cloud. |

---

## I. Observe

| Feature area | RE | App | Pri | Notes |
|--------------|----|-----|-----|-------|
| API catalog / Insights / reports | N/A | missing | P3 | Cloud observability. Sentry/NR keys in config only (`04`). |
| Console | partial | stub | P1 | `html/console.html` (`01`). App: pipeline_log string only in response pane. |

---

## J. Publish

| Feature area | RE | App | Pri | Notes |
|--------------|----|-----|-----|-------|
| Published docs / API Network | N/A | missing | P3 | Cloud. |
| SDK / client code generators | missing | missing | P2 | RE never looked. |
| Collection documentation fields | missing | stub | P2 | `Collection.description` exists; no docs UI. |

---

## K. Administer / enterprise

| Feature area | RE | App | Pri | Notes |
|--------------|----|-----|-----|-------|
| Team admin / SSO / SCIM | N/A | missing | P3 | Cloud. OIDC deps present for auth flows (`05`), not admin. |
| Audit / compliance | N/A | missing | P3 | — |

---

## L. CLI / Plugin / Reference tooling

| Feature area | RE | App | Pri | Notes |
|--------------|----|-----|-----|-------|
| Postman CLI (vendored) | partial | missing | P2 | `Resources/data/postman-cli/` (`01`). Sibling binary, not integrated into Rust app. |
| Desktop Plugin installer | partial | missing | P3 | `plugin-installer/` (`01`). |
| Ripgrep in-app search | partial | missing | P2 | `main-ripgrep-ipc-handlers`, channel `ripgrep-search` (`02`). |
| Themes | partial | missing | P3 | `THEME_*.js` modules (`02`); app has CSS only. |

---

## M. Part B — UI / settings flows (docs map)

| Feature area | RE | App | Pri | Notes |
|--------------|----|-----|-----|-------|
| Request builder tabs (Params/Headers/Body/Auth) | partial | implemented | P0 | RE: remote UI (`02`, `08`). App: `request_builder.rs`. |
| Scripts tab | partial | stub | P1 | Editable text; no VM (`pipeline` stubs). |
| Response viewer (status / timing / body) | partial | implemented | P0 | App: `response_viewer.rs` — no headers tab, cookies, pretty/raw toggle. |
| Response headers / cookies / size breakdown | missing | missing | P1 | Headers captured in model but not shown in UI. |
| Auth types: Bearer / Basic / API Key | partial | implemented | P0 | App injects headers (`pipeline`). RE: no exhaustive enum (`05`). |
| Auth types: OAuth1/2, Digest, Hawk, AWS, NTLM, … | partial | missing | P1–P2 | Modules: OAuth2WindowManager, authentication.js truncated (`05`). |
| Proxy settings (system / custom) | partial | missing | P1 | `ProxyNodeProcess`, `GET_SYSTEM_PROXY`, proxyAuth HTML (`01`–`03`). |
| Client certificates | partial | missing | P1 | `proxyCertificateService` (`03`, `05`). Paths/encryption unknown. |
| SSL certificate verification toggle | missing | missing | P1 | RE never looked specifically. |
| Settings: general / shortcuts / data folder | missing | missing | P2 | Persistence root via `dirs` only (`storage/json_store.rs`). |

---

## N. Architecture / runtime (docs-adjacent, RE-heavy)

| Feature area | RE | App | Pri | Notes |
|--------------|----|-----|-----|-------|
| Electron main / renderer / preload | covered | N/A → Dioxus | P0 | `reports/01`, `02`. App: single Dioxus desktop process (`main.rs`). |
| Runtime IPC `@postman/runtime` | covered | N/A | P1 | Observed channel + connect/message/disconnect (`08`). App: in-process `send_request`. |
| `postman-runtime` / `postman-request` | partial | implemented (reqwest) | P0 | Deps + bundle sizes (`03`); semantics incomplete. App substitutes `reqwest`. |
| Execution workers isolation | partial | missing | P2 | ExecutionProcess / execution-worker (`03`); app blocking send. |
| Live CDP Send capture | missing | — | P1 | Boot logs only (`08`). Still open RE action. |

---

## Missed RE targets (docs imply, reports thin)

Worth extracting from `/Applications/Postman.app` with **REA + Ghidra (native only)**:

1. **`pm.*` sandbox API** — strings / Evaluator in `postman-runtime` + `tester_sandbox.html` + execution-worker.
2. **Protocol clients** — GraphQL / gRPC / WS / MQTT / MCP wiring from deps → entrypoints (not just package.json).
3. **Collection Runner vs performance runner** — separate `runner.html` path from `perftest*` / `process-manager`.
4. **Vault script syntax `{{vault:…}}`** vs CryptoService / VaultScriptService — compare to app `secret://`.
5. **Proxy + client-cert plumbing** — `ProxyNodeProcess`, `proxyCertificateService`, settings schema.
6. **Collection format v3 YAML** — parsers/importers in ASAR or CLI; schema vs v2.1.
7. **Auth-type enum + OAuth2 loopback** — finish `js/authentication.js` / OAuth2WindowManager (minified string mining).
8. **Variable resolution implementation** — recover replace/mustache inside runtime (currently product-docs only).
9. **Body modes** — form-data / urlencoded / file in collection model + runtime.
10. **Cookie jar + redirect policy** — postman-request options surface.
11. **Dataset engine** — native `postman-dataset-engine-darwin-arm64.tar.br` (Ghidra if needed) + daemon IPC.
12. **Live Send path** — stable CDP / `capture_electron_scenario` once UI CDN loads.
13. **mock-server-runtime** internals (simulation HTTP worker) — design/mock parity.
14. **Git resolver IPC** — if local Native Git is in scope later.

---

## Missed app extraction (port next, ordered)

Priority order for local-first Rust+Dioxus (not implementing here):

1. **P0** — Multi-environment maps + persist env vars separately from one flat context.
2. **P0** — Response headers / cookies pane; fix timeouts/redirects as request settings.
3. **P1** — Body modes: `urlencoded`, `form-data`; Content-Type helpers.
4. **P1** — Collection import/export (Postman Collection v2.1 JSON first).
5. **P1** — History of sent requests (local JSON).
6. **P1** — Real script sandbox (even minimal `pm.environment` / `pm.response`) — after RE of `pm.*`.
7. **P1** — Collection Runner (sequential folder/collection execute).
8. **P1** — Proxy + custom CA / client cert settings wired into `reqwest` client.
9. **P1** — OAuth2 authorization-code + token refresh (browser/loopback).
10. **P1** — Align vault refs: optional `{{vault:}}` alias beside `secret://`.
11. **P2** — GraphQL request mode; then WS; then gRPC.
12. **P2** — Local mock server from collection examples.
13. **P2** — Collection v3 YAML + Native Git folder sync.
14. **P3** — Agent Mode, Context Graph, cloud sync, monitors, Spec Hub, Plugin — defer.

---

## Cross-walk: strongest RE ↔ app already done

| RE finding | App mapping |
|------------|-------------|
| Send pipeline shape (`07`) | `runtime/pipeline.rs` |
| Variable scopes (`04`, docs) | `core/variables.rs` |
| Collections tree (`04`) | `core/collection.rs` + `ui/sidebar.rs` |
| Keytar / CryptoService (`05`, `08`) | `secrets/keychain.rs` |
| Requester shell UX (`02`) | `ui/{workbench,request_builder,response_viewer}.rs` |
| Persistence candidates NeDB/json-storage (`04`) | Minimal `storage/json_store.rs` (not schema-compatible) |
