//! Request history — one entry per Send.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::request::{HttpMethod, RequestDraft};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: Uuid,
    pub at: DateTime<Utc>,
    pub method: HttpMethod,
    pub url: String,
    pub status: Option<u16>,
    pub elapsed_ms: Option<u128>,
    /// Snapshot of the draft at send time (for reopen).
    pub draft: RequestDraft,
}

impl HistoryEntry {
    pub fn from_send(
        draft: &RequestDraft,
        resolved_url: &str,
        status: Option<u16>,
        elapsed_ms: Option<u128>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            at: Utc::now(),
            method: draft.method,
            url: resolved_url.to_string(),
            status,
            elapsed_ms,
            draft: draft.clone(),
        }
    }
}

/// Newest-first; cap length.
pub fn push_history(history: &mut Vec<HistoryEntry>, entry: HistoryEntry, max: usize) {
    history.insert(0, entry);
    if history.len() > max {
        history.truncate(max);
    }
}

pub const HISTORY_MAX: usize = 100;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::RequestDraft;

    #[test]
    fn push_caps_length() {
        let mut h = Vec::new();
        for _ in 0..5 {
            push_history(
                &mut h,
                HistoryEntry::from_send(&RequestDraft::default(), "https://x", Some(200), Some(1)),
                3,
            );
        }
        assert_eq!(h.len(), 3);
    }
}
