# 01 — App structure

**Target:** Postman Desktop 12.32.3 (`com.postmanlabs.mac`)  
**Primary artifact:** `/Applications/Postman.app/Contents/Resources/app.asar`  
**SHA-256 (REA):** `f07f297001091e1672382de853d2dc7d447259278ac1898f8245235c777f7571`

## Evidence sources

| Tool | Evidence / note |
|------|-----------------|
| `inspect_electron_fuses` | `ev_fd5514ffa8bd23a2ad55ebf6d3ba77e04c71331ea6b1086801e2f5a456f275a6` |
| `open_binary` (asar) | format `asar`, digest above |
| `inspect_artifact` (summary) | parent `ev_1b63e5d8bcfb367a3450e08b4ffc92cdca4586f00168a2ccd993a6f28799ea94` — 618 nodes, 677 occurrences, **source_complete** |
| `analyze_javascript_application` (full ASAR, detail summary) | parent `ev_86985f56c5339657d605888162b24ae2d2ae92eebce30f354059ce69314d7103` — **partial** (parent result budget exhausted) |
| `analyze_javascript_application` (entry source_paths) | parent `ev_5d12fac6ad3d0fd45d791d1dd1b825ccba653e4e91b1f04a11f30f7d2c8f0190` |
| Info.plist / Resources listing | host filesystem (bundle metadata) |
| `config.json` (ASAR) | `__WP_DESKTOP_UI_UPDATE_URL__` = `https://desktop.postman.com` (**observed**) |
| Deeper RE | see [08-deeper-re](../08-deeper-re/deeper-re.md) — remote UI gap, Ghidra keytar |

---

## Bundle layout (observed)

### App package

- **Executable:** `Contents/MacOS/Postman` (Electron host).
- **Helpers (observed Frameworks):** `Postman Helper.app`, `Postman Helper (Renderer).app`, `Postman Helper (GPU).app`, `Postman Helper (Plugin).app`.
- **Electron Framework:** arm64 Mach-O under `Contents/Frameworks/Electron Framework.framework/...`.
- **Version (Info.plist):** `CFBundleShortVersionString` / `CFBundleVersion` = `12.32.3`.
- **URL scheme:** `postman://` (`CFBundleURLSchemes`).
- **ATS:** `NSAllowsArbitraryLoads` = true (**observed** plist).

### Resources

| Path | Role (observed / inferred) |
|------|----------------------------|
| `Resources/app.asar` (~154 MB) | Primary app JS/HTML/assets (**observed** inventory) |
| `Resources/data/worker/` | Unpacked worker assets incl. large `postman.bundled.js`, `conoutSocketWorker.js` (**observed** FS) |
| `Resources/data/postman-cli/` | Vendored CLI bin/lib (**observed** FS) |
| `Resources/data/plugin-installer/` | Plugin installer assets (**observed** FS) |
| `Resources/data/badge_overlays/` | Badge overlay PNGs (referenced from ASAR main-side badge service text) |
| `Resources/profile.json` | Build/profile stub (**observed** FS) |
| `Contents/postman-dataset-engine-darwin-arm64.tar.br` | Dataset engine binary archive (outside ASAR) |
| `Contents/spawn-helper` | Native helper binary |

### Electron fuses (**observed** `inspect_electron_fuses`)

| Fuse | State | Implication |
|------|-------|-------------|
| RunAsNode | enabled | `ELECTRON_RUN_AS_NODE` honored |
| EnableCookieEncryption | disabled | — |
| EnableNodeOptionsEnvironmentVariable | enabled | — |
| EnableNodeCliInspectArguments | disabled | Inspector CLI args not honored via fuse |
| EnableEmbeddedAsarIntegrityValidation | disabled | ASAR integrity **not enforced** by fuse |
| OnlyLoadAppFromAsar | disabled | Not restricted to app.asar-only |
| LoadBrowserProcessSpecificV8Snapshot | disabled | — |
| GrantFileProtocolExtraPrivileges | enabled | — |

---

## ASAR inventory highlights (**observed** `inspect_artifact`)

- **618** content nodes, **677** occurrences, **113** directories, **564** files.
- No nested ASAR containers (`nested_asar_containers: 0` in JS analysis stats).
- Key entry files (sizes from inventory occurrences):

| Logical path | Declared size | Kind |
|--------------|---------------|------|
| `package.json` | 3330 | resource |
| `main.js` | 1 659 717 | javascript-bundle |
| `startup.js` | 291 132 | javascript-bundle |
| `preload.js` | 9 779 | javascript-bundle |
| `preload_desktop.js` | 12 643 | javascript-bundle |
| `js/preload.js` | 374 | javascript-bundle |
| `AgentWorker.js` | 10 794 503 | javascript-bundle |
| `execution-worker.js` | 5 318 140 | javascript-bundle |
| `postman-runtime-981eed2565622025.min.js` | 4 710 809 | javascript-bundle |
| `ExecutionProcess.js` | 542 349 | javascript-bundle |
| `ProxyNodeProcess.js` | 273 593 | javascript-bundle |
| `process-manager.js` | 13 292 | javascript-bundle |
| `config.json` | 1 987 | resource |

---

## Process model (**observed** + **inferred**)

### Main process (**observed** naming / package.json `main`)

- **`main.js`** — Electron main entry (`package.json` field `"main":"main.js"`; ASAR member text at inventoried path).
- **`startup.js`** — large startup/bootstrap companion (**inferred** from name + size; semantic graph for `main.js` truncated).
- **Named IPC handler chunks (observed paths):**
  - `main-file-system-ipc-handlers-*.min.js`
  - `main-file-system-watcher-ipc-handlers-*.min.js`
  - `main-git-ipc-handlers-*.min.js`
  - `main-dataset-daemon-ipc-handlers-*.min.js`
  - `main-performance-test-ipc-handlers-*.min.js`
  - `main-ripgrep-ipc-handlers-*.min.js`
  - `main-recent-folders-ipc-handlers-*.min.js`
  - `main-webhooks-tunnel-forwarding-*.min.js`

### Renderer shells (**observed** HTML inventory)

Separate HTML entry documents under `html/`:

| HTML | Size | Likely UI surface (**inferred** from name + product-docs context) |
|------|------|---------------------------------------------------------------------|
| `requester.html` | 11816 | Primary request builder / workspace UI |
| `runner.html` | 4641 | Collection Runner |
| `scratchpad.html` | 9572 | Offline/scratchpad UI |
| `console.html` | 4846 | Console |
| `tester_sandbox.html` | 396 | Script sandbox page |
| `auth/auth.html` | 2113 | Auth browser flow |
| `proxyAuth.html` | 2340 | Proxy auth UI |
| `webProxy.html` | 198 | Web proxy |
| `desktop-offline.html` | 1122 | Offline mode |
| `loader.html` | 1000 | Boot loader |

`requester.html` (**ASAR member text**, path REA-inventoried) loads:

- `../js/preload.js`
- `../js/requester.css`, `../js/vendor-ui.js`, `../js/requester.js` (preload hints)

### Preload / bridge scripts (**observed** paths + JS analysis)

REA entry analysis (`ev_5d12...`): `context_bridge_apis: 3`, `exposed_api_members: 2`, IPC ops present.

| File | Notes |
|------|-------|
| `preload_desktop.js` | Renderer-side IPC wrapper (`ipcRenderer`) — **observed** webpack modules in ASAR text |
| `preload.js` (root) | Webpack bundle installing `epm.sdk` with **`ipcMain`** handlers — **observed** ASAR text; acts as main-side SDK, despite filename |
| `js/preload.js` | Tiny renderer stub referenced by requester HTML |
| `paymentPreload.js`, `browser_tab_recorder_preload.js`, `visualizer_view_preload.js` | Feature-specific preloads (**observed** paths) |
| `js/*/preload.js` | Migration/scratchpad firmware preloads |

### Worker / child processes (**observed** filenames; roles **inferred**)

| Module | Size | Inferred role |
|--------|------|---------------|
| `AgentWorker.js` | ~10.8 MB | Agent / network agent worker (heap exhausted during REA semantic analysis) |
| `execution-worker.js` | ~5.3 MB | Request/script execution worker |
| `ExecutionProcess.js` | ~542 KB | Execution process entry |
| `ProxyNodeProcess.js` | ~274 KB | Proxy node process |
| `process-manager.js` | ~13 KB | Perf-test / executor process manager |
| `ThreadGroupExecutable.js`, `perftest*.js` | — | Performance testing |
| `mock-server-runtime/` | directory | Local mock HTTP server runtime |
| Unpacked `data/worker/postman.bundled.js` | ~36 MB | Large bundled worker payload outside ASAR |

---

## package.json identity (**observed** ASAR member at inventoried path)

- **name:** Postman; **version:** 12.32.3; **engines.node:** 24.14.0; **packageManager:** yarn@4.18.0
- **main:** `main.js`
- Notable dependencies (architecture-relevant):
  - `postman-runtime`, `postman-collection`, `postman-request`
  - `@postman/runtime.platform`, `@postman/runtime.runtime-rpc-electron`
  - `keytar`, `@1password/sdk`, `@azure/msal-node`, `openid-client`
  - `electron-json-storage`, `nedb` (`@seald-io/nedb`), `node-ipc`
  - gRPC / MQTT / WebSocket stacks (`@postman/grpc-js`, `mqtt`, `ws`, `postman-ws`)
  - `@modelcontextprotocol/sdk`, DuckDB/dataset engine packages

---

## JS analysis coverage (full ASAR)

From `ev_86985...` summary:

- Parsed JS files: **149**; modules: **10538**; findings: **31899**
- Electron IPC recovered (counts only — names mostly dynamic):
  - `operations: 38`, `literal_channels: 8`, `dynamic_channel_operations: 30`
  - `renderer_transmissions: 27`, `main_handlers: 7`, **paired: 0**
- **Gaps:** semantic expansion truncated for `main.js`, `AgentWorker.js`, `ExecutionProcess.js`, `ProxyNodeProcess.js`, `execution-worker.js`, `js/authentication.js`, theme bundles, etc.
- `list_electron_targets` **not usable** — Postman not confirmed running with CDP endpoint in this session.

---

## Observed vs inferred

| Claim | Label |
|-------|-------|
| ASAR digest, file inventory, sizes, HTML shells, fuse states | **observed** |
| Main = `main.js`; multi-window HTML shells | **observed** (entry + inventory) |
| Request send path lives in `postman-runtime` + execution workers | **inferred** (deps + filenames; lifecycle detail in report 03) |
| Exact BrowserWindow construction / webPreferences | **not resolved** — REA `browser_windows: 0`, `explicit_web_preferences: 0` (dynamic or in truncated `main.js`) |
