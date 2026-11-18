//! Minimal performance runner: N concurrent VUs hitting a collection for a duration.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::core::{Collection, CookieJar, RequestDraft, VariableContext};
use crate::runtime::{ordered_requests, send_request_with_options, SendOptions};

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PerfReport {
    pub vus: u32,
    pub duration_secs: u64,
    pub total_requests: u64,
    pub errors: u64,
    pub latency_sum_ms: u64,
    pub latency_max_ms: u64,
    pub latency_min_ms: u64,
    #[serde(default)]
    pub p50_latency_ms: u64,
    #[serde(default)]
    pub p95_latency_ms: u64,
    #[serde(default)]
    pub p99_latency_ms: u64,
    #[serde(default)]
    pub status_2xx: u64,
    #[serde(default)]
    pub status_3xx: u64,
    #[serde(default)]
    pub status_4xx: u64,
    #[serde(default)]
    pub status_5xx: u64,
}

fn percentile_sorted(sorted: &[u64], pct: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() * pct).saturating_sub(1) / 100).min(sorted.len() - 1);
    sorted[idx]
}

impl PerfReport {
    pub fn avg_latency_ms(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.latency_sum_ms as f64 / self.total_requests as f64
        }
    }

    pub fn rps(&self) -> f64 {
        let dur = self.duration_secs.max(1);
        (self.total_requests as f64 / dur as f64 * 100.0).round() / 100.0
    }

    pub fn error_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            ((self.errors as f64 / self.total_requests as f64) * 10000.0).round() / 100.0
        }
    }
}

/// Run collection requests concurrently for `duration`. Blocking.
pub fn run_performance(
    collection: &Collection,
    ctx: &VariableContext,
    vus: u32,
    duration: Duration,
) -> PerfReport {
    run_performance_with_cancel(collection, ctx, vus, duration, None)
}

/// Run collection requests concurrently for `duration` with optional cancellation flag. Blocking.
pub fn run_performance_with_cancel(
    collection: &Collection,
    ctx: &VariableContext,
    vus: u32,
    duration: Duration,
    cancel: Option<Arc<AtomicBool>>,
) -> PerfReport {
    let reqs: Vec<RequestDraft> = ordered_requests(collection);
    if reqs.is_empty() || vus == 0 {
        return PerfReport {
            vus,
            duration_secs: duration.as_secs(),
            ..Default::default()
        };
    }

    let started_run = Instant::now();
    let total = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));
    let lat_sum = Arc::new(AtomicU64::new(0));
    let lat_max = Arc::new(AtomicU64::new(0));
    let lat_min = Arc::new(AtomicU64::new(u64::MAX));
    let s2xx = Arc::new(AtomicU64::new(0));
    let s3xx = Arc::new(AtomicU64::new(0));
    let s4xx = Arc::new(AtomicU64::new(0));
    let s5xx = Arc::new(AtomicU64::new(0));
    let stop_at = started_run + duration;
    let reqs = Arc::new(reqs);
    let ctx = Arc::new(ctx.clone());
    let col_auth = Arc::new(collection.auth.clone());

    let mut handles = Vec::new();
    for _ in 0..vus {
        let total = Arc::clone(&total);
        let errors = Arc::clone(&errors);
        let lat_sum = Arc::clone(&lat_sum);
        let lat_max = Arc::clone(&lat_max);
        let lat_min = Arc::clone(&lat_min);
        let s2xx = Arc::clone(&s2xx);
        let s3xx = Arc::clone(&s3xx);
        let s4xx = Arc::clone(&s4xx);
        let s5xx = Arc::clone(&s5xx);
        let reqs = Arc::clone(&reqs);
        let ctx = Arc::clone(&ctx);
        let col_auth = Arc::clone(&col_auth);
        let cancel = cancel.clone();
        handles.push(std::thread::spawn(move || {
            let mut i = 0usize;
            let mut jar = CookieJar::default();
            let mut samples = Vec::with_capacity(256);
            while Instant::now() < stop_at {
                if cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
                    break;
                }
                let draft = &reqs[i % reqs.len()];
                i += 1;
                let started = Instant::now();
                let mut local_ctx = (*ctx).clone();
                let view = send_request_with_options(
                    draft,
                    &mut local_ctx,
                    SendOptions {
                        collection_auth: Some(&col_auth),
                        cookie_jar: Some(&mut jar),
                        cancel: cancel.clone(),
                        ..Default::default()
                    },
                );
                if cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
                    break;
                }
                let ms = started.elapsed().as_millis() as u64;
                total.fetch_add(1, Ordering::Relaxed);
                lat_sum.fetch_add(ms, Ordering::Relaxed);
                lat_max.fetch_max(ms, Ordering::Relaxed);
                lat_min.fetch_min(ms, Ordering::Relaxed);
                if samples.len() < 50_000 {
                    samples.push(ms);
                }
                match view {
                    Ok(v) => {
                        match v.status {
                            200..=299 => {
                                s2xx.fetch_add(1, Ordering::Relaxed);
                            }
                            300..=399 => {
                                s3xx.fetch_add(1, Ordering::Relaxed);
                            }
                            400..=499 => {
                                s4xx.fetch_add(1, Ordering::Relaxed);
                            }
                            500..=599 => {
                                s5xx.fetch_add(1, Ordering::Relaxed);
                            }
                            _ => {}
                        }
                        if v.error.is_some() || v.status == 0 || v.status >= 400 {
                            errors.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(_) => {
                        errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            samples
        }));
    }
    let mut all_samples: Vec<u64> = Vec::new();
    for h in handles {
        if let Ok(mut s) = h.join() {
            all_samples.append(&mut s);
        }
    }
    all_samples.sort_unstable();

    let min = lat_min.load(Ordering::Relaxed);
    let actual_secs = started_run.elapsed().as_secs().max(1).min(duration.as_secs().max(1));
    PerfReport {
        vus,
        duration_secs: actual_secs,
        total_requests: total.load(Ordering::Relaxed),
        errors: errors.load(Ordering::Relaxed),
        latency_sum_ms: lat_sum.load(Ordering::Relaxed),
        latency_max_ms: lat_max.load(Ordering::Relaxed),
        latency_min_ms: if min == u64::MAX { 0 } else { min },
        p50_latency_ms: percentile_sorted(&all_samples, 50),
        p95_latency_ms: percentile_sorted(&all_samples, 95),
        p99_latency_ms: percentile_sorted(&all_samples, 99),
        status_2xx: s2xx.load(Ordering::Relaxed),
        status_3xx: s3xx.load(Ordering::Relaxed),
        status_4xx: s4xx.load(Ordering::Relaxed),
        status_5xx: s5xx.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CollectionItem, HttpMethod};

    #[test]
    fn empty_collection_noop() {
        let col = Collection::new("empty");
        let r = run_performance(&col, &VariableContext::default(), 2, Duration::from_millis(50));
        assert_eq!(r.total_requests, 0);
    }

    #[test]
    fn runs_briefly_against_bad_host() {
        let mut col = Collection::new("perf");
        let mut d = RequestDraft {
            method: HttpMethod::Get,
            url: "http://127.0.0.1:1/nope".into(),
            ..Default::default()
        };
        d.settings.timeout_ms = 200;
        col.items.push(CollectionItem::Request(d));
        let r = run_performance(&col, &VariableContext::default(), 2, Duration::from_millis(300));
        assert!(r.total_requests > 0);
        assert!(r.errors > 0);
    }
}
