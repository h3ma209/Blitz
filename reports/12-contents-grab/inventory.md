# 12 — Contents grab inventory

**Date:** 2026-10-17  
**Target:** `/Applications/Postman.app/Contents` (bundle **12.32.4**)  
**Extract used:** `.rea-scratch/re-complete-extract/app` (existing ASAR unpack; re-validated)  
**REA:** `analyze_javascript_application` focused on protocol UI/proxy/perf — Evidence `ev_c7a7741523f2310ba9658f49f472e5b99ca9b45a8cefde483fefc894968a05fd` (parent `ev_bcf5082b5d7692064cd918b2074159914007895e55c25e567614f2ed504087cc`).  
**Native RE:** inventory via `file` / `otool` / `strings` / `npx @electron/asar` — no Hopper; no `open_binary` fight for “complete clone”.

---

## 1. Tree of Contents (**observed**)

```
Contents/
├── _CodeSignature/CodeResources
├── Frameworks/
│   ├── Electron Framework.framework     (~207M)
│   ├── Mantle.framework
│   ├── ReactiveObjC.framework
│   ├── Squirrel.framework
│   ├── Postman Helper.app
│   ├── Postman Helper (GPU).app
│   ├── Postman Helper (Plugin).app
│   └── Postman Helper (Renderer).app
├── Info.plist
├── MacOS/
│   └── Postman                          (69K, Mach-O arm64)
├── PkgInfo
└── Resources/
    ├── app.asar                         (~154M)
    ├── app.icns
    ├── Credits.rtf
    ├── profile.json
    └── data/
        ├── badge_overlays/
        ├── plugin-installer/
        ├── postman-cli/
        ├── worker/
        ├── postman-dataset-engine-darwin-arm64.tar.br
        ├── dataset-engine-bin-package.json
        └── spawn-helper                 (69K helper)
```

### Info.plist highlights (**observed**)

| Key | Value |
|-----|--------|
| `CFBundleIdentifier` | `com.postmanlabs.mac` |
| `CFBundleShortVersionString` / `CFBundleVersion` | `12.32.4` |
| `CFBundleExecutable` | `Postman` |
| `LSMinimumSystemVersion` | `11.0` |
| `NSPrincipalClass` | `AtomApplication` (Electron) |
| `CFBundleURLSchemes` | `postman` |
| `NSAppTransportSecurity.NSAllowsArbitraryLoads` | `true` |
| `ElectronAsarIntegrity` | hash for `Resources/default_app.asar` (integrity metadata present) |

---

## 2. Role of `MacOS/Postman` (**observed** + **inferred**)

| Fact | Label |
|------|--------|
| Size ~69 KB; `file` → `Mach-O 64-bit executable arm64` | observed |
| `otool -L` → `@rpath/Electron Framework.framework/Electron Framework` | observed |
| Strings include `ELECTRON_RUN_AS_NODE` | observed |
| Same size class as `Resources/data/spawn-helper` | observed |
| Thin Electron bootstrap that loads Framework + `Resources/app.asar` — not the product logic binary | **inferred** |

Product logic lives in ASAR JS + Frameworks helpers (GPU / Renderer / Plugin), not in the 69K launcher.

---

## 3. ASAR members of interest vs notPostman

Dependencies from extracted `package.json` (**observed**):

| Package / member | Role (product) | notPostman before this pass | After Phase B |
|------------------|----------------|-----------------------------|-----------------|
| `ws`, `postman-ws`, `graphql-ws`, `subscriptions-transport-ws` | WebSocket / GraphQL-WS | unused | **WS client shipped** |
| `socket.io`, `socket.io-client-v2/v4` | Socket.IO | unused | **deferred** (not in Phase B scope) |
| `mqtt` + `@postman/runtime` mqtt-client bundles | MQTT | unused | **MQTT shipped** |
| `@postman/grpc-js`, grpc-reflection, runtime grpc-client | gRPC | unused | **unary happy path shipped** |
| `mcp-server-*.min.js`, `toolset-mcp-server/` | MCP server/tooling in desktop | unused | **MCP STDIO client shipped** (local; not Postman cloud MCP) |
| `js/webProxy.js`, `html/webProxy.html`, `http-proxy` | Desktop web proxy UI/process | unused | **capture proxy shipped** (lightweight local HTTP log proxy) |
| `main-performance-test-ipc-handlers-*.min.js`, performance-test vendors | Perf / VU IPC | unused | **minimal perf runner shipped** |
| `services_interceptorBridgeInstaller_*.min.js` | Browser interceptor bridge | unused | **deferred** (needs browser extension) |
| `mock-server-runtime/simulation-http-server-worker.js` | Simulation HTTP worker | partial (local mock only) | simulation scenarios **deferred** |

Scratchpad WS workbench: `js/scratchpad/ECWebsocketWorkbench.js` (**observed** path).

---

## 4. What cannot be cloned from desktop alone (**observed** / **inferred**)

| Capability | Why desktop ASAR is insufficient |
|------------|----------------------------------|
| Cloud sync / workspaces / roles / SSO / SCIM / billing | Server + account APIs; not in local bundle as product surface |
| Agent Mode / AI / Passport / Fabric | Cloud-backed; CDN requester UI already absent locally (`js/requester.js` missing — prior reports) |
| Spec Hub, Flows, cloud Monitors, API Catalog / Insights | Cloud products |
| Published cloud mocks / monitors | Require Postman Cloud |
| Full Socket.IO / GraphQL subscriptions UI parity | Libraries present; Phase B scoped to WS/MQTT/gRPC/MCP |
| Browser Interceptor MITM | Needs extension + privileged install bridge |
| Exact Postman sandbox (`postman-sandbox` / uvm) | Different JS engine; we ship boa subset + `pm.sendRequest` |

Honest framing: notPostman = **local desktop API workbench**, Postman-inspired architecture from RE — **not** a trademark clone or Cloud substitute.

---

## 5. Build gaps (Phase B resolved)

| Feature | Shipped with | Gap |
|---------|--------------|-----|
| WebSocket | `tokio-tungstenite` 0.26 | Socket.IO / GraphQL-WS UI deferred |
| MQTT | `rumqttc` 0.24 | TLS brokers / auth UI minimal |
| gRPC unary | `hyper` HTTP/2 + `prost-types` Struct | No tonic; no reflection / `.proto` import |
| MCP | hand-rolled STDIO JSON-RPC | Not Postman cloud MCP / toolset host |
| Capture proxy | std `TcpListener` + reqwest forward | No CONNECT MITM |
| Perf VUs | threads + send pipeline | Not Postman’s performance-test IPC |

See [app-shipped.md](app-shipped.md).

### REA evidence (this pass)

| Operation | Evidence ID |
|-----------|-------------|
| `analyze_javascript_application` (package + webProxy + WS workbench + perf IPC) | `ev_c7a7741523f2310ba9658f49f472e5b99ca9b45a8cefde483fefc894968a05fd` |
