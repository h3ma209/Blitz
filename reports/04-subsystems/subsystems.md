# 04 — Subsystems

## Evidence sources

- ASAR inventory (`inspect_artifact` / occurrences)
- `package.json` ASAR member
- `config.json` ASAR member
- `process-manager.js` / mock-server / MCP paths
- Unpacked `Resources/data/*` filesystem listing
- Full/partial `analyze_javascript_application` summaries

---

## Collections & folders

| Finding | Label |
|---------|-------|
| Dependency `postman-collection` | **observed** (package.json) |
| Scratchpad `CollectionSidebar.js` | **observed** path |
| Requester-side collection store implementation | **not recovered** (minified `js/requester.js`) |
| Collection foldering model | **inferred** via collection schema (v2.1) — not REA-decompiled here |
| Schema URL `https://schema.getpostman.com/json/collection/v2.1.0/collection.json` | **observed** (`postman-collection` vendor chunk, 2026-10-17) |
| Collection format v3 schema URL in desktop ASAR | **missing** (0 hits); `*-pme_v3.worker*` = Monaco editor workers only — see [09 re-followups](../09-docs-gap-matrix/re-followups.md) |

## Variable scopes

| Scope | Status |
|-------|--------|
| Runtime sandbox context list `_variables`, `environment`, `collectionVariables`, `globals`, `cookies`, `data`, `request`, `response` + run-state `vaultSecrets` | **observed** (ASAR/`postman-runtime` strings, 2026-10-17) |
| `VariableScope` + `Property.replaceSubstitutions` / `replaceIn` | **observed** (`postman-collection` + runtime) |
| Environment storage on disk | **unknown** — candidates: `electron-json-storage`, NeDB (`nedb` → `@seald-io/nedb`) from package.json (**observed** deps; usage sites not traced) |
| Vault VariableScope `prefix:"vault:"`, files root `join(homeDir(),"Postman","files")` via `initializeDefaultWorkingDirectory`, `_domains`/`domainPatterns`, gated `_allowScriptAccess` | **observed** — [10](../10-re-complete/re-complete.md); userData `createdDefaultWorkingDir:true` |

## Scripting sandbox (`pm.*`)

| Finding | Label |
|---------|-------|
| `html/tester_sandbox.html` + `js/tester/Evaluator.js` | **observed** (Evaluator = window.open block + postMessage echo only) |
| `postman-sandbox` required from `postman-runtime` (+ `uvm` / `uniscope`) | **observed** |
| uniscope `create({eval:true, ignore:["require"], block:["bridge"]})` + allowed-globals scrub; uvm `__uvm_*` bridge | **observed** (2026-10-17 complete — [10](../10-re-complete/re-complete.md)) |
| Expanded `pm.*` string table (env/globals/variables/cookies/response/test/execution/expect/sendRequest/vault interface/…) | **observed** — still not full SDK decompile; see [10](../10-re-complete/re-complete.md) |

## Collection Runner

| Finding | Label |
|---------|-------|
| `html/runner.html` (“Collection Runner Webview”) → `js/runner.js` + `js/vendor-ui.js` | **observed** HTML; both JS files **absent** from ASAR extract (**observed** 2026-10-17 complete — CDN-missing like requester) |
| Perf / load tooling: `perftest.js`, `perftestExecute.js`, `ThreadGroupExecutable.js`, `process-manager.js` | **observed** paths + executor event strings |
| Classic Collection Runner vs performance runner | **observed** separation — Runner UI remote + `postman-runtime` Run; perf = `process-manager` → `execution-handler` VU executor ([10](../10-re-complete/re-complete.md)) |

## Cookies

| Finding | Label |
|---------|-------|
| Dep `@postman/tough-cookie`; runtime `cookieJar` / `disableCookies` / `perPartitionCookieJar` | **observed** (2026-10-17) |
| Send path: `getCookieJarFor(coords)` → `requester.create`; jar `getCookieString`/`setCookie` | **observed** ([10](../10-re-complete/re-complete.md)) |
| Electron partition id `persist:postman_user_cookies[_<user>]`; userData `Partitions/postman_user_cookies_*/Cookies` | **observed** code + on-disk |
| Sandbox CookieStore bridge (`putCookie` / `findCookie` / …) | **observed** |

## Protocol clients (non-HTTP)

| Finding | Label |
|---------|-------|
| ASAR chunks `@postman/runtime_{grpc,mqtt,websocket}-client`, `postman-ws`, scratchpad GraphQL/WS workbench | **observed** inventory + deps |
| High-level surfaces: gRPC `Client`/`invoke`/`stream`; MQTT `MqttClient` connect/subscribe/publish; WS `connect`/`send`/`close` | **observed** string/verb mining ([10](../10-re-complete/re-complete.md)) |

## Mock servers

| Finding | Label |
|---------|-------|
| `mock-server-runtime/` directory in ASAR | **observed** |
| Unpacked `data/worker/mock-server-worker.js` (path listed in Resources tree; file may be under asar-out extract) | **observed** listing earlier under Resources/data/worker |
| Simulation HTTP worker mentioned in REA error: `mock-server-runtime/simulation-http-server-worker.js` | **observed** (analysis stopped after retaining facts for that file) |

## Cloud sync / workspaces

| Finding | Label |
|---------|-------|
| `config.json` identity / analytics / desktop UI URLs | **observed** (see § config) |
| `__WP_DESKTOP_UI_UPDATE_URL__` → `https://desktop.postman.com` | **observed** |
| `__WP_IDENTITY_URL__`, `__WP_GLOBAL_IDENTITY_URL__` | **observed** |
| `__WP_GOD_INTEGRATIONS__URL` → `https://app.getpostman.com` | **observed** |
| Internal monorepo workspace packages in package.json `workspaces` | **observed** (build metadata; not shipped as separate ASAR trees) |
| Sync protocol / CRDT / websocket sync implementation | **unknown** — not recovered |

### config.json excerpts (**observed** ASAR member)

- Sentry DSN, New Relic keys, crash reporting flag
- Interceptor Chrome extension id + bridge socket name `postman`
- Vault Azure app id + redirect `http://127.0.0.1:10534`
- Tunnel liaison `https://tunnel-liaison.pstmn.io`
- Stripe payment hosts allowlist
- `__WP_ENV__: production`, `__WP_RELEASE_CHANNEL__: prod`

## Other subsystems (**observed** paths)

| Path / dep | Role (**inferred**) |
|------------|---------------------|
| `mcp-server-*.min.js`, `toolset-mcp-server/` | MCP server control plane |
| DuckDB / dataset-daemon modules | Dataset engine integration |
| `main-webhooks-tunnel-forwarding-*.min.js` | Webhook tunneling |
| `services_interceptorBridgeInstaller_*.min.js` | Browser interceptor bridge |
| `services_git_GitResolver_*.min.js` | Git resolution |
| `postman-cli` under Resources/data | CLI sibling |
| `@vscode/ripgrep` + `main-ripgrep-ipc-handlers` | In-app search |
| Playwright-core under node_modules | Browser automation support (tab recorder related?) |

## Gaps

- Full `replaceSubstitutions` precedence algorithm not decompiled (class names **observed**).
- No REA-proven sync protocol.
- Mock server internals only partially analyzed before memory budget exhaustion.
- Collection v3 YAML parser still **missing** in desktop ASAR (CLI not re-walked 2026-10-17).
