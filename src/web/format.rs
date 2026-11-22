//! Human-readable strings for the templates.

use chrono::{Datelike, NaiveDate};

use crate::app::KINDS;
use crate::domain::Schedule;

pub fn date(d: NaiveDate) -> String {
    d.format("%-d %b %Y").to_string()
}

/// "today", "tomorrow", "Wed 25 Nov", or "25 Nov 2027" outside the current year.
pub fn relative_date(d: NaiveDate, today: NaiveDate) -> String {
    match (d - today).num_days() {
        0 => "today".into(),
        1 => "tomorrow".into(),
        -1 => "yesterday".into(),
        2..=6 => d.format("%A").to_string(),
        _ if d.year() == today.year() => d.format("%a %-d %b").to_string(),
        _ => date(d),
    }
}

pub fn plural(unit: &str, n: f64) -> String {
    let one = (n - 1.0).abs() < f64::EPSILON;
    match unit {
        "ml" | "g" => unit.to_string(),
        "gummy" if !one => "gummies".into(),
        _ if one => unit.to_string(),
        _ => format!("{unit}s"),
    }
}

pub fn units(n: u32, unit: &str) -> String {
    format!("{n} {}", plural(unit, f64::from(n)))
}

/// Up to two decimals, trailing zeros trimmed.
pub fn number(x: f64) -> String {
    let s = format!("{x:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn rate(per_day: f64, unit: &str) -> String {
    format!("{} {}/day", number(per_day), plural(unit, per_day))
}

pub fn schedule(s: Schedule, unit: &str) -> String {
    let every = match s.every_days {
        1 => "every day".to_string(),
        2 => "every other day".to_string(),
        7 => "every week".to_string(),
        n => format!("every {n} days"),
    };
    format!("{} {every}", units(s.units, unit))
}

pub fn kind(kind: &str) -> &'static str {
    KINDS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, label)| *label)
        .unwrap_or("Other")
}

pub fn product_subtitle(brand: &str, strength: &str) -> String {
    if strength.is_empty() {
        brand.to_string()
    } else {
        format!("{brand} · {strength}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plurals() {
        assert_eq!(units(1, "capsule"), "1 capsule");
        assert_eq!(units(2, "gummy"), "2 gummies");
        assert_eq!(units(5, "ml"), "5 ml");
        assert_eq!(rate(1.0 / 3.0, "softgel"), "0.33 softgels/day");
        assert_eq!(rate(1.0, "tablet"), "1 tablet/day");
    }

    #[test]
    fn schedules() {
        assert_eq!(
            schedule(
                Schedule {
                    units: 2,
                    every_days: 1
                },
                "capsule"
            ),
            "2 capsules every day"
        );
        assert_eq!(
            schedule(
                Schedule {
                    units: 1,
                    every_days: 2
                },
                "scoop"
            ),
            "1 scoop every other day"
        );
        assert_eq!(
            schedule(
                Schedule {
                    units: 3,
                    every_days: 10
                },
                "drop"
            ),
            "3 drops every 10 days"
        );
    }
}
