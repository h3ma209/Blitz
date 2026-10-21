//! Workspace assets management for file payloads, metadata, and binary attachments.
//! Stored in the git-synced `assets/` directory at workspace root.

use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_ASSETS_DIR: &str = "assets";

/// Ensures that the workspace `assets/` directory (and subfolders) exists.
pub fn ensure_assets_dir(workspace_root: &Path, subfolder: Option<&str>) -> std::io::Result<PathBuf> {
    let mut dir = workspace_root.join(DEFAULT_ASSETS_DIR);
    if let Some(sub) = subfolder {
        dir = dir.join(sub);
    }
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Copies an external file into `<workspace_root>/assets/[subfolder]/<filename>`
/// and returns the relative path from the workspace root (e.g. `assets/payloads/data.json`).
/// If destination file already exists with identical content, avoids redundant writes.
pub fn copy_file_to_assets(
    workspace_root: &Path,
    source_path: &Path,
    subfolder: Option<&str>,
) -> Result<String, String> {
    if !source_path.exists() {
        return Err(format!("Source file does not exist: {}", source_path.display()));
    }
    let file_name = source_path
        .file_name()
        .ok_or_else(|| "Invalid file name".to_string())?;

    let dest_dir = ensure_assets_dir(workspace_root, subfolder)
        .map_err(|e| format!("Failed to create assets directory: {e}"))?;
    let dest_path = dest_dir.join(file_name);

    // If source and target are already the same file, nothing to copy
    if source_path.canonicalize().ok() == dest_path.canonicalize().ok() {
        let rel_path = dest_path
            .strip_prefix(workspace_root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| dest_path.to_string_lossy().to_string());
        return Ok(rel_path);
    }

    fs::copy(source_path, &dest_path)
        .map_err(|e| format!("Failed to copy file to assets: {e}"))?;

    let rel_path = dest_path
        .strip_prefix(workspace_root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| dest_path.to_string_lossy().to_string());

    Ok(rel_path)
}

/// Resolves a file path for transmission.
/// If `file_path` is relative, resolves against `workspace_root`.
/// If absolute, returns as-is.
pub fn resolve_asset_path(workspace_root: &Path, file_path: &str) -> PathBuf {
    let p = Path::new(file_path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        workspace_root.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assets_dir_creation_and_copy() {
        let unique_dir = std::env::temp_dir().join(format!("blitz_assets_test_{}", uuid::Uuid::new_v4()));
        let _ = fs::create_dir_all(&unique_dir);
        let ws = unique_dir.as_path();

        // create dummy source
        let src_file = ws.join("sample.pdf");
        fs::write(&src_file, b"%PDF-dummy-content").unwrap();

        let rel = copy_file_to_assets(ws, &src_file, Some("payloads")).unwrap();
        let normalized = rel.replace('\\', "/");
        assert_eq!(normalized, "assets/payloads/sample.pdf");

        let dest = resolve_asset_path(ws, &rel);
        assert!(dest.exists());
        assert_eq!(fs::read(&dest).unwrap(), b"%PDF-dummy-content");

        let _ = fs::remove_dir_all(&unique_dir);
    }

    #[test]
    fn test_resolve_asset_path_absolute() {
        let ws = std::path::Path::new("/dummy/ws");
        let abs = if cfg!(windows) { "C:\\foo\\bar.txt" } else { "/var/data/foo.txt" };
        let resolved = resolve_asset_path(ws, abs);
        assert_eq!(resolved, PathBuf::from(abs));
    }
}
