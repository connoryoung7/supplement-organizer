//! Pure domain logic: no I/O, no clock. Vocabulary follows `CONTEXT.md`.

pub mod projection;
pub mod rate;
pub mod replay;
pub mod scorecard;

use chrono::{Datelike, Days, NaiveDate};

pub type BottleId = i64;

/// "X units every Y days".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    pub units: u32,
    pub every_days: u32,
}

impl Schedule {
    pub fn planned_rate(self) -> f64 {
        f64::from(self.units) / f64::from(self.every_days)
    }

    /// Due when never dosed, or when at least `every_days` have passed since the last dose.
    pub fn is_due(self, last_dose: Option<NaiveDate>, today: NaiveDate) -> bool {
        self.next_due(last_dose, today) <= today
    }

    pub fn next_due(self, last_dose: Option<NaiveDate>, today: NaiveDate) -> NaiveDate {
        match last_dose {
            None => today,
            Some(d) => d + Days::new(u64::from(self.every_days)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottleStatus {
    New,
    Started,
    Finished,
}

impl BottleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Started => "started",
            Self::Finished => "finished",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "new" => Some(Self::New),
            "started" => Some(Self::Started),
            "finished" => Some(Self::Finished),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductStatus {
    Active,
    Paused,
    Archived,
}

impl ProductStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Archived => "archived",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "active" => Some(Self::Active),
            "paused" => Some(Self::Paused),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

/// Parses an expiry date as printed on a label. Accepts `2027-03-15`, `03/15/2027`,
/// and month-only forms `2027-03`, `03/2027`, `03/27`. A month-only date means the
/// last day of that month.
pub fn parse_expiry(input: &str) -> Option<NaiveDate> {
    let parts: Vec<&str> = input.trim().split(['-', '/', '.']).collect();
    let num = |s: &str| s.trim().parse::<u32>().ok();
    let year = |s: &str| {
        let y = num(s)?;
        match s.trim().len() {
            2 => Some(2000 + y as i32),
            4 => Some(y as i32),
            _ => None,
        }
    };
    match parts.as_slice() {
        [y, m, d] if y.trim().len() == 4 => NaiveDate::from_ymd_opt(year(y)?, num(m)?, num(d)?),
        [m, d, y] => NaiveDate::from_ymd_opt(year(y)?, num(m)?, num(d)?),
        [y, m] if y.trim().len() == 4 => last_day_of_month(year(y)?, num(m)?),
        [m, y] => last_day_of_month(year(y)?, num(m)?),
        _ => None,
    }
}

pub fn last_day_of_month(year: i32, month: u32) -> Option<NaiveDate> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    let next = first.checked_add_months(chrono::Months::new(1))?;
    next.pred_opt().filter(|d| d.month() == month)
}

#[cfg(test)]
pub(crate) fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_only_expiry_is_last_day_of_month() {
        assert_eq!(parse_expiry("2027-02"), Some(d("2027-02-28")));
        assert_eq!(parse_expiry("03/2027"), Some(d("2027-03-31")));
        assert_eq!(parse_expiry(" 04/28 "), Some(d("2028-04-30")));
        assert_eq!(parse_expiry("2028-02"), Some(d("2028-02-29")));
    }

    #[test]
    fn full_expiry_dates() {
        assert_eq!(parse_expiry("2027-03-15"), Some(d("2027-03-15")));
        assert_eq!(parse_expiry("03/15/2027"), Some(d("2027-03-15")));
    }

    #[test]
    fn rejects_nonsense_expiry() {
        assert_eq!(parse_expiry(""), None);
        assert_eq!(parse_expiry("13/2027"), None);
        assert_eq!(parse_expiry("2027"), None);
        assert_eq!(parse_expiry("soon"), None);
    }

    #[test]
    fn due_after_interval() {
        let every_other = Schedule {
            units: 1,
            every_days: 2,
        };
        let today = d("2026-01-10");
        assert!(every_other.is_due(None, today));
        assert!(!every_other.is_due(Some(d("2026-01-09")), today));
        assert!(every_other.is_due(Some(d("2026-01-08")), today));

        let daily = Schedule {
            units: 2,
            every_days: 1,
        };
        assert!(!daily.is_due(Some(today), today));
        assert!(daily.is_due(Some(d("2026-01-09")), today));
        assert_eq!(daily.planned_rate(), 2.0);
    }
}
