# 08 — Deeper RE (offline bridges + Ghidra keytar)

**Target:** Postman Desktop **12.32.3**  
**Constraint:** Native binary analysis via **Ghidra only** (no Hopper).  
**Scope:** Deeper offline ASAR/JS bridge recovery + `keytar.node` native architecture. No credential dump. No Rust app work.

---

## Evidence sources

| Source | Label | Note |
|--------|-------|------|
| Ghidra 12.1.4 headless | **observed** | Project `/tmp/postman-keytar-ghidra`, program `/keytar.node`, language `AARCH64:LE:64:AppleSilicon` |
| Dump artifact | **observed** | `.rea-scratch/keytar-ghidra/ghidra-dump.txt` (~138 KB, 4264 lines); excerpts in `keytar-ghidra-excerpts.txt` |
| REA `binary_overview` via Ghidra upstream | **observed** | `ev_8277150eb3cfd1d5f913bf2b3d83a5110bab5c185ae8a47e04146c1d8a0ff701` — 281 procs, arm64 Mach-O, digest `908bec8d…` |
| ASAR text (`runtime_agents_IPCAgent_js-*.min.js`) | **observed** | Embeds `CryptoService`, `keytar` JS wrapper, `@postman/runtime` IPC |
| `config.json` | **observed** | `__WP_DESKTOP_UI_UPDATE_URL__` = `https://desktop.postman.com` |
| REA MCP `procedure_pseudo_code` (Ghidra provider) | **failed** | Repeated `cleanup_incomplete` on keytar symbols — **not** used as evidence |
| Hopper | **not used** | Forbidden under current constraint |

---

## 1. Remote UI vs local ASAR (**observed**)

- `html/requester.html` references `js/requester.js`.
- Extracted ASAR: **`js/requester.js` absent**.
- `config.json`: desktop UI update base `https://desktop.postman.com`.
- **Inference:** primary requester UI loaded/updated from remote desktop UI CDN, not fully present in local ASAR. Local shells (scratchpad, auth HTML, sandboxes, main/runtime chunks) remain on-disk.

**Live Electron:** prior `capture_electron_scenario` boot logs showed RuntimeIPCAgent + UI build id `12.32.3-ui-261015-0231` and transient DevTools port; CDP attach for interactive Send was unreliable. Treat Send→HTTP live path as **still open**.

---

## 2. Runtime IPC bridge (**observed** ASAR text)

From webpack chunk `runtime_agents_IPCAgent_js-be7c27a27ea9d8c6.min.js`:

| Item | Value |
|------|-------|
| Module | `./runtime/agents/IPCAgent.js` + `@postman/runtime.runtime-rpc-electron` |
| Default IPC channel | `@postman/runtime` |
| Session ops | `connect` / `message` / `disconnect` (literal strings) |
| Related services in same chunk | `UnifiedRuntimeService`, `VaultIntegrationService`, `VaultScriptService`, `CryptoService` |

`common_services_UnifiedRuntimeService_*.min.js` contains literal `platform.execute` (**observed** string count ≥1). Exact Send-button → `platform.execute` call graph still **not** recovered end-to-end (UI remote + minification).

---

## 3. Script sandbox (**observed**)

- `html/tester_sandbox.html` present in ASAR; loads `js/tester/Evaluator.js` (window.open shim + `sandboxEchoText` postMessage only).
- Real host: **`postman-sandbox`** inside `postman-runtime` (uvm/uniscope). Isolation + expanded `epm.*` / vault / cookies: [10 re-complete](../10-re-complete/re-complete.md) (supersedes partial [09 re-followups](../09-docs-gap-matrix/re-followups.md)).
- Execution workers / `execution-system-manager` / `process-manager.js` remain offline anchors for script/OAuth executor events (see report 03).
- **Also CDN-missing (**observed** 10):** `js/runner.js` + `js/vendor-ui.js` referenced by `html/runner.html` but absent from ASAR (same pattern as `requester.js`).

---

## 4. keytar native — Ghidra-backed architecture (**observed**)

### Binary identity

| Field | Value |
|-------|-------|
| Path | `node_modules/native_modules/build/Release/keytar.node` (also webpack-resolved under `keytar/build/Release/`) |
| Size | 133888 bytes |
| SHA-256 | `908bec8db4b698a842466daf6993dafed3fda98ee789ff0c4b2f4e8cbcb88a24` |
| Format | Mach-O arm64 |
| Ghidra language | `AARCH64:LE:64:AppleSilicon` |
| Linked frameworks (otool) | AppKit, CoreFoundation, libc++, libSystem |

### Security / Keychain imports (Ghidra EXTERNAL)

- `_SecKeychainFindGenericPassword`
- `_SecKeychainAddGenericPassword`
- `_SecKeychainItemModifyAttributesAndData`
- `_SecKeychainItemDelete`
- `_SecKeychainItemFreeContent`
- `_SecItemCopyMatching`
- `_SecCopyErrorMessageString`
- CFString helpers (`_CFStringCreateWithCString`, …)

### Layering (Ghidra decompile)

```
N-API exports (anonymous_namespace::{Get,Set,Delete,Find}Password, FindCredentials)
  → *PasswordWorker / FindCredentialsWorker (AsyncWorker: Execute / OnOK / OnError)
    → keytar::{Get,Set,Add,Delete,Find}Password / FindCredentials / getCredentialsForItem
      → macOS SecKeychain* (CRUD) or SecItemCopyMatching (find-all / attribute query)
```

**CRUD path (generic password):**

- `keytar::GetPassword` → `_SecKeychainFindGenericPassword` → copy bytes → `_SecKeychainItemFreeContent`; status `-0x62d4` mapped as “not found” (return `2`).
- `keytar::SetPassword` → find existing → `_SecKeychainItemModifyAttributesAndData`, else `AddPassword` → `_SecKeychainAddGenericPassword`.
- `keytar::DeletePassword` → find → `_SecKeychainItemDelete`.

**Query path:**

- `keytar::FindCredentials` builds CFDictionary: `kSecClassGenericPassword`, `kSecAttrService`, `kSecMatchLimitAll`, return ref+attributes → `_SecItemCopyMatching`.
- `keytar::getCredentialsForItem` uses `kSecAttrService` / `kSecAttrAccount` / `kSecMatchLimitOne` / return attributes+data → `_SecItemCopyMatching`.

**JS API surface (N-API + keytar.js wrapper in same chunk):** `getPassword(service, account)`, `setPassword(service, account, password)`, `deletePassword`, `findPassword`, `findCredentials` — workers validate string params (`Parameter 'service' must be a string`, etc.).

Full pseudocode excerpts: [`keytar-ghidra-excerpts.txt`](./keytar-ghidra-excerpts.txt). Raw dump: `.rea-scratch/keytar-ghidra/ghidra-dump.txt`.

---

## 5. App-level keychain keys — CryptoService (**observed** JS, not native)

Same IPCAgent chunk embeds `./common/services/CryptoService.js`:

```js
// reconstructed from minified ASAR text — architecture only
const keytar = require('keytar');
const { app } = require('electron');
const defaultName = app.getName();
const account = 'userKey';

storeInKeychain(name = defaultName, secret, id) {
  const service = `${name} (${id})`;
  return keytar.setPassword(service, account, secret);
}

retrieveFromKeychain(name = defaultName, id) {
  const service = `${name} (${id})`;
  return keytar.getPassword(service, account);
}
```

| Field | Value | Label |
|-------|-------|-------|
| Keychain **account** | literal `"userKey"` | **observed** |
| Keychain **service** | `` `${app.getName()} (${id})` `` | **observed** pattern |
| Purpose | “user key” store/retrieve (log strings) | **observed** strings; payload semantics **not** dumped |

This closes prior gap “service/account keys unknown” for the CryptoService path. Other secret backends (Azure Vault, 1Password, AES package) remain parallel — see report 05.

---

## 6. Tooling status

| Tool | Result |
|------|--------|
| Ghidra headless import/analyze + custom `KeytarDumpScript` | **Success** — decompile dump written |
| REA `analyze_function` / overview with upstream Ghidra | **Partial success** — overview `ev_8277…`; per-symbol decompile via MCP hit `cleanup_incomplete` |
| Hopper | **Not used** (constraint) |
| Live CDP / Electron scenario | Boot logs only; interactive Send unrecovered |

---

## Gaps remaining

1. End-to-end Send click → HTTP (remote UI + no reliable CDP).
2. Exhaustive `epm.*` table (partial strings recovered 2026-10-17; Evaluator.js is not the API host).
3. Whether CryptoService `id` equals user/team GUID (call-site args not fully mapped).
4. REA MCP Ghidra session stability for interactive `procedure_pseudo_code` (headless dump is authoritative for keytar).
5. ASAR digest on disk may differ across installs — this pass measured `30abb7db…1564f4` (see [09](../09-docs-gap-matrix/re-followups.md)); older reports cite `f07f297…`.

---

## Recreation notes (non-code)

- Treat OS keychain as generic-password service/account pairs; CryptoService pattern is a good default for local “user key”.
- Runtime bridge: Electron IPC channel `@postman/runtime` with connect/message/disconnect + RPC payloads.
- Do not assume requester UI lives entirely in ASAR — plan for remote UI or ship local equivalent.
