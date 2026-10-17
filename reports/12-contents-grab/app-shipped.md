# 12 — App shipped checklist (Phase B)

**Date:** 2026-10-17  
**Verify:** `cargo test` → **53 passed**, **5 ignored** (keychain + network smokes)

## Shipped

| Item | Status | How to try |
|------|--------|------------|
| WebSocket connect / send / disconnect | ✅ | Protocol → WebSocket → URL `wss://echo.websocket.events` → Connect → type message → Send |
| gRPC unary (Struct JSON) | ✅ | Protocol → gRPC → endpoint + method → JSON body → Invoke |
| MQTT publish / subscribe-once | ✅ | Protocol → MQTT → `mqtt://127.0.0.1:1883` + topic → Publish / Subscribe once |
| MCP STDIO list/call tools | ✅ | Protocol → MCP → command `npx` + args → List tools / Call tool |
| `pm.sendRequest` in boa sandbox | ✅ | HTTP Scripts tab: `pm.sendRequest(url, function(err,res){…})` |
| Capture proxy → history | ✅ | Top bar **Proxy** → point client at `http://127.0.0.1:{port}` (absolute-form HTTP) → **Stop proxy** flushes history |
| Perf VU runner | ✅ | Top bar **Perf** → set VUs + seconds → Start (active collection) |
| Local mock (existing) | ✅ | Save example → **Mock** → hit `http://127.0.0.1:{port}` |

## Deferred (documented)

| Item | Reason |
|------|--------|
| Socket.IO | Out of Phase B scope; deps observed in ASAR |
| Browser Interceptor / CONNECT MITM | Needs privileged bridge; proxy is HTTP-only |
| Full gRPC protoset / reflection UI | Unary Struct path only |
| Simulation scenarios | Local mock only |
| Cloud sync / SSO / Agent / Spec Hub / Flows / billing | Not cloneable from desktop alone |

## Crate choices (macOS)

| Feature | Crate | Note |
|---------|-------|------|
| WS | `tokio-tungstenite` 0.26 | rustls |
| MQTT | `rumqttc` 0.24 | sync Client |
| gRPC | `hyper` + `prost`/`prost-types` | no full `tonic` stack |
| MCP | hand-rolled STDIO JSON-RPC | Content-Length framing |

## Inventory

See [inventory.md](inventory.md).
