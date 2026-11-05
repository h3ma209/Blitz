//! Environment comparison, variable drift detection, and synchronization.

use std::collections::BTreeSet;
use serde::{Deserialize, Serialize};
use crate::core::environment::{Environment, EnvVar};

/// Status of a variable comparison between baseline and target environments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffStatus {
    /// Key exists in both with identical effective value and enabled status.
    Matching,
    /// Key exists in both, but value or enabled status differs.
    Drift,
    /// Key exists in baseline, but is missing in target.
    MissingInTarget,
    /// Key exists in target, but is missing in baseline.
    ExtraInTarget,
}

impl DiffStatus {
    pub fn label(&self) -> &'static str {
        match self {
            DiffStatus::Matching => "MATCH",
            DiffStatus::Drift => "DRIFT",
            DiffStatus::MissingInTarget => "MISSING",
            DiffStatus::ExtraInTarget => "EXTRA",
        }
    }

    pub fn css_class(&self) -> &'static str {
        match self {
            DiffStatus::Matching => "diff-status-match",
            DiffStatus::Drift => "diff-status-drift",
            DiffStatus::MissingInTarget => "diff-status-missing",
            DiffStatus::ExtraInTarget => "diff-status-extra",
        }
    }
}

/// Detailed variable row comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarDiffRow {
    pub key: String,
    pub baseline_var: Option<EnvVar>,
    pub target_var: Option<EnvVar>,
    pub status: DiffStatus,
}

/// Complete report of environment variable diff and drift.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvDiffReport {
    pub baseline_name: String,
    pub target_name: String,
    pub rows: Vec<VarDiffRow>,
    pub total_keys: usize,
    pub matching_count: usize,
    pub drift_count: usize,
    pub missing_in_target_count: usize,
    pub extra_in_target_count: usize,
}

/// Compute variable diff report between baseline and target environments.
pub fn compare_environments(baseline: &Environment, target: &Environment) -> EnvDiffReport {
    let mut all_keys = BTreeSet::new();
    for v in &baseline.values {
        if !v.key.trim().is_empty() {
            all_keys.insert(v.key.clone());
        }
    }
    for v in &target.values {
        if !v.key.trim().is_empty() {
            all_keys.insert(v.key.clone());
        }
    }

    let mut rows = Vec::with_capacity(all_keys.len());
    let mut matching_count = 0;
    let mut drift_count = 0;
    let mut missing_in_target_count = 0;
    let mut extra_in_target_count = 0;

    for key in all_keys {
        let b = baseline.values.iter().find(|v| v.key == key).cloned();
        let t = target.values.iter().find(|v| v.key == key).cloned();

        let status = match (&b, &t) {
            (Some(_), None) => {
                missing_in_target_count += 1;
                DiffStatus::MissingInTarget
            }
            (None, Some(_)) => {
                extra_in_target_count += 1;
                DiffStatus::ExtraInTarget
            }
            (Some(bv), Some(tv)) => {
                if bv.effective() == tv.effective() && bv.enabled == tv.enabled {
                    matching_count += 1;
                    DiffStatus::Matching
                } else {
                    drift_count += 1;
                    DiffStatus::Drift
                }
            }
            (None, None) => continue,
        };

        rows.push(VarDiffRow {
            key,
            baseline_var: b,
            target_var: t,
            status,
        });
    }

    let total_keys = rows.len();
    EnvDiffReport {
        baseline_name: baseline.name.clone(),
        target_name: target.name.clone(),
        rows,
        total_keys,
        matching_count,
        drift_count,
        missing_in_target_count,
        extra_in_target_count,
    }
}

/// Copy a single variable from baseline to target environment.
/// If key already exists in target, updates value and enabled status.
/// If missing, appends new variable.
pub fn sync_var_to_target(target: &mut Environment, baseline_var: &EnvVar, copy_current_as_initial: bool) {
    let val_to_copy = if copy_current_as_initial {
        baseline_var.effective().to_string()
    } else {
        baseline_var.initial_value.clone()
    };

    if let Some(existing) = target.values.iter_mut().find(|v| v.key == baseline_var.key) {
        existing.initial_value = val_to_copy.clone();
        existing.current_value = val_to_copy;
        existing.enabled = baseline_var.enabled;
    } else {
        target.values.push(EnvVar {
            key: baseline_var.key.clone(),
            initial_value: val_to_copy.clone(),
            current_value: val_to_copy,
            enabled: baseline_var.enabled,
        });
    }
}

/// Copy all missing variables from baseline into target. Returns count of added variables.
pub fn sync_all_missing_to_target(target: &mut Environment, baseline: &Environment, copy_current_as_initial: bool) -> usize {
    let mut added = 0;
    for b_var in &baseline.values {
        if b_var.key.trim().is_empty() {
            continue;
        }
        let exists = target.values.iter().any(|tv| tv.key == b_var.key);
        if !exists {
            sync_var_to_target(target, b_var, copy_current_as_initial);
            added += 1;
        }
    }
    added
}

/// Export diff report as a clean Markdown table.
pub fn export_diff_markdown(report: &EnvDiffReport) -> String {
    let mut md = String::new();
    md.push_str(&format!(
        "# Environment Diff: {} vs {}\n\n",
        report.baseline_name, report.target_name
    ));
    md.push_str(&format!(
        "- **Total Keys**: {}\n- **Matching**: {}\n- **Drift**: {}\n- **Missing in {}**: {}\n- **Extra in {}**: {}\n\n",
        report.total_keys,
        report.matching_count,
        report.drift_count,
        report.target_name,
        report.missing_in_target_count,
        report.target_name,
        report.extra_in_target_count
    ));
    md.push_str(&format!(
        "| Status | Variable | {} (Baseline) | {} (Target) |\n",
        report.baseline_name, report.target_name
    ));
    md.push_str("| :--- | :--- | :--- | :--- |\n");

    for r in &report.rows {
        let b_val = r.baseline_var.as_ref().map(|v| v.effective()).unwrap_or("(none)");
        let t_val = r.target_var.as_ref().map(|v| v.effective()).unwrap_or("(none)");
        md.push_str(&format!(
            "| `{}` | `{}` | `{}` | `{}` |\n",
            r.status.label(),
            r.key,
            b_val.replace('|', "\\|"),
            t_val.replace('|', "\\|")
        ));
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_environments_detection() {
        let mut dev = Environment::new("Development");
        dev.values.push(EnvVar::new("baseUrl", "http://localhost:3000"));
        dev.values.push(EnvVar::new("apiKey", "dev-secret-123"));
        dev.values.push(EnvVar::new("debug", "true"));

        let mut prod = Environment::new("Production");
        prod.values.push(EnvVar::new("baseUrl", "https://api.example.com"));
        prod.values.push(EnvVar::new("apiKey", "prod-secret-999"));
        prod.values.push(EnvVar::new("metricsEnabled", "true"));

        let report = compare_environments(&dev, &prod);
        assert_eq!(report.total_keys, 4);
        assert_eq!(report.drift_count, 2); // baseUrl and apiKey differ in values
        assert_eq!(report.missing_in_target_count, 1); // debug missing in prod
        assert_eq!(report.extra_in_target_count, 1); // metricsEnabled extra in prod
        assert_eq!(report.matching_count, 0);

        let debug_row = report.rows.iter().find(|r| r.key == "debug").unwrap();
        assert_eq!(debug_row.status, DiffStatus::MissingInTarget);

        let metrics_row = report.rows.iter().find(|r| r.key == "metricsEnabled").unwrap();
        assert_eq!(metrics_row.status, DiffStatus::ExtraInTarget);
    }

    #[test]
    fn test_sync_all_missing_to_target() {
        let mut dev = Environment::new("Dev");
        dev.values.push(EnvVar::new("k1", "v1"));
        dev.values.push(EnvVar::new("k2", "v2"));

        let mut prod = Environment::new("Prod");
        prod.values.push(EnvVar::new("k1", "prod_v1"));

        let added = sync_all_missing_to_target(&mut prod, &dev, true);
        assert_eq!(added, 1);
        assert_eq!(prod.values.len(), 2);
        assert_eq!(prod.values[1].key, "k2");
        assert_eq!(prod.values[1].effective(), "v2");
    }

    #[test]
    fn test_export_diff_markdown() {
        let mut e1 = Environment::new("A");
        e1.values.push(EnvVar::new("host", "http://a"));
        let mut e2 = Environment::new("B");
        e2.values.push(EnvVar::new("host", "http://b"));

        let rep = compare_environments(&e1, &e2);
        let md = export_diff_markdown(&rep);
        assert!(md.contains("# Environment Diff: A vs B"));
        assert!(md.contains("| `DRIFT` | `host` | `http://a` | `http://b` |"));
    }
}
