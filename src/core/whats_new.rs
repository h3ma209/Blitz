//! Release notes and post-update "What's New" helpers for Blitz.

use serde::Deserialize;

use crate::core::updater::{CURRENT_VERSION, GITHUB_REPO};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseSection {
    pub title: &'static str,
    pub items: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseEntry {
    pub version: &'static str,
    pub date: &'static str,
    pub headline: &'static str,
    pub summary: &'static str,
    pub sections: &'static [ReleaseSection],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteReleaseNote {
    pub tag: String,
    pub name: String,
    pub body: String,
    pub html_url: String,
    pub published_at: String,
}

#[derive(Deserialize)]
struct GitHubReleaseItem {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    published_at: Option<String>,
    draft: Option<bool>,
    prerelease: Option<bool>,
}

/// Returns true when the current app version differs from `last_seen_version`,
/// indicating an update (or fresh install) completed and What's New should open.
pub fn should_open_whats_new(last_seen_version: &str, current_version: &str) -> bool {
    let seen = last_seen_version.trim().trim_start_matches('v');
    let cur = current_version.trim().trim_start_matches('v');
    !cur.is_empty() && seen != cur
}

/// Fetch recent non-draft GitHub release notes for live changelog inspection.
pub fn fetch_github_release_notes(limit: usize) -> Result<Vec<RemoteReleaseNote>, String> {
    let url = format!(
        "https://api.github.com/repos/{GITHUB_REPO}/releases?per_page={}",
        limit.clamp(1, 20)
    );
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("Blitz/{CURRENT_VERSION}"))
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;
    let resp = client
        .get(&url)
        .send()
        .map_err(|e| format!("Request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub API returned {}", resp.status()));
    }
    let raw: Vec<GitHubReleaseItem> = resp.json().map_err(|e| format!("Invalid JSON: {e}"))?;
    Ok(raw
        .into_iter()
        .filter(|r| !r.draft.unwrap_or(false) && !r.prerelease.unwrap_or(false))
        .map(|r| RemoteReleaseNote {
            tag: r.tag_name.clone(),
            name: r.name.unwrap_or(r.tag_name),
            body: r.body.unwrap_or_default(),
            html_url: r.html_url,
            published_at: r.published_at.unwrap_or_default(),
        })
        .collect())
}

/// Built-in curated release notes shipped with Blitz (works offline immediately after update).
pub fn release_notes() -> &'static [ReleaseEntry] {
    RELEASE_NOTES
}

static RELEASE_NOTES: &[ReleaseEntry] = &[
    ReleaseEntry {
        version: "0.2.26",
        date: "2026-11-24",
        headline: "Post-Update What's New Stage Tab",
        summary: "Automatically opens a dedicated What's New tab in the main workbench after updating Blitz, detailing new features, fixes, and quick links.",
        sections: &[
            ReleaseSection {
                title: "New Features",
                items: &[
                    "What's New stage tab opens automatically after finishing an update (tracked via last_seen_version in settings)",
                    "Version timeline lets you browse recent Blitz releases offline plus load live GitHub Release Notes on demand",
                    "Quick-launch shortcuts to jump straight into Settings, Environments, Runner, and Lint",
                    "Command Palette (Cmd/Ctrl+K) command: What's New / Release Notes",
                ],
            },
        ],
    },
    ReleaseEntry {
        version: "0.2.25",
        date: "2026-11-24",
        headline: "Full MCP Collection, Folder & Request Management",
        summary: "AI agents connected via Blitz MCP can now create, edit, rename, move, reorder, duplicate, and sort collections, folders, and requests.",
        sections: &[
            ReleaseSection {
                title: "MCP Server",
                items: &[
                    "blitz_create_collection & blitz_update_collection — manage name, description, favorite, icon, color, scripts, and auth",
                    "blitz_update_folder — edit folder name, description, pre_request_script, test_script, and auth",
                    "blitz_rename_item — rename any collection, folder, or request by UUID, name, or Parent/Child path",
                    "blitz_move_item — reparent folders/requests across or within collections, reorder before a sibling or at a 0-based position",
                    "blitz_duplicate_item — deep-clone collections, folders, or requests in-place with fresh IDs",
                    "blitz_sort_collection — sort folders, collections, or all collections (folders_first_asc, name_asc/desc, method_order, etc.)",
                    "blitz_create_folder now accepts optional position index",
                ],
            },
        ],
    },
    ReleaseEntry {
        version: "0.2.24",
        date: "2026-11-24",
        headline: "Virtualized Large Lists, Git Switch Guard & Conflict Line Diff",
        summary: "Smoother scrolling on large collections/diffs, safer Git repo switching, and line-by-line conflict inspection.",
        sections: &[
            ReleaseSection {
                title: "Performance & UI",
                items: &[
                    "Virtualized Runner, Environment Diff, and Lint lists (>100 rows) for snappy rendering on huge workspaces",
                    "Clickable active Git repo + branch pill in the top bar",
                    "Conflict Resolver now supports Line Diff and Side-by-Side view toggles",
                ],
            },
            ReleaseSection {
                title: "Cloud Sync & Stability",
                items: &[
                    "Git repo switch guard confirms before discarding dirty tabs or uncommitted changes",
                    "Fixed Windows path sanitization in cloud sync multi-user workspace initialization",
                ],
            },
        ],
    },
    ReleaseEntry {
        version: "0.2.23",
        date: "2026-11-24",
        headline: "Stage Tabs for Runner, Performance, Team & Lint",
        summary: "Moved workspace tools out of cramped modals into full-width main stage tabs with improved split layouts.",
        sections: &[
            ReleaseSection {
                title: "Workspace Stage",
                items: &[
                    "Runner, Performance, Team Activity, and Lint now open as closable main stage tabs",
                    "Runner Execution Sequence fills the right pane pre-run with Results / Execution Sequence switcher",
                    "Environments Diff & Drift fills full stage height with a draggable Baseline/Target column resizer",
                ],
            },
        ],
    },
    ReleaseEntry {
        version: "0.2.22",
        date: "2026-11-24",
        headline: "Environments Stage Tab & Z-Index Layering",
        summary: "Dedicated Environments & Vault stage tab plus strict modal/palette/toast z-index hierarchy.",
        sections: &[
            ReleaseSection {
                title: "UI & Cloud Sync",
                items: &[
                    "Environments & Vault moved into main stage tab strip (supports close button, middle-click, Cmd/Ctrl+W)",
                    "Strict z-index hierarchy across top bar, modals, Command Palette, confirms, and toasts",
                    "Remote URL comparison ignores embedded userinfo credentials so token-authenticated origins match cleanly",
                ],
            },
        ],
    },
    ReleaseEntry {
        version: "0.2.21",
        date: "2026-11-24",
        headline: "Per-Remote Git Sync Isolation & Script/Editor Fixes",
        summary: "Isolated clone directories per Git remote, Recent Git Repositories switcher, and smoother code editing.",
        sections: &[
            ReleaseSection {
                title: "Cloud Sync & Runtime",
                items: &[
                    "Per-remote isolated clone directories under cloud-repos/<slug>-<hash8> + Recent Git Repositories MRU switcher",
                    "Auto-detects remote default branch via git ls-remote --symref",
                    "Post-response scripts now receive resolved & comment-stripped request body/headers",
                    "Fixed WebKit Enter/Tab scroll jumps and autocomplete Enter handling in code editors",
                ],
            },
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_open_when_version_changes_or_unset() {
        assert!(should_open_whats_new("", "0.2.26"));
        assert!(should_open_whats_new("0.2.25", "0.2.26"));
        assert!(should_open_whats_new("v0.2.25", "0.2.26"));
        assert!(!should_open_whats_new("0.2.26", "0.2.26"));
        assert!(!should_open_whats_new("v0.2.26", "0.2.26"));
    }

    #[test]
    fn built_in_release_notes_are_nonempty() {
        let notes = release_notes();
        assert!(!notes.is_empty());
        assert!(notes.iter().all(|n| !n.version.is_empty() && !n.sections.is_empty()));
    }
}
