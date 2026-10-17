# 14b — EvilPostman git-based team sync (design)

**Date:** 2026-10-17 · **Updated:** 2026-10-17 (impl pass)  
**Companion RE:** [postman-cloud-sync.md](./postman-cloud-sync.md)  
**Canonical remote:** `https://github.com/h3ma209/evilpostman-cloud.git`  
**Framing:** local-first alternative to Postman Cloud Bifrost — **not** trademark/cloud feature parity.

### Implementation status (P0 + save-pipeline)

| Decision | Choice |
|----------|--------|
| App / meta dir | EvilPostman / `.evilpostman/` (not `.notpostman`) |
| Env ↔ branch | **`env/<sanitized-name>`** (e.g. Development → `env/development`) |
| Default branch | `main` or `master` (detected); env branches created from it |
| On save | Local `snapshot.json` + export tree → `git add -A` → commit (skip empty) → `git push` (never force) |
| Secrets | Redacted in env JSON (`type: secret`, empty value); vault/cookies/history local-only |
| Split layout | One `*.request.json` per request under `collections/<slug>/` |

---

## Why git > reinventing Postman Cloud

| Postman Cloud (desktop-observed) | notPostman choice |
|----------------------------------|-------------------|
| Bifrost SyncClient + changesets + IndexedDB + conflict UI | Standard **git** remotes (GitHub/GitLab/…) |
| Closed wire protocol; server owns merge | Text files + human/CI review |
| Identity + roles hosted by Postman | Host permissions (repo roles) |
| Offline queue custom | Git working tree + commit when online |
| Native Git already exists in Postman desktop as parallel path | Make git **the** team plane |

Reasons for this repo:

1. **Already local-first** — `JsonStore` → `~/.local/share/evilpostman/snapshot.json` monolith.
2. Collections already export as Postman JSON (`exported-postman-jsons/`, impex).
3. RE shows Bifrost is large SaaS; cloning it is out of scope (reports 12–13).
4. Git gives free history, branches, PRs, CODEOWNERS, signed commits, CI gates.

---

## On-disk layout

Target: **repo = workspace**. One clone per team project.

```text
my-api-workspace/                 # git root (clone under ~/.local/share/evilpostman/cloud/)
  .evilpostman/
    project.json                  # schemaVersion, name, defaultEnv, remote
  collections/
    hakki/
      collection.json             # Collection v2.1 meta + variables + auth + item refs
      folders/
        auth/
          _folder.json
          login.request.json      # one request per file
          refresh.request.json
      requests/                   # optional flat requests (no folder)
        health.request.json
  environments/
    development.environment.json  # values; secrets redacted / externalized
    staging.environment.json
  .gitignore
  README.md
```

**Branches:** baseline on `main`/`master`; each environment is `env/<slug>`. Switching the active environment checks out (or creates) that branch, then reloads the tree.

### File formats

| Artifact | Format | Notes |
|----------|--------|-------|
| Collection meta | JSON (Collection v2.1 subset) or YAML later | P0: JSON matching existing impex |
| Request | JSON `{ id, name, method, url, headers, body, auth, events, … }` | Stable `id` (UUID) for rename-safe merges |
| Environment | Postman env JSON export shape | `enabled` flags; secret values via ref |
| Project | `.evilpostman/project.json` | `{ "schemaVersion": 1, "name": "…", "defaultEnv": "development", "remote": "…" }` |

**Conflict reduction:** prefer **folder-per-request** (one file per request) over single mega `collection.json`. Two people editing different requests → no merge conflict.

### Migration from current `snapshot.json`

| Step | Action |
|------|--------|
| 1 | Keep `snapshot.json` as **user cache** (open tabs, UI state, trash) under app data dir — **not** in git |
| 2 | Export/import path: snapshot ↔ `collections/` + `environments/` tree |
| 3 | “Link folder” = open directory that contains `.notpostman/project.json` |
| 4 | Autosave writes files; git status reflects dirty tree |

---

## Sync UX

| Action | Behavior |
|--------|----------|
| **Clone / link** | “Open folder” or “Clone URL…” → `git clone` then load `.notpostman/` |
| **Status** | Sidebar badge: branch, ahead/behind, dirty file count (`git status --porcelain`) |
| **Commit** | Default **explicit** “Commit…” dialog (message required). Optional setting: auto-commit on save (P2) with template `chore(api): update {path}` |
| **Pull** | `git pull --rebase` (configurable merge); reload tree after success |
| **Push** | `git push`; surface auth errors (SSH agent / gh credential) |
| **Branches** | Recommend **branch-per-feature** for shared repos; main/master protected via host rules — notPostman does not invent roles |
| **Diff** | Show file list + unified diff for selected request/env file (reuse `git diff`) |

No always-on websocket. Offline = edit files; sync when network returns (normal git).

---

## Conflict handling

| Layer | Strategy |
|-------|----------|
| **Prevent** | One request = one file; avoid reordering arrays in shared mega-files; stable keys |
| **Detect** | `git pull` conflict markers; UI lists conflicted paths |
| **Resolve** | Open conflicted JSON side-by-side; optional JSON-aware 3-way later; never silent LWW on whole collection |
| **Env secrets** | Conflicts on secret refs rare if secrets not in git |

Do **not** port Postman `conflict-resolution` event bus. Map conceptually: Bifrost conflict UI → git merge conflict UI.

---

## Secrets

| Store | In git? |
|-------|---------|
| OS keychain / vault (`vault.json`, keychain) | **Never** |
| Environment secret values | **Never** in cleartext |
| Env structure (keys, enabled, `type: secret` placeholders) | OK — values = `{{vault:…}}` or empty + local overlay |
| Cookies, history, OAuth tokens | App data only — gitignore |

### `.gitignore` patterns (recommended)

```gitignore
# notPostman local / secrets
.notpostman/local.json
.notpostman/cache/
**/*.local.json
**/secrets/**
.env
.env.*
vault.json
*.pem
*.p12
*.key

# OS / editor
.DS_Store
.idea/
.vscode/*
!.vscode/extensions.json
```

### Optional encrypted secrets

- **SOPS** or **git-crypt** for team-shared non-prod secrets if org requires — document only in P3; default remains “secrets stay local / vault”.
- Local overlay file `environments/development.local.json` (gitignored) merges over committed skeleton.

---

## Mapping: Postman Cloud → git

| Postman Cloud concept | notPostman / git |
|----------------------|------------------|
| Workspace | Git repository (folder with `.notpostman/`) |
| Team | Git host org/group |
| Roles (Admin/Editor/Viewer) | GitHub/GitLab roles + branch protection + CODEOWNERS |
| Version history | `git log` / commits |
| Bifrost pull/push | `git pull` / `git push` |
| Changeset | Commit (or commit hunk) |
| Timeline / revision | Commit SHA + branch tip |
| Conflict-resolution UI | Merge conflict editor |
| Force sync | `git fetch` + reset/rebase (dangerous — explicit confirm) |
| Scratchpad offline | Working tree uncommitted |
| Native Git (Postman feature) | **First-class** team mode for notPostman |
| Cloud monitors / published mocks / SSO | Out of scope (use host CI / local mock server) |
| Bifrost realtime presence | Optional later: none in P0–P2 |

---

## Phased implementation plan

### P0 — File export layout (recommended start)

- Writer: `AppSnapshot` → `collections/**` + `environments/**` + `.notpostman/project.json`
- Reader: folder → snapshot (or hybrid load)
- CLI: `not_postman export-tree <dir>` / `import-tree <dir>`
- Keep existing `snapshot.json` for unlinked personal use
- Ship example tree from Hakki collection split by request (script OK)

**Done when:** round-trip tree ↔ app without git UI.

### P1 — Git status in UI

- Detect `.git` under linked root
- Show branch + dirty count; buttons: Pull / Push / Commit (shell out to `git`, like Postman `GitResolver`)
- No auto-commit

**Done when:** team can share via existing remote + UI status.

### P2 — Auto-commit + watch

- Setting: commit-on-save (debounced) or commit-on-blur
- File watcher reload when external `git pull` changes tree
- Simple conflict list view

### P3 — PR / review

- “Create pull request” via `gh` / host API (optional)
- CODEOWNERS-aware hints; diff review pane
- Optional SOPS/git-crypt docs + overlay merge

---

## Relation to Postman desktop Native Git

Postman ships `main-git-ipc-handlers` + `GitResolver` (**observed** in [postman-cloud-sync.md](./postman-cloud-sync.md)): local `git` for status/checkout/push — parallel to Bifrost. notPostman skips Bifrost and implements the **Native Git–shaped** path as the only team sync.

---

## Non-goals

- Bifrost protocol compatibility
- Postman account login for sync
- Realtime multi-cursor / presence
- Cloud monitors, Spec Hub, Flows
- Claiming Postman feature parity

---

## P0 acceptance checklist

- [x] Schema for `.evilpostman/project.json`
- [x] Split writer: one file per request
- [x] Env export with secret redaction
- [x] Import tree into running app
- [x] Root `.gitignore` template written on first export
- [x] Docs link from FEATURES.md
- [x] Save → commit → push + env-as-branch (P1/P2 partial; no full conflict UI)
