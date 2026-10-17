# EvilPostman

Local-first API client (Rust + Dioxus desktop). Architecture guided by reverse-engineering notes in [`reports/`](reports/) — requester workbench, scripts, runner, local mocks.

**Not affiliated with Postman.** Recreation for learning / local use.

## Requirements

- Rust 1.83+ (tested with 1.95)
- macOS / Linux / Windows with WebView support (Dioxus desktop uses the system WebView)
- Optional team sync: system `git` + GitHub auth (`gh auth login` or SSH agent)

Optional: [dioxus-cli](https://dioxuslabs.com/) (`cargo install dioxus-cli`). Homebrew `dx` may be **Deno’s** `dx` — prefer `cargo run`.

## Build / run

```bash
cd /Users/hema/Desktop/Experiments/notPostman
cargo check
cargo run
cargo test
```

## Git cloud sync (team)

Canonical remote: [`h3ma209/evilpostman-cloud`](https://github.com/h3ma209/evilpostman-cloud.git).

1. Auth once: `gh auth login` (HTTPS) or SSH key + `ssh -T git@github.com`
2. Run app → top-nav **Cloud → Clone / link remote…** (defaults to that URL; clone under `{data_dir}/evilpostman/cloud/`)
3. Pick an environment (Sync hidden for **No Environment**). Edit → **Save** commits locally when cloud+clone linked. **Sync** = pull → commit → push to share with team.
4. Advanced: **Cloud → Pull / Push**. No auto-pull. Local/script vars never leave this machine.

Never commit `vault.json`, cookies, or history — redacted in env JSON. Design: [`reports/14-cloud-sync-re/git-team-sync-design.md`](reports/14-cloud-sync-re/git-team-sync-design.md).

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
| Git team sync (evilpostman-cloud) | **Real** (Cloud menu + save→push; secrets stay local) |
| Postman Cloud / Agent / Spec Hub / Flows / SSO | **Out of scope** |

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
