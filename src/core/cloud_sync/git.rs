//! Shell-out to system `git` (P1 Native-Git shaped path).

use std::path::Path;
use std::process::Command;

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

/// `git add -A` → commit (skip empty) → `git push -u origin HEAD`. Never force-push.
pub fn push_commit(cwd: &Path, message: &str) -> Result<SyncResult, CloudGitError> {
    let branch = run_git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();

    run_git(cwd, &["add", "-A"])?;

    let porcelain = run_git(cwd, &["status", "--porcelain"])?;
    if porcelain.is_empty() {
        // Still try push in case ahead of remote
        match run_git(cwd, &["push", "-u", "origin", "HEAD"]) {
            Ok(_) => {
                return Ok(SyncResult {
                    ok: true,
                    skipped: true,
                    message: format!("cloud synced · {branch} (nothing to commit)"),
                    branch,
                });
            }
            Err(e) => {
                let msg = e.to_string();
                // "everything up-to-date" style may still fail if no upstream yet with empty
                if msg.to_lowercase().contains("up-to-date")
                    || msg.to_lowercase().contains("no upstream")
                    || msg.to_lowercase().contains("does not have")
                {
                    return Ok(SyncResult {
                        ok: true,
                        skipped: true,
                        message: format!("cloud · {branch} clean"),
                        branch,
                    });
                }
                // If nothing committed and push fails because no commits, soft-ok
                if msg.to_lowercase().contains("src refspec")
                    || msg.to_lowercase().contains("no commits yet")
                {
                    return Ok(SyncResult {
                        ok: true,
                        skipped: true,
                        message: format!("cloud · {branch} (no commits yet)"),
                        branch,
                    });
                }
                return Ok(SyncResult {
                    ok: false,
                    skipped: false,
                    message: format!("cloud push: {msg}"),
                    branch,
                });
            }
        }
    }

    // Commit; allow empty skip via status check already done
    match run_git(cwd, &["commit", "-m", message]) {
        Ok(_) => {}
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            if msg.contains("nothing to commit") || msg.contains("no changes added") {
                // fall through to push
            } else {
                return Ok(SyncResult {
                    ok: false,
                    skipped: false,
                    message: format!("cloud commit: {e}"),
                    branch,
                });
            }
        }
    }

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

/// Short branch + dirty summary for status bar.
pub fn status_summary(cwd: &Path) -> Result<String, CloudGitError> {
    let branch = run_git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let porcelain = run_git(cwd, &["status", "--porcelain"])?;
    let dirty = porcelain.lines().filter(|l| !l.is_empty()).count();
    if dirty == 0 {
        Ok(format!("{branch} · synced"))
    } else {
        Ok(format!("{branch} · {dirty} dirty"))
    }
}
