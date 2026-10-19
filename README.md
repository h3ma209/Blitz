# ⚡ Blitz

<p align="center">
  <img src="assets/logo.png" alt="Blitz Logo" width="128" />
</p>

<p align="center">
  <strong>Fast. Lightweight. Offline-first. The API client built for developers who hate bloat.</strong>
</p>

<p align="center">
  <a href="#why-blitz">Why Blitz</a> •
  <a href="#features">Features</a> •
  <a href="#quick-start">Quick Start</a> •
  <a href="#cli-automation">CLI Runner</a> •
  <a href="#git-team-sync">Git Team Sync</a>
</p>

---

## Why Blitz?

Most API clients today are 1.5 GB Electron monsters that force you to create a cloud account just to send a `GET` request.

**Blitz is different:**
- **Zero cloud hostage:** Your collections, requests, and environments live on your machine as plain files.
- **Blazing fast:** Written in Rust with native WebView. Boots instantly, uses ~40MB RAM.
- **No mandatory login:** Open the app and start hacking immediately.
- **Git-native collaboration:** Sync with your team through standard Git repos, not a $49/seat SaaS subscription.
- **Drop-in migration:** Import your existing Postman collections and OpenAPI specs in seconds.

---

## Features

### 🔌 Multi-Protocol Powerhouse
Beyond plain HTTP — Blitz natively speaks modern protocols:
- **REST / HTTP** (Full headers, params, GraphQL, URL-encoded, multipart form-data + file upload)
- **WebSockets** (Interactive bidirectional sessions)
- **gRPC** (Unary + streaming RPCs)
- **MQTT** (IoT message publisher & subscriber)
- **SSE** (Server-Sent Events stream viewer)
- **Socket.IO** (Real-time engine support)
- **MCP** (Model Context Protocol client for AI agents)

### 🧪 Real JavaScript Sandbox
Test scripts powered by an embedded engine with Postman syntax compatibility:
- `epm.test()` and `epm.expect()` assertions
- `epm.sendRequest()` for chained, pre-request HTTP calls
- `CryptoJS` built-in (`SHA256`, `HmacSHA256`, Hex, Base64)
- Dynamic variables (`{{$guid}}`, `{{$timestamp}}`, `{{var}}`)

### 🔐 Native Secrets Vault
- OS Keychain integration (macOS Keychain, Linux Secret Service, Windows Credential Manager).
- Sensitive tokens and secrets stay strictly local and are never accidentally committed or leaked.

### 🔄 Open Ecosystem & Migration
- **Postman:** Import and export v2.1 collection format.
- **OpenAPI:** Import and export OpenAPI 3.0 specs with non-destructive 3-way merge.
- **cURL & HAR:** Paste raw cURL commands or import browser network HAR logs directly.
- **Local Mocks:** Spin up instant, zero-config mock servers bound to `127.0.0.1` directly from saved response examples.

---

## Quick Start

### Prerequisites
- [Rust](https://rustup.rs/) (1.83+)
- macOS, Linux, or Windows

### Run from Source
```bash
git clone https://github.com/h3ma209/Blitz.git
cd Blitz

# Run development build
cargo run

# Or run optimized release
cargo run --release
```

### Install as Desktop App
```bash
make install
```

| OS | Default Install Path | Uninstall |
|---|---|---|
| **macOS** | `~/Applications/Blitz.app` | `make uninstall` |
| **Linux** | `~/.local/share/blitz/` + `~/.local/bin/blitz` | `make uninstall` |
| **Windows** | `%LOCALAPPDATA%\Blitz\` | `powershell -File scripts/install.ps1 -Uninstall` |

---

## CLI Automation

Automate your API tests in GitHub Actions or any CI/CD pipeline without extra tools:

```bash
# Run collection headless (exits 0 on success, 1 on test failure)
blitz run collection.json --env staging.json
```

---

## Git Team Sync

Collaborate with your team without handing your data to a third-party cloud.

1. Top bar: **Cloud → Clone / link remote…**
2. Work, edit collections, and save locally.
3. Hit **Sync**: Blitz pulls, commits, and pushes through standard Git (`gh` or SSH).
4. Sensitive values (`vault.json`, session cookies, local auth tokens) are automatically redacted before push.

---

## License

Free and open-source. Not affiliated with Postman Inc. Built for developers who value speed, privacy, and simplicity.
