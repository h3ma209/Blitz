# 03 — Request lifecycle

## Evidence sources

| Tool | Note |
|------|------|
| `inspect_artifact` | Paths: `postman-runtime-*.min.js`, `ExecutionProcess.js`, `execution-worker.js`, `AgentWorker.js`, `ProxyNodeProcess.js`, `common_services_UnifiedRuntimeService_*.min.js`, `execution-system-manager-*.min.js`, `runtime-platform-*.min.js`, `services_proxyCertificateService_*.min.js` |
| `analyze_javascript_application` execution set | `ev_99bc592560423429598fc8e08398c7dba518877b3c895e8a7d4ba3f3ce3ca851` — 9 files parsed; **0** Electron IPC in that subset; request-family relations retained **0** |
| Full ASAR JS analysis | `ev_86985...` — heap/budget failures on AgentWorker / ExecutionProcess / execution-worker |
| `package.json` ASAR member | deps `postman-runtime`, `postman-request`, `postman-collection`, `@postman/runtime.platform`, `@postman/runtime.runtime-rpc-electron` |
| `process-manager.js` ASAR text | Executor event names (perf-test oriented) |
| `trace_application_feature` seed `postman-runtime` | Completed but MCP response **too large** (`ev_570eb692...`); summary-only via `get_evidence_bundle` — no path detail recoverable in-session |

---

## High-level pipeline (**inferred** from observed modules + deps; not a recovered call graph)

```
[Requester UI] Send click
    → (renderer) build Collection/Request object (postman-collection)
    → IPC / runtime RPC bridge (UnifiedRuntimeService / IPCAgent / runtime-rpc-electron)
    → ExecutionProcess / execution-worker / AgentWorker
    → postman-runtime (+ postman-request HTTP)
    → optional ProxyNodeProcess / proxyCertificateService
    → response events back to renderer → response pane
```

**Confidence:** architectural **inference**. REA **did not** recover a complete static call path from Send button → HTTP due to minification + budget truncation + `request` semantic family showing 0 retained relations in focused execution analysis.

---

## Components (**observed**)

### HTTP / collection runtime

| Artifact | Size | Observation |
|----------|------|-------------|
| `postman-runtime-981eed2565622025.min.js` | ~4.7 MB | Bundled runtime (**observed** inventory) |
| `package.json` dependency `postman-runtime` ^7.56.1 | — | Declares official Postman runtime |
| `postman-request` 2.88.1-postman.49 | — | Forked request HTTP client |
| `postman-collection` ^5.3.1 | — | Request/collection model |
| `@postman/runtime.platform` / `runtime.runtime-rpc-electron` | — | Platform + Electron RPC bridge |
| `common_services_UnifiedRuntimeService_*.min.js` | 21588 | Unified runtime service chunk |
| `runtime_agents_IPCAgent_*.min.js` | 13353 | IPC agent for runtime |
| `execution-system-manager-*.min.js` | 11341 | Execution system manager |
| `runtime-platform-*.min.js` | 201 | Tiny stub/re-export (**observed** size) |

### Execution isolation

| Artifact | REA note |
|----------|----------|
| `ExecutionProcess.js` | Static facts retained; **semantic graph exceeded budget** |
| `execution-worker.js` | Same — relationships omitted |
| `AgentWorker.js` | Worker **heap exhausted** during semantic analysis (~10.8 MB) |
| `ProxyNodeProcess.js` | Semantic graph budget exceeded |
| `html/tester_sandbox.html` | Separate sandbox document for scripts (**observed**) |

### Proxy / certs

- `services_proxyCertificateService_*.min.js` (6966 bytes) — **observed** path.
- `ProxyNodeProcess.js` — proxy process (**inferred** role).
- `restricted-addresses.js` — likely SSRF / private-net guard (**inferred** from name only).

### Perf-test executor events (**observed** strings in `process-manager.js`)

**Distinct from Collection Runner** (webview + `postman-runtime` Run). Perf = VU child-process path — see [10](../10-re-complete/re-complete.md).

- Lifecycle: `init`, `start`, `done`, `error`, `status`, …
- `ExecutionManagerEvents`: `START/STOP/PAUSE/RESUME`, `ADD_USERS`/`REMOVE_USERS`, `GET_OAUTH_REFRESH_TOKEN`, `GET_SYSTEM_PROXY`, `SCRIPT_ERROR_DATA`, …
- Forks `execution-handler.js` via `child_process.fork` (**observed**); load profiles `fixed`/`ramp-up`/`spike`/`peak` + `ThreadGroupExecutable.js`

---

## Variable resolution `{{vars}}`

- **Observed (2026-10-17):** `VariableScope`, `Property.replaceSubstitutions` / `replaceIn`; sandbox scopes include `_variables`, `environment`, `collectionVariables`, `globals`, `vaultSecrets` (see [09 re-followups](../09-docs-gap-matrix/re-followups.md)).
- Vault refs: VariableScope `prefix:"vault:"` → `{{vault:<key>}}`; passport path builds `{{vault:"+secretReference+"}}` (**observed** ASAR text).
- Precedence UX still **product-docs** / **inferred** for exact override order (not fully decompiled).

## Cookies on the send path

- **Observed:** `requester.cookieJar` / `disableCookies`; `@postman/tough-cookie` jar `getCookieString` / `setCookie`; optional `perPartitionCookieJar` on runner partitions ([09](../09-docs-gap-matrix/re-followups.md)).
- **Observed (2026-10-17 complete):** when `!disableCookies`, requester sets `jar = cookieJar || true`; send uses `run.getCookieJarFor(coords)` in `requester.create({…})`; `perPartitionCookieJar` mutually exclusive with shared `requester.cookieJar`. Electron partitions `persist:postman_user_cookies[_<id>]` under userData `Partitions/` — see [10](../10-re-complete/re-complete.md).

## Pre-request / test scripts

- **Observed:** `html/tester_sandbox.html` + `js/tester/Evaluator.js`; `postman-sandbox` + uvm/uniscope; partial `pm.*` string table; `process-manager` `SCRIPT_ERROR_DATA`.
- **Observed (2026-10-17 complete):** uniscope `create({eval:true, ignore:["require"], block:["bridge"]})` + allowed-globals scrub; uvm bridge symbols; fuller `pm.*` table in [10](../10-re-complete/re-complete.md).
- **Inferred order (runtime structure; not live-proven):** resolve vars (incl. vaultSecrets) → pre-request scripts → auth → send (cookie jar) → tests → visualizer.

## Auth injection

See report 05. Lifecycle touchpoints **observed** as modules: `js/authentication.js` (large; expansion truncated), `services_OAuth2WindowManager_*.min.js`, `services_AppRefreshTokenManager_*.min.js`, process-manager OAuth refresh token events.

## Gaps (tool-named)

1. `analyze_javascript_application` full ASAR — truncated before complete request-edge recovery.
2. Focused execution analysis — `request` family `retained_relations: 0`.
3. `trace_application_feature(postman-runtime)` — result exceeded MCP budget; detail not inspectable via summary.
4. CDP / live Send: prior capture got boot logs (RuntimeIPCAgent, UI `12.32.3-ui-261015-0231`) but interactive Send still unrecovered — see [08](../08-deeper-re/deeper-re.md).
5. Offline bridge progress (08): IPC channel `@postman/runtime`; `platform.execute` string in UnifiedRuntimeService; requester UI not in local ASAR.
