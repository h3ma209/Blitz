# 02 — UI architecture

## Evidence sources

| Tool | ID / note |
|------|-----------|
| `inspect_artifact` inventory | HTML + JS paths under `html/`, `js/` |
| `analyze_javascript_application` entry set | `ev_5d12fac6ad3d0fd45d791d1dd1b825ccba653e4e91b1f04a11f30f7d2c8f0190` |
| `analyze_javascript_application` auth/preload set | `ev_e913e1f44bef62931657babeb16676adf900781c1f7dd32cd753bb36eef8e976` |
| ASAR member text | `html/requester.html`, `preload_desktop.js`, root `preload.js` (paths REA-inventoried) |
| Full ASAR JS summary | `ev_86985f56c5339657d605888162b24ae2d2ae92eebce30f354059ce69314d7103` |

**product-docs context (not binary evidence):** Postman UI has request builder tabs (Params, Headers, Body, Auth), response viewer, sidebar (Collections/History/Environments), and multi-tab workspaces. Map those UX concepts onto shells below — do **not** treat as REA class names.

---

## Renderer shells (**observed**)

Primary UI is multi-document Electron webviews:

1. **Requester** — `html/requester.html` → scripts `js/requester.js`, `js/vendor-ui.js`, CSS `js/requester.css`, stub `js/preload.js`. **Gap (deeper RE):** extracted ASAR has **no** `js/requester.js`; UI update URL `https://desktop.postman.com` (**observed** config) — primary UI likely remote-served.
2. **Runner** — `html/runner.html` (+ inferred `js/runner*` assets if present; not all inventoried in this pass).
3. **Scratchpad** — `html/scratchpad.html` + large `js/scratchpad/**` chunk tree (CollectionSidebar, ECItemWorkbench, workers, importers).
4. **Console** — `html/console.html`.
5. **Tester sandbox** — `html/tester_sandbox.html` (script isolation surface).
6. **Auth / proxy** — `html/auth/auth.html`, `html/proxyAuth.html`, `html/webProxy.html`.

### Request builder / response / sidebar

| Concern | REA status |
|---------|------------|
| Request builder “classes” | **Not found** as clear unminified class names. UI lives in **minified webpack bundles** (`js/requester.js` referenced by HTML; full semantic expansion of large UI bundles **not completed** — budget/heap). |
| Response viewer | **Not isolated** in REA graph. **Inferred:** same requester renderer bundle. |
| Sidebar (collections/history/env) | **Observed** scratchpad path `js/scratchpad/CollectionSidebar.js`. Requester-side equivalent likely inside `js/requester.js` (**inferred**, unrecovered). |
| Tab/workspace manager | **Not observed** as named module. Cloud/workspace deps appear in `package.json` workspaces metadata pointing at internal `workspace-data` / `workspace-scope` packages (build-time monorepo paths, not shipped as separate ASAR roots). |

**State management:** REA did **not** recover a clear Redux/MobX/Zustand root from summary views. Scratchpad tree includes many controllers (`*Controller.js`) — **observed** filenames only. Exact store library for requester shell = **unknown** (minified).

---

## Preload / IPC bridge (**observed**)

### Desktop renderer preload — `preload_desktop.js`

Webpack modules recovered in ASAR text (path REA-inventoried):

- `./preload/common/index.js` — `isElectron` detection
- `./preload/desktop/file-path.js` — `webUtils.getPathForFile`, deprecated `File.path` shim
- `./preload/desktop/ipc.js` — class wrapping `electron.ipcRenderer`:
  - `subscribe`, `send`, `sendToHost`, `sendSync` (deprecated warn), `invoke`

Literal channel strings recovered from that file / evidence scan (**observed** ASAR text + REA evidence channel mining):

- `deprecated-api-event`
- `get-runtime-config-overrides`
- `runtime-config-unavailable`

### Root `preload.js` (main-side SDK)

Despite name, bundle uses **`ipcMain`** and sets `pm.sdk` (**observed** ASAR text). Handlers:

| Channel / API | Role |
|---------------|------|
| `check-app-ownership` | `handle` → ownership check |
| `app-badge:set-count` | `handle` → dock/taskbar badge |
| `app-badge:analytics-ready` | `handle` |
| `screen-event` | `webContents.send` from powerMonitor lock/unlock |
| `app-badge:foregrounded-with-badge-visible` | notify renderer |

Also references `pm.eventBus.channel("model-events")` for badge suppress on logout/switch (**observed** string).

### Feature preloads (auth analysis set)

REA `ev_e913...`: `context_bridge_apis: 3`, `exposed_api_members: 2`, `literal_channels: 3`.

Evidence-mined channel names also include:

- `payments:view:close|open|copyToClipboard|update-bounds`
- `browser-tab:interactions:dom-event`
- `ripgrep-search`
- `@electron/remote` internal `REMOTE_*` channel constants (from `@electron/remote` dependency — **observed** in ASAR `node_modules/@electron/remote`)

---

## Main ↔ renderer IPC (**observed** counts + named handler modules)

### Counts (full ASAR analysis — partial graph)

- 38 IPC operations; **8 literal** channels; **30 dynamic** channel ops
- 27 renderer transmissions; 7 main handlers; **0 paired** (static pairing failed — dynamic names)
- REA limitation (verbatim): *“Dynamic IPC channel expressions remain unresolved and are never paired by textual similarity.”*

### Named main IPC chunks (**observed** inventory paths)

| Module | Size | Domain (**inferred** from filename) |
|--------|------|-------------------------------------|
| `main-file-system-ipc-handlers-*.min.js` | 33789 | FS access |
| `main-file-system-watcher-ipc-handlers-*.min.js` | — | FS watch |
| `main-git-ipc-handlers-*.min.js` | 8371 | Git |
| `main-dataset-daemon-ipc-handlers-*.min.js` | 7243 | Dataset daemon |
| `main-performance-test-ipc-handlers-*.min.js` | 8675 | Perf tests |
| `main-ripgrep-ipc-handlers-*.min.js` | 768 | Search (`ripgrep-search` channel seen) |
| `main-recent-folders-ipc-handlers-*.min.js` | — | Recent folders |
| `main-webhooks-tunnel-forwarding-*.min.js` | 12134 | Webhook tunnel |
| `runtime_agents_IPCAgent_js-*.min.js` | 13353 | Runtime IPC agent — channel `@postman/runtime`, ops `connect`/`message`/`disconnect` (**observed** ASAR text; see 08) |

**Do not invent** channel name catalogs beyond literals listed above — majority are dynamic.

---

## Themes / chrome (**observed**)

Multiple `js/THEME_*.js` modules in ASAR graph (Monokai, Night Owl, Solarized, Ayu, …). Semantic expansion often truncated (budget). Confirms theming is client-side JS modules, not OS-only.

---

## Gaps

1. No recoverable unminified React/Vue component tree for request builder tabs.
2. No static BrowserWindow / `webPreferences.preload` wiring (`browser_windows: 0` in REA).
3. Full IPC channel map **unavailable** without runtime CDP (`list_electron_targets` unused — no CDP endpoint).
4. State management library for requester = **unknown**.
