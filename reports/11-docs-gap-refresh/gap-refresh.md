# 11 — Docs-map gap refresh (2026-10-17)

**Supersedes** stale rows in [09-docs-gap-matrix/gap-matrix.md](../09-docs-gap-matrix/gap-matrix.md) for **app status**. RE facts still cite [10-re-complete](../10-re-complete/re-complete.md) + [11 re-notes](re-notes.md).

**Scope:** local HTTP workbench. Out of scope: Agent Mode, Spec Hub, Flows, cloud Monitors, Native Git sync, SSO, MCP/AI/gRPC/MQTT/WS full clients.

| Docs area | RE | App (pre-11 extract) | Priority | Action this pass |
|-----------|----|----------------------|----------|------------------|
| Request builder (params/headers/body/auth/scripts/settings) | partial (CDN UI) | **done** | — | — |
| Body form-data file upload | missing | stub path | P1 | **file path → multipart file** |
| Response Pretty/Raw/Headers/Tests | partial | **done** | — | — |
| Response Cookies / Preview / Save example | cookies jar RE | cookies tab thin; no preview/example | P1 | **ship** |
| Variables / environments / vault | observed | **done** | — | eye quick-look |
| Auth Bearer/Basic/API Key/Digest/Inherit | partial | **done** | — | — |
| OAuth 2.0 | OAuth2WindowManager + grants **observed** | missing | P1 | **client_credentials + auth_code** |
| Scripts / epm.* | sandbox table in 10 | minimal boa | P1 | **expand epm.*** |
| Collection/folder scripts | runtime events **observed** | request-only | P1 | **inherit order** |
| Collections tree CRUD | schema v2.1 | **done**; rename/dup thin | P1 | duplicate/favorite/sort |
| Import/export v2.1 | observed | **done** | — | — |
| cURL / OpenAPI import | shallow | missing | P1 | **ship subset** |
| Code snippets | `postman-code-generators.js` **observed** | missing | P1 | curl/fetch/python |
| History / Console / Runner | shells | **done** | — | iterations/data file |
| GraphQL body | protocol chunks | missing | P1 | **HTTP GraphQL body** |
| Local mocks | `mock-server-runtime/` **observed** | missing | P1 | **localhost mock** |
| Multi-tab / palette / trash | product-docs | missing | P1 | **ship** |
| Proxy / PEM certs | observed | **done**; PKCS#12 TODO | P2 | PKCS#12 or clear error |
| Performance / monitors / cloud / Agent / Spec / Flows | partial/N/A | out of scope | — | TODO only |

## Shipped before this pass (keep)

Environments, history, vault `{{vault:}}`, boa sandbox (minimal), Digest, proxy, PEM certs, Collection Runner sequential, v2.1 impex, console, cookies jar, shortcuts ⌘↵/⌘S/⌘T.
