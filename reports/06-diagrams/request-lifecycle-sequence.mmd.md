# 06b — Request lifecycle sequence

**Caution:** Sequence is **inferred** from observed module inventory + `postman-runtime` / Electron RPC dependencies. REA did **not** recover a complete static call graph from Send → HTTP (`request` family relations = 0 in focused analysis; large workers truncated).

```mermaid
sequenceDiagram
  autonumber
  actor User
  participant UI as Requester renderer<br/>(html/requester.html)
  participant Pre as preload_desktop.js<br/>(ipcRenderer)
  participant Main as Electron main<br/>(main.js + IPC handlers)
  participant URS as UnifiedRuntimeService<br/>/ IPCAgent
  participant Exec as ExecutionProcess<br/>/ execution-worker
  participant RT as postman-runtime<br/>(+ postman-request)
  participant Proxy as ProxyNodeProcess<br/>/ proxyCertificateService
  participant Net as Target HTTP(S)

  User->>UI: Click Send
  Note over UI: Build Request model<br/>(postman-collection) — inferred
  UI->>UI: Resolve {{vars}} scopes — inferred
  UI->>UI: Queue pre-request scripts — inferred
  UI->>Pre: IPC send/invoke (channel often dynamic)
  Pre->>Main: ipcRenderer → ipcMain
  Main->>URS: Dispatch run
  URS->>Exec: Spawn/message worker
  Exec->>Exec: Run pre-request in sandbox<br/>(tester_sandbox) — inferred
  Exec->>Exec: Apply auth / OAuth refresh<br/>(AppRefreshTokenManager / process events)
  Exec->>RT: execute(collection item)
  alt Proxy / custom certs
    RT->>Proxy: Tunnel / cert selection — inferred
    Proxy->>Net: HTTPS request
  else Direct
    RT->>Net: HTTPS via postman-request
  end
  Net-->>RT: Response
  RT-->>Exec: Response + timings
  Exec->>Exec: Run test scripts — inferred
  Exec-->>URS: Result events
  URS-->>Main: IPC / RPC
  Main-->>Pre: reply / event
  Pre-->>UI: Render response pane — inferred
  UI-->>User: Status, headers, body
```

## Observed anchors used

- HTML shell `requester.html`
- Preload IPC wrapper
- Modules: UnifiedRuntimeService, IPCAgent, ExecutionProcess, execution-worker, AgentWorker, postman-runtime bundle, ProxyNodeProcess, proxyCertificateService
- Deps: `postman-runtime`, `postman-request`, `@postman/runtime.runtime-rpc-electron`

## Not proven by REA

- Exact channel name for Send
- Exact order of var resolution vs scripts in this binary build
- Whether AgentWorker vs ExecutionProcess owns the socket
