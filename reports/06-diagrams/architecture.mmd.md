# 06a — Component / architecture diagram

Derived from REA inventory + Electron fuse/package observations. Solid boxes = **observed** artifacts; dashed = **inferred** roles.

```mermaid
flowchart TB
  subgraph Host["Postman.app Electron Host — observed"]
    EXE["MacOS/Postman"]
    EF["Electron Framework arm64"]
    HELPERS["Helper Renderer/GPU/Plugin apps"]
  end

  subgraph Resources["Contents/Resources — observed"]
    ASAR["app.asar\nsha256 f07f2970…"]
    DATA["data/\nworker, postman-cli,\nplugin-installer, badge_overlays"]
    DS["dataset-engine tar.br / spawn-helper"]
  end

  subgraph Main["Main process — observed entry + chunks"]
    MAIN["main.js"]
    START["startup.js"]
    IPCFS["main-*-ipc-handlers*.min.js"]
    PMSDK["preload.js as epm.sdk\nipcMain handlers"]
    OAUTH["OAuth2WindowManager"]
    PROXYCERT["proxyCertificateService"]
    KEYTAR["keytar.node native"]
  end

  subgraph Renderers["Renderer HTML shells — observed"]
    REQ["html/requester.html\n+ js/requester.js"]
    RUN["html/runner.html"]
    SCR["html/scratchpad.html"]
    CON["html/console.html"]
    AUTH["html/auth/*.html"]
    TEST["html/tester_sandbox.html"]
  end

  subgraph Preloads["Preloads — observed"]
    PDD["preload_desktop.js\nipcRenderer wrapper"]
    JP["js/preload.js stub"]
    PAY["payment / visualizer /\nbrowser_tab_recorder preloads"]
  end

  subgraph Runtime["Request execution — observed modules; flow inferred"]
    URS["UnifiedRuntimeService"]
    IPCA["IPCAgent"]
    PR["postman-runtime bundle"]
    EP["ExecutionProcess.js"]
    EW["execution-worker.js"]
    AW["AgentWorker.js"]
    PNP["ProxyNodeProcess.js"]
  end

  subgraph Mocks["Mocks / extras — observed"]
    MSR["mock-server-runtime/"]
    MCP["mcp-server / toolset-mcp-server"]
    DUCK["duckdb / dataset-daemon"]
  end

  EXE --> EF
  EXE --> ASAR
  EXE --> DATA
  MAIN --> START
  MAIN --> IPCFS
  MAIN --> PMSDK
  MAIN --> KEYTAR
  MAIN --> OAUTH
  MAIN --> PROXYCERT

  REQ --> JP
  REQ --> PDD
  RUN --> PDD
  SCR --> PDD

  PDD -. IPC dynamic channels .-> MAIN
  PMSDK --> MAIN

  REQ -. Send inferred .-> URS
  URS --> IPCA
  IPCA --> EP
  EP --> EW
  EP --> AW
  EP --> PR
  PR --> PNP
  PROXYCERT -.-> PNP

  MAIN --> MSR
  MAIN --> MCP
  MAIN --> DUCK
```

## Notes

- IPC edges mostly **dynamic** — REA reported 0 statically paired transmissions.
- `BrowserWindow` construction not recovered (`browser_windows: 0`).
