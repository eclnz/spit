//! The shapes a source path rule can narrow a placeholder to, as in
//! `{date:date}`.

use std::fmt;

/// A shape a value must have to match a placeholder. A closed set, not
/// patterns: each is a function over the placeholder's text, which holds
/// only ASCII letters, digits and `-`, so the shapes read an encoded value
/// the same as a decoded one.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum Shape {
    /// One or more ASCII digits, as `07` or `120`.
    Digits,
    /// Four digits, from 1900 to 2099.
    Year,
    /// `YYYY-MM-DD`, a real day in a year from 1900 to 2099.
    Date,
}

impl Shape {
    /// Every shape, in the order messages list them.
    pub(crate) const ALL: [Shape; 3] = [Shape::Digits, Shape::Year, Shape::Date];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Digits => "digits",
            Self::Year => "year",
            Self::Date => "date",
        }
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|shape| shape.name() == name)
    }

    /// How long every value of this shape is, if all are the same length;
    /// then a placeholder's end needs no searching.
    pub(crate) fn fixed_length(self) -> Option<usize> {
        match self {
            Self::Digits => None,
            Self::Year => Some(4),
            Self::Date => Some(10),
        }
    }

    /// Whether `value` has this shape.
    ///
    /// Keep in step with `is_value_character` in `inputs/pattern.rs`: no
    /// shape may accept a character a value cannot hold.
    pub(crate) fn matches(self, value: &str) -> bool {
        match self {
            Self::Digits => !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()),
            Self::Year => is_year(value),
            Self::Date => is_date(value),
        }
    }
}

impl fmt::Display for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The names of every shape, as `` `digits`, `year` and `date` ``.
pub(crate) fn shape_names() -> String {
    let names: Vec<_> = Shape::ALL
        .iter()
        .map(|shape| format!("`{}`", shape.name()))
        .collect();
    match names.split_last() {
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// Whether `value` is a year from 1900 to 2099, as `2024`.
pub(crate) fn is_year(value: &str) -> bool {
    value.len() == 4
        && (value.starts_with("19") || value.starts_with("20"))
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Whether `value` is a date written `2024-01-15`: a year from 1900 to
/// 2099, and a month and day that exist.
pub(crate) fn is_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' || !is_year(&value[..4]) {
        return false;
    }
    let number = |text: &str| {
        text.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| text.parse::<u32>().ok())
            .flatten()
    };
    let (Some(year), Some(month), Some(day)) = (
        number(&value[..4]),
        number(&value[5..7]),
        number(&value[8..]),
    ) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

#[cfg(test)]
mod tests {
    use super::{shape_names, Shape};

    #[test]
    fn a_date_must_be_a_real_day() {
        for good in ["2026-09-01", "2024-02-29", "1900-12-31", "2099-01-01"] {
            assert!(Shape::Date.matches(good), "{good}");
        }
        for bad in [
            "2026-9-1",
            "20260901",
            "2026-13-01",
            "2026-00-10",
            "2026-02-30",
            "2023-02-29",
            "1899-01-01",
            "2100-01-01",
            "2026-09-01-final",
            "2026_09_01",
            "+026-09-01",
            "",
        ] {
            assert!(!Shape::Date.matches(bad), "{bad}");
        }
    }

    #[test]
    fn years_and_digits() {
        assert!(Shape::Year.matches("1999") && Shape::Year.matches("2024"));
        assert!(!Shape::Year.matches("2124") && !Shape::Year.matches("202"));
        assert!(Shape::Digits.matches("007") && !Shape::Digits.matches(""));
        assert!(!Shape::Digits.matches("1-2") && !Shape::Digits.matches("1a"));
        assert_eq!(shape_names(), "`digits`, `year` and `date`");
    }
}
