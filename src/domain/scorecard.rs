//! Counts expired bottles — the number the app exists to keep low.

use chrono::{Months, NaiveDate};

/// A bottle is expired once its expiry date has passed while it still held units —
/// including bottles that were finished, but only after their expiry date.
pub fn is_expired(expires_on: NaiveDate, finished_on: Option<NaiveDate>, today: NaiveDate) -> bool {
    expires_on < today && finished_on.is_none_or(|f| f > expires_on)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Scorecard {
    pub expired_all_time: u32,
    pub expired_last_12_months: u32,
    pub finished_on_time: u32,
}

/// `bottles` yields `(expires_on, finished_on)` for every bottle.
pub fn scorecard(
    bottles: impl IntoIterator<Item = (NaiveDate, Option<NaiveDate>)>,
    today: NaiveDate,
) -> Scorecard {
    let year_ago = today - Months::new(12);
    let mut card = Scorecard::default();
    for (expires_on, finished_on) in bottles {
        if is_expired(expires_on, finished_on, today) {
            card.expired_all_time += 1;
            if expires_on >= year_ago {
                card.expired_last_12_months += 1;
            }
        } else if finished_on.is_some() {
            card.finished_on_time += 1;
        }
    }
    card
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::d;

    #[test]
    fn expiry_counts_only_bottles_that_still_had_units() {
        let today = d("2026-06-01");
        assert!(!is_expired(d("2026-05-31"), Some(d("2026-05-31")), today)); // finished on the day
        assert!(is_expired(d("2026-05-31"), Some(d("2026-06-01")), today)); // finished a day late
        assert!(is_expired(d("2026-05-31"), None, today)); // still in the cupboard
        assert!(!is_expired(d("2026-06-01"), None, today)); // expires today: not yet
    }

    #[test]
    fn scorecard_tallies() {
        let today = d("2026-06-01");
        let card = scorecard(
            [
                (d("2026-05-31"), None),                  // expired, recent
                (d("2024-01-31"), Some(d("2024-03-01"))), // expired, long ago
                (d("2026-04-30"), Some(d("2026-04-01"))), // finished on time
                (d("2027-01-31"), None),                  // still good
            ],
            today,
        );
        assert_eq!(
            card,
            Scorecard {
                expired_all_time: 2,
                expired_last_12_months: 1,
                finished_on_time: 1
            }
        );
    }
}
