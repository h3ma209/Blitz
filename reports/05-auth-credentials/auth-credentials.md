# 05 — Auth & credentials

## Evidence sources

| Tool | Note |
|------|------|
| `inspect_artifact` | `keytar.node`, OAuth/refresh/proxy-cert services, 1Password vendor bundle |
| `analyze_javascript_application` auth set | `ev_e913e1f44bef62931657babeb16676adf900781c1f7dd32cd753bb36eef8e976` |
| `package.json` | `keytar`, `@1password/sdk`, `@azure/msal-node`, `openid-client`, `@postman/aes-crypto-js` |
| `config.json` | Vault Azure settings, identity URLs |
| Full ASAR analysis | `js/authentication.js` relationship expansion **truncated** |
| **Ghidra 12.1.4** headless on `keytar.node` | **observed** — dump `.rea-scratch/keytar-ghidra/ghidra-dump.txt`; excerpts `reports/08-deeper-re/keytar-ghidra-excerpts.txt` |
| REA `binary_overview` (upstream Ghidra) | `ev_8277150eb3cfd1d5f913bf2b3d83a5110bab5c185ae8a47e04146c1d8a0ff701` |
| ASAR `runtime_agents_IPCAgent_js-*.min.js` | Embeds `CryptoService` + keytar JS wrapper (**observed**) |

**Native constraint:** Ghidra only (Hopper not used).

---

## Auth UI / flows (**observed** paths)

| Artifact | Size | Notes |
|----------|------|-------|
| `html/auth/auth.html` | 2113 | Auth window document |
| `html/auth/error.html` | — | Auth error page |
| `html/proxyAuth.html` + `js/proxyAuth.js` | — | Proxy authentication UI |
| `js/authentication.js` | large | Auth UI/logic bundle — REA expansion truncated |
| `services_OAuth2WindowManager_js-*.min.js` | 5545 | OAuth2 browser/window manager |
| `services_AppRefreshTokenManager_js-*.min.js` | 1503 | App refresh token manager |
| Vendor chunk with `1password` + `openid-client` | ~517 KB | External IdP / 1Password SDK |

**product-docs context (not REA):** UI exposes many auth types (Basic, Bearer, Digests, OAuth1/2, API Key, Hawk, AWS, NTLM, …). **REA did not recover an exhaustive enum** of auth type string constants from summaries in this pass — do not invent the list as “observed.”

---

## Secret storage patterns (**observed**)

### OS keychain — keytar

- Path: `node_modules/native_modules/build/Release/keytar.node`
- Kind: **native-addon**, Mach-O arm64, **133888** bytes, SHA-256 `908bec8d…b88a24` (**observed**)
- Dependency: `"keytar":"^7.9.0"` in package.json (**observed**)
- Ghidra language: `AARCH64:LE:64:AppleSilicon` (**observed**)

#### Native architecture (**Ghidra-backed**, architecture only — no secret dump)

```
N-API {get,set,delete,find}Password / findCredentials
  → AsyncWorkers (Execute/OnOK/OnError)
    → keytar::* → SecKeychain* (CRUD) / SecItemCopyMatching (queries)
```

Imports (**observed** Ghidra EXTERNAL): `_SecKeychainFindGenericPassword`, `_SecKeychainAddGenericPassword`, `_SecKeychainItemModifyAttributesAndData`, `_SecKeychainItemDelete`, `_SecKeychainItemFreeContent`, `_SecItemCopyMatching`.  
`SetPassword`: find → modify, else add. `GetPassword`: find → copy → free content; not-found OSStatus mapped to JS “missing”. Details: report 08.

#### App call site — CryptoService (**observed** ASAR text)

From `./common/services/CryptoService.js` inside `runtime_agents_IPCAgent_js-*.min.js`:

| Keychain field | Value |
|----------------|-------|
| **account** | literal `"userKey"` |
| **service** | `` `${app.getName()} (${id})` `` |
| APIs | `storeInKeychain` → `keytar.setPassword(service, "userKey", secret)`; `retrieveFromKeychain` → `getPassword` |

Log strings: `"Error while storing user key in keychain"` / `"retrieving user key"`. **Inference:** local encryption/user-key material — payload bytes **not** extracted.

### Electron cookie encryption fuse

- `EnableCookieEncryption` fuse = **disabled** (**observed** `inspect_electron_fuses`).
- Does **not** prove cookies are plaintext at rest; only that this fuse is off.

### App-level crypto / vault

| Signal | Label |
|--------|-------|
| `@postman/aes-crypto-js` dependency | **observed** |
| `config.json` `__WP_VAULT_AZURE_APP_ID__`, scopes `https://vault.azure.net/user_impersonation`, redirect `http://127.0.0.1:10534` | **observed** — Azure Key Vault integration for Postman Vault |
| `@1password/sdk` + vendor bundle | **observed** — 1Password secret provider |
| `@azure/msal-node`, `openid-client` | **observed** — OAuth/OIDC clients |
| `electron-json-storage`, NeDB | **observed** deps — possible local persistence; **not proven** as secret stores |
| Vault VariableScope `prefix:"vault:"`; files root `join(homeDir(),"Postman","files")` (`initializeDefaultWorkingDirectory`); gated `_allowScriptAccess`; domain-scoped `__getMatchingVariables` | **observed** (2026-10-17 ASAR) — [10](../10-re-complete/re-complete.md) |
| Passport → `resolvedValue:"{{vault:"+secretReference+"}}"` | **observed** |
| IPC `vault-ipc` / `saveVaultUserKey` → CryptoService keytar | **observed** (`08` / IPCAgent) |
| At-rest encryption under `~/Postman/files` | **unknown** — dir empty on sample machine; no new native vault binary for Ghidra |

### Proxy certificates

- `services_proxyCertificateService_*.min.js` (**observed**) — cert handling for proxy TLS.
- **Observed paths:** `{userData}/proxy/certificates/cert.json`, `{userData}/proxy/postman-proxy-ca.{crt,key,p12}`; `ProxyConstants` (`CA_SUFFIX_NAME=postman-proxy-ca`, 2048/sha256/500d, channel CN); `ProxyNodeProcess` → `http-proxy` `createProxyServer({secure:false, ssl:{key,cert}})`.
- **Settings keys (**observed** scratchpad):** `useSystemProxy`, `useProxyEnvironmentVariables`, `SSLCertVerify`, `isCACertEnabled`, `CACertPath` → runtime `systemProxy` / `strictSSL`.
- Passphrase/encryption of stored site cert keys still **unknown**. Sample userData had **no** `proxy/` tree yet.

### OAuth refresh during execution

- `process-manager.js` events: `GET_OAUTH_REFRESH_TOKEN`, `OAUTH_REFRESH_TOKEN_RECEIVED`, `OAUTH_REFRESH_TOKEN_NOT_FOUND` (**observed** strings) — ties auth refresh into executor processes.

---

## What REA could not resolve

1. ~~Exact keytar service/account keys~~ — **resolved for CryptoService** (`userKey` + `` `${app.getName()} (${id})` ``). Other call sites may exist.
2. Whether *all* secrets live in keytar vs encrypted NeDB vs Azure Vault vs 1Password at runtime (likely **multiple** backends — **inferred** from deps; not proven).
3. Full auth-type switch table inside `js/authentication.js` (budget truncation).
4. Electron `safeStorage` — **not observed**; macOS Keychain reached via keytar → SecKeychain/SecItem (**Ghidra-observed**).
5. Concrete `id` argument mapping (user/team GUID?) at CryptoService call sites — **unknown**.

---

## Recreation notes (non-code)

For notPostman: plan for pluggable secret backends (OS keychain + optional cloud vault), OAuth2 system browser/loopback (Azure redirect pattern suggests local HTTP callback), and keep proxy cert store separate from request auth headers.
