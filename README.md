# ⚡ Blitz

<p align="center">
  <img src="assets/logo.png" alt="Blitz Logo" width="128" />
</p>

<p align="center">
  <strong>Fast, native, offline-first API development client built with Rust.</strong><br/>
  <em>Lightweight performance, multi-protocol versatility, and zero cloud lock-in.</em>
</p>

<p align="center">
  <a href="#why-blitz">Why Blitz</a> •
  <a href="#key-features">Features</a> •
  <a href="#quick-start">Quick Start</a> •
  <a href="#ci-cd-automation">CLI Runner</a> •
  <a href="#team-collaboration">Team Sync</a> •
  <a href="#license">License</a>
</p>

---

## Why Blitz?

Most modern API clients have become bloated, memory-heavy cloud platforms that gate basic workflows behind subscriptions and force private endpoints onto remote servers.

Blitz is built from the ground up to be fast, private, and developer-friendly:

- **Ultra-lightweight:** Starts in milliseconds and consumes under 40 MB of RAM (native OS WebView, no Chromium bloat).
- **Offline-First & Local:** Collections, environments, and mocks live as transparent JSON files directly in your repository.
- **Zero Cloud Lock-in:** No forced logins, accounts, or telemetry. Your endpoints and tokens never leave your machine.
- **Git-Native Team Sync:** Collaborate with your team using standard Git workflows (`git pull` / `git push`) instead of per-seat cloud subscriptions.

---

## Key Features

### 🔌 Multi-Protocol Support
Interact with any modern API standard in one place:
- **REST / HTTP:** Query parameters, headers, GraphQL, form-data, multipart uploads.
- **WebSockets:** Interactive bidirectional streaming and frame inspection.
- **gRPC:** Unary calls and streaming with Protobuf schema reflection and decoding.
- **MQTT:** Real-time publish and subscribe to IoT brokers.
- **SSE (Server-Sent Events):** Live streaming event reader.
- **Socket.IO:** Native engine handshake and event listener.
- **MCP (Model Context Protocol):** Direct connection to AI agent tooling and context servers.

### 📑 Comprehensive Collection Workspace
Manage complex API suites with dedicated tabs:
- **Overview:** Collection documentation, structure preview, and real-time statistics.
- **Authorization:** Inherited authentication (Bearer, Basic, API Key, OAuth2) applied across child requests.
- **Pre-request & Test Scripts:** Collection-wide scripting for setup, teardown, and validation.
- **Variables:** Environment and collection variable scope resolution.
- **Runner:** Batch test execution with iteration controls, delays, and pass/fail diagnostics.

### 🧪 Postman-Compatible JavaScript Testing
Write familiar assertions and workflows locally without remote runtime limits:
- `blitz.test()` and `blitz.expect()` assertion suites.
- `blitz.sendRequest()` for chained requests and token pre-fetching.
- Built-in `CryptoJS` library (`SHA256`, `HmacSHA256`, `MD5`, Base64, Hex).
- Dynamic variables (`{{$guid}}`, `{{$timestamp}}`, `{{var}}`).

### 🔐 Secure Credential Storage
Protect sensitive environment secrets and tokens:
- Native OS Keychain integration (macOS Keychain, Linux Secret Service, Windows Credential Manager).
- Secrets stay encrypted locally and are automatically stripped before committing to Git remotes.

### 🔄 Seamless Migration & Local Mocks
- **Postman:** Import and export Postman v2.1 collections with full script fidelity.
- **OpenAPI:** Import and export OpenAPI 3.0 specifications.
- **cURL & HAR:** Paste raw terminal cURL commands or drop browser HAR files directly into the workbench.
- **Local Mock Server:** Instant zero-config HTTP mocks on `127.0.0.1` served from your saved examples.

---

## Quick Start

### Prerequisites
- [Rust Toolchain](https://rustup.rs/) (1.83+)
- macOS, Linux, or Windows

### Build and Run from Source
```bash
# Clone repository
git clone https://github.com/h3ma209/Blitz.git
cd Blitz

# Run debug build
cargo run

# Or run optimized release build
cargo run --release
```

### Install as Native Desktop App
```bash
make install
```

| Operating System | Default Install Destination | Uninstall |
|---|---|---|
| **macOS** | `~/Applications/Blitz.app` | `make uninstall` |
| **Linux** | `~/.local/share/blitz/` + `~/.local/bin/blitz` | `make uninstall` |
| **Windows** | `%LOCALAPPDATA%\Blitz\` | `powershell -File scripts/install.ps1 -Uninstall` |

---

## CI/CD Automation

Run test collections headless in your CI/CD pipelines without cloud subscriptions or proprietary API keys:

```bash
# Execute collection headless (returns exit code 0 on pass, 1 on failure)
blitz run collection.json --env staging.json
```

---

## Team Collaboration via Git

Sync team workspaces directly through Git without cloud intermediaries:

1. **Link Git Remote:** In the top toolbar, select **Team → Link Git Remote…**
2. **Work Locally:** Add endpoints, edit environments, and write tests.
3. **Sync:** Click **Sync** — Blitz pulls changes, handles merges, and pushes through standard Git (SSH or `gh` CLI).
4. **Leak Prevention:** Passwords, tokens, and local secret variables are kept local and excluded from commits.

---

## License

Distributed under the MIT / Apache 2.0 open-source licenses. Not affiliated with or endorsed by Postman Inc.
