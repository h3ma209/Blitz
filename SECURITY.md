# Security Policy

Blitz is a **local-first** API client. It stores collections on disk, secrets in the **OS keychain** (when available), and runs a **stdio MCP server** so AI agents can call your APIs. This document describes how we handle security and how to report vulnerabilities.

## Supported Versions

| Version | Supported |
| ------- | --------- |
| Latest release on [GitHub Releases](https://github.com/h3ma209/Blitz/releases) | Yes |
| `master` branch (commit at or after latest tag) | Yes, best-effort |
| Older releases | No |

## Reporting a Vulnerability

**Please do not open public GitHub issues for exploitable security bugs.**

1. Open a **[private GitHub security advisory](https://github.com/h3ma209/Blitz/security/advisories/new)** (preferred), or email the maintainer via the contact listed on their GitHub profile with subject `Blitz Security`.
2. Include: affected version, platform (macOS / Linux / Windows), reproduction steps, and impact (local privilege, secret exfiltration, RCE, etc.).
3. We aim to acknowledge within **72 hours** and share a remediation timeline within **14 days** for confirmed issues.

We do not currently offer a paid bug bounty. Credit in release notes is given with your permission.

## Scope

### In scope

- Blitz desktop app and `blitz` CLI from this repository
- Built-in MCP server (`blitz mcp` / stdio JSON-RPC tools)
- OAuth flows, vault/keychain integration, collection/import parsers
- Local mock server and capture proxy bound to localhost by default

### Out of scope

- Vulnerabilities in **your** API backends, MCP clients (Cursor, Claude Desktop, etc.), or third-party Git remotes
- Social engineering, denial-of-service against arbitrary public URLs you configure in requests
- Issues that require physical access to an already-unlocked machine with an active Blitz session (treat as device compromise)

## Security Model (what Blitz does and does not do)

### Data location

- **Collections, environments, history** — JSON files under your user data directory (platform-specific; not uploaded to Blitz-operated servers).
- **Telemetry** — Blitz does not phone home for analytics. Network traffic is initiated by **you** (HTTP/gRPC/WebSocket/MQTT requests, Git remotes you configure, optional update checks from GitHub Releases).

### Secrets

- Sensitive values intended for vault storage use the OS keychain where supported (`Blitz ({account_id})` service naming; see `src/secrets/keychain.rs`).
- Set `BLITZ_SECRETS=memory` only for CI/headless tests — secrets stay in-process and are **not** persisted.
- **You** are responsible for not committing secrets into Git-tracked collection JSON; use vault references and `.gitignore` as needed.

### MCP and AI agents

- The MCP server runs **locally** over stdio when enabled by you or your IDE config.
- Tools can **read and mutate** local collections, send HTTP requests using active environments, and access values available to the Blitz process — same trust boundary as using the GUI yourself.
- Only connect Blitz MCP to agents and machines you trust.

### Scripting sandbox

- Pre-request and test scripts run in an embedded JS engine (Boa) with a Postman-compatible API (`blitz.*` / `pm.*`).
- Scripts can trigger sub-requests and read response data configured in your workspace. Import collections only from trusted sources.

### Distribution integrity

- Official binaries are published on [GitHub Releases](https://github.com/h3ma209/Blitz/releases) from project CI.
- Releases are **not** Apple-notarized or Windows EV-signed yet. macOS Gatekeeper and Windows SmartScreen may warn on first launch; prefer **Homebrew**, **winget**, or **Scoop** once listed, or verify checksums from the release page before running.

## Secure Development

- CI builds from source in [`.github/workflows/build.yml`](.github/workflows/build.yml).
- Dependencies are pinned in `Cargo.lock`. Report supply-chain concerns through the same private advisory channel.

## Hardening Recommendations for Users

1. Install from tagged releases or trusted package managers; verify SHA256 when downloading manually.
2. Keep Blitz updated to the latest release.
3. Restrict MCP configuration to local agents; do not expose the Blitz process on a network share.
4. Use OS keychain vault entries for tokens; avoid plain-text secrets in shared Git repos.
5. Review imported Postman/OpenAPI/HAR files before running scripts or MCP automation.

## Security-related Documentation

- [Documentation — Vault & OS Keychain](https://h3ma209.github.io/Blitz/docs.html#vault-secrets)
- [Documentation — MCP setup & tools](https://h3ma209.github.io/Blitz/docs.html#mcp-overview)

---

MIT / Apache-2.0 licensed software — see [LICENSE](LICENSE). Security fixes are released as semver patch/minor versions when possible.
