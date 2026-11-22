//! Projects each bottle in a product's queue forward at the product's rate, to find the
//! bottles that will still hold units when they expire.

use chrono::{Days, NaiveDate};

use super::{BottleId, BottleStatus};

/// Tolerance for float rounding when converting fractional days to dates.
const EPS: f64 = 1e-9;

#[derive(Debug, Clone, Copy)]
pub struct QueueEntry {
    pub id: BottleId,
    pub status: BottleStatus,
    pub remaining: u32,
    pub expires_on: NaiveDate,
    pub added_on: NaiveDate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Projection {
    pub id: BottleId,
    /// `None` when the rate is zero: the bottle will never be reached.
    pub start: Option<NaiveDate>,
    pub finish: Option<NaiveDate>,
    pub left_at_expiry: u32,
}

impl Projection {
    pub fn at_risk(&self) -> bool {
        self.left_at_expiry > 0
    }
}

/// The started bottle first, then new bottles by expiry date (ties: added first).
/// Finished bottles are dropped.
pub fn queue(mut bottles: Vec<QueueEntry>) -> Vec<QueueEntry> {
    bottles.retain(|b| b.status != BottleStatus::Finished);
    bottles.sort_by_key(|b| {
        (
            b.status != BottleStatus::Started,
            b.expires_on,
            b.added_on,
            b.id,
        )
    });
    bottles
}

/// Walks the queue from the start of `today`, consuming `rate` units per day.
pub fn project(queue: &[QueueEntry], rate: f64, today: NaiveDate) -> Vec<Projection> {
    let day = |offset: f64| today + Days::new(offset.max(0.0) as u64);
    let mut offset = 0.0_f64; // days from the start of today
    queue
        .iter()
        .map(|b| {
            if rate <= 0.0 {
                return Projection {
                    id: b.id,
                    start: None,
                    finish: None,
                    left_at_expiry: b.remaining,
                };
            }
            let remaining = f64::from(b.remaining);
            let lasts = remaining / rate;
            // Days from the start of today to the end of the expiry date.
            let usable = ((b.expires_on - today).num_days() + 1) as f64;
            let used_by_expiry = (rate * (usable - offset)).clamp(0.0, remaining);
            let left = remaining - used_by_expiry;
            let projection = Projection {
                id: b.id,
                start: Some(day((offset + EPS).floor())),
                finish: Some(day((offset + lasts - EPS).ceil() - 1.0)),
                left_at_expiry: if left < EPS {
                    0
                } else {
                    (left - EPS).ceil() as u32
                },
            };
            offset += lasts;
            projection
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::d;

    fn entry(id: BottleId, status: BottleStatus, remaining: u32, expires: &str) -> QueueEntry {
        QueueEntry {
            id,
            status,
            remaining,
            expires_on: d(expires),
            added_on: d("2026-01-01"),
        }
    }

    #[test]
    fn queue_is_started_then_new_by_expiry() {
        let q = queue(vec![
            entry(1, BottleStatus::New, 10, "2027-06-30"),
            entry(2, BottleStatus::Finished, 0, "2026-01-31"),
            entry(3, BottleStatus::Started, 4, "2027-12-31"),
            entry(4, BottleStatus::New, 10, "2027-03-31"),
        ]);
        assert_eq!(q.iter().map(|b| b.id).collect::<Vec<_>>(), [3, 4, 1]);
    }

    #[test]
    fn finishes_on_the_expiry_date_is_not_at_risk() {
        let today = d("2026-01-01");
        let p = project(
            &[entry(1, BottleStatus::Started, 10, "2026-01-10")],
            1.0,
            today,
        );
        assert_eq!(p[0].start, Some(today));
        assert_eq!(p[0].finish, Some(d("2026-01-10")));
        assert!(!p[0].at_risk());
    }

    #[test]
    fn one_day_short_leaves_a_dose_at_expiry() {
        let p = project(
            &[entry(1, BottleStatus::Started, 10, "2026-01-09")],
            1.0,
            d("2026-01-01"),
        );
        assert_eq!(p[0].left_at_expiry, 1);
    }

    #[test]
    fn later_bottles_start_when_earlier_ones_finish() {
        let today = d("2026-01-01");
        let q = [
            entry(1, BottleStatus::Started, 60, "2026-12-31"),
            entry(2, BottleStatus::New, 90, "2026-04-30"),
        ];
        // At 2/day bottle 1 lasts 30 days; bottle 2 starts Jan 31 and needs 45 days, but
        // expires Apr 30 (day 120) — fine.
        let p = project(&q, 2.0, today);
        assert_eq!(p[1].start, Some(d("2026-01-31")));
        assert_eq!(p[1].finish, Some(d("2026-03-16")));
        assert!(!p[1].at_risk());

        // At 0.5/day bottle 1 lasts 120 days, so bottle 2 starts after it has expired.
        let p = project(&q, 0.5, today);
        assert_eq!(p[1].start, Some(d("2026-05-01")));
        assert_eq!(p[1].left_at_expiry, 90);
    }

    #[test]
    fn partial_leftover_rounds_up_to_whole_units() {
        // 0.4/day for 10 days through expiry uses 4 of 10.
        let p = project(
            &[entry(1, BottleStatus::Started, 10, "2026-01-10")],
            0.4,
            d("2026-01-01"),
        );
        assert_eq!(p[0].left_at_expiry, 6);
        let p = project(
            &[entry(1, BottleStatus::Started, 10, "2026-01-10")],
            0.35,
            d("2026-01-01"),
        );
        assert_eq!(p[0].left_at_expiry, 7); // 3.5 used, 6.5 left
    }

    #[test]
    fn already_expired_bottles_are_fully_at_risk() {
        let p = project(
            &[entry(1, BottleStatus::Started, 10, "2025-12-31")],
            1.0,
            d("2026-01-01"),
        );
        assert_eq!(p[0].left_at_expiry, 10);
    }

    #[test]
    fn zero_rate_never_reaches_any_bottle() {
        let q = [
            entry(1, BottleStatus::Started, 10, "2030-01-01"),
            entry(2, BottleStatus::New, 10, "2030-01-01"),
        ];
        let p = project(&q, 0.0, d("2026-01-01"));
        assert!(p
            .iter()
            .all(|p| p.start.is_none() && p.left_at_expiry == 10));
    }
}
