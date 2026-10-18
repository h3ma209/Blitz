//! Shell-out to system `git` (P1 Native-Git shaped path).

use std::path::Path;
use std::process::Command;

use chrono::{SecondsFormat, Utc};
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
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
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
    let lower = raw.to_lowercase();
    if lower.contains("authentication")
        || lower.contains("permission denied")
        || lower.contains("could not read username")
        || lower.contains("403")
        || lower.contains("401")
        || lower.contains("publickey")
    {
        format!(
            "auth failure — use SSH agent or `gh auth login` / credential helper. Detail: {raw}"
        )
    } else if lower.contains("conflict")
        || lower.contains("non-fast-forward")
        || lower.contains("rejected")
        || lower.contains("needs merge")
    {
        format!(
            "conflict / non-fast-forward — pull & resolve in the clone, then save again. Detail: {raw}"
        )
    } else {
        raw.to_string()
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
        .args(["clone", remote, name])
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

/// Build one-line commit message from `git diff --cached --name-status` (or porcelain-ish) text.
///
/// Format: `{ISO8601} A file.json M other.json` — or `{ISO8601} update N files` when many.
pub fn build_commit_message(name_status: &str, timestamp: &str) -> String {
    let mut entries: Vec<(char, String)> = Vec::new();
    for line in name_status.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // name-status: "M\tpath" or "A\tpath"; rename: "R100\told\tnew"
        let mut parts = line.split('\t');
        let status_tok = match parts.next() {
            Some(s) => s.trim(),
            None => continue,
        };
        let status = status_tok.chars().next().unwrap_or('M');
        let path = match parts.next_back().or_else(|| parts.next()) {
            Some(p) => p.trim(),
            None => continue,
        };
        if path.is_empty() {
            continue;
        }
        let file = Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path)
            .to_string();
        entries.push((status, file));
    }

    if entries.is_empty() {
        return format!("{timestamp} sync");
    }
    const MAX_LISTED: usize = 5;
    if entries.len() > MAX_LISTED {
        return format!("{timestamp} update {} files", entries.len());
    }
    let body = entries
        .iter()
        .map(|(s, f)| format!("{s} {f}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!("{timestamp} {body}")
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

    match run_git(cwd, &["commit", "-m", &message]) {
        Ok(_) => Ok(SyncResult {
            ok: true,
            skipped: false,
            message: format!("committed · {branch}: {message}"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_git_repo() -> std::path::PathBuf {
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
            .join(format!("evilpostman-git-{nanos}"));
        let root = base.join("wt");
        let git_meta = base.join("git-meta");
        fs::create_dir_all(&root).unwrap();
        // Empty template avoids installing hooks (sandbox often blocks hook dirs).
        run_git(
            &root,
            &[
                "init",
                "--template=",
                &format!("--separate-git-dir={}", git_meta.display()),
            ],
        )
        .unwrap();
        run_git(&root, &["config", "user.email", "test@example.com"]).unwrap();
        run_git(&root, &["config", "user.name", "Test"]).unwrap();
        let _ = run_git(&root, &["checkout", "-b", "main"]);
        root
    }

    #[test]
    fn build_commit_message_lists_few_files() {
        let msg = build_commit_message(
            "A\tcollections/hakki/folders/auth/login.request.json\nM\tcollections/hakki/requests/health.request.json\n",
            "2026-10-17T16:03:00Z",
        );
        assert_eq!(
            msg,
            "2026-10-17T16:03:00Z A login.request.json M health.request.json"
        );
    }

    #[test]
    fn build_commit_message_summarizes_many_files() {
        let mut status = String::new();
        for i in 0..6 {
            status.push_str(&format!("M\tfile{i}.json\n"));
        }
        let msg = build_commit_message(&status, "2026-10-17T16:03:00Z");
        assert_eq!(msg, "2026-10-17T16:03:00Z update 6 files");
    }

    #[test]
    fn resolve_commit_message_override() {
        assert_eq!(
            resolve_commit_message(Some("  custom msg  "), "M\ta.json\n"),
            "custom msg"
        );
        let auto = resolve_commit_message(None, "D\told.request.json\n");
        assert!(auto.contains("D old.request.json"), "{auto}");
    }

    #[test]
    fn commit_local_changes_no_push() {
        let root = temp_git_repo();
        fs::write(root.join("hello.txt"), "one").unwrap();
        let r = commit_local_changes(&root, None).unwrap();
        assert!(r.ok, "{}", r.message);
        assert!(!r.skipped);
        assert!(r.message.contains("committed"), "{}", r.message);

        // Second call: clean → skipped, still ok (no push attempted).
        let r2 = commit_local_changes(&root, None).unwrap();
        assert!(r2.ok);
        assert!(r2.skipped);

        // Confirm we have a commit locally.
        let log = run_git(&root, &["log", "-1", "--pretty=%s"]).unwrap();
        assert!(!log.is_empty());

        // root is …/evilpostman-git-N/wt — remove the whole base (wt + git-meta).
        if let Some(base) = root.parent() {
            let _ = fs::remove_dir_all(base);
        }
    }
}
