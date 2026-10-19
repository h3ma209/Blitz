# ⚡ Blitz: The API Manifesto

<p align="center">
  <img src="assets/logo.png" alt="Blitz Logo" width="128" />
</p>

<p align="center">
  <strong>A spectre is haunting software engineering — the spectre of Cloud-Hostage API Tooling.</strong><br/>
  <em>Workers of the terminal, unite! You have nothing to lose but your subscriptions.</em>
</p>

<p align="center">
  <a href="#i-bourgeois-and-proletarians-the-crisis-of-modern-tooling">The Crisis</a> •
  <a href="#ii-the-ten-commandments-of-blitz">Ten Commandments</a> •
  <a href="#iii-the-arsenal-features">The Arsenal</a> •
  <a href="#iv-mobilization-quick-start">Mobilization</a> •
  <a href="#v-ci-automation-for-the-people">CLI Automation</a> •
  <a href="#vi-collective-ownership-git-sync">Git Team Sync</a>
</p>

---

## I. Bourgeois and Proletarians: The Crisis of Modern Tooling

All the decaying powers of legacy SaaS have entered into an unholy alliance to tax your HTTP requests:
- **1.5 GB Electron Goliaths** consuming your workstation's RAM just to send an unauthenticated `GET`.
- **Mandatory Cloud Logins** holding your private endpoints, local environments, and team tokens hostage on remote servers.
- **Paywalled Collaboration** charging \$49 per seat per month for features Git solved twenty-five years ago.
- **Corporate Telemetry** snooping on every endpoint, payload, and query param you test.

**Blitz is the revolutionary alternative:**
Fast. Native. Offline-first. Written in Rust. Your collections belong to you, stored as plain files under your sovereign control.

---

## II. The Ten Commandments of Blitz

1. **Abolition of Cloud Captivity:** No cloud account shall ever be required to transmit a single byte.
2. **Expropriation of Bloat:** Memory usage under 40 MB. Sub-second cold start. Native OS WebView instead of Chromium tyranny.
3. **Sovereignty of Plain Files:** Collections, requests, environments, and runs remain transparent JSON files in your local repo.
4. **Democratic Team Collaboration:** Sync team workspaces directly through Git repositories (`git pull` / `git push`), abolishing per-seat SaaS extortion.
5. **Universal Protocol Liberation:** Equal rights for REST, WebSockets, gRPC, MQTT, SSE, Socket.IO, and MCP.
6. **Inviolable Secrets:** Tokens reside strictly in your OS Keychain (macOS Keychain, Linux Secret Service, Windows Credential Manager). Zero leaks to public remotes.
7. **Free Movement of Collections:** Seamless, lossless import and export of Postman v2.1 and OpenAPI 3.0 specs.
8. **Power to the Workspace:** Deep, tabbed collection editing right in the main pane — Overview docs, inherited Authorization, Pre/Test Scripts, Variables, and Runs.
9. **Headless Execution for All:** A zero-dependency CLI runner (`blitz run`) ready for any CI pipeline without proprietary tokens.
10. **Seize the Means of Production:** 100% Free and Open Source for every engineer across all operating systems.

---

## III. The Arsenal (Features)

### 🔌 Multi-Protocol Vanguard
Break free from plain REST isolation. Blitz natively speaks the entire dialect of modern systems:
- **REST / HTTP:** Query params, headers, GraphQL, form-urlencoded, multipart file uploads.
- **WebSockets:** Live interactive bidirectional stream inspection.
- **gRPC:** Unary RPCs and streaming client with protobuf decoding.
- **MQTT:** Publish and subscribe to IoT brokers in real time.
- **SSE (Server-Sent Events):** Real-time streaming event viewer.
- **Socket.IO:** Native engine handshake and event listener.
- **MCP (Model Context Protocol):** Direct connection to AI agent tooling and context servers.

### 📑 Main-Pane Collection Command Center
Edit entire collections without cramped sidebars or claustrophobic modals. Open any collection into the main workbench:
- **Overview:** Project documentation with live statistics (requests, folders, variables).
- **Authorization:** Collection-level Bearer, Basic, API Key, or OAuth2 inherited automatically by every child request.
- **Scripts:** Collection Pre-request and Test scripts executing uniformly across all endpoints.
- **Variables:** Full key-value dictionary with real-time resolution and persistence.
- **Runs:** Integrated test runner with iterations, delays, live pass/fail diagnostics, and test metrics.

### 🧪 Real JavaScript Testing Engine
Compatible with standard Postman syntax without cloud surveillance:
- `blitz.test()` and `blitz.expect()` assertions.
- `blitz.sendRequest()` for chained, pre-request HTTP calls.
- `CryptoJS` built-in (`SHA256`, `HmacSHA256`, `MD5`, Base64, Hex).
- Dynamic variable expansion (`{{$guid}}`, `{{$timestamp}}`, `{{var}}`).

### 🔐 OS Keychain Secret Vault
Sensitive tokens, Bearer strings, and environment passwords belong in the worker's hardware, not plain text files:
- macOS Keychain, Linux Secret Service, and Windows Credential Manager integration.
- Sensitive environment values stay masked locally and are stripped before Git commits.

### 🔄 Open Migration & Local Mocking
- **Postman:** Import and export v2.1 collections with zero syntax loss.
- **OpenAPI:** Import/export OpenAPI 3.0 with non-destructive 3-way schema merging.
- **cURL & HAR:** Paste raw terminal cURL commands or drag browser HAR logs straight into the workbench.
- **Local Mocks:** Instant zero-config HTTP mock servers bound to `127.0.0.1` based on your saved response examples.

---

## IV. Mobilization (Quick Start)

### Requirements
- [Rust Toolchain](https://rustup.rs/) (1.83+)
- macOS, Linux, or Windows

### Run from Source
```bash
# Clone the peoples' repository
git clone https://github.com/h3ma209/Blitz.git
cd Blitz

# Run development build
cargo run

# Or launch high-performance release binary
cargo run --release
```

### Install as Native Desktop App
```bash
make install
```

| Operating System | Default Install Destination | Strike / Uninstall |
|---|---|---|
| **macOS** | `~/Applications/Blitz.app` | `make uninstall` |
| **Linux** | `~/.local/share/blitz/` + `~/.local/bin/blitz` | `make uninstall` |
| **Windows** | `%LOCALAPPDATA%\Blitz\` | `powershell -File scripts/install.ps1 -Uninstall` |

---

## V. CI Automation for the People

Automate your test suites without proprietary SaaS runners or cloud credits:

```bash
# Execute collection headless in CI/CD (exits 0 on test pass, 1 on test failure)
blitz run collection.json --env staging.json
```

---

## VI. Collective Ownership: Git Team Sync

Collaborate with comrades across repos without submitting to closed cloud monopolies:

1. **Link Remote:** In the top toolbar, select **Team** or **Cloud → Link Git Remote…**
2. **Work Autonomously:** Build requests, write tests, tweak environment parameters locally.
3. **Commit & Push:** Click **Sync** — Blitz pulls, resolves changes, and pushes through standard Git (`gh` CLI or SSH keys).
4. **Class Defense:** Passwords, tokens, and local session variables are automatically redacted from commits.

---

## VII. Proclamation

*The API clients of the past have only interpreted the network in various ways; the point, however, is to liberate it.*

Distributed under the MIT / Apache 2.0 open-source licenses. Not affiliated with, endorsed by, or indebted to Postman Inc.
