# ⚡ Blitz

<p align="center">
  <img src="assets/logo.png" alt="Blitz Logo" width="128" />
</p>

<p align="center">
  <strong>Lightweight, native Postman alternative built with Rust & Dioxus.</strong><br/>
  <em>Offline-first, multi-protocol (REST, gRPC, WebSocket, MQTT, SSE, MCP), ~10MB RAM runner, zero cloud lock-in.</em>
</p>

<p align="center">
  <a href="https://github.com/h3ma209/Blitz/releases"><img src="https://img.shields.io/github/v/release/h3ma209/Blitz?include_prereleases&label=release&color=22c55e" alt="Release"></a>
  <a href="https://github.com/h3ma209/Blitz/actions/workflows/build.yml"><img src="https://img.shields.io/github/actions/workflow/status/h3ma209/Blitz/build.yml?branch=master&label=build" alt="CI Build"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue" alt="License"></a>
  <a href="https://h3ma209.github.io/Blitz/"><img src="https://img.shields.io/badge/docs-GitHub%20Pages-brightgreen" alt="Docs"></a>
</p>

<p align="center">
  <a href="#downloads">Downloads</a> •
  <a href="#interface-preview">Interface</a> •
  <a href="#comparison">Comparison</a> •
  <a href="#key-features">Features</a> •
  <a href="#ci-cd-automation">CLI Runner</a> •
  <a href="#team-collaboration">Git Sync</a> •
  <a href="#license">License</a>
</p>

---

## 📸 Interface Preview

<p align="center">
  <img src="docs/app-screenshot.png" alt="Blitz Desktop Interface" width="900" style="border-radius: 8px; border: 1px solid rgba(255,255,255,0.1);" />
</p>

---

## 📦 Downloads & Installation

### Quick Install (macOS & Linux)
```bash
curl -fsSL https://raw.githubusercontent.com/h3ma209/Blitz/master/install.sh | bash
```
*Automatically downloads the latest binary, installs to `/Applications` or `~/.local/bin`, and clears Gatekeeper quarantine.*

### Homebrew Tap
```bash
brew tap h3ma209/blitz https://github.com/h3ma209/homebrew-blitz
brew install --cask blitz
```

### Direct Downloads
Pre-built binaries for macOS, Linux, and Windows are available directly from the **[Releases Page](https://github.com/h3ma209/Blitz/releases)**.

| Platform | Download | Package / Command |
|---|---|---|
| **macOS (Apple Silicon)** | [Blitz-macos-arm64.dmg](https://github.com/h3ma209/Blitz/releases/latest) | Drag to Applications |
| **macOS (Tarball)** | [blitz-macos-arm64.tar.gz](https://github.com/h3ma209/Blitz/releases/latest) | `tar -xzf blitz-macos-arm64.tar.gz` |
| **Linux (x86_64)** | [blitz-linux-x86_64.tar.gz](https://github.com/h3ma209/Blitz/releases/latest) | `tar -xzf blitz-linux-x86_64.tar.gz` |
| **Windows (x86_64)** | [blitz-windows-x86_64.zip](https://github.com/h3ma209/Blitz/releases/latest) | Unzip & run `blitz.exe` |
| **Cargo (Any OS)** | Source crate | `cargo install --git https://github.com/h3ma209/Blitz.git` |

> **Manual DMG install note**: If macOS displays *"Blitz is damaged"* or *"Apple cannot verify"*, clear the Gatekeeper quarantine flag in Terminal:
> ```bash
> xattr -cr /Applications/Blitz.app
> ```

---

## ⚡ Comparison

| Feature | Postman | Bruno | Yaak | ⚡ Blitz |
|---|:---:|:---:|:---:|:---:|
| **Language / Architecture** | Electron + Node | Electron + Node | Tauri + React | **Pure Rust + Dioxus** |
| **Cold Start Latency** | ~3,500 ms | ~1,200 ms | ~450 ms | **< 150 ms** |
| **CLI Runner Footprint** | ~800 MB RSS | ~120 MB RSS | N/A | **~10.2 MB RSS** |
| **Account Mandatory** | Yes (forced cloud) | No | No | **No (100% offline)** |
| **Team Collaboration** | Cloud servers ($19+/user) | Git (.bru format) | Git / Sync | **Git-native (any remote)** |
| **Secret Storage** | Cloud / Local | Plaintext env | SQLite | **OS Keychain (Encrypted)** |
| **Built-in Mock Server** | Cloud hosted only | No | No | **Localhost 127.0.0.1** |
| **Traffic Proxy Capture** | Cloud proxy | No | No | **Local HTTP/HTTPS Proxy** |
| **License** | Proprietary | MIT | Fair-Core / Commercial | **MIT / Apache-2.0** |

---

## Why Blitz?

Most modern API clients have become bloated, memory-heavy cloud platforms that gate basic workflows behind subscriptions and force private endpoints onto remote servers.

Blitz is built from the ground up to be fast, private, and developer-friendly:

- **Ultra-lightweight:** Cold starts in ~120ms with a headless CLI runner that sits at ~10 MB RAM and a desktop GUI that uses native OS WebViews (no bundled Chromium or multi-process Electron bloat).
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
