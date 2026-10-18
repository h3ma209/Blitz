# 11 — Targeted RE notes (OAuth, codegen, mock, import, scripts)

**Date:** 2026-10-17  
**Target:** ASAR extract `.rea-scratch/re-complete-extract/app` (SHA prior: `30abb7db…`)  
**Native:** no new Ghidra this pass (no new `.node` crypto target). **Hopper unused.**  
**Method:** offline extract + Python/string mining (**observed**). REA live CDP not required for these module names.

---

## 1. OAuth 2.0 (**observed**)

| Artifact | Role |
|----------|------|
| `services_OAuth2WindowManager_js-84cab6633f2a7b7f.min.js` (~5.5 KB) | Desktop OAuth browser/window manager |
| `js/authentication.js` (~732 KB) | Auth surface (minified) |
| ASAR string counts in `js/` | `oauth2` ×450; `authorization_code` ×37; `grant_type` ×26; `client_credentials` ×16 |

**Inferred for notPostman:** support `client_credentials` (token POST) and `authorization_code` (authorize URL + local loopback redirect + token exchange). Store `access_token` on auth config.

---

## 2. Code generators (**observed**)

| Path | Notes |
|------|-------|
| `js/scratchpad/postman-code-generators.js` (~2.4 MB) | Bundled generators |

String frequency (case-insensitive samples): `curl` 205, `Python` 190, `javascript` 25, `NodeJs` 14, `fetch` 5, plus Go/Java/Ruby/php hits.

**Inferred:** ship cURL, JS `fetch`, Python `requests` templates locally (no need to re-host Postman bundle).

---

## 3. Mock server (**observed**)

| Path | Notes |
|------|-------|
| `mock-server-runtime/http-server-worker.js` (~820 KB) | HTTP mock worker |
| `mock-server-runtime/simulation-http-server-worker.js` | Simulation variant |

Interesting literals: `match` / `matches` / `matching`, `method`, `path`, `urlTemplate`, `mockHost`, headers `x-mock-scenario`, `x-mock-session`.

**Inferred matching for local mock:** method + path (URL template) → saved example response (status + body + headers). Scenarios via optional header later.

---

## 4. OpenAPI / cURL import

No dedicated small importer module isolated this pass (CDN UI likely). **Product-docs** + common Postman behavior: paste cURL / OpenAPI 3 JSON|YAML → collection.

**Inferred:** implement happy-path parsers in Rust (`impex`).

---

## 5. Script inheritance / runner control (**observed**)

From `vendors-node_modules_postman-runtime_index_js-*.min.js` string counts:

| Token | Count |
|-------|------:|
| `prerequest` | 8 |
| `"test"` | 7 |
| `setNextRequest` | 4 |
| `skipRequest` | 5 |
| `iterationData` | 2 |
| `folder` | 6 |

**Inferred run order (product-docs + runtime events):** collection pre → folder pre → request pre → send → request test → folder test → collection test.  
**Inferred execution:** `epm.execution.setNextRequest(name|null)`, `epm.execution.skipRequest()`, data file → `epm.iterationData`.

---

## 6. Failed / skipped

| Call | Result |
|------|--------|
| Live CDP / `list_electron_targets` | not required this pass |
| New Ghidra | skipped — no new native target |
| Hopper | **not used** |
| Full decompile of `authentication.js` / mock worker | minified; string-level only |

---

## App mapping

Use these notes to implement Phase 3: OAuth2, snippets, local mock, impex, sandbox expansion, runner data files.
