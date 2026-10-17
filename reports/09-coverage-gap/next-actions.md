# 09 — Next actions (RE + app)

Derived from [coverage-matrix.md](./coverage-matrix.md). No app implementation in this pass.

**RE update (2026-10-17):** Top RE #1–#5 and cookie/partition pieces largely closed in [10-re-complete/re-complete.md](../10-re-complete/re-complete.md). Remaining RE: live CDP Send, exhaustive `pm.*`, vault at-rest crypto, CLI YAML, CryptoService `id`.

---

## Top 10 RE actions (Postman.app → reports)

| # | Action | Why | Tooling | Primary ASAR / bundle anchors |
|---|--------|-----|---------|-------------------------------|
| 1 | Recover **`pm.*` sandbox API table** (string mining + Evaluator host) | Scripts are P1 parity; reports only note sandbox HTML | REA: `analyze_javascript_application` / `search_strings` / `trace_application_feature` on runtime + execution-worker; offline ASAR text | `postman-runtime-*.min.js`, `execution-worker.js`, `html/tester_sandbox.html`, `execution-system-manager-*.min.js` |
| 2 | Map **protocol clients** (GraphQL, gRPC, WS, MQTT, MCP) entrypoints | Docs protocols; only deps listed today | REA JS analysis on protocol packages + webpack chunks; inventory `node_modules` usage sites | `@postman/grpc-js`, `mqtt`, `ws`/`postman-ws`, `@modelcontextprotocol/sdk`, `mcp-server-*.min.js`, `toolset-mcp-server/` |
| 3 | Separate **Collection Runner** vs **performance runner** | Docs Test APIs; boundary unclear in `04` | REA on runner HTML/JS + `process-manager` / `perftest*` event graphs | `html/runner.html`, `perftest.js`, `perftestExecute.js`, `ThreadGroupExecutable.js`, `process-manager.js`, `main-performance-test-ipc-handlers-*.min.js` |
| 4 | Extract **Vault / `{{vault:}}`** resolution vs CryptoService | App uses `secret://`; need syntax + backend selection | REA strings in Vault* services + runtime variable resolve | `VaultIntegrationService`, `VaultScriptService` (IPCAgent chunk), Azure vault keys in `config.json` (`reports/05`, `08`) |
| 5 | Trace **proxy + client-cert** settings → HTTP agent | P1 local settings; modules named only | REA `ProxyNodeProcess` + `proxyCertificateService`; live settings UI if CDP works | `ProxyNodeProcess.js`, `services_proxyCertificateService_*.min.js`, `html/proxyAuth.html`, `GET_SYSTEM_PROXY` events |
| 6 | Find **collection format v3 YAML** parsers / CLI paths | Docs Native Git / collections; **RE never looked** | REA grep ASAR + `Resources/data/postman-cli`; schema samples | `postman-collection`, CLI under `Resources/data/postman-cli/`, any `*.yaml` importers |
| 7 | Finish **auth-type enum + OAuth2 loopback** | App only Bearer/Basic/API Key | String mining `js/authentication.js`, OAuth2WindowManager; follow redirect `127.0.0.1:10534` pattern | `js/authentication.js`, `services_OAuth2WindowManager_*.min.js`, `html/auth/auth.html` |
| 8 | Recover **variable replace** implementation (mustache / scopes / data) | App has convention; RE never proved | Focused analysis inside `postman-runtime` / `postman-collection` (budgeted subsets) | `postman-runtime-*.min.js`, collection model; compare to `07` |
| 9 | **Live CDP Send** capture (end-to-end) | Still open (`08`); closes lifecycle gap | REA `list_electron_targets` / `capture_electron_scenario` with Postman running + remote UI loaded | RuntimeIPCAgent `@postman/runtime`, `platform.execute` |
| 10 | Inventory **body modes + cookie jar + redirect** options on `postman-request` | P0/P1 HTTP parity holes | REA strings / options objects in runtime + postman-request fork | `postman-request`, runtime requester options |

### Secondary RE (after top 10)

- Dataset engine: unpack `Contents/postman-dataset-engine-darwin-arm64.tar.br` → Ghidra only if native; + `main-dataset-daemon-ipc-handlers`.
- Mock/simulation worker internals (`mock-server-runtime/simulation-http-server-worker.js`).
- Native Git IPC payload shapes (`main-git-ipc-handlers`, `GitResolver`).
- CryptoService `id` call-site mapping (user/team GUID?).
- Stabilize REA MCP Ghidra `procedure_pseudo_code` (headless dump already OK for keytar).

**Constraint reminder:** Ghidra for `.node`/`.dylib`/`.so` only — no Hopper.

---

## Top 10 app build actions (notPostman)

Ordered for local-first Rust + Dioxus. Do **not** start P3 cloud/AI.

| # | Action | Pri | Touch points |
|---|--------|-----|--------------|
| 1 | **Multi-environment** store (named env → var map) + selector that actually swaps maps | P0 | `core/variables.rs`, `storage/json_store.rs`, `ui/env_selector.rs` |
| 2 | **Request settings**: timeout, follow redirects, TLS verify → `reqwest::Client` builder | P0 | `core/request.rs`, `runtime/pipeline.rs`, request builder UI |
| 3 | **Response headers pane** (+ optional cookies) | P0–P1 | `ui/response_viewer.rs` (headers already on `ResponseView`) |
| 4 | **Body modes**: `urlencoded` + `form-data` (multipart) | P1 | `BodyMode` in `core/request.rs`, builder UI, `pipeline.rs` |
| 5 | **Import/export Collection v2.1 JSON** | P1 | new `storage/import_export.rs` (or similar); map to `Collection` |
| 6 | **Send history** (method, URL, status, time) persisted | P1 | `storage`, `ui/sidebar.rs` History section |
| 7 | **Script sandbox MVP** after RE #1 — evaluate pre-request/tests with minimal `pm` | P1 | `runtime/` (VM crate TBD), un-stub `run_*_stub` |
| 8 | **Collection Runner** — sequential execute folder/collection with shared env | P1 | new `runtime/runner.rs`, runner UI shell |
| 9 | **Proxy + client certs** on HTTP client | P1 | settings model + `reqwest` proxy/identity; secrets for cert passphrases |
| 10 | **OAuth2** auth-code + refresh; optional `{{vault:}}` alias → existing keychain | P1 | `AuthType`, browser/loopback, `secrets/` |

### Next wave (P2 — after top 10)

- GraphQL mode → WebSocket → gRPC.
- Local mock from saved examples (`mock-server` inspiration from `04`).
- Collection v3 YAML + optional folder-as-git workflow.
- Console pane (structured logs, not only `pipeline_log`).

### Explicit defer (P3)

Agent Mode, Context Graph, cloud workspaces/sync, monitors, Spec Hub, API Network, desktop plugins, team admin.

---

## Suggested sequencing

```
RE #1 pm.*  ──┬──► App #7 sandbox
RE #4 vault ──┘
RE #5 proxy ──────► App #9 proxy/certs
RE #6/#8 formats ─► App #5 import/export
RE #3 runner ─────► App #8 collection runner
RE #2 protocols ──► App P2 GraphQL/WS/…
App #1–#4, #6     ► can start now (no new RE required)
```

---

## Success criteria (short)

| Track | Done when |
|-------|-----------|
| RE | New report folder(s) with observed `pm` method list, protocol entry map, vault syntax, proxy/cert settings schema, v3 YAML notes |
| App | Env switch works; headers visible; form bodies send; v2.1 import; history; runner stub→real; scripts execute one assertion |
