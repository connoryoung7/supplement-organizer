//! Planned, actual and effective consumption rates (units per day).

use chrono::{Days, NaiveDate};

use super::{ProductStatus, Schedule};

/// Length of the look-back window for the actual rate, ending yesterday.
pub const WINDOW_DAYS: u64 = 30;
/// Active days of history needed before the actual rate replaces the planned one.
pub const MIN_HISTORY_DAYS: u32 = 14;

/// A paused (or archived) stretch: `from` is paused, `until` is the first active day again.
#[derive(Debug, Clone, Copy)]
pub struct Pause {
    pub from: NaiveDate,
    pub until: Option<NaiveDate>,
}

impl Pause {
    fn covers(&self, day: NaiveDate) -> bool {
        self.from <= day && self.until.is_none_or(|u| day < u)
    }
}

/// Units per day over the last 30 days ending yesterday, counting only non-paused days
/// on or after the first dose ever (`first_dose`). `doses` must include every dose in the
/// window; older ones are ignored. `None` until there are `MIN_HISTORY_DAYS` such days.
pub fn actual_rate(
    first_dose: Option<NaiveDate>,
    doses: &[(NaiveDate, u32)],
    pauses: &[Pause],
    today: NaiveDate,
) -> Option<f64> {
    let first = first_dose?;
    let end = today.pred_opt()?;
    let start = (today - Days::new(WINDOW_DAYS)).max(first);
    let active =
        |day: NaiveDate| day >= start && day <= end && !pauses.iter().any(|p| p.covers(day));

    let active_days = start
        .iter_days()
        .take_while(|&day| day <= end)
        .filter(|&day| active(day))
        .count() as u32;
    if active_days < MIN_HISTORY_DAYS {
        return None;
    }
    let units: u32 = doses
        .iter()
        .filter(|&&(day, _)| active(day))
        .map(|&(_, u)| u)
        .sum();
    Some(f64::from(units) / f64::from(active_days))
}

/// The rate projections use: zero unless active; otherwise actual if known, else planned.
pub fn effective_rate(status: ProductStatus, schedule: Schedule, actual: Option<f64>) -> f64 {
    match status {
        ProductStatus::Active => actual.unwrap_or_else(|| schedule.planned_rate()),
        ProductStatus::Paused | ProductStatus::Archived => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::d;

    fn daily(from: &str, days: u64, units: u32) -> Vec<(NaiveDate, u32)> {
        d(from)
            .iter_days()
            .take(days as usize)
            .map(|day| (day, units))
            .collect()
    }

    fn rate(doses: &[(NaiveDate, u32)], pauses: &[Pause], today: NaiveDate) -> Option<f64> {
        actual_rate(
            doses.iter().map(|&(day, _)| day).min(),
            doses,
            pauses,
            today,
        )
    }

    #[test]
    fn no_actual_rate_without_two_weeks_of_history() {
        let today = d("2026-03-01");
        assert_eq!(rate(&[], &[], today), None);
        let doses = daily("2026-02-16", 13, 1); // Feb 16..28 = 13 days
        assert_eq!(rate(&doses, &[], today), None);
        let doses = daily("2026-02-15", 14, 1);
        assert_eq!(rate(&doses, &[], today), Some(1.0));
    }

    #[test]
    fn averages_over_the_window_including_skipped_days() {
        let today = d("2026-03-01");
        // 30 days of history, a dose of 2 every other day.
        let doses: Vec<_> = daily("2026-01-30", 30, 2).into_iter().step_by(2).collect();
        assert_eq!(rate(&doses, &[], today), Some(1.0));
    }

    #[test]
    fn ignores_doses_older_than_the_window_and_today() {
        let today = d("2026-03-01");
        let mut doses = daily("2026-01-01", 60, 1);
        doses.push((today, 50));
        assert_eq!(rate(&doses, &[], today), Some(1.0));
    }

    #[test]
    fn a_long_gap_counts_against_the_rate() {
        // First dose long ago, then nothing until 5 days ago: the window is all 30 days.
        let today = d("2026-03-01");
        let doses = daily("2026-02-24", 5, 2);
        assert_eq!(
            actual_rate(Some(d("2025-06-01")), &doses, &[], today),
            Some(10.0 / 30.0)
        );
    }

    #[test]
    fn paused_days_do_not_drag_the_rate_down() {
        let today = d("2026-03-01");
        // Daily doses except a 10-day pause in the middle of the window.
        let pause = Pause {
            from: d("2026-02-10"),
            until: Some(d("2026-02-20")),
        };
        let doses: Vec<_> = daily("2026-01-30", 30, 2)
            .into_iter()
            .filter(|&(day, _)| !pause.covers(day))
            .collect();
        assert_eq!(rate(&doses, &[pause], today), Some(2.0));
    }

    #[test]
    fn effective_rate_is_zero_unless_active() {
        let s = Schedule {
            units: 1,
            every_days: 2,
        };
        assert_eq!(effective_rate(ProductStatus::Active, s, None), 0.5);
        assert_eq!(effective_rate(ProductStatus::Active, s, Some(0.25)), 0.25);
        assert_eq!(effective_rate(ProductStatus::Paused, s, Some(0.25)), 0.0);
        assert_eq!(effective_rate(ProductStatus::Archived, s, None), 0.0);
    }
}
