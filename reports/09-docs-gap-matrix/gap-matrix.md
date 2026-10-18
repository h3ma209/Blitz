# 09 — Docs gap matrix (v12 map × RE × notPostman)

> **App status superseded** by [11-docs-gap-refresh/gap-refresh.md](../11-docs-gap-refresh/gap-refresh.md) after the 2026-10-17 extract wave. Keep this file for RE priority history.

**Purpose:** Feature checklist for local Rust client. Docs TOC = completeness map only (**product-docs**), not copyrighted text.  
**Sources:** `reports/01`–`08`, this folder’s [re-followups.md](./re-followups.md), `src/{core,runtime,ui,storage,secrets}`.  
**Date:** 2026-10-17 · Postman Desktop **12.32.3** · ASAR SHA-256 **`30abb7db…1564f4`** (REA this pass; older reports used `f07f297…`) · notPostman **0.1**.

### Column keys

| Column | Values |
|--------|--------|
| **RE status** | `observed` / `partial` / `missing` / `N/A` — architecture recovered from binary/ASAR enough to recreate? |
| **In notPostman** | `yes` / `partial` / `no` |
| **Priority** | `P0` core HTTP · `P1` local parity · `P2` advanced · `P3` cloud/enterprise/AI — defer |

### Claim labels (in Notes)

- **observed** — REA / ASAR string / Ghidra
- **inferred** — architecture guess from deps/paths
- **product-docs** — UX from docs map only
- **app-implemented** — notPostman source

Related earlier checklist: [09-coverage-gap/coverage-matrix.md](../09-coverage-gap/coverage-matrix.md) (superseded for priority language by this file; keep for history).

---

## Gap matrix

| Docs area | RE status | In notPostman | Priority | Notes |
|-----------|-----------|---------------|----------|-------|
| Request builder UI (method/URL/params/headers/body/auth tabs) | partial | yes | P0 | RE: remote `requester.js` absent; CDN UI (**observed** `02`/`08`). App: `request_builder.rs` (**app-implemented**). |
| Response pane (status/timing/body) | partial | yes | P0 | App: `response_viewer.rs`. Headers/cookies panes thin (**app-implemented** partial). |
| Response headers / cookies / size | missing → partial | partial | P1 | RE 2026-10-17: cookie jar via `@postman/tough-cookie` + `requester.cookieJar` (**observed**, re-followups). App shows body/status; headers on model not fully UI. |
| Query params / headers / raw body | missing | yes | P0 | RE never isolated UI model; app has them. |
| Body form-data / urlencoded | missing | partial | P1 | App: `BodyMode` stubs + pipeline log stub (**app-implemented** partial). |
| Variables / environments | partial | partial | P0 | RE: `VariableScope`, `replaceSubstitutions`, scopes `_variables`/`environment`/`globals`/`collectionVariables`/`vaultSecrets` (**observed** runtime). App: single `VariableContext` + env selector stub (**app-implemented**). Precedence local>env>collection>global = **product-docs** + app convention. |
| Vault refs `{{vault:}}` / secret storage | observed | partial | P1 | RE: VariableScope `prefix:"vault:"`; `~/Postman/files` via `initializeDefaultWorkingDirectory`; domainPatterns; CryptoService keytar; at-rest file crypto **unknown** (**observed**/gap [10](../10-re-complete/re-complete.md)). App: `secret://` only. |
| Auth types Bearer/Basic/API Key | partial | yes | P0 | App injects headers. RE enum incomplete (`05`). |
| Auth OAuth1/2, Digest, Hawk, AWS, NTLM, … | partial | no | P1–P2 | OAuth2WindowManager, process-manager refresh events (**observed** paths). Digest stub in app. |
| Scripts / `epm.*` sandbox | observed | partial | P1 | RE: `postman-sandbox` + uniscope/uvm isolation + expanded `epm.*` table (**observed**, [10](../10-re-complete/re-complete.md)). App: script text stored, not executed. |
| Collections / folders / requests | partial | yes | P0 | `postman-collection` + schema URL v2.1.0 (**observed**). App CRUD tree. |
| Collection import/export v2.1 JSON | partial | no | P1 | Schema URL + `importCollection` string hits (**observed**). No app importer. |
| Collection format v3 YAML | missing | no | P1 | Docs imply. ASAR: **no** `collection/v3` schema URL. `*-pme_v3.worker*` = Monaco editor workers (**observed**), not collection format. |
| Native Git | partial | no | P2 | `main-git-ipc-handlers`, GitResolver (**observed** paths). |
| Agent Mode | partial | no | P3 | `AgentWorker.js` huge; analysis budget fail (`01`/`03`). |
| History | missing | partial | P1 | RE never recovered store. App sidebar “not wired”. |
| Console | partial | partial | P1 | `html/console.html` (**observed**). App: `pipeline_log` only. |
| Collection Runner | observed | no | P1 | `html/runner.html`; `js/runner.js`+`vendor-ui.js` **absent** (CDN-like); engine = `postman-runtime` Run (**observed** [10](../10-re-complete/re-complete.md)). |
| Performance runner | observed | no | P2 | Distinct: `process-manager`→`execution-handler`, VU profiles, `ThreadGroupExecutable` (**observed** [10](../10-re-complete/re-complete.md)). |
| Protocols beyond HTTP (GraphQL/gRPC/WS/MQTT) | observed | no | P1–P2 | Chunk roles + verbs: gRPC invoke/stream; MQTT connect/subscribe/publish; WS connect/send (**observed** [10](../10-re-complete/re-complete.md)). |
| MCP protocol | partial | no | P3 | MCP server bundles (`01`/`04`). |
| Mocks / simulations | partial | no | P2 | `mock-server-runtime/` (**observed**). |
| Monitors | N/A | no | P3 | Cloud scheduled. |
| Spec Hub | missing | no | P3 | Not RE’d; cloud. |
| Flows | missing | no | P3 | Not RE’d in desktop ASAR this pass. |
| Cloud sync / workspaces | N/A | no | P3 | Identity URLs (**observed**); sync protocol unknown. |
| Proxy / certificates | observed | no | P1 | Paths + `ProxyConstants` + settings keys `useSystemProxy`/`SSLCertVerify`/`CACertPath`; MITM ssl from root CA (**observed** [10](../10-re-complete/re-complete.md)). |
| Import/export (OpenAPI etc.) | missing | no | P2 | `importCollection` only shallow. |
| Code gen / snippets | missing | no | P2 | Not RE’d. |
| Cookies jar | observed | no | P1 | Send: `getCookieJarFor` + jar get/setCookie; partitions `persist:postman_user_cookies_*` on disk (**observed** [10](../10-re-complete/re-complete.md)). |
| Settings (general/proxy/certs/SSL verify) | observed | no | P1 | Scratchpad settings keys incl. `SSLCertVerify`, `useSystemProxy`, `isCACertEnabled`, `CACertPath` (**observed** [10](../10-re-complete/re-complete.md)). |
| Capture / interceptor | partial | no | P2 | Interceptor bridge + Chrome ext id (`04`). |
| CLI / Plugin | partial | no | P2–P3 | Vendored `postman-cli`, `plugin-installer` (`01`). |
| Collaborate / Observe / Publish / Administer | N/A | no | P3 | Cloud. |

---

## P0 for local recreation (ordered)

1. Multi-environment maps + persist (app partial; RE scopes **observed**).
2. Request settings: timeout / redirects / TLS verify → client builder.
3. Response headers (+ cookies) UI.
4. Align vault: optional `{{vault:key}}` beside `secret://` using VariableScope-prefix semantics (**observed**).
5. Keep HTTP send path solid (already **app-implemented** via reqwest).

## P1 (RE-backed, local-client)

1. Script sandbox MVP — mirror `epm.environment|globals|collectionVariables|cookies|request|response|test|execution|vault` surface (**observed** strings).
2. Cookie jar (`tough-cookie` semantics).
3. Collection Runner sequential execute.
4. Collection v2.1 import/export.
5. Proxy + client/root CA paths wired to HTTP client.
6. Protocol clients: GraphQL first, then WS/MQTT/gRPC using shipped `@postman/runtime_*-client` shapes as reference.
7. OAuth2 loopback (Azure redirect pattern **observed** in config).

## Explicit defer (P3 / N/A)

Agent Mode, Spec Hub, Flows, monitors, workspaces/SSO/billing, MCP product UX, desktop plugins.
