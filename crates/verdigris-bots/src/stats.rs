use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DurationStats {
    pub n: usize,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub p99_ms: Option<f64>,
    pub max_ms: Option<f64>,
}

impl DurationStats {
    pub fn from_durations(samples: &[Duration]) -> DurationStats {
        let mut millis: Vec<f64> = samples.iter().map(duration_ms).collect();
        millis.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        DurationStats {
            n: millis.len(),
            p50_ms: nearest_rank(&millis, 50),
            p95_ms: nearest_rank(&millis, 95),
            p99_ms: nearest_rank(&millis, 99),
            max_ms: millis.last().copied(),
        }
    }
}

pub fn duration_ms(d: &Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn nearest_rank(sorted: &[f64], p: usize) -> Option<f64> {
    let n = sorted.len();
    if n == 0 {
        return None;
    }
    let rank = (p * n).div_ceil(100).clamp(1, n);
    Some(sorted[rank - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(values: &[u64]) -> Vec<Duration> {
        values.iter().map(|&v| Duration::from_millis(v)).collect()
    }

    #[test]
    fn empty_samples_are_none() {
        let stats = DurationStats::from_durations(&[]);
        assert_eq!(stats.n, 0);
        assert_eq!(stats.p50_ms, None);
        assert_eq!(stats.p95_ms, None);
        assert_eq!(stats.p99_ms, None);
        assert_eq!(stats.max_ms, None);
    }

    #[test]
    fn single_sample_is_every_percentile() {
        let stats = DurationStats::from_durations(&ms(&[42]));
        assert_eq!(stats.n, 1);
        assert_eq!(stats.p50_ms, Some(42.0));
        assert_eq!(stats.p95_ms, Some(42.0));
        assert_eq!(stats.p99_ms, Some(42.0));
        assert_eq!(stats.max_ms, Some(42.0));
    }

    #[test]
    fn hundred_samples_nearest_rank() {
        let values: Vec<u64> = (1..=100).collect();
        let stats = DurationStats::from_durations(&ms(&values));
        assert_eq!(stats.n, 100);
        assert_eq!(stats.p50_ms, Some(50.0));
        assert_eq!(stats.p95_ms, Some(95.0));
        assert_eq!(stats.p99_ms, Some(99.0));
        assert_eq!(stats.max_ms, Some(100.0));
    }

    #[test]
    fn unsorted_input_is_sorted_first() {
        let stats = DurationStats::from_durations(&ms(&[30, 10, 20]));
        assert_eq!(stats.p50_ms, Some(20.0));
        assert_eq!(stats.max_ms, Some(30.0));
    }
}
