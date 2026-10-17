# 07 — Pseudocode: primary request send

**Status:** Reconstruction pseudocode for notPostman guidance.  
**Authority:** Steps marked **inferred** unless tied to an **observed** artifact.

REA did **not** decompile a single named `sendRequest` export with full data-flow. This is a synthesis of observed modules (`postman-runtime`, ExecutionProcess/workers, UnifiedRuntimeService/IPCAgent, auth/proxy services) + Postman runtime conventions (**product-docs context** only where noted).

```
function handleSendClick(uiRequestDraft):                     // inferred UI entry
    request := Collection.Request.from(uiRequestDraft)       // observed dep: postman-collection
    context := buildVariableContext(
        globals, collectionVars, environment, local, data    // scopes: product-docs; storage unknown
    )

    // --- Variable resolution ---
    request := resolveMustacheTemplates(request, context)    // inferred inside postman-runtime

    // --- Bridge to main / runtime ---
    // observed: preload_desktop ipcRenderer wrapper;
    //           UnifiedRuntimeService, IPCAgent, runtime-rpc-electron
    handle := RuntimeBridge.startRun({                       // inferred API shape
        request,
        context,
        scripts: {
            prerequest: uiRequestDraft.events.prerequest,
            test:       uiRequestDraft.events.test
        },
        settings: { timeout, followRedirects, ... },
        certificates: ProxyCertificateService.lookup(...),   // observed module name
        proxy: resolveProxy(system | custom)                 // observed GET_SYSTEM_PROXY events in process-manager
    })

    // --- Execution process / worker ---
    // observed: ExecutionProcess.js, execution-worker.js, AgentWorker.js
    worker := ExecutionSystem.acquireWorker()                // inferred

    on worker:
        // Pre-request scripts (sandbox: html/tester_sandbox.html observed)
        runScripts(prerequest, pmApi, context)               // inferred; pm.* shape unrecovered

        // Auth
        // observed modules: OAuth2WindowManager, AppRefreshTokenManager,
        //                   js/authentication.js (truncated), keytar.node, 1Password SDK
        request := injectAuth(request, authConfig, secretsStore)

        // Network
        // observed: postman-runtime + postman-request; optional ProxyNodeProcess
        response := postmanRuntime.execute(request, {
            requester: postmanRequest,
            agents: AgentWorker | ProxyNodeProcess
        })

        // Tests
        runScripts(test, pmApi.with(response), context)      // inferred

        emit(response, assertions, consoleLogs)              // inferred events

    // --- Renderer ---
    ui.renderResponse(response)                              // inferred requester.js
    ui.updateHistory(request, response)                      // inferred
    return response
```

## Credential handling (detail)

```
function secretsStore.get(key):
    // observed deps/backends — selection policy unrecovered
    try return Keytar.getPassword(service, account)          // keytar.node observed
    try return OnePasswordSdk.resolve(ref)                   // @1password/sdk observed
    try return AzureVault.getSecret(ref)                     // config vault Azure IDs observed
    try return AesCrypto.decrypt(localBlob)                  // @postman/aes-crypto-js observed
    fail closed
```

## Script order (convention — not REA-proven for this build)

1. Variable resolve  
2. Pre-request scripts  
3. Auth injection (may refresh OAuth — observed refresh-token manager + executor events)  
4. Send  
5. Test scripts  
6. Visualizer (visualizer_view_preload.js observed as separate preload)

## Gaps to close before trusting this as exact Postman behavior

1. Capture live Send with `list_electron_targets` + `capture_electron_scenario` / CDP.  
2. Re-run `analyze_javascript_application` on `js/requester.js` alone if extractable.  
3. Inspect exported `ev_570eb...` postman-runtime feature trace offline (exceeded MCP size).  
4. Open `keytar` call sites via targeted string/module traces after smaller graphs.
