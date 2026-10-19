//! Billing-cycle maths for the monthly usage figure.
//!
//! The controller's daily buckets start at local midnight in the controller's
//! timezone and the newest bucket already holds today's partial total, so the
//! monthly figure is a plain sum of buckets since the cycle start.

use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;

use crate::unifi::DayBucket;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Totals {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

impl Totals {
    pub fn total(&self) -> u64 {
        self.rx_bytes.saturating_add(self.tx_bytes)
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_next = NaiveDate::from_ymd_opt(ny, nm, 1).expect("valid date");
    first_next.pred_opt().expect("valid date").day()
}

fn local_midnight(tz: Tz, year: i32, month: u32, billing_day: u32) -> DateTime<Utc> {
    let day = billing_day.clamp(1, days_in_month(year, month));
    tz.with_ymd_and_hms(year, month, day, 0, 0, 0)
        .earliest()
        // Midnight skipped by a DST jump: fall back to the first valid hour.
        .unwrap_or_else(|| tz.with_ymd_and_hms(year, month, day, 1, 0, 0).unwrap())
        .with_timezone(&Utc)
}

/// Start of the billing cycle containing `now`: local midnight on `billing_day`
/// (clamped to the month's length, so day 31 means "last day" in short months).
pub fn cycle_start(now: DateTime<Utc>, billing_day: u32, tz: Tz) -> DateTime<Utc> {
    let (y, m) = {
        let l = now.with_timezone(&tz);
        (l.year(), l.month())
    };
    let this = local_midnight(tz, y, m, billing_day);
    if now >= this {
        return this;
    }
    let (py, pm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
    local_midnight(tz, py, pm, billing_day)
}

pub fn monthly_totals(buckets: &[DayBucket], start: DateTime<Utc>) -> Totals {
    buckets
        .iter()
        .filter(|b| b.start_ms >= start.timestamp_millis())
        .fold(Totals::default(), |acc, b| Totals {
            rx_bytes: acc.rx_bytes.saturating_add(b.rx_bytes),
            tx_bytes: acc.tx_bytes.saturating_add(b.tx_bytes),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unifi::parse_daily_report;
    use chrono_tz::Australia::Sydney;

    fn utc(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
    }

    #[test]
    fn cycle_starts_on_first_in_current_month() {
        // 2026-10-18 22:29 UTC is 09:29 on the 19th in Sydney (AEDT).
        let s = cycle_start(utc(2026, 10, 18, 22), 1, Sydney);
        // Sydney midnight 1 Oct 2026 is AEST (+10): 14:00 UTC on 30 Sep.
        assert_eq!(s, utc(2026, 9, 30, 14));
    }

    #[test]
    fn cycle_before_billing_day_rolls_to_previous_month() {
        let s = cycle_start(utc(2026, 10, 18, 22), 20, Sydney);
        // 20 Sep 2026 00:00 AEST.
        assert_eq!(s, utc(2026, 9, 19, 14));
    }

    #[test]
    fn cycle_on_billing_day_boundary_is_inclusive() {
        let start = utc(2026, 10, 19, 13); // 20 Oct 00:00 AEDT
        assert_eq!(cycle_start(start, 20, Sydney), start);
        let before = start - chrono::Duration::seconds(1);
        assert_eq!(cycle_start(before, 20, Sydney), utc(2026, 9, 19, 14));
    }

    #[test]
    fn cycle_rolls_back_across_year_boundary() {
        let s = cycle_start(utc(2027, 1, 10, 0), 25, Sydney);
        // 25 Dec 2026 00:00 AEDT (+11).
        assert_eq!(s, utc(2026, 12, 24, 13));
    }

    #[test]
    fn billing_day_31_clamps_to_month_end() {
        // Mid-Feb 2027: cycle began on the last day of January (day 31).
        let s = cycle_start(utc(2027, 2, 15, 0), 31, Sydney);
        assert_eq!(s, utc(2027, 1, 30, 13));
        // In Feb the clamped day is the 28th.
        let s = cycle_start(utc(2027, 3, 1, 0), 31, Sydney);
        assert_eq!(s, utc(2027, 2, 27, 13));
    }

    fn b(start_ms: i64, rx: u64, tx: u64) -> DayBucket {
        DayBucket {
            start_ms,
            rx_bytes: rx,
            tx_bytes: tx,
        }
    }

    #[test]
    fn totals_include_bucket_at_cycle_start_and_exclude_earlier() {
        let start = utc(2026, 9, 30, 14);
        let t0 = start.timestamp_millis();
        let buckets = [
            b(t0 - 86_400_000, 100, 10),
            b(t0, 5, 1),
            b(t0 + 86_400_000, 7, 2),
        ];
        let t = monthly_totals(&buckets, start);
        assert_eq!((t.rx_bytes, t.tx_bytes, t.total()), (12, 3, 15));
    }

    #[test]
    fn totals_of_nothing_is_zero() {
        assert_eq!(monthly_totals(&[], utc(2026, 10, 1, 0)), Totals::default());
    }

    #[test]
    fn totals_saturate_instead_of_overflowing() {
        let start = utc(2026, 10, 1, 0);
        let t0 = start.timestamp_millis();
        let t = monthly_totals(&[b(t0, u64::MAX, 0), b(t0 + 1, 1, 0)], start);
        assert_eq!(t.rx_bytes, u64::MAX);
    }

    #[test]
    fn real_report_sums_only_the_current_cycle() {
        let report = include_str!("../tests/fixtures/daily_report.json");
        let buckets = parse_daily_report(report).unwrap();
        let start = cycle_start(utc(2026, 10, 18, 22), 1, Sydney);
        let in_cycle = monthly_totals(&buckets, start);
        let all = monthly_totals(&buckets, utc(2000, 1, 1, 0));
        assert_eq!(in_cycle, all); // fixture only covers October
        assert!(in_cycle.rx_bytes > in_cycle.tx_bytes);
    }
}
