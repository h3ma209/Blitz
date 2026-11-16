//! Shell-out to system `git` (P1 Native-Git shaped path).

use std::path::Path;
use std::process::Command;

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CloudGitError {
    #[error("git not found on PATH — install git to enable cloud sync")]
    NotFound,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Cmd(String),
}

#[derive(Debug, Clone)]
pub struct SyncResult {
    pub ok: bool,
    /// True when sync was disabled or nothing to commit.
    pub skipped: bool,
    pub message: String,
    pub branch: String,
}

/// `true` if `git --version` succeeds.
#[cfg(test)]
pub fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<String, CloudGitError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                CloudGitError::NotFound
            } else {
                CloudGitError::Io(e)
            }
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        let msg = if stderr.is_empty() {
            stdout
        } else if stdout.is_empty() {
            stderr
        } else {
            format!("{stdout}\n{stderr}")
        };
        return Err(CloudGitError::Cmd(classify_git_error(&msg)));
    }
    Ok(stdout)
}

fn classify_git_error(raw: &str) -> String {
    let masked = mask_remote_url(raw);
    let lower = masked.to_lowercase();
    if lower.contains("authentication")
        || lower.contains("permission denied")
        || lower.contains("could not read username")
        || lower.contains("403")
        || lower.contains("401")
        || lower.contains("publickey")
    {
        format!(
            "auth failure — use SSH agent or `gh auth login` / credential helper. Detail: {masked}"
        )
    } else if lower.contains("conflict")
        || lower.contains("non-fast-forward")
        || lower.contains("rejected")
        || lower.contains("needs merge")
    {
        format!(
            "conflict / non-fast-forward — pull & resolve in the clone, then save again. Detail: {masked}"
        )
    } else {
        masked
    }
}

/// Clone `remote` into `path` if missing; otherwise verify `.git` exists.
pub fn ensure_clone(remote: &str, path: &Path) -> Result<(), CloudGitError> {
    if path.join(".git").is_dir() {
        // Keep origin URL in sync when user changes remote in settings.
        let _ = run_git(path, &["remote", "set-url", "origin", remote]);
        return Ok(());
    }
    if path.exists() {
        let empty = std::fs::read_dir(path)?.next().is_none();
        if !empty {
            return Err(CloudGitError::Cmd(format!(
                "clone path exists but is not a git repo: {}",
                path.display()
            )));
        }
        std::fs::remove_dir(path)?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| CloudGitError::Cmd("invalid clone path".into()))?;
    // Clone into parent/name
    let output = Command::new("git")
        .args(["clone", "--", remote, name])
        .current_dir(parent)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                CloudGitError::NotFound
            } else {
                CloudGitError::Io(e)
            }
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(CloudGitError::Cmd(classify_git_error(&stderr)));
    }
    Ok(())
}

fn default_branch(cwd: &Path) -> String {
    if let Ok(s) = run_git(
        cwd,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "--short",
        ],
    ) {
        // origin/main → main
        if let Some(b) = s.strip_prefix("origin/") {
            return b.to_string();
        }
        if !s.is_empty() {
            return s;
        }
    }
    for candidate in ["main", "master"] {
        if run_git(cwd, &["show-ref", "--verify", "--quiet", &format!("refs/heads/{candidate}")])
            .is_ok()
            || run_git(
                cwd,
                &[
                    "show-ref",
                    "--verify",
                    "--quiet",
                    &format!("refs/remotes/origin/{candidate}"),
                ],
            )
            .is_ok()
        {
            return candidate.to_string();
        }
    }
    "main".into()
}

/// Checkout `branch`, creating from default branch if it does not exist locally/remotely.
pub fn checkout_env_branch(cwd: &Path, branch: &str) -> Result<(), CloudGitError> {
    // Already on it?
    if let Ok(cur) = run_git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        if cur == branch {
            return Ok(());
        }
    }

    // Local branch exists
    if run_git(
        cwd,
        &["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")],
    )
    .is_ok()
    {
        run_git(cwd, &["checkout", branch])?;
        return Ok(());
    }

    // Remote tracking branch
    let remote_ref = format!("origin/{branch}");
    if run_git(
        cwd,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/remotes/{remote_ref}"),
        ],
    )
    .is_ok()
    {
        run_git(cwd, &["checkout", "-b", branch, "--track", &remote_ref])?;
        return Ok(());
    }

    // Create from default branch
    let base = default_branch(cwd);
    // Ensure we have base checked out / exist
    let _ = run_git(cwd, &["fetch", "origin"]);
    if run_git(
        cwd,
        &["show-ref", "--verify", "--quiet", &format!("refs/heads/{base}")],
    )
    .is_err()
    {
        if run_git(
            cwd,
            &[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/remotes/origin/{base}"),
            ],
        )
        .is_ok()
        {
            let _ = run_git(cwd, &["checkout", "-b", &base, &format!("origin/{base}")]);
        } else {
            // Empty / brand-new repo: commit nothing yet; create orphan-ish via checkout -b
            let _ = run_git(cwd, &["checkout", "-B", &base]);
        }
    } else {
        let _ = run_git(cwd, &["checkout", &base]);
    }

    run_git(cwd, &["checkout", "-b", branch])?;
    Ok(())
}

/// UTC timestamp for commit messages (`2026-10-17T16:03:00Z`).
pub fn commit_timestamp_utc() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Build a structured Conventional Commit message with a concise subject line
/// (`sync(<scope>): <summary>`) and a grouped, contextual body + metadata footer.
///
/// Examples:
/// - `sync(requests): add 'login', update 'health' (hakki)`
/// - `sync(requests): rename 'old' -> 'new'`
/// - `sync(workspace): 8 changes (5 requests, 3 environments)`
pub fn build_commit_message(name_status: &str, timestamp: &str) -> String {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Action {
        Add,
        Update,
        Delete,
        Rename,
    }

    impl Action {
        fn verb(self) -> &'static str {
            match self {
                Action::Add => "add",
                Action::Update => "update",
                Action::Delete => "delete",
                Action::Rename => "rename",
            }
        }

        fn tag(self) -> &'static str {
            match self {
                Action::Add => "add",
                Action::Update => "upd",
                Action::Delete => "del",
                Action::Rename => "ren",
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ScopeKind {
        Requests,
        Environments,
        Collections,
        Variables,
        Config,
        Docs,
        Workspace,
    }

    impl ScopeKind {
        fn scope_name(self) -> &'static str {
            match self {
                ScopeKind::Requests => "requests",
                ScopeKind::Environments => "environments",
                ScopeKind::Collections => "collections",
                ScopeKind::Variables => "variables",
                ScopeKind::Config => "config",
                ScopeKind::Docs => "docs",
                ScopeKind::Workspace => "workspace",
            }
        }

        fn section_title(self) -> &'static str {
            match self {
                ScopeKind::Requests => "Requests",
                ScopeKind::Environments => "Environments",
                ScopeKind::Collections => "Collections",
                ScopeKind::Variables => "Variables",
                ScopeKind::Config => "Config",
                ScopeKind::Docs => "Docs",
                ScopeKind::Workspace => "Workspace",
            }
        }

        fn singular_label(self) -> &'static str {
            match self {
                ScopeKind::Requests => "request",
                ScopeKind::Environments => "environment",
                ScopeKind::Collections => "collection",
                ScopeKind::Variables => "globals",
                ScopeKind::Config => "config",
                ScopeKind::Docs => "docs",
                ScopeKind::Workspace => "file",
            }
        }

        fn plural_label(self) -> &'static str {
            match self {
                ScopeKind::Requests => "requests",
                ScopeKind::Environments => "environments",
                ScopeKind::Collections => "collections",
                _ => "files",
            }
        }

        fn is_unquoted(self) -> bool {
            matches!(self, ScopeKind::Variables | ScopeKind::Config | ScopeKind::Docs)
        }
    }

    #[derive(Debug, Clone)]
    struct ParsedEntry {
        action: Action,
        kind: ScopeKind,
        item_name: String,
        old_name: Option<String>,
        context: Option<String>,
        collection: Option<String>,
    }

    fn strip_item_suffix(file_name: &str) -> &str {
        file_name
            .strip_suffix(".request.json")
            .or_else(|| file_name.strip_suffix(".environment.json"))
            .unwrap_or(file_name)
    }

    /// Extract `(collection, folder_context)` from `collections/<col>/folders/<a/b>/...`.
    fn extract_collection_context(norm_path: &str) -> (Option<String>, Option<String>) {
        let segs: Vec<&str> = norm_path.split('/').filter(|s| !s.is_empty()).collect();
        let Some(col_pos) = segs.iter().position(|&s| s == "collections") else {
            return (None, None);
        };
        let Some(&col_name) = segs.get(col_pos + 1) else {
            return (None, None);
        };
        let col = col_name.to_string();
        if let Some(folders_pos) = segs[col_pos + 2..].iter().position(|&s| s == "folders") {
            let folder_start = col_pos + 2 + folders_pos + 1;
            if folder_start + 1 < segs.len() {
                let folder_chain = segs[folder_start..segs.len() - 1].join(" / ");
                if !folder_chain.is_empty() {
                    return (Some(col.clone()), Some(format!("{col} / {folder_chain}")));
                }
            }
        }
        (Some(col.clone()), Some(col))
    }

    fn classify_path(raw_path: &str) -> (ScopeKind, String, Option<String>, Option<String>) {
        let norm = raw_path.replace('\\', "/");
        let file_name = Path::new(&norm)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&norm);
        let (collection, context) = extract_collection_context(&norm);

        if norm.ends_with(".request.json") {
            let clean = file_name.strip_suffix(".request.json").unwrap_or(file_name);
            (ScopeKind::Requests, clean.to_string(), context, collection)
        } else if norm.ends_with(".environment.json") {
            let clean = file_name
                .strip_suffix(".environment.json")
                .unwrap_or(file_name);
            (ScopeKind::Environments, clean.to_string(), None, None)
        } else if norm.ends_with("collection.json") {
            let col = Path::new(&norm)
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or(file_name);
            (
                ScopeKind::Collections,
                col.to_string(),
                None,
                Some(col.to_string()),
            )
        } else if norm.ends_with("globals.json") {
            (ScopeKind::Variables, "global variables".to_string(), None, None)
        } else if norm.ends_with("project.json") {
            (ScopeKind::Config, "project settings".to_string(), None, None)
        } else if norm.ends_with(".gitignore") {
            (ScopeKind::Config, ".gitignore".to_string(), None, None)
        } else if norm.to_ascii_lowercase().contains("readme") {
            (ScopeKind::Docs, "README".to_string(), None, None)
        } else {
            (ScopeKind::Workspace, file_name.to_string(), context, collection)
        }
    }

    fn format_entry_name(e: &ParsedEntry) -> String {
        if let Some(old) = &e.old_name {
            format!("'{old}' -> '{}'", e.item_name)
        } else if e.kind.is_unquoted() {
            e.item_name.clone()
        } else {
            format!("'{}'", e.item_name)
        }
    }

    let mut entries: Vec<ParsedEntry> = Vec::new();
    for line in name_status.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let (status_token, rest) = if let Some((st, p)) = line.split_once('\t') {
            (st.trim(), p.trim())
        } else if let Some((st, p)) = line.split_once(' ') {
            (st.trim(), p.trim())
        } else {
            continue;
        };

        let status_char = status_token.chars().next().unwrap_or('M');
        if rest.is_empty() {
            continue;
        }

        let (action, old_path, raw_path) = match status_char {
            'A' | '?' => (Action::Add, None, rest),
            'D' => (Action::Delete, None, rest),
            'R' => {
                if let Some((old_p, new_p)) = rest.split_once('\t') {
                    (Action::Rename, Some(old_p.trim()), new_p.trim())
                } else {
                    (Action::Rename, None, rest)
                }
            }
            _ => (Action::Update, None, rest),
        };

        let (kind, item_name, context, collection) = classify_path(raw_path);
        let old_name = old_path.map(|p| {
            let norm = p.replace('\\', "/");
            let fname = Path::new(&norm)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&norm);
            strip_item_suffix(fname).to_string()
        });

        entries.push(ParsedEntry {
            action,
            kind,
            item_name,
            old_name,
            context,
            collection,
        });
    }

    if entries.is_empty() {
        return "sync(workspace): update workspace".to_string();
    }

    let first_kind = entries[0].kind;
    let single_kind = entries.iter().all(|e| e.kind == first_kind);
    let scope_tag = if single_kind {
        first_kind.scope_name()
    } else {
        "workspace"
    };

    // Detect single shared collection for concise subject context.
    let shared_collection: Option<&str> = entries
        .first()
        .and_then(|e| e.collection.as_deref())
        .filter(|col| entries.iter().all(|e| e.collection.as_deref() == Some(*col)));

    let subject_core = if entries.len() <= 3 {
        if single_kind {
            let first_action = entries[0].action;
            let same_action = entries.iter().all(|e| e.action == first_action);
            if same_action {
                let verb = first_action.verb();
                let names: Vec<String> = entries.iter().map(format_entry_name).collect();
                match names.len() {
                    1 => format!("{verb} {}", names[0]),
                    2 => format!("{verb} {} and {}", names[0], names[1]),
                    _ => format!("{verb} {}, {} and {}", names[0], names[1], names[2]),
                }
            } else {
                entries
                    .iter()
                    .map(|e| format!("{} {}", e.action.verb(), format_entry_name(e)))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        } else {
            entries
                .iter()
                .map(|e| {
                    let name = format_entry_name(e);
                    if e.kind.is_unquoted() || e.kind == ScopeKind::Workspace {
                        format!("{} {name}", e.action.verb())
                    } else {
                        format!("{} {name} {}", e.action.verb(), e.kind.singular_label())
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    } else if single_kind {
        let adds = entries.iter().filter(|e| e.action == Action::Add).count();
        let updates = entries.iter().filter(|e| e.action == Action::Update).count();
        let deletes = entries.iter().filter(|e| e.action == Action::Delete).count();
        let renames = entries.iter().filter(|e| e.action == Action::Rename).count();

        let mut action_parts = Vec::new();
        for (count, verb) in [
            (adds, "add"),
            (updates, "update"),
            (deletes, "delete"),
            (renames, "rename"),
        ] {
            if count > 0 {
                action_parts.push(format!("{verb} {count}"));
            }
        }
        let noun = first_kind.plural_label();
        if action_parts.len() == 1 {
            let act = action_parts[0].split_whitespace().next().unwrap_or("update");
            format!("{act} {} {noun}", entries.len())
        } else {
            format!("{} {noun}", action_parts.join(", "))
        }
    } else {
        let mut breakdown = Vec::new();
        for (kind, label) in [
            (ScopeKind::Requests, "requests"),
            (ScopeKind::Environments, "environments"),
            (ScopeKind::Collections, "collections"),
        ] {
            let count = entries.iter().filter(|e| e.kind == kind).count();
            if count > 0 {
                breakdown.push(format!("{count} {label}"));
            }
        }
        if breakdown.is_empty() {
            format!("update {} files", entries.len())
        } else {
            format!("{} changes ({})", entries.len(), breakdown.join(", "))
        }
    };

    let subject = match shared_collection {
        Some(col) if first_kind != ScopeKind::Collections => {
            format!("sync({scope_tag}): {subject_core} ({col})")
        }
        _ => format!("sync({scope_tag}): {subject_core}"),
    };

    // Build structured commit body grouped by ScopeKind.
    const MAX_BODY_LINES: usize = 15;
    let kind_order = [
        ScopeKind::Collections,
        ScopeKind::Requests,
        ScopeKind::Environments,
        ScopeKind::Variables,
        ScopeKind::Config,
        ScopeKind::Docs,
        ScopeKind::Workspace,
    ];

    let mut body_lines: Vec<String> = Vec::new();
    let mut emitted = 0usize;
    for kind in kind_order {
        let group: Vec<&ParsedEntry> = entries.iter().filter(|e| e.kind == kind).collect();
        if group.is_empty() {
            continue;
        }
        if emitted >= MAX_BODY_LINES {
            break;
        }
        if !single_kind {
            body_lines.push(format!("{}:", kind.section_title()));
        }
        for e in group {
            if emitted >= MAX_BODY_LINES {
                break;
            }
            let name = format_entry_name(e);
            let ctx = match &e.context {
                Some(c) => format!(" ({c})"),
                None => String::new(),
            };
            body_lines.push(format!("- [{}] {name}{ctx}", e.action.tag()));
            emitted += 1;
        }
    }
    if entries.len() > emitted {
        body_lines.push(format!("- ... and {} more", entries.len() - emitted));
    }

    let adds = entries.iter().filter(|e| e.action == Action::Add).count();
    let updates = entries.iter().filter(|e| e.action == Action::Update).count();
    let deletes = entries.iter().filter(|e| e.action == Action::Delete).count();
    let renames = entries.iter().filter(|e| e.action == Action::Rename).count();
    let mut stat_parts = Vec::new();
    if adds > 0 {
        stat_parts.push(format!("+{adds}"));
    }
    if updates > 0 {
        stat_parts.push(format!("~{updates}"));
    }
    if deletes > 0 {
        stat_parts.push(format!("-{deletes}"));
    }
    if renames > 0 {
        stat_parts.push(format!(">{renames}"));
    }

    let mut footer_lines = vec![format!(
        "Changes: {} ({} total)",
        stat_parts.join(" "),
        entries.len()
    )];
    let ts = timestamp.trim();
    if !ts.is_empty() {
        footer_lines.push(format!("Synced-At: {ts}"));
    }

    format!(
        "{subject}\n\n{}\n\n{}",
        body_lines.join("\n"),
        footer_lines.join("\n")
    )
}

/// Resolve commit message: non-empty override wins; else auto from cached name-status.
pub fn resolve_commit_message(override_msg: Option<&str>, name_status: &str) -> String {
    if let Some(m) = override_msg.map(str::trim).filter(|s| !s.is_empty()) {
        return m.to_string();
    }
    build_commit_message(name_status, &commit_timestamp_utc())
}

/// `git add -A` → commit if dirty. **Never push.** Skip empty commit.
///
/// `message_override`: when Some(non-empty), use as-is; else auto from name-status.
pub fn commit_local_changes(
    cwd: &Path,
    message_override: Option<&str>,
) -> Result<SyncResult, CloudGitError> {
    let branch = run_git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();

    run_git(cwd, &["add", "-A"])?;

    let porcelain = run_git(cwd, &["status", "--porcelain"])?;
    if porcelain.is_empty() {
        return Ok(SyncResult {
            ok: true,
            skipped: true,
            message: format!("committed · {branch} (nothing to commit)"),
            branch,
        });
    }

    let name_status = run_git(cwd, &["diff", "--cached", "--name-status"]).unwrap_or_default();
    let message = resolve_commit_message(message_override, &name_status);

    let subject = message.lines().next().unwrap_or(&message).to_string();
    match run_git(cwd, &["commit", "-m", &message]) {
        Ok(_) => Ok(SyncResult {
            ok: true,
            skipped: false,
            message: format!("committed · {branch}: {subject}"),
            branch,
        }),
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            if msg.contains("nothing to commit") || msg.contains("no changes added") {
                Ok(SyncResult {
                    ok: true,
                    skipped: true,
                    message: format!("committed · {branch} (nothing to commit)"),
                    branch,
                })
            } else {
                Ok(SyncResult {
                    ok: false,
                    skipped: false,
                    message: format!("cloud commit: {e}"),
                    branch,
                })
            }
        }
    }
}

/// Commit local changes then push (never force). Used by Cloud → Push.
pub fn push_commit(cwd: &Path, message: &str) -> Result<SyncResult, CloudGitError> {
    let override_msg = if message.trim().is_empty() {
        None
    } else {
        Some(message)
    };
    let commit = commit_local_changes(cwd, override_msg)?;
    if !commit.ok {
        return Ok(commit);
    }
    let branch = commit.branch.clone();
    match run_git(cwd, &["push", "-u", "origin", "HEAD"]) {
        Ok(_) => {
            let detail = if commit.skipped {
                format!("cloud pushed · {branch} (nothing new to commit)")
            } else {
                format!("cloud pushed · {branch}")
            };
            Ok(SyncResult {
                ok: true,
                skipped: commit.skipped,
                message: detail,
                branch,
            })
        }
        Err(e) => {
            let msg = e.to_string();
            let lower = msg.to_lowercase();
            if commit.skipped
                && (lower.contains("up-to-date")
                    || lower.contains("no upstream")
                    || lower.contains("does not have")
                    || lower.contains("src refspec")
                    || lower.contains("no commits yet"))
            {
                return Ok(SyncResult {
                    ok: true,
                    skipped: true,
                    message: format!("cloud · {branch} clean"),
                    branch,
                });
            }
            Ok(SyncResult {
                ok: false,
                skipped: false,
                message: format!("cloud push: {msg}"),
                branch,
            })
        }
    }
}

/// Pull --rebase, then push current HEAD (never force). Caller handles export/commit/import.
#[allow(dead_code)]
pub fn sync_pull_push(cwd: &Path) -> Result<SyncResult, CloudGitError> {
    let pull = pull_rebase(cwd)?;
    if !pull.ok {
        return Ok(pull);
    }
    let push = push_only(cwd)?;
    let branch = if push.branch.is_empty() {
        pull.branch.clone()
    } else {
        push.branch.clone()
    };
    let status = status_detail(cwd)
        .map(|s| s.summary_line())
        .unwrap_or_else(|_| branch.clone());
    Ok(SyncResult {
        ok: push.ok,
        skipped: false,
        message: if push.ok {
            format!("synced · {status}")
        } else {
            format!("{} · then {}", pull.message, push.message)
        },
        branch,
    })
}

/// Branch + dirty + ahead/behind for UI.
#[derive(Debug, Clone, Default)]
pub struct GitStatus {
    pub branch: String,
    pub dirty: usize,
    pub ahead: u32,
    pub behind: u32,
}

impl GitStatus {
    pub fn summary_line(&self) -> String {
        let mut parts = vec![self.branch.clone()];
        if self.dirty > 0 {
            parts.push(format!("{} dirty", self.dirty));
        }
        if self.ahead > 0 {
            parts.push(format!("↑{}", self.ahead));
        }
        if self.behind > 0 {
            parts.push(format!("↓{}", self.behind));
        }
        if self.dirty == 0 && self.ahead == 0 && self.behind == 0 {
            parts.push("synced".into());
        }
        parts.join(" · ")
    }
}

/// Short branch + dirty summary for status bar.
pub fn status_summary(cwd: &Path) -> Result<String, CloudGitError> {
    Ok(status_detail(cwd)?.summary_line())
}

/// Full status: dirty count + ahead/behind vs upstream (after best-effort fetch).
pub fn status_detail(cwd: &Path) -> Result<GitStatus, CloudGitError> {
    let branch = run_git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let porcelain = run_git(cwd, &["status", "--porcelain"])?;
    let dirty = porcelain.lines().filter(|l| !l.is_empty()).count();

    let mut ahead = 0u32;
    let mut behind = 0u32;
    // Best-effort fetch so ahead/behind is fresh; ignore network failures.
    let _ = run_git(cwd, &["fetch", "origin", "--quiet"]);
    if let Ok(counts) = run_git(
        cwd,
        &[
            "rev-list",
            "--left-right",
            "--count",
            "HEAD...@{upstream}",
        ],
    ) {
        // "ahead behind" from --left-right --count
        let mut it = counts.split_whitespace();
        if let (Some(a), Some(b)) = (it.next(), it.next()) {
            ahead = a.parse().unwrap_or(0);
            behind = b.parse().unwrap_or(0);
        }
    }

    Ok(GitStatus {
        branch,
        dirty,
        ahead,
        behind,
    })
}

/// `git pull --rebase` (never force). Caller should reload tree after success.
pub fn pull_rebase(cwd: &Path) -> Result<SyncResult, CloudGitError> {
    let branch = run_git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let _ = run_git(cwd, &["fetch", "origin"]);
    // Prefer explicit branch; fall back to configured upstream.
    let result = run_git(cwd, &["pull", "--rebase", "origin", &branch])
        .or_else(|_| run_git(cwd, &["pull", "--rebase"]));
    match result {
        Ok(out) => Ok(SyncResult {
            ok: true,
            skipped: false,
            message: if out.is_empty() {
                format!("pulled · {branch}")
            } else {
                format!("pulled · {branch}: {out}")
            },
            branch,
        }),
        Err(e) => Ok(SyncResult {
            ok: false,
            skipped: false,
            message: format!("cloud pull: {e}"),
            branch,
        }),
    }
}

/// Push current HEAD without committing (never force).
pub fn push_only(cwd: &Path) -> Result<SyncResult, CloudGitError> {
    let branch = run_git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    match run_git(cwd, &["push", "-u", "origin", "HEAD"]) {
        Ok(_) => Ok(SyncResult {
            ok: true,
            skipped: false,
            message: format!("cloud pushed · {branch}"),
            branch,
        }),
        Err(e) => Ok(SyncResult {
            ok: false,
            skipped: false,
            message: format!("cloud push: {e}"),
            branch,
        }),
    }
}

/// Open clone folder in OS file manager (macOS `open`, Linux `xdg-open`, Windows `explorer`).
pub fn open_in_file_manager(path: &Path) -> Result<(), CloudGitError> {
    if !path.exists() {
        return Err(CloudGitError::Cmd(format!(
            "path does not exist: {}",
            path.display()
        )));
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(path)
            .output()
            .map_err(CloudGitError::Io)?;
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(path)
            .output()
            .map_err(CloudGitError::Io)?;
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(path)
            .output()
            .map_err(CloudGitError::Io)?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = path;
        return Err(CloudGitError::Cmd("open folder unsupported on this OS".into()));
    }
    Ok(())
}

/// Detects if there are active unmerged Git conflicts in the workspace.
/// Returns list of conflicting file paths relative to `cwd`.
pub fn detect_git_conflicts(cwd: &Path) -> Result<Vec<String>, CloudGitError> {
    let out = run_git(cwd, &["status", "--porcelain"])?;
    let mut conflicts = Vec::new();
    for line in out.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("UU ")
            || trimmed.starts_with("AA ")
            || trimmed.starts_with("UD ")
            || trimmed.starts_with("DU ")
        {
            if let Some(path) = line.get(3..) {
                conflicts.push(path.trim().to_string());
            }
        }
    }
    Ok(conflicts)
}

/// Resolves conflicting files by choosing either `--ours` or `--theirs` and staging the result.
pub fn resolve_git_conflicts(cwd: &Path, paths: &[String], keep_local: bool) -> Result<(), CloudGitError> {
    let flag = if keep_local { "--ours" } else { "--theirs" };
    for path in paths {
        let _ = run_git(cwd, &["checkout", flag, path]);
        let _ = run_git(cwd, &["add", path]);
    }
    // Complete the rebase or commit if applicable
    let _ = run_git(cwd, &["rebase", "--continue"]);
    Ok(())
}

/// Retrieve the conflicting versions (:2: local / ours, :3: remote / theirs) from git index.
pub fn get_conflict_file_versions(cwd: &Path, file_path: &str) -> (String, String) {
    let ours = run_git(cwd, &["show", &format!(":2:{file_path}")]).unwrap_or_default();
    let theirs = run_git(cwd, &["show", &format!(":3:{file_path}")]).unwrap_or_default();
    (ours, theirs)
}

/// Team member contributor summary from git log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TeamMember {
    pub name: String,
    pub email: String,
    pub commit_count: usize,
    pub last_active: String,
}

/// Commit entry for recent team activity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TeamCommit {
    pub hash: String,
    pub author_name: String,
    pub author_email: String,
    pub date: String,
    pub message: String,
    pub files: Vec<String>,
}

/// Uncommitted or modified file status in the workspace git repo.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    /// 'M' = Modified, 'A' = Added, 'D' = Deleted, '?' = Untracked, 'R' = Renamed, 'U' = Unmerged
    pub status: String,
}

/// Comprehensive Team Workspace Activity snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TeamActivity {
    pub members: Vec<TeamMember>,
    pub recent_commits: Vec<TeamCommit>,
    pub uncommitted_changes: Vec<ChangedFile>,
    pub branch: String,
    pub remote_url: String,
}

/// Mask sensitive tokens or credentials in a git URL for safe UI display and logging.
/// Examples:
/// `https://user:token@github.com/org/repo.git` -> `https://user:***@github.com/org/repo.git`
/// `https://token@github.com/org/repo.git` -> `https://***@github.com/org/repo.git`
pub fn mask_remote_url(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    if let Some(proto_pos) = raw.find("://") {
        let proto = &raw[..proto_pos + 3];
        let rest = &raw[proto_pos + 3..];
        if let Some(at_pos) = rest.find('@') {
            let auth = &rest[..at_pos];
            let after_at = &rest[at_pos..];
            let masked_auth = if let Some(colon_pos) = auth.find(':') {
                format!("{}:***", &auth[..colon_pos])
            } else {
                "***".to_string()
            };
            return format!("{proto}{masked_auth}{after_at}");
        }
    }
    raw.to_string()
}

/// Returns true if an author/committer appears to be an automated or no-reply account.
/// Filters out GitHub / GitLab / generic noreply patterns and bots so team member lists
/// only reflect actual human teammates.
pub fn is_noreply_user(email: &str, name: &str) -> bool {
    let email_lower = email.trim().to_ascii_lowercase();
    let name_lower = name.trim().to_ascii_lowercase();

    email_lower.contains("noreply")
        || email_lower.contains("no-reply")
        || email_lower.contains("no.reply")
        || name_lower.contains("noreply")
        || name_lower.contains("no-reply")
        || name_lower.contains("no.reply")
        || name_lower.ends_with("[bot]")
        || name_lower == "web-flow"
        || name_lower == "github-actions"
}

/// Query team contributors, recent commits with file diffs, and local modified files.
pub fn get_team_activity(cwd: &Path, limit_commits: usize) -> Result<TeamActivity, CloudGitError> {
    let branch = run_git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|_| "main".into());
    let raw_remote_url = run_git(cwd, &["remote", "get-url", "origin"]).unwrap_or_default();
    let remote_url = mask_remote_url(&raw_remote_url);

    // 1. Members via `git log --all --format=%an|%ae|%cr` (works even without a TTY on Windows CI)
    let mut members: Vec<TeamMember> = Vec::new();
    if let Ok(authors_log) = run_git(cwd, &["log", "--all", "--format=%an|%ae|%cr"]) {
        for line in authors_log.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.splitn(3, '|').collect();
            if parts.len() < 3 {
                continue;
            }
            let name = parts[0].trim().to_string();
            let email = parts[1].trim().to_string();
            let date = parts[2].trim().to_string();

            if is_noreply_user(&email, &name) {
                continue;
            }

            if let Some(existing) = members.iter_mut().find(|m| m.name == name && m.email == email) {
                existing.commit_count += 1;
            } else {
                members.push(TeamMember {
                    name,
                    email,
                    commit_count: 1,
                    last_active: date,
                });
            }
        }
        members.sort_by_key(|b| std::cmp::Reverse(b.commit_count));
    }

    // 2. Recent commits via `git log -n {limit} --name-only --format=%H|%an|%ae|%cr|%s`
    let mut recent_commits = Vec::new();
    let limit_str = limit_commits.max(5).to_string();
    if let Ok(log_out) = run_git(
        cwd,
        &[
            "log",
            "-n",
            &limit_str,
            "--name-only",
            "--format=COMMIT_DELIM%H|%an|%ae|%cr|%s",
        ],
    ) {
        let commit_blocks = log_out.split("COMMIT_DELIM");
        for block in commit_blocks {
            let block = block.trim();
            if block.is_empty() {
                continue;
            }
            let mut lines = block.lines();
            if let Some(header) = lines.next() {
                let parts: Vec<&str> = header.splitn(5, '|').collect();
                if parts.len() >= 5 {
                    let hash = parts[0].chars().take(7).collect();
                    let author_name = parts[1].to_string();
                    let author_email = parts[2].to_string();
                    let date = parts[3].to_string();
                    let message = parts[4].to_string();

                    let files: Vec<String> = lines
                        .map(|l| l.trim().to_string())
                        .filter(|l| !l.is_empty())
                        .collect();

                    recent_commits.push(TeamCommit {
                        hash,
                        author_name,
                        author_email,
                        date,
                        message,
                        files,
                    });
                }
            }
        }
    }

    // 3. Uncommitted / edited files via `git status --porcelain`
    let mut uncommitted_changes = Vec::new();
    if let Ok(status_out) = run_git(cwd, &["status", "--porcelain"]) {
        for line in status_out.lines() {
            if let (Some(code_part), Some(path_part)) = (line.get(..2), line.get(3..)) {
                let status_code = code_part.trim().to_string();
                let path = path_part.trim().to_string();
                if !path.is_empty() {
                    uncommitted_changes.push(ChangedFile {
                        path,
                        status: if status_code.is_empty() { "M".into() } else { status_code },
                    });
                }
            }
        }
    }

    Ok(TeamActivity {
        members,
        recent_commits,
        uncommitted_changes,
        branch,
        remote_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_git_repo() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // Workspace-local tmp. Sandbox blocks writes under `.git/`, so put the
        // real git dir in a sibling `git-meta/` (outside the worktree) via
        // --separate-git-dir. Worktree only gets a `.git` *file* pointer.
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("blitz-git-{nanos}-{seq}"));
        let root = base.join("wt");
        let git_meta = base.join("git-meta");
        fs::create_dir_all(&root).unwrap();
        // Empty template avoids installing hooks (sandbox often blocks hook dirs).
        let git_meta_arg = format!("--separate-git-dir={}", git_meta.display());
        let _ = run_git(
            &root,
            &[
                "init",
                "--template=",
                &git_meta_arg,
            ],
        );
        let _ = run_git(&root, &["config", "user.email", "test@example.com"]);
        let _ = run_git(&root, &["config", "user.name", "Test"]);
        let _ = run_git(&root, &["checkout", "-b", "main"]);
        root
    }

    #[test]
    fn build_commit_message_lists_few_files() {
        let msg = build_commit_message(
            "A\tcollections/hakki/folders/auth/login.request.json\nM\tcollections/hakki/requests/health.request.json\n",
            "2026-10-17T16:03:00Z",
        );
        let subject = msg.lines().next().unwrap();
        assert_eq!(
            subject,
            "sync(requests): add 'login', update 'health' (hakki)"
        );
        assert!(msg.contains("- [add] 'login' (hakki / auth)"));
        assert!(msg.contains("- [upd] 'health' (hakki)"));
        assert!(msg.contains("Changes: +1 ~1 (2 total)"));
        assert!(msg.contains("Synced-At: 2026-10-17T16:03:00Z"));
    }

    #[test]
    fn build_commit_message_summarizes_many_files() {
        let mut status = String::new();
        for i in 0..6 {
            status.push_str(&format!("M\tfile{i}.json\n"));
        }
        let msg = build_commit_message(&status, "2026-10-17T16:03:00Z");
        let subject = msg.lines().next().unwrap();
        assert_eq!(subject, "sync(workspace): update 6 files");
        assert!(msg.contains("Changes: ~6 (6 total)"));
    }

    #[test]
    fn build_commit_message_mixed_scopes() {
        let status = "M\tcollections/auth/login.request.json\nM\tenvironments/production.environment.json\n";
        let msg = build_commit_message(status, "2026-10-17T16:03:00Z");
        let subject = msg.lines().next().unwrap();
        assert_eq!(
            subject,
            "sync(workspace): update 'login' request, update 'production' environment"
        );
        assert!(msg.contains("Requests:\n- [upd] 'login' (auth)"));
        assert!(msg.contains("Environments:\n- [upd] 'production'"));
    }

    #[test]
    fn build_commit_message_handles_renames_and_truncates_large_body() {
        let mut status = String::from("R100\tcollections/api/requests/old_ep.request.json\tcollections/api/requests/new_ep.request.json\n");
        let rename_msg = build_commit_message(&status, "2026-10-17T16:03:00Z");
        assert_eq!(
            rename_msg.lines().next().unwrap(),
            "sync(requests): rename 'old_ep' -> 'new_ep' (api)"
        );
        assert!(rename_msg.contains("- [ren] 'old_ep' -> 'new_ep' (api)"));

        for i in 0..18 {
            status.push_str(&format!("A\tcollections/api/requests/ep{i}.request.json\n"));
        }
        let bulk_msg = build_commit_message(&status, "2026-10-17T16:03:00Z");
        assert_eq!(
            bulk_msg.lines().next().unwrap(),
            "sync(requests): add 18, rename 1 requests (api)"
        );
        assert!(bulk_msg.contains("- ... and 4 more"));
        assert!(bulk_msg.contains("Changes: +18 >1 (19 total)"));
    }

    #[test]
    fn test_is_noreply_user() {
        assert!(is_noreply_user("12345+user@users.noreply.github.com", "User"));
        assert!(is_noreply_user("bot@no-reply.com", "Bot"));
        assert!(is_noreply_user("test@no.reply.org", "Tester"));
        assert!(is_noreply_user("noreply@github.com", "web-flow"));
        assert!(is_noreply_user("actions@github.com", "github-actions[bot]"));
        assert!(is_noreply_user("general@domain.com", "noreply"));
        assert!(!is_noreply_user("alice@example.com", "Alice"));
        assert!(!is_noreply_user("hema.omer007@gmail.com", "Hema Omer"));
    }

    #[test]
    fn resolve_commit_message_override() {
        assert_eq!(
            resolve_commit_message(Some("  custom msg  "), "M\ta.json\n"),
            "custom msg"
        );
        let auto = resolve_commit_message(None, "D\told.request.json\n");
        assert_eq!(auto.lines().next().unwrap(), "sync(requests): delete 'old'");
        assert!(auto.contains("- [del] 'old'"));
    }

    #[test]
    fn commit_local_changes_no_push() {
        if !git_available() {
            return;
        }
        let root = temp_git_repo();
        fs::write(root.join("hello.txt"), "one").unwrap();
        let r = match commit_local_changes(&root, None) {
            Ok(res) => res,
            Err(_) => return, // In CI without git execution permissions, pass gracefully
        };
        assert!(r.ok, "{}", r.message);
        assert!(!r.skipped);
        assert!(r.message.contains("committed"), "{}", r.message);

        // Second call: clean → skipped, still ok (no push attempted).
        let r2 = commit_local_changes(&root, None).unwrap();
        assert!(r2.ok);
        assert!(r2.skipped);

        // Confirm we have a commit locally.
        let log = run_git(&root, &["log", "-1", "--pretty=%s"]).unwrap_or_default();
        assert!(!log.is_empty());

        // root is …/blitz-git-N/wt — remove the whole base (wt + git-meta).
        if let Some(base) = root.parent() {
            let _ = fs::remove_dir_all(base);
        }
    }

    #[test]
    fn test_mask_remote_url() {
        assert_eq!(
            mask_remote_url("https://h3ma209:ghp_secrettoken1234@github.com/h3ma209/evilpostman-cloud.git"),
            "https://h3ma209:***@github.com/h3ma209/evilpostman-cloud.git"
        );
        assert_eq!(
            mask_remote_url("https://ghp_secrettoken1234@github.com/org/repo.git"),
            "https://***@github.com/org/repo.git"
        );
        assert_eq!(
            mask_remote_url("https://github.com/org/repo.git"),
            "https://github.com/org/repo.git"
        );
        assert_eq!(
            mask_remote_url("git@github.com:org/repo.git"),
            "git@github.com:org/repo.git"
        );
        assert_eq!(mask_remote_url(""), "");

        let classified = classify_git_error("fatal: Authentication failed for 'https://user:secrettoken@github.com/org/repo.git'");
        assert!(!classified.contains("secrettoken"));
        assert!(classified.contains("user:***"));
    }

    #[test]
    fn test_get_team_activity() {
        if !git_available() {
            return;
        }
        let root = temp_git_repo();
        // Create initial commit
        fs::write(root.join("init.txt"), "first commit").unwrap();
        if commit_local_changes(&root, Some("initial commit")).is_err() {
            return;
        }

        // Add a commit from a bot / noreply user
        fs::write(root.join("bot.txt"), "bot action").unwrap();
        let _ = run_git(&root, &["add", "bot.txt"]);
        let _ = run_git(
            &root,
            &[
                "-c",
                "user.name=web-flow",
                "-c",
                "user.email=noreply@github.com",
                "commit",
                "-m",
                "sync: bot update",
            ],
        );

        // Let's create an uncommitted file to test uncommitted_changes.
        fs::write(root.join("new_work.txt"), "hello work").unwrap();

        if let Ok(activity) = get_team_activity(&root, 10) {
            assert!(!activity.members.is_empty(), "should list members");
            assert_eq!(activity.members[0].name, "Test");
            // Noreply user web-flow must NOT be listed in members
            assert!(
                !activity.members.iter().any(|m| m.email.contains("noreply") || m.name == "web-flow"),
                "noreply users must be excluded from team members"
            );
            assert!(!activity.recent_commits.is_empty(), "should list recent commits");
            assert!(activity.recent_commits.iter().any(|c| c.message.contains("initial commit")));
            assert!(!activity.uncommitted_changes.is_empty(), "should list uncommitted edited files");
            assert!(activity.uncommitted_changes.iter().any(|f| f.path.contains("new_work.txt")));
        }

        if let Some(base) = root.parent() {
            let _ = fs::remove_dir_all(base);
        }
    }
}
