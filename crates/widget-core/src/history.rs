//! Throughput history for the graph's longer windows, from the gateway's own reports.
//!
//! The gateway keeps 5-minute buckets for 24 hours and hourly buckets for 7 days
//! (`stat/sysinfo` retention, confirmed on a UCG Ultra: 288 and 168 buckets). Each
//! bucket holds the bytes moved in it, so the rate is bytes over the bucket's length:
//! an average, which smooths out short peaks the live graph would show.

use crate::unifi::DayBucket;

/// Which gateway report backs a history window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    /// `stat/report/5minutes.site`, kept for 24 hours.
    FiveMinutes,
    /// `stat/report/hourly.site`, kept for 7 days.
    Hourly,
}

impl Scale {
    /// The report name in the URL.
    pub fn report(self) -> &'static str {
        match self {
            Scale::FiveMinutes => "5minutes",
            Scale::Hourly => "hourly",
        }
    }

    /// Length of one bucket in seconds.
    pub fn bucket_s(self) -> i64 {
        match self {
            Scale::FiveMinutes => 300,
            Scale::Hourly => 3600,
        }
    }

    /// How far back the gateway keeps this report, in seconds.
    pub fn span_s(self) -> i64 {
        match self {
            Scale::FiveMinutes => 24 * 3600,
            Scale::Hourly => 7 * 24 * 3600,
        }
    }

    /// "24h" picks 5-minute buckets, "7d" hourly ones; anything else is None.
    pub fn for_window(window: &str) -> Option<Scale> {
        match window {
            "24h" => Some(Scale::FiveMinutes),
            "7d" => Some(Scale::Hourly),
            _ => None,
        }
    }
}

/// Average throughput over one bucket, in bytes per second, stamped at the bucket's
/// midpoint so the line sits under the time it describes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RatePoint {
    pub t_ms: i64,
    pub rx: u64,
    pub tx: u64,
}

/// Turns report buckets into average rates. The newest bucket is still filling, so it
/// is divided by the time it has covered so far (at least a minute, to keep a bucket
/// that has only just started from swinging wildly), not its full length.
pub fn bucket_rates(buckets: &[DayBucket], scale: Scale, now_ms: i64) -> Vec<RatePoint> {
    let full_ms = scale.bucket_s() * 1000;
    buckets
        .iter()
        .filter(|b| b.start_ms <= now_ms)
        .map(|b| {
            let covered_ms = (now_ms - b.start_ms).clamp(60_000, full_ms);
            let secs = covered_ms as f64 / 1000.0;
            RatePoint {
                t_ms: b.start_ms + covered_ms / 2,
                rx: (b.rx_bytes as f64 / secs).round() as u64,
                tx: (b.tx_bytes as f64 / secs).round() as u64,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bucket(start_ms: i64, rx_bytes: u64, tx_bytes: u64) -> DayBucket {
        DayBucket {
            start_ms,
            rx_bytes,
            tx_bytes,
        }
    }

    #[test]
    fn full_buckets_are_bytes_over_their_length() {
        let pts = bucket_rates(&[bucket(0, 300_000, 30_000)], Scale::FiveMinutes, 3_600_000);
        assert_eq!(
            pts,
            vec![RatePoint {
                t_ms: 150_000,
                rx: 1000,
                tx: 100
            }]
        );
    }

    #[test]
    fn the_filling_bucket_uses_the_time_covered_so_far() {
        // Started 3 minutes ago with 180 kB: 1000 B/s, not 600 B/s.
        let now = 10_000_000;
        let pts = bucket_rates(
            &[bucket(now - 180_000, 180_000, 0)],
            Scale::FiveMinutes,
            now,
        );
        assert_eq!(pts[0].rx, 1000);
        assert_eq!(pts[0].t_ms, now - 90_000);
    }

    #[test]
    fn a_just_started_bucket_counts_at_least_a_minute() {
        let now = 10_000_000;
        let pts = bucket_rates(&[bucket(now - 5_000, 60_000, 0)], Scale::Hourly, now);
        assert_eq!(pts[0].rx, 1000);
    }

    #[test]
    fn future_buckets_are_dropped() {
        let pts = bucket_rates(&[bucket(5_000, 1, 1)], Scale::Hourly, 1_000);
        assert!(pts.is_empty());
    }

    #[test]
    fn windows_map_to_reports() {
        assert_eq!(Scale::for_window("24h"), Some(Scale::FiveMinutes));
        assert_eq!(Scale::for_window("7d"), Some(Scale::Hourly));
        assert_eq!(Scale::for_window("5m"), None);
        assert_eq!(Scale::Hourly.report(), "hourly");
        assert_eq!(Scale::FiveMinutes.span_s(), 86_400);
    }
}
