//! The assessment in plain English for pilots: the words the app and the
//! EFB share (values, where on the approach, what holds a grade down).
//! Criterion ids and rule ids are never part of them.

use crate::method::{Height, Method};
use crate::score::Grade;

/// Criteria whose value is an offset from a target (speed, crossing
/// height, touchdown point): shown signed.
const SIGNED: [&str; 4] = ["A1", "B1", "B2", "C1"];

/// A criterion's value `x` in its unit as the app and the EFB show it:
/// signed for an offset, two decimals in g, one in dots or under 10 when
/// not whole, levels as "L2".
pub fn value_text(id: &str, unit: Option<&str>, x: f64) -> String {
    let Some(unit) = unit else {
        return if x != 0.0 && !x.is_nan() {
            format!("L{}", number(x, 0))
        } else {
            "none".into()
        };
    };
    let digits = match unit {
        "g" => 2,
        "dot" => 1,
        _ if x.abs() < 10.0 && x.fract() != 0.0 => 1,
        _ => 0,
    };
    let text = number(x, digits);
    let zero = !text.bytes().any(|b| (b'1'..=b'9').contains(&b));
    let sign = if SIGNED.contains(&id) && x > 0.0 && !zero {
        "+"
    } else {
        ""
    };
    let unit = match unit {
        "deg" => "\u{b0}".to_owned(),
        u => format!(" {u}"),
    };
    format!("{sign}{text}{unit}")
}

/// `x` with `digits` decimals, halves away from zero, thousands grouped
/// with commas (en-US); a value that rounds to zero is unsigned.
pub fn number(x: f64, digits: usize) -> String {
    let scale = 10f64.powi(digits as i32);
    let r = (x * scale).round() / scale;
    let text = format!("{:.*}", digits, r.abs());
    let (int, frac) = text.split_at(text.find('.').unwrap_or(text.len()));
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let sign = if r < 0.0 { "-" } else { "" };
    format!("{sign}{grouped}{frac}")
}

/// A criterion's measured value as read in an item: `value_text`, except the
/// touchdown point, which says its side ("46 ft before the target").
pub fn reading_text(id: &str, unit: Option<&str>, x: f64) -> String {
    let text = value_text(id, unit, x);
    if id != "C1" || text.starts_with('0') {
        return text;
    }
    let side = if x < 0.0 { "before" } else { "past" };
    format!("{} {side} the target", text.trim_start_matches(['+', '-']))
}

/// Where in the approach window a criterion was graded: "above 300 ft",
/// "below 300 ft"; `None` for one graded over the whole window.
pub fn height_text(height: Height, split_ft: f64) -> Option<String> {
    match height {
        Height::All => None,
        Height::Above => Some(format!("above {} ft", number(split_ft, 0))),
        Height::Below => Some(format!("below {} ft", number(split_ft, 0))),
    }
}

/// A graded item in words: "Descent rate 1,565 fpm above 300 ft".
pub fn item_text(title: &str, value: &str, height: Option<&str>) -> String {
    [Some(title), Some(value), height]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `item` with its level: "Descent rate 1,565 fpm above 300 ft (level 2)".
pub fn with_level(item: &str, level: u8) -> String {
    format!("{item} (level {level})")
}

/// "a", "a and b", "a, b and c".
pub fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// Why a landing has no score: the required measurements in `missing`
/// (`runway`, `peak_g`, `sink`, `rollout position`) were not recorded.
pub fn not_scored_text(missing: &[&str]) -> String {
    let words: Vec<String> = missing
        .iter()
        .map(|m| {
            match *m {
                "runway" => "runway data",
                "peak_g" => "the touchdown g-load",
                "sink" => "the touchdown sink rate",
                "rollout position" => "the rollout position on the runway",
                other => other,
            }
            .to_owned()
        })
        .collect();
    format!(
        "Not scored: {} not recorded, and a score needs {}. Missing data never earns credit, so this landing counts through its findings only (event points).",
        list(&words),
        if missing.len() == 1 { "it" } else { "them" }
    )
}

impl Method {
    /// What a cap at `score` allows, in words: "below Good (max 84.9)", or
    /// "at zero".
    pub fn ceiling(&self, score: f64) -> String {
        if score <= 0.0 {
            return "at zero".into();
        }
        let max = number(score, 1);
        match self.grade(score) {
            Grade::Fair => format!("below Good (max {max})"),
            Grade::Poor => format!("below Fair (max {max})"),
            Grade::Good => format!("at {max} at most"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_landing_without_a_score_says_what_was_missing() {
        assert_eq!(
            not_scored_text(&["sink"]),
            "Not scored: the touchdown sink rate not recorded, and a score needs it. Missing data never earns credit, so this landing counts through its findings only (event points)."
        );
        let two = not_scored_text(&["runway", "peak_g"]);
        assert!(
            two.starts_with("Not scored: runway data and the touchdown g-load not recorded, and a score needs them."),
            "{two}"
        );
    }

    /// As the app showed them: offsets signed, dots to 0.1, g to 0.01,
    /// thousands grouped, small fractions to 0.1, levels as "L2".
    #[test]
    fn values_read_as_the_app_shows_them() {
        let cases = [
            ("A1", Some("kt"), 12.4, "+12 kt"),
            ("A1", Some("kt"), -3.25, "-3.3 kt"),
            ("B1", Some("ft"), 0.0, "0 ft"),
            ("C1", Some("ft"), 1234.5, "+1,235 ft"),
            ("A2", Some("fpm"), -1562.0, "-1,562 fpm"),
            ("A3", Some("dot"), 1.0, "1.0 dot"),
            ("A3", Some("dot"), 0.84, "0.8 dot"),
            ("E1", Some("g"), 1.6, "1.60 g"),
            ("D1", Some("deg"), 4.5, "4.5\u{b0}"),
            ("A2", Some("fpm"), 12.5, "13 fpm"),
            ("A6", None, 2.0, "L2"),
            ("A6", None, 0.0, "none"),
        ];
        for (id, unit, x, want) in cases {
            assert_eq!(value_text(id, unit, x), want, "{id} {x}");
        }
        assert_eq!(
            reading_text("C1", Some("ft"), -46.4),
            "46 ft before the target"
        );
        assert_eq!(
            reading_text("C1", Some("ft"), 558.0),
            "558 ft past the target"
        );
        assert_eq!(reading_text("C1", Some("ft"), 0.02), "0.0 ft");
        assert_eq!(reading_text("A1", Some("kt"), 12.4), "+12 kt");
        assert_eq!(number(1_234_567.891, 2), "1,234,567.89");
        assert_eq!(number(-0.4, 0), "0");
        assert_eq!(number(-0.04, 1), "0.0");
        assert_eq!(number(-0.0, 1), "0.0");
        assert_eq!(number(-0.05, 1), "-0.1");
        assert_eq!(value_text("A1", Some("ft"), -0.02), "0.0 ft");
        assert_eq!(value_text("A1", Some("ft"), 0.02), "0.0 ft");
    }

    #[test]
    fn items_read_as_plain_sentences() {
        let item = item_text(
            "Descent rate",
            "1,565 fpm",
            height_text(Height::Above, 300.0).as_deref(),
        );
        assert_eq!(
            with_level(&item, 2),
            "Descent rate 1,565 fpm above 300 ft (level 2)"
        );
        assert_eq!(item_text("Bounce", "", None), "Bounce");
        assert_eq!(list(&["a".into(), "b".into(), "c".into()]), "a, b and c");
    }
}
