//! Minimal performance runner: N concurrent VUs hitting a collection for a duration.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::core::{Collection, CookieJar, RequestDraft, VariableContext};
use crate::runtime::{ordered_requests, send_request_with_options, SendOptions};

#[derive(Debug, Clone, Default)]
pub struct PerfReport {
    pub vus: u32,
    pub duration_secs: u64,
    pub total_requests: u64,
    pub errors: u64,
    pub latency_sum_ms: u64,
    pub latency_max_ms: u64,
    pub latency_min_ms: u64,
}

impl PerfReport {
    pub fn avg_latency_ms(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.latency_sum_ms as f64 / self.total_requests as f64
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
    let reqs: Vec<RequestDraft> = ordered_requests(collection);
    if reqs.is_empty() || vus == 0 {
        return PerfReport {
            vus,
            duration_secs: duration.as_secs(),
            ..Default::default()
        };
    }

    let total = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));
    let lat_sum = Arc::new(AtomicU64::new(0));
    let lat_max = Arc::new(AtomicU64::new(0));
    let lat_min = Arc::new(AtomicU64::new(u64::MAX));
    let stop_at = Instant::now() + duration;
    let reqs = Arc::new(reqs);
    let ctx = Arc::new(ctx.clone());

    let mut handles = Vec::new();
    for _ in 0..vus {
        let total = Arc::clone(&total);
        let errors = Arc::clone(&errors);
        let lat_sum = Arc::clone(&lat_sum);
        let lat_max = Arc::clone(&lat_max);
        let lat_min = Arc::clone(&lat_min);
        let reqs = Arc::clone(&reqs);
        let ctx = Arc::clone(&ctx);
        handles.push(std::thread::spawn(move || {
            let mut i = 0usize;
            let mut jar = CookieJar::default();
            while Instant::now() < stop_at {
                let draft = &reqs[i % reqs.len()];
                i += 1;
                let started = Instant::now();
                let mut local_ctx = (*ctx).clone();
                let view = send_request_with_options(
                    draft,
                    &mut local_ctx,
                    SendOptions {
                        cookie_jar: Some(&mut jar),
                        ..Default::default()
                    },
                );
                let ms = started.elapsed().as_millis() as u64;
                total.fetch_add(1, Ordering::Relaxed);
                lat_sum.fetch_add(ms, Ordering::Relaxed);
                lat_max.fetch_max(ms, Ordering::Relaxed);
                lat_min.fetch_min(ms, Ordering::Relaxed);
                match view {
                    Ok(v) if v.error.is_none() && v.status < 400 => {}
                    _ => {
                        errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }

    let min = lat_min.load(Ordering::Relaxed);
    PerfReport {
        vus,
        duration_secs: duration.as_secs(),
        total_requests: total.load(Ordering::Relaxed),
        errors: errors.load(Ordering::Relaxed),
        latency_sum_ms: lat_sum.load(Ordering::Relaxed),
        latency_max_ms: lat_max.load(Ordering::Relaxed),
        latency_min_ms: if min == u64::MAX { 0 } else { min },
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
        let mut d = RequestDraft::default();
        d.method = HttpMethod::Get;
        d.url = "http://127.0.0.1:1/nope".into();
        d.settings.timeout_ms = 200;
        col.items.push(CollectionItem::Request(d));
        let r = run_performance(&col, &VariableContext::default(), 2, Duration::from_millis(300));
        assert!(r.total_requests > 0);
        assert!(r.errors > 0);
    }
}
