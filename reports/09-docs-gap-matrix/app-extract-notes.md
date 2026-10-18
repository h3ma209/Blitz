# App extract notes (through docs-map gap close)

## Wave A (earlier P0/P1)

Environments, history, vault `{{vault:}}`, boa sandbox (minimal), Digest, proxy, PEM certs, Collection Runner sequential, v2.1 impex, console, cookies, shortcuts.

## Wave B (plan: Docs-map gap close)

| Feature | Where |
|---------|--------|
| OAuth2 client_credentials + auth_code loopback | `core/oauth.rs`, Auth UI, pipeline Bearer inject |
| Form-data file path upload | pipeline multipart `Part::bytes` |
| GraphQL body mode | `BodyMode::Graphql`, pipeline JSON |
| Expanded `epm.*` (vault, cookies, request, execution, iterationData, headers) | `core/sandbox.rs` |
| Collection/folder scripts + inheritance | `SendOptions.inherited_*`, runner |
| Runner iterations / delay / CSV\|JSON data | `runtime/runner.rs`, RunnerPanel |
| cURL + OpenAPI 3 import | `core/impex.rs` |
| Code snippets | `core/codegen.rs`, `</>` modal |
| Local mock from examples | `core/mock_server.rs`, Mock button |
| Save as example | ResponseViewer + `SavedExample` |
| Response Cookies / Preview tabs | `response_viewer.rs` |
| Trash soft-delete | `trash_collection` / restore |
| Favorite / duplicate / sort | sidebar + collection |
| Multi-tab + palette ⌘⇧P + eye pane | workbench |
| PKCS#12 | clear error — use PEM |

## Still out of scope

Cloud sync, Agent Mode, Spec Hub, Flows, Monitors, gRPC/WS/MQTT/MCP, Native Git, SSO, Interceptor, `epm.sendRequest`.
