//! Derives every bottle's state by replaying a product's history in date order.
//! See docs/adr/0002-bottle-state-by-replay.md.

use chrono::NaiveDate;

use super::{BottleId, BottleStatus};

#[derive(Debug, Clone)]
pub struct BottleSpec {
    pub id: BottleId,
    pub initial_count: u32,
    pub expires_on: NaiveDate,
    pub added_on: NaiveDate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Dose {
        units: u32,
    },
    /// Override: start this bottle instead of the one the queue would open.
    Start {
        bottle: BottleId,
    },
    /// Count correction: this bottle actually holds `count` units.
    SetCount {
        bottle: BottleId,
        count: u32,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct Event {
    pub on: NaiveDate,
    /// Tie-breaker for events on the same day: the order they were recorded in.
    pub seq: i64,
    pub kind: EventKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BottleState {
    pub id: BottleId,
    pub status: BottleStatus,
    pub remaining: u32,
    pub started_on: Option<NaiveDate>,
    pub finished_on: Option<NaiveDate>,
    /// Units drawn by doses since the bottle was first started.
    pub drawn: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplayError {
    #[error(
        "On {date} your bottles were {short} short for this dose. Add a bottle, or correct a \
         bottle's count if it held more than recorded."
    )]
    NotEnoughUnits { date: NaiveDate, short: u32 },
    #[error("That bottle hadn't been added yet on {0}.")]
    NotYetAdded(NaiveDate),
    #[error("Only a new bottle can be started; that one is already {}.", .0.as_str())]
    NotNew(BottleStatus),
    #[error(
        "The bottle that's already started has had doses taken from it by then, so it can't go \
         back to new. If you actually opened this one earlier, date the switch to the day you \
         opened it; if the other bottle is empty, mark it finished first."
    )]
    StartedInUse,
    #[error(
        "The bottle opened after this one has had doses taken from it, so this one can't be \
         re-opened."
    )]
    ReopenBlocked,
    #[error("A new bottle's count can't be corrected — edit the bottle instead.")]
    CorrectNew,
    #[error("Unknown bottle {0}.")]
    UnknownBottle(BottleId),
}

pub fn replay(bottles: &[BottleSpec], events: &[Event]) -> Result<Vec<BottleState>, ReplayError> {
    let mut r = Replayer {
        specs: bottles,
        states: bottles
            .iter()
            .map(|b| BottleState {
                id: b.id,
                status: BottleStatus::New,
                remaining: b.initial_count,
                started_on: None,
                finished_on: None,
                drawn: 0,
            })
            .collect(),
        started: None,
    };

    let mut ordered: Vec<&Event> = events.iter().collect();
    ordered.sort_by_key(|e| (e.on, e.seq));
    for e in ordered {
        match e.kind {
            EventKind::Dose { units } => r.dose(units, e.on)?,
            EventKind::Start { bottle } => r.start(bottle, e.on)?,
            EventKind::SetCount { bottle, count } => r.set_count(bottle, count, e.on)?,
        }
    }
    Ok(r.states)
}

struct Replayer<'a> {
    specs: &'a [BottleSpec],
    states: Vec<BottleState>,
    started: Option<usize>,
}

impl Replayer<'_> {
    fn index(&self, id: BottleId) -> Result<usize, ReplayError> {
        self.specs
            .iter()
            .position(|b| b.id == id)
            .ok_or(ReplayError::UnknownBottle(id))
    }

    fn dose(&mut self, units: u32, on: NaiveDate) -> Result<(), ReplayError> {
        if self.started.is_none() && !self.specs.iter().any(|b| b.added_on <= on) {
            // Taken before any bottle of this product was tracked: it came from a bottle
            // the app never knew about (an already-open bottle's count already reflects it).
            // It still counts toward the actual rate, but draws from nothing.
            return Ok(());
        }
        let mut left = units;
        while left > 0 {
            let i = match self.started.or_else(|| self.auto_start(on)) {
                Some(i) => i,
                None => {
                    return Err(ReplayError::NotEnoughUnits {
                        date: on,
                        short: left,
                    })
                }
            };
            let s = &mut self.states[i];
            let take = left.min(s.remaining);
            s.remaining -= take;
            s.drawn += take;
            left -= take;
            if s.remaining == 0 {
                self.finish(i, on);
            }
        }
        Ok(())
    }

    fn start(&mut self, id: BottleId, on: NaiveDate) -> Result<(), ReplayError> {
        let i = self.index(id)?;
        if self.specs[i].added_on > on {
            return Err(ReplayError::NotYetAdded(on));
        }
        if self.states[i].status != BottleStatus::New {
            return Err(ReplayError::NotNew(self.states[i].status));
        }
        if let Some(c) = self.started {
            if self.states[c].drawn > 0 {
                return Err(ReplayError::StartedInUse);
            }
            self.unstart(c);
        }
        self.open(i, on);
        Ok(())
    }

    fn set_count(&mut self, id: BottleId, count: u32, on: NaiveDate) -> Result<(), ReplayError> {
        let i = self.index(id)?;
        match self.states[i].status {
            BottleStatus::New => Err(ReplayError::CorrectNew),
            BottleStatus::Started => {
                self.states[i].remaining = count;
                if count == 0 {
                    self.finish(i, on);
                }
                Ok(())
            }
            BottleStatus::Finished if count == 0 => Ok(()),
            BottleStatus::Finished => {
                if let Some(c) = self.started {
                    if self.states[c].drawn > 0 {
                        return Err(ReplayError::ReopenBlocked);
                    }
                    self.unstart(c);
                }
                let s = &mut self.states[i];
                s.status = BottleStatus::Started;
                s.finished_on = None;
                s.remaining = count;
                self.started = Some(i);
                Ok(())
            }
        }
    }

    /// Starts the earliest-expiring new bottle that had been added by `on`.
    fn auto_start(&mut self, on: NaiveDate) -> Option<usize> {
        let next = (0..self.specs.len())
            .filter(|&i| self.states[i].status == BottleStatus::New && self.specs[i].added_on <= on)
            .min_by_key(|&i| {
                let b = &self.specs[i];
                (b.expires_on, b.added_on, b.id)
            })?;
        self.open(next, on);
        Some(next)
    }

    fn open(&mut self, i: usize, on: NaiveDate) {
        let s = &mut self.states[i];
        s.status = BottleStatus::Started;
        s.started_on = Some(on);
        self.started = Some(i);
    }

    fn unstart(&mut self, i: usize) {
        let s = &mut self.states[i];
        s.status = BottleStatus::New;
        s.started_on = None;
        self.started = None;
    }

    fn finish(&mut self, i: usize, on: NaiveDate) {
        let s = &mut self.states[i];
        s.status = BottleStatus::Finished;
        s.finished_on = Some(on);
        self.started = None;
        self.auto_start(on);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::d;

    fn bottle(id: BottleId, count: u32, expires: &str, added: &str) -> BottleSpec {
        BottleSpec {
            id,
            initial_count: count,
            expires_on: d(expires),
            added_on: d(added),
        }
    }

    fn ev(on: &str, seq: i64, kind: EventKind) -> Event {
        Event {
            on: d(on),
            seq,
            kind,
        }
    }

    fn dose(on: &str, seq: i64, units: u32) -> Event {
        ev(on, seq, EventKind::Dose { units })
    }

    fn state(states: &[BottleState], id: BottleId) -> &BottleState {
        states.iter().find(|s| s.id == id).unwrap()
    }

    #[test]
    fn first_dose_opens_earliest_expiring_bottle() {
        let bottles = [
            bottle(1, 10, "2027-06-30", "2026-01-01"),
            bottle(2, 10, "2027-03-31", "2026-01-01"),
        ];
        let s = replay(&bottles, &[dose("2026-01-02", 1, 2)]).unwrap();
        assert_eq!(state(&s, 1).status, BottleStatus::New);
        let b2 = state(&s, 2);
        assert_eq!(b2.status, BottleStatus::Started);
        assert_eq!(b2.remaining, 8);
        assert_eq!(b2.started_on, Some(d("2026-01-02")));
    }

    #[test]
    fn dose_spills_into_next_bottle_which_starts_automatically() {
        let bottles = [
            bottle(1, 3, "2027-01-31", "2026-01-01"),
            bottle(2, 10, "2027-02-28", "2026-01-01"),
        ];
        let s = replay(
            &bottles,
            &[dose("2026-01-02", 1, 2), dose("2026-01-03", 2, 2)],
        )
        .unwrap();
        let b1 = state(&s, 1);
        assert_eq!(
            (b1.status, b1.remaining, b1.finished_on),
            (BottleStatus::Finished, 0, Some(d("2026-01-03")))
        );
        let b2 = state(&s, 2);
        assert_eq!(
            (b2.status, b2.remaining, b2.started_on),
            (BottleStatus::Started, 9, Some(d("2026-01-03")))
        );
    }

    #[test]
    fn emptying_a_bottle_exactly_opens_the_next() {
        let bottles = [
            bottle(1, 2, "2027-01-31", "2026-01-01"),
            bottle(2, 10, "2027-02-28", "2026-01-01"),
        ];
        let s = replay(&bottles, &[dose("2026-01-02", 1, 2)]).unwrap();
        assert_eq!(state(&s, 1).status, BottleStatus::Finished);
        assert_eq!(state(&s, 2).status, BottleStatus::Started);
        assert_eq!(state(&s, 2).remaining, 10);
    }

    #[test]
    fn running_out_is_an_error() {
        let bottles = [bottle(1, 3, "2027-01-31", "2026-01-01")];
        let err = replay(&bottles, &[dose("2026-01-02", 1, 5)]).unwrap_err();
        assert_eq!(
            err,
            ReplayError::NotEnoughUnits {
                date: d("2026-01-02"),
                short: 2
            }
        );
    }

    #[test]
    fn doses_from_before_any_bottle_was_added_draw_from_nothing() {
        // Onboarding: an already-open bottle added on Jan 10 with what's left in it, plus a
        // week of backfilled doses. Its count already reflects those doses.
        let bottles = [bottle(1, 45, "2027-01-31", "2026-01-10")];
        let mut events: Vec<Event> = (3..10)
            .map(|day| dose(&format!("2026-01-{day:02}"), day, 1))
            .collect();
        events.push(ev("2026-01-10", 20, EventKind::Start { bottle: 1 }));
        events.push(dose("2026-01-10", 21, 1));
        let s = replay(&bottles, &events).unwrap();
        assert_eq!(state(&s, 1).remaining, 44);

        // Once a bottle has been tracked, running out is still an error.
        let err = replay(&bottles, &[dose("2026-01-10", 1, 46)]).unwrap_err();
        assert!(matches!(err, ReplayError::NotEnoughUnits { short: 1, .. }));
    }

    #[test]
    fn a_bottle_added_later_does_not_rewrite_which_bottle_was_opened() {
        // Bottle 3 expires earliest but was bought after bottle 1 ran out and 2 was opened.
        let bottles = [
            bottle(1, 1, "2027-01-31", "2026-01-01"),
            bottle(2, 10, "2027-12-31", "2026-01-01"),
            bottle(3, 10, "2027-06-30", "2026-02-01"),
        ];
        let s = replay(
            &bottles,
            &[dose("2026-01-05", 1, 1), dose("2026-02-05", 2, 1)],
        )
        .unwrap();
        assert_eq!(state(&s, 2).status, BottleStatus::Started);
        assert_eq!(state(&s, 2).remaining, 9);
        assert_eq!(state(&s, 3).status, BottleStatus::New);
    }

    #[test]
    fn events_replay_in_date_order_not_recorded_order() {
        // A dose backdated to before a count correction is already reflected in the count.
        let bottles = [bottle(1, 30, "2027-01-31", "2026-01-01")];
        let events = [
            dose("2026-01-02", 1, 1),
            ev(
                "2026-01-10",
                2,
                EventKind::SetCount {
                    bottle: 1,
                    count: 20,
                },
            ),
            dose("2026-01-09", 3, 1), // logged after the correction, but taken before it
        ];
        let s = replay(&bottles, &events).unwrap();
        assert_eq!(state(&s, 1).remaining, 20);
    }

    #[test]
    fn override_swaps_an_untouched_started_bottle_back_to_new() {
        let bottles = [
            bottle(1, 10, "2027-01-31", "2026-01-01"),
            bottle(2, 10, "2027-06-30", "2026-01-01"),
        ];
        let events = [
            ev("2026-01-01", 1, EventKind::Start { bottle: 1 }),
            ev("2026-01-01", 2, EventKind::Start { bottle: 2 }),
            dose("2026-01-02", 3, 1),
        ];
        let s = replay(&bottles, &events).unwrap();
        assert_eq!(state(&s, 1).status, BottleStatus::New);
        assert_eq!(state(&s, 1).started_on, None);
        assert_eq!(state(&s, 2).remaining, 9);
    }

    #[test]
    fn override_is_refused_once_the_started_bottle_has_been_used() {
        let bottles = [
            bottle(1, 10, "2027-01-31", "2026-01-01"),
            bottle(2, 10, "2027-06-30", "2026-01-01"),
        ];
        let events = [
            dose("2026-01-02", 1, 1),
            ev("2026-01-03", 2, EventKind::Start { bottle: 2 }),
        ];
        assert_eq!(
            replay(&bottles, &events).unwrap_err(),
            ReplayError::StartedInUse
        );
    }

    #[test]
    fn override_needs_a_new_bottle_that_existed_then() {
        let bottles = [bottle(1, 10, "2027-01-31", "2026-01-05")];
        let early = [ev("2026-01-01", 1, EventKind::Start { bottle: 1 })];
        assert_eq!(
            replay(&bottles, &early).unwrap_err(),
            ReplayError::NotYetAdded(d("2026-01-01"))
        );
        let twice = [
            ev("2026-01-05", 1, EventKind::Start { bottle: 1 }),
            ev("2026-01-05", 2, EventKind::Start { bottle: 1 }),
        ];
        assert_eq!(
            replay(&bottles, &twice).unwrap_err(),
            ReplayError::NotNew(BottleStatus::Started)
        );
    }

    #[test]
    fn correcting_to_zero_finishes_early_and_opens_the_next() {
        let bottles = [
            bottle(1, 90, "2027-01-31", "2026-01-01"),
            bottle(2, 90, "2027-06-30", "2026-01-01"),
        ];
        let events = [
            dose("2026-01-02", 1, 1),
            ev(
                "2026-03-01",
                2,
                EventKind::SetCount {
                    bottle: 1,
                    count: 0,
                },
            ),
        ];
        let s = replay(&bottles, &events).unwrap();
        assert_eq!(state(&s, 1).finished_on, Some(d("2026-03-01")));
        assert_eq!(state(&s, 2).started_on, Some(d("2026-03-01")));
    }

    #[test]
    fn correcting_a_finished_bottle_upward_reopens_it() {
        let bottles = [
            bottle(1, 2, "2027-01-31", "2026-01-01"),
            bottle(2, 90, "2027-06-30", "2026-01-01"),
        ];
        let events = [
            dose("2026-01-02", 1, 2),
            ev(
                "2026-01-02",
                2,
                EventKind::SetCount {
                    bottle: 1,
                    count: 5,
                },
            ),
        ];
        let s = replay(&bottles, &events).unwrap();
        let b1 = state(&s, 1);
        assert_eq!(
            (b1.status, b1.remaining, b1.finished_on),
            (BottleStatus::Started, 5, None)
        );
        assert_eq!(state(&s, 2).status, BottleStatus::New);
    }

    #[test]
    fn reopening_is_refused_once_the_next_bottle_has_been_used() {
        let bottles = [
            bottle(1, 2, "2027-01-31", "2026-01-01"),
            bottle(2, 90, "2027-06-30", "2026-01-01"),
        ];
        let events = [
            dose("2026-01-02", 1, 3),
            ev(
                "2026-01-03",
                2,
                EventKind::SetCount {
                    bottle: 1,
                    count: 5,
                },
            ),
        ];
        assert_eq!(
            replay(&bottles, &events).unwrap_err(),
            ReplayError::ReopenBlocked
        );
    }

    #[test]
    fn new_bottles_cannot_be_corrected() {
        let bottles = [bottle(1, 2, "2027-01-31", "2026-01-01")];
        let events = [ev(
            "2026-01-02",
            1,
            EventKind::SetCount {
                bottle: 1,
                count: 5,
            },
        )];
        assert_eq!(
            replay(&bottles, &events).unwrap_err(),
            ReplayError::CorrectNew
        );
    }

    #[test]
    fn expiry_ties_open_the_bottle_added_first() {
        let bottles = [
            bottle(7, 10, "2027-01-31", "2026-01-03"),
            bottle(8, 10, "2027-01-31", "2026-01-01"),
        ];
        let s = replay(&bottles, &[dose("2026-01-04", 1, 1)]).unwrap();
        assert_eq!(state(&s, 8).status, BottleStatus::Started);
    }
}
