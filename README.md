# notPostman

Local-first API client (Rust + Dioxus desktop). Architecture guided by reverse-engineering notes in [`reports/`](reports/) — requester workbench, scripts, runner, local mocks.

**Not affiliated with Postman.** Recreation for learning / local use.

## Requirements

- Rust 1.83+ (tested with 1.95)
- macOS / Linux / Windows with WebView support (Dioxus desktop uses the system WebView)

Optional: [dioxus-cli](https://dioxuslabs.com/) (`cargo install dioxus-cli`). Homebrew `dx` may be **Deno’s** `dx` — prefer `cargo run`.

## Build / run

```bash
cd /Users/hema/Desktop/Experiments/notPostman
cargo check
cargo run
cargo test
```

## Layout

```
src/
  main.rs
  core/       # request, collection, env, history, cookies, impex, vault, oauth, sandbox, codegen, mock
  runtime/    # send pipeline + collection runner
  storage/    # JSON snapshot + trash
  secrets/    # macOS Keychain + memory fallback
  ui/         # workbench shell
reports/      # RE notes (Ghidra-only for native)
```

## What’s real vs stub

| Piece | Status |
|-------|--------|
| Request builder (params/headers/body/auth/scripts/settings) | **Real** |
| Body: none, raw, urlencoded, form-data (+ file path upload), GraphQL | **Real** |
| Response Pretty/Raw/Preview/Headers/Cookies/Tests + Save example | **Real** |
| `{{var}}` + `{{vault:key}}` + `secret://` | **Real** |
| Environments, history, console | **Real** |
| Auth: Bearer/Basic/API Key/Digest/Inherit/**OAuth2** (client_creds + auth_code) | **Real** |
| boa JS sandbox (`pm.*` expanded) | **Real** (no `pm.sendRequest`) |
| Collection tree, favorite/dup/sort, Trash | **Real** |
| Import v2.1 / cURL / OpenAPI 3; export v2.1 | **Real** |
| Code snippets (cURL / fetch / Python) | **Real** |
| Collection Runner (iterations, delay, data file) | **Real** |
| Local mock server from examples | **Real** |
| Proxy + PEM client cert | **Real**; PKCS#12 → convert to PEM |
| Multi-tab, ⌘⇧P palette, eye variables pane | **Real** |
| Cloud / Agent / Spec Hub / Flows / gRPC / WS / MQTT / MCP / Native Git / SSO | **Out of scope** |

## Try

1. `cargo run`
2. Send `{{baseUrl}}/get` (⌘↵)
3. Scripts → Tests with `pm.test` / `pm.expect`
4. Save example → **Mock** (binds `127.0.0.1`)
5. **Runner** with iterations / CSV data file
6. Auth → OAuth 2.0 → Get New Access Token
7. `</>` for code snippets; import cURL or OpenAPI JSON via sidebar ↓

## Feature catalog

Full inventory of shipped behavior, limits, and non-goals: **[FEATURES.md](FEATURES.md)**.

## Reports

See [`reports/README.md`](reports/README.md). Latest: `11-docs-gap-refresh`, `10-re-complete`. Native RE = **Ghidra only** (no Hopper).
