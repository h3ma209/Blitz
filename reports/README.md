# notPostman — Postman Desktop RE Reports

Architecture reverse-engineering of `/Applications/Postman.app` (v **12.32.4**) to guide Rust+Dioxus recreation.

**Evidence policy:** Claims labeled **observed** (REA / ASAR member at REA-inventoried path) or **inferred**. Product UX cited only as **product-docs context**. No invented IPC/class catalogs.

## Report index

| # | Report | Topic |
|---|--------|-------|
| 01 | [01-app-structure/app-structure.md](01-app-structure/app-structure.md) | Main/renderer/preload, ASAR layout, fuses |
| 02 | [02-ui-architecture/ui-architecture.md](02-ui-architecture/ui-architecture.md) | Shells, IPC bridge, UI gaps |
| 03 | [03-request-lifecycle/request-lifecycle.md](03-request-lifecycle/request-lifecycle.md) | Send → runtime → response |
| 04 | [04-subsystems/subsystems.md](04-subsystems/subsystems.md) | Collections, vars, runner, mocks, sync |
| 05 | [05-auth-credentials/auth-credentials.md](05-auth-credentials/auth-credentials.md) | Auth modules, keytar, vault |
| 06a | [06-diagrams/architecture.mmd.md](06-diagrams/architecture.mmd.md) | Mermaid component diagram |
| 06b | [06-diagrams/request-lifecycle-sequence.mmd.md](06-diagrams/request-lifecycle-sequence.mmd.md) | Mermaid sequence (inferred) |
| 07 | [07-pseudocode/send-request.pseudocode.md](07-pseudocode/send-request.pseudocode.md) | Send-path pseudocode |
| 08 | [08-deeper-re/deeper-re.md](08-deeper-re/deeper-re.md) | Deeper offline RE + **Ghidra** keytar |
| 09 | [09-docs-gap-matrix/gap-matrix.md](09-docs-gap-matrix/gap-matrix.md) | **Primary** docs × RE × app gap matrix (P0–P3) |
| 09r | [09-docs-gap-matrix/re-followups.md](09-docs-gap-matrix/re-followups.md) | P0/P1 RE follow-ups (vault, epm.*, runner, protocols, proxy, cookies, schema) |
| 09a | [09-coverage-gap/coverage-matrix.md](09-coverage-gap/coverage-matrix.md) | Earlier coverage checklist (superseded by 09 for priority language) |
| 09b | [09-coverage-gap/next-actions.md](09-coverage-gap/next-actions.md) | Top 10 RE + top 10 app build actions |
| 10 | [10-re-complete/re-complete.md](10-re-complete/re-complete.md) | **Final local-client RE** — sandbox/uvm, vault files, proxy/certs, runner vs perf, protocols, cookies |
| 11 | [11-docs-gap-refresh/gap-refresh.md](11-docs-gap-refresh/gap-refresh.md) | **Docs map refresh** vs live app (supersedes stale 09 app rows) |
| 11r | [11-docs-gap-refresh/re-notes.md](11-docs-gap-refresh/re-notes.md) | Targeted RE: OAuth2, codegen, mock matching, script inheritance |
| 12 | [12-contents-grab/inventory.md](12-contents-grab/inventory.md) | **Contents grab** — MacOS/Resources/ASAR protocols vs notPostman |
| 12s | [12-contents-grab/app-shipped.md](12-contents-grab/app-shipped.md) | Phase B ship checklist (WS/gRPC/MQTT/MCP/proxy/perf) |
| 13 | [13-remaining-gaps/remaining-gaps.md](13-remaining-gaps/remaining-gaps.md) | **Still missing** vs Postman (local grab vs cloud) |
| 14a | [14-cloud-sync-re/postman-cloud-sync.md](14-cloud-sync-re/postman-cloud-sync.md) | **Cloud sync RE** — Bifrost, changesets, identity hosts |
| 14b | [14-cloud-sync-re/git-team-sync-design.md](14-cloud-sync-re/git-team-sync-design.md) | **notPostman git team sync** design (local-first) |

## Target

- App: `/Applications/Postman.app`
- ASAR: `Contents/Resources/app.asar`  
  SHA-256 (2026-10-17 RE pass): `30abb7db2af633bf854f32b60fde0a1dc3e8241140cc8f9e033ee6c1091564f4`  
  Earlier reports may cite `f07f297001091e1672382de853d2dc7d447259278ac1898f8245235c777f7571`
- Unpacked: `Contents/Resources/data/` (worker, postman-cli, plugin-installer, badge_overlays)

## Key REA evidence IDs

| Operation | Evidence ID |
|-----------|-------------|
| `inspect_electron_fuses` | `ev_fd5514ffa8bd23a2ad55ebf6d3ba77e04c71331ea6b1086801e2f5a456f275a6` |
| `inspect_artifact` (parent) | `ev_1b63e5d8bcfb367a3450e08b4ffc92cdca4586f00168a2ccd993a6f28799ea94` |
| `analyze_javascript_application` full ASAR (partial) | `ev_86985f56c5339657d605888162b24ae2d2ae92eebce30f354059ce69314d7103` |
| Entry preloads/startup analysis | `ev_5d12fac6ad3d0fd45d791d1dd1b825ccba653e4e91b1f04a11f30f7d2c8f0190` |
| ExecutionProcess subset | `ev_99bc592560423429598fc8e08398c7dba518877b3c895e8a7d4ba3f3ce3ca851` |
| Auth/preload subset | `ev_e913e1f44bef62931657babeb16676adf900781c1f7dd32cd753bb36eef8e976` |
| Session evidence export | `.rea-scratch/evidence-bundle.json` (~957 MB) |
| Ghidra keytar overview (REA upstream) | `ev_8277150eb3cfd1d5f913bf2b3d83a5110bab5c185ae8a47e04146c1d8a0ff701` |
| Ghidra headless dump | `.rea-scratch/keytar-ghidra/ghidra-dump.txt` |

**Native RE constraint:** Ghidra only for `.node`/`.dylib`/`.so` (Hopper forbidden).

## Strongest findings (summary)

1. Classic Electron multi-process app: `main.js` + many HTML renderer shells (`requester`, `runner`, `scratchpad`, sandbox, auth).
2. Request execution centered on **`postman-runtime` / `postman-request`** plus dedicated workers (`ExecutionProcess`, `execution-worker`, `AgentWorker`, `ProxyNodeProcess`). Runtime IPC channel **`@postman/runtime`** (**observed**).
3. Secrets: **`keytar.node`** (Ghidra: SecKeychain/SecItem) + CryptoService keys `account=userKey`, `service=${app.getName()} (${id})`; Vault VariableScope `prefix:"vault:"` + `{{vault:}}`; Azure Vault, **1Password SDK**, AES — multi-backend.
4. Requester UI: `js/requester.js` **absent** from ASAR; `__WP_DESKTOP_UI_UPDATE_URL__` → `https://desktop.postman.com` (**observed**).
5. Scripts: **`postman-sandbox`** + uniscope/uvm isolation + expanded `epm.*` (**observed** [10](10-re-complete/re-complete.md)); cookies `@postman/tough-cookie` + partition jars; protocols `@postman/runtime_*-client`.
6. Most IPC channel names **dynamic**; REA pairing = 0. Named main IPC handler chunks for FS/git/dataset/perf/ripgrep/webhooks.
7. Full ASAR semantic analysis **memory/budget limited** — large workers and `main.js` incomplete.
8. Runner/requester UI JS **absent** locally (`js/runner.js`, `js/requester.js`) — CDN pattern (**observed**).

## Gaps / next REA steps

- Reliable CDP → `list_electron_targets` / `capture_electron_scenario` for live Send (Postman not running in 10 pass).
- Exhaustive `epm.*` from unminified sandbox; vault at-rest crypto if files appear; CLI YAML; CryptoService `id` mapping.
- Backlog: [10-re-complete/re-complete.md](10-re-complete/re-complete.md) (primary); [09 gap-matrix](09-docs-gap-matrix/gap-matrix.md).

### 2026-10-17 REA evidence (gap pass)

| Operation | Evidence ID |
|-----------|-------------|
| `analyze_javascript_application` focused (sandbox/proxy/IPC/package) | `ev_d64c427521e0895d16eddf2c117fc50be4ebf2900c07230bc42697b9c4dd62d8` |
| Summary view of above | `ev_76791f522ca5de28cb6f07767d44788c99216ed57c55face46ca5a1bf1ebfef7` |
| `trace_application_feature` seed `vault-ipc` | **no-match** `ev_36c48debc699fabe857e5f0e3d3c4042549ab86ae12c1f88b7dcb6e663d0a1f0` (vault recovered via ASAR strings instead) |

### 2026-10-17 REA evidence (re-complete pass)

| Operation | Evidence ID |
|-----------|-------------|
| `analyze_javascript_application` (sandbox/runner/process-manager) | parent `ev_1928090e472b3d7f11485511165bfd8a5525e92bdf672d5e651b9e9b63b65c16` · view `ev_83bf3a53940a46f8c383ad77804fc098343d3055980bf32ffdcd7dc9b1b958ad` |
| `analyze_javascript_application` (proxy/perftest/webProxy) | parent `ev_f6e411837034f68b751a4997db2feb6f489aa5e6f497cb99a912dadba3fa918c` · view `ev_f5596f19ce230732fdecff6b12d550134346b55618e45071204694d2d9d9283b` |
| Offline ASAR extract | `.rea-scratch/re-complete-extract/` |
| `analyze_javascript_application` (auth/config/sync WP URLs) | parent `ev_a64944ae7e90306d5fce7fd132bde66b97b62c22914f89c2112be519a6c7e52e` · summary `ev_d2087bf28b59d2988f57e97d847aa397b63113d56c95a7b40cdeccb104c979cd` |
