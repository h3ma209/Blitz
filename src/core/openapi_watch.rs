//! Local OpenAPI spec path watch helpers (mtime drift vs last sync).

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::collection::Collection;

pub fn spec_mtime_secs(path: &str) -> Option<u64> {
    let p = Path::new(path.trim());
    if path.trim().is_empty() || !p.is_file() {
        return None;
    }
    let modified = std::fs::metadata(p).ok()?.modified().ok()?;
    modified.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

pub fn collection_spec_drift(col: &Collection) -> bool {
    let path = col.openapi_spec_path.trim();
    if path.is_empty() {
        return false;
    }
    let Some(current) = spec_mtime_secs(path) else {
        return col.openapi_synced_mtime.is_some();
    };
    match col.openapi_synced_mtime {
        Some(synced) => current != synced,
        None => true,
    }
}

pub fn mark_collection_spec_synced(col: &mut Collection) {
    col.openapi_synced_mtime = spec_mtime_secs(&col.openapi_spec_path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn drift_when_mtime_changes() {
        let path = std::env::temp_dir().join(format!("blitz-openapi-watch-{}.yaml", uuid::Uuid::new_v4()));
        {
            let mut f = std::fs::File::create(&path).unwrap();
            writeln!(f, "openapi: 3.0.0").unwrap();
        }
        let mtime = spec_mtime_secs(path.to_str().unwrap()).unwrap();
        let mut col = Collection::default();
        col.openapi_spec_path = path.to_str().unwrap().to_string();
        assert!(collection_spec_drift(&col));
        col.openapi_synced_mtime = Some(mtime);
        assert!(!collection_spec_drift(&col));
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(&path, "openapi: 3.0.1\n").unwrap();
        assert!(collection_spec_drift(&col));
        let _ = std::fs::remove_file(&path);
    }
}
