//! Shared client-side aggregation helpers for telemetry.
//!
//! The transport layer (`one_api_list_request`, `mongo` dispatch) returns
//! flat lists. These helpers turn those lists into the percentiles, top-N
//! tables, and time buckets that telemetry subcommands surface.

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use std::collections::BTreeMap;

/// Percentile of a sorted-or-unsorted sample. `pct` is 0.0..=100.0.
///
/// Returns `None` if the slice is empty. Uses linear interpolation between
/// the two nearest ranks (NIST C=1).
pub fn percentile(values: &[f64], pct: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pct = pct.clamp(0.0, 100.0);
    if sorted.len() == 1 {
        return Some(sorted[0]);
    }
    let rank = pct / 100.0 * (sorted.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        return Some(sorted[lo]);
    }
    let frac = rank - lo as f64;
    Some(sorted[lo] + (sorted[hi] - sorted[lo]) * frac)
}

/// Group a list of `(key, value)` pairs by key, returning a sorted Vec.
/// `sort_desc` controls direction.
#[allow(dead_code)] // reserved for richer top-N callers in Phase 2/3.
pub fn top_n_by<K, V, F>(
    items: &[V],
    key_of: F,
    score_of: impl Fn(&V) -> f64,
    n: usize,
) -> Vec<(K, Vec<V>, f64)>
where
    K: Ord + Clone,
    V: Clone,
    F: Fn(&V) -> K,
{
    let mut groups: BTreeMap<K, Vec<V>> = BTreeMap::new();
    for item in items {
        groups.entry(key_of(item)).or_default().push(item.clone());
    }
    let mut rows: Vec<(K, Vec<V>, f64)> = groups
        .into_iter()
        .map(|(k, vs)| {
            let score: f64 = vs.iter().map(&score_of).sum();
            (k, vs, score)
        })
        .collect();
    rows.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    rows.into_iter().take(n).collect()
}

/// Compute count, mean, p50, p95, p99 over a numeric sample. Empty samples
/// produce zeros and `None` percentiles.
#[derive(Debug, Clone, Default)]
pub struct DurationStats {
    pub count: usize,
    pub mean_ms: Option<f64>,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub p99_ms: Option<f64>,
    pub min_ms: Option<f64>,
    pub max_ms: Option<f64>,
}

impl DurationStats {
    pub fn from_durations_ms(durations: &[f64]) -> Self {
        let filtered: Vec<f64> = durations
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .collect();
        if filtered.is_empty() {
            return Self {
                count: 0,
                ..Default::default()
            };
        }
        let sum: f64 = filtered.iter().sum();
        let count = filtered.len();
        let mut sorted = filtered.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Self {
            count,
            mean_ms: Some(sum / count as f64),
            p50_ms: percentile(&sorted, 50.0),
            p95_ms: percentile(&sorted, 95.0),
            p99_ms: percentile(&sorted, 99.0),
            min_ms: Some(sorted[0]),
            max_ms: Some(sorted[count - 1]),
        }
    }
}

/// 7-day × 24-hour run-count matrix. `day_of_week` is Mon=0..Sun=6.
#[derive(Debug, Clone)]
pub struct WeeklyMatrix {
    pub buckets: Vec<WeeklyBucket>,
}

#[derive(Debug, Clone, Copy)]
pub struct WeeklyBucket {
    pub day_of_week: u8,
    pub hour: u8,
    pub count: u64,
}

impl WeeklyMatrix {
    /// Zeroed 7×24 = 168-bucket matrix. Iterate through the supplied
    /// timestamps and increment the corresponding bucket.
    pub fn from_timestamps(stamps: &[DateTime<Utc>]) -> Self {
        let mut counts = [[0u64; 24]; 7];
        for ts in stamps {
            let dow = ts.weekday().num_days_from_monday() as usize;
            let hour = ts.hour() as usize;
            counts[dow][hour] = counts[dow][hour].saturating_add(1);
        }
        let mut buckets = Vec::with_capacity(168);
        for (d, hours) in counts.iter().enumerate() {
            for (h, count) in hours.iter().enumerate() {
                buckets.push(WeeklyBucket {
                    day_of_week: d as u8,
                    hour: h as u8,
                    count: *count,
                });
            }
        }
        Self { buckets }
    }
}

/// Bucket a sorted set of timestamps into fixed-width windows starting at
/// `start` and ending at `end`. Returns `(bucket_start, count)` pairs with
/// zero-count buckets included so the time series is stable.
#[allow(dead_code)] // reserved for Phase 2 server-side timelines.
pub fn time_series(
    stamps: &[DateTime<Utc>],
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    bucket_hours: i64,
) -> Vec<(DateTime<Utc>, u64)> {
    if bucket_hours <= 0 || end <= start {
        return Vec::new();
    }
    let step = Duration::hours(bucket_hours);
    let mut out: Vec<(DateTime<Utc>, u64)> = Vec::new();
    let mut t = start;
    while t < end {
        out.push((t, 0));
        t += step;
    }
    for s in stamps {
        if *s < start || *s >= end {
            continue;
        }
        let offset = (*s - start).num_hours() / bucket_hours;
        if let Some(slot) = out.get_mut(offset as usize) {
            slot.1 = slot.1.saturating_add(1);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn percentile_single_value() {
        assert_eq!(percentile(&[42.0], 50.0), Some(42.0));
        assert_eq!(percentile(&[42.0], 99.0), Some(42.0));
    }

    #[test]
    fn percentile_interpolates() {
        let v: Vec<f64> = (1..=100).map(|x| x as f64).collect();
        // p50 of 1..100 with linear interp: rank = 49.5 -> 50.5
        assert!((percentile(&v, 50.0).unwrap() - 50.5).abs() < 0.001);
        assert!((percentile(&v, 99.0).unwrap() - 99.01).abs() < 0.1);
    }

    #[test]
    fn percentile_empty_returns_none() {
        assert_eq!(percentile(&[], 50.0), None);
    }

    #[test]
    fn duration_stats_basic() {
        let s = DurationStats::from_durations_ms(&[100.0, 200.0, 300.0, 400.0]);
        assert_eq!(s.count, 4);
        assert_eq!(s.mean_ms, Some(250.0));
        assert_eq!(s.min_ms, Some(100.0));
        assert_eq!(s.max_ms, Some(400.0));
    }

    #[test]
    fn duration_stats_empty_returns_zero_count() {
        let s = DurationStats::from_durations_ms(&[]);
        assert_eq!(s.count, 0);
        assert!(s.mean_ms.is_none());
    }

    #[test]
    fn weekly_matrix_always_168_buckets() {
        let m = WeeklyMatrix::from_timestamps(&[]);
        assert_eq!(m.buckets.len(), 168);
        assert!(m.buckets.iter().all(|b| b.count == 0));
    }

    #[test]
    fn weekly_matrix_increments_correct_bucket() {
        // 2026-05-11 is a Monday (day_of_week = 0). 14:30 UTC.
        let ts = Utc.with_ymd_and_hms(2026, 5, 11, 14, 30, 0).unwrap();
        let m = WeeklyMatrix::from_timestamps(&[ts, ts]);
        let b = m
            .buckets
            .iter()
            .find(|b| b.day_of_week == 0 && b.hour == 14)
            .unwrap();
        assert_eq!(b.count, 2);
    }

    #[test]
    fn time_series_zero_fills_buckets() {
        let start = Utc.with_ymd_and_hms(2026, 5, 10, 0, 0, 0).unwrap();
        let end = start + Duration::hours(6);
        let ts = vec![start + Duration::hours(2)];
        let series = time_series(&ts, start, end, 1);
        assert_eq!(series.len(), 6);
        assert_eq!(series[2].1, 1);
        assert_eq!(series.iter().map(|(_, c)| *c).sum::<u64>(), 1);
    }

    #[test]
    fn top_n_groups_and_sorts() {
        let pairs: Vec<(String, f64)> = vec![
            ("a".into(), 1.0),
            ("b".into(), 5.0),
            ("a".into(), 2.0),
            ("c".into(), 1.0),
        ];
        let top = top_n_by(&pairs, |p| p.0.clone(), |p| p.1, 2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, "b");
        assert_eq!(top[0].2, 5.0);
        assert_eq!(top[1].0, "a");
        assert_eq!(top[1].2, 3.0);
    }
}
