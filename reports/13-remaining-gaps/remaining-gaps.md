# 13 — Remaining gaps vs Postman (local grab)

**Date:** 2026-10-17 (updated after local wave)  
**Sources:** FEATURES.md, reports/11–12, Hakki + Initiate Envs exports, ASAR `package.json` / postman-runtime system headers  
**Framing:** local desktop parity — **not** Postman Cloud clone.

---

## Status legend

| Mark | Meaning |
|------|---------|
| **SHIPPED** | In app this wave (see FEATURES.md) |
| **PARTIAL** | Subset / stub — usable but not Postman-complete |
| **DEFERRED** | Still missing locally |

---

## A. High impact for *your* Hakki / IDM collections

| Gap | Status | Notes |
|-----|--------|-------|
| **Collection / env variable UI** | **SHIPPED** | Col vars modal + env manager; sync → `VariableContext.collection` |
| **Environment import** (Postman env JSON) | **SHIPPED** | File → Import / Export environment |
| **Script fidelity** | **PARTIAL** | Login tests + `epm.collectionVariables` OK; Chai-heavy / async / `require` still fail |
| **Authorization header already on many requests** | OK | Unchanged |
| **Binary / file body mode** | **SHIPPED** | Body → binary + path / Browse |
| **Response Visualize / Timeline** | **SHIPPED** | Visualize tab (`epm.visualizer.set` / HTML); Timeline approximated |
| **Request description / docs pane** | **SHIPPED** | Docs tab on request builder |
| **Cookie manager UI** | **SHIPPED** | Cookies toolbar modal — view/edit/delete |
| **Collection search** | **SHIPPED** | Sidebar search filter |

---

## B. Local desktop (ASAR-proven)

### Protocols
| Item | Status |
|------|--------|
| Socket.IO client | **SHIPPED** (`rust_socketio` — connect/emit/on session) |
| GraphQL **subscriptions** (graphql-ws) | **SHIPPED** (subscribe + event log) |
| gRPC **reflection / .proto / protoset** UI | **PARTIAL** — reflection list stub + unary Struct; no .proto load |
| gRPC streaming (client/server/bidi) | **PARTIAL** — server-stream stub note; unary remains primary |
| SSE / EventSource | **SHIPPED** |
| Full MQTT TLS / auth UX | **PARTIAL** — minimal connect only (unchanged) |

### Auth
| Type | Status |
|------|--------|
| OAuth 1.0 | **SHIPPED** (HMAC-SHA1/SHA256/PLAINTEXT) |
| Hawk | **SHIPPED** |
| NTLM | **PARTIAL** — Type1 negotiate only |
| AWS Signature | **SHIPPED** (SigV4 helper) |
| Akamai EdgeGrid | **SHIPPED** (EG1) |
| ASAP | **DEFERRED** |
| JWT Bearer (dedicated helper) | **PARTIAL** via raw Bearer |

### Proxy / intercept
| Item | Status |
|------|--------|
| CONNECT / HTTPS MITM | **PARTIAL** — CONNECT **tunnels** (no decrypt); MITM CA not shipped (macOS trust-store) |
| Browser Interceptor bridge | **DEFERRED** — UI explains limits via `interceptor_limits_blurb` |
| System proxy auto-detect UI polish | **PARTIAL** |
| PKCS#12 client cert load | **SHIPPED** — openssl CLI → PEM when available |

### Runner / quality
| Item | Status |
|------|--------|
| Newman / CLI parity | **SHIPPED** — `not_postman run collection.json [--env env.json]` |
| Delayed / scheduled runs | **DEFERRED** |
| Data files beyond CSV/JSON rows | **PARTIAL** |
| Perf: thresholds, HTML report | **DEFERRED** / minimal VUs |
| Mock **scenarios** | **SHIPPED** — match by header / example name filters |

### Impex / codegen
| Item | Status |
|------|--------|
| Collection **v2.0** / RAML / WSDL | **DEFERRED** |
| HAR import | **SHIPPED** |
| Environment + globals JSON import/export | **SHIPPED** (env; globals still via variables map) |
| OpenAPI **YAML** + richer `$ref` | **PARTIAL** — YAML paths happy-path; `$ref` thin |
| Codegen: Java, Go, C# | **SHIPPED** (+ existing curl/fetch/python) |
| Export as cURL one-click | **PARTIAL** — snippet modal |

### UX
| Item | Status |
|------|--------|
| Find / search across collections | **SHIPPED** |
| Bulk edit / bulk run select | **DEFERRED** |
| Sibling reorder (DnD order) | **SHIPPED** — `sort_key` + drop-on-request reorder |
| Dark/light themes | **DEFERRED** — fixed dark/orange |
| Cookie manager UI | **SHIPPED** |
| History search / export | **DEFERRED** / thin |
| Request examples picker | **PARTIAL** — save one; no gallery |
| Prettify / schema lint | **PARTIAL** — comment strip only |

### Sandbox
| Item | Status |
|------|--------|
| `epm.visualizer` | **SHIPPED** (basic `set(template, data)`) |
| `epm.execution.location` / full API | **PARTIAL** |
| `require('…')` / packages | **DEFERRED** |
| Async `epm.sendRequest` / promises | **PARTIAL** — sync callback only |
| Exact Postman sandbox globals | **PARTIAL** — boa subset |

---

## C. Not clonable from `/Applications/Postman.app/Contents` alone

Cloud sync, workspaces, roles, SSO/SCIM, billing, Agent Mode / AI, Spec Hub, Flows, cloud Monitors, API Catalog / Insights, published cloud mocks, Native Git team views.  
**(Still out of scope — do not claim.)**

---

## D. Suggested next local wave (remaining priority)

1. Full NTLM handshake + ASAP / JWT helper  
2. gRPC `.proto` / reflection client (tonic) + real streaming  
3. Optional local CA MITM (user must trust) — only if product asks  
4. Bulk edit / history search / themes  
5. Richer OpenAPI `$ref` + collection v2.0  

---

## E. What we already matched (don't re-do)

HTTP workbench, Postman system headers, JSON comment strip, v2.1 impex, OAuth2 CC+AC, Digest, vault, runner iterations, local mock, WS/MQTT/MCP/gRPC-unary, capture proxy, perf VUs, sidebar CRUD/DnD/context menus, bundled Hakki seed.

**Also now matched (this wave):** env JSON, collection vars UI, cookies manager, search, binary body, docs, visualizer, timeline, Socket.IO, GraphQL-WS, SSE, OAuth1/Hawk/AWS/EdgeGrid, HAR, OpenAPI YAML, Java/Go/C# snippets, PKCS#12 via openssl, mock scenarios, CLI `run`, sibling reorder.
