//! Detects the type and formatting of leaf values (numbers, dates, booleans…)
//! and describes how two sets of values differ in format. Content is never
//! compared, only the way it is written.

use regex_lite::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tz {
    None,
    Utc,
    Offset,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ValueFormat {
    Empty,
    Bool,
    Integer { grouped: Option<char>, digits: u8 },
    Decimal { grouped: Option<char>, sep: char, frac: u8, int_digits: u8 },
    Scientific,
    /// NaN, INF, Infinity
    Special,
    /// null, undefined, None
    NullLiteral,
    Date { style: &'static str, tz: Tz },
    DateTime { style: &'static str, frac: u8, tz: Tz },
    Time,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Family {
    Bool,
    Number,
    Date,
    DateTime,
    Time,
    Text,
}

impl Family {
    fn label(self) -> &'static str {
        match self {
            Family::Bool => "boolean",
            Family::Number => "number",
            Family::Date => "date",
            Family::DateTime => "date-time",
            Family::Time => "time",
            Family::Text => "text",
        }
    }
}

impl ValueFormat {
    pub fn family(&self) -> Option<Family> {
        Some(match self {
            ValueFormat::Bool => Family::Bool,
            ValueFormat::Integer { .. } | ValueFormat::Decimal { .. } | ValueFormat::Scientific => {
                Family::Number
            }
            ValueFormat::Date { .. } => Family::Date,
            ValueFormat::DateTime { .. } => Family::DateTime,
            ValueFormat::Time => Family::Time,
            ValueFormat::Text => Family::Text,
            ValueFormat::Empty | ValueFormat::Special | ValueFormat::NullLiteral => return None,
        })
    }
}

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}

re!(RE_INT, r"^[+-]?(\d+)$");
re!(RE_DEC_DOT, r"^[+-]?(\d+)\.(\d+)$");
re!(RE_DEC_COMMA, r"^[+-]?(\d+),(\d+)$");
re!(RE_SCI, r"^[+-]?\d+(\.\d+)?[eE][+-]?\d+$");
re!(RE_GROUP_EN, r"^[+-]?(\d{1,3}(?:,\d{3})+)(?:\.(\d+))?$");
re!(RE_GROUP_DE, r"^[+-]?(\d{1,3}(?:\.\d{3})+),(\d+)$");
re!(RE_GROUP_DE_INT, r"^[+-]?(\d{1,3}(?:\.\d{3}){2,})$");
re!(RE_GROUP_SPACE, r"^[+-]?(\d{1,3}(?:[ '\u{a0}]\d{3})+)(?:[.,](\d+))?$");
re!(RE_DATE_ISO, r"^\d{4}-\d{2}-\d{2}(Z|[+-]\d{2}:?\d{2})?$");
re!(RE_DATE_DE, r"^\d{1,2}\.\d{1,2}\.\d{4}$");
re!(RE_DATE_DE_SHORT, r"^\d{1,2}\.\d{1,2}\.\d{2}$");
re!(RE_DATE_SLASH, r"^\d{1,2}/\d{1,2}/\d{4}$");
re!(RE_DATE_SLASH_ISO, r"^\d{4}/\d{2}/\d{2}$");
re!(
    RE_DT_ISO,
    r"^\d{4}-\d{2}-\d{2}([T ])\d{2}:\d{2}(:\d{2})?(?:[.,](\d+))?(Z|[+-]\d{2}(?::?\d{2})?)?$"
);
re!(RE_DT_DE, r"^\d{1,2}\.\d{1,2}\.\d{4} \d{1,2}:\d{2}(:\d{2})?$");
re!(RE_TIME, r"^\d{2}:\d{2}(:\d{2})?(\.\d+)?$");
re!(RE_NULL_TOKEN, r"(?:^|[^A-Za-z0-9_])(null|undefined|NaN)(?:$|[^A-Za-z0-9_])");

fn tz_of(s: Option<&str>) -> Tz {
    match s {
        None | Some("") => Tz::None,
        Some("Z") => Tz::Utc,
        Some(_) => Tz::Offset,
    }
}

fn digits(s: &str) -> u8 {
    s.chars().filter(|c| c.is_ascii_digit()).count().min(255) as u8
}

pub fn classify(raw: &str) -> ValueFormat {
    let v = raw.trim();
    if v.is_empty() {
        return ValueFormat::Empty;
    }
    if v.len() > 64 {
        return ValueFormat::Text;
    }
    match v {
        "true" | "false" | "TRUE" | "FALSE" | "True" | "False" => return ValueFormat::Bool,
        "NaN" | "INF" | "-INF" | "+INF" | "Infinity" | "-Infinity" => return ValueFormat::Special,
        "null" | "NULL" | "Null" | "undefined" | "None" | "nil" => return ValueFormat::NullLiteral,
        _ => {}
    }
    if let Some(c) = RE_INT.captures(v) {
        return ValueFormat::Integer { grouped: None, digits: digits(&c[1]) };
    }
    if let Some(c) = RE_DEC_DOT.captures(v) {
        return ValueFormat::Decimal {
            grouped: None,
            sep: '.',
            frac: c[2].len().min(255) as u8,
            int_digits: digits(&c[1]),
        };
    }
    if let Some(c) = RE_GROUP_EN.captures(v) {
        return match c.get(2) {
            Some(f) => ValueFormat::Decimal {
                grouped: Some(','),
                sep: '.',
                frac: f.as_str().len().min(255) as u8,
                int_digits: digits(&c[1]),
            },
            None => ValueFormat::Integer { grouped: Some(','), digits: digits(&c[1]) },
        };
    }
    if let Some(c) = RE_GROUP_DE.captures(v) {
        return ValueFormat::Decimal {
            grouped: Some('.'),
            sep: ',',
            frac: c[2].len().min(255) as u8,
            int_digits: digits(&c[1]),
        };
    }
    if let Some(c) = RE_GROUP_DE_INT.captures(v) {
        return ValueFormat::Integer { grouped: Some('.'), digits: digits(&c[1]) };
    }
    if let Some(c) = RE_DEC_COMMA.captures(v) {
        return ValueFormat::Decimal {
            grouped: None,
            sep: ',',
            frac: c[2].len().min(255) as u8,
            int_digits: digits(&c[1]),
        };
    }
    if let Some(c) = RE_GROUP_SPACE.captures(v) {
        return match c.get(2) {
            Some(f) => ValueFormat::Decimal {
                grouped: Some(' '),
                sep: if v.contains(',') { ',' } else { '.' },
                frac: f.as_str().len().min(255) as u8,
                int_digits: digits(&c[1]),
            },
            None => ValueFormat::Integer { grouped: Some(' '), digits: digits(&c[1]) },
        };
    }
    if RE_SCI.is_match(v) {
        return ValueFormat::Scientific;
    }
    if let Some(c) = RE_DATE_ISO.captures(v) {
        return ValueFormat::Date { style: "yyyy-MM-dd", tz: tz_of(c.get(1).map(|m| m.as_str())) };
    }
    if RE_DATE_DE.is_match(v) {
        return ValueFormat::Date { style: "dd.MM.yyyy", tz: Tz::None };
    }
    if RE_DATE_DE_SHORT.is_match(v) {
        return ValueFormat::Date { style: "dd.MM.yy", tz: Tz::None };
    }
    if RE_DATE_SLASH.is_match(v) {
        return ValueFormat::Date { style: "dd/MM/yyyy", tz: Tz::None };
    }
    if RE_DATE_SLASH_ISO.is_match(v) {
        return ValueFormat::Date { style: "yyyy/MM/dd", tz: Tz::None };
    }
    if let Some(c) = RE_DT_ISO.captures(v) {
        let style = match (&c[1], c.get(2).is_some()) {
            ("T", true) => "yyyy-MM-dd'T'HH:mm:ss",
            ("T", false) => "yyyy-MM-dd'T'HH:mm",
            (_, true) => "yyyy-MM-dd HH:mm:ss",
            (_, false) => "yyyy-MM-dd HH:mm",
        };
        return ValueFormat::DateTime {
            style,
            frac: c.get(3).map(|m| m.as_str().len().min(255) as u8).unwrap_or(0),
            tz: tz_of(c.get(4).map(|m| m.as_str())),
        };
    }
    if let Some(c) = RE_DT_DE.captures(v) {
        return ValueFormat::DateTime {
            style: if c.get(1).is_some() { "dd.MM.yyyy HH:mm:ss" } else { "dd.MM.yyyy HH:mm" },
            frac: 0,
            tz: Tz::None,
        };
    }
    if RE_TIME.is_match(v) {
        return ValueFormat::Time;
    }
    ValueFormat::Text
}

#[derive(Clone, Debug)]
pub struct Sample {
    pub count: usize,
    pub text: String,
    pub line: u32,
}

/// Formats seen for one leaf element across all its occurrences.
#[derive(Clone, Debug, Default)]
pub struct ValueProfile {
    pub formats: BTreeMap<ValueFormat, Sample>,
    /// A text value that contains "null", "undefined" or "NaN" as a word.
    pub null_token: Option<Sample>,
}

impl ValueProfile {
    pub fn add(&mut self, raw: &str, line: u32) {
        let f = classify(raw);
        let trimmed = raw.trim();
        if f == ValueFormat::Text && self.null_token.is_none() && RE_NULL_TOKEN.is_match(trimmed) {
            self.null_token = Some(Sample { count: 1, text: shorten(trimmed, 60), line });
        }
        self.formats
            .entry(f)
            .and_modify(|s| s.count += 1)
            .or_insert_with(|| Sample { count: 1, text: shorten(trimmed, 40), line });
    }

    fn families(&self) -> Vec<Family> {
        let mut v: Vec<Family> = self.formats.keys().filter_map(|f| f.family()).collect();
        v.sort();
        v.dedup();
        v
    }

    fn sample_of(&self, pred: impl Fn(&ValueFormat) -> bool) -> Option<&Sample> {
        self.formats.iter().filter(|(f, _)| pred(f)).max_by_key(|(_, s)| s.count).map(|(_, s)| s)
    }

    fn any_sample(&self) -> Option<&Sample> {
        self.sample_of(|f| f.family().is_some())
    }

    /// Short description of what the values look like, e.g. "decimal, 2 fraction digits".
    pub fn describe(&self) -> String {
        let fams = self.families();
        if fams.is_empty() {
            return "empty".into();
        }
        fams.iter().map(|f| f.label()).collect::<Vec<_>>().join(" / ")
    }
}

pub fn shorten(s: &str, max: usize) -> String {
    let one_line: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max {
        one_line
    } else {
        let cut: String = one_line.chars().take(max).collect();
        format!("{cut}…")
    }
}

/// One formatting difference between the reference and the candidate values
/// of the same element.
#[derive(Clone, Debug)]
pub struct FormatNote {
    /// Stable category, used for grouping.
    pub category: &'static str,
    /// Heading for the group of all elements with this difference.
    pub heading: &'static str,
    /// What differs, for this element.
    pub message: String,
    pub line_ref: Option<u32>,
    pub line_cand: Option<u32>,
}

#[derive(Default)]
struct NumStats {
    has_int: bool,
    has_dec: bool,
    grouped: Option<char>,
    ungrouped_large: bool,
    seps: Vec<char>,
    frac_min: u8,
    frac_max: u8,
    dec_formats: usize,
    scientific: bool,
}

fn num_stats(p: &ValueProfile) -> NumStats {
    let mut s = NumStats { frac_min: u8::MAX, ..Default::default() };
    for f in p.formats.keys() {
        match f {
            ValueFormat::Integer { grouped, digits } => {
                s.has_int = true;
                if grouped.is_some() {
                    s.grouped = *grouped;
                } else if *digits >= 4 {
                    s.ungrouped_large = true;
                }
            }
            ValueFormat::Decimal { grouped, sep, frac, int_digits } => {
                s.has_dec = true;
                s.dec_formats += 1;
                if grouped.is_some() {
                    s.grouped = *grouped;
                } else if *int_digits >= 4 {
                    s.ungrouped_large = true;
                }
                if !s.seps.contains(sep) {
                    s.seps.push(*sep);
                }
                s.frac_min = s.frac_min.min(*frac);
                s.frac_max = s.frac_max.max(*frac);
            }
            ValueFormat::Scientific => s.scientific = true,
            _ => {}
        }
    }
    s
}

fn q(s: Option<&Sample>) -> String {
    s.map(|s| format!("“{}”", s.text)).unwrap_or_else(|| "–".into())
}

fn tz_label(t: Tz) -> &'static str {
    match t {
        Tz::None => "no time zone",
        Tz::Utc => "UTC marker “Z”",
        Tz::Offset => "a time-zone offset",
    }
}

fn group_label(c: char) -> &'static str {
    match c {
        ',' => "comma",
        '.' => "dot",
        _ => "space",
    }
}

/// Compares how values are written. `a` is the reference, `b` the candidate.
pub fn diff_profiles(a: &ValueProfile, b: &ValueProfile) -> Vec<FormatNote> {
    let mut out = Vec::new();
    let mut note = |category, heading, message: String, sa: Option<&Sample>, sb: Option<&Sample>| {
        out.push(FormatNote {
            category,
            heading,
            message,
            line_ref: sa.map(|s| s.line),
            line_cand: sb.map(|s| s.line),
        });
    };

    // Literals that usually mean "no value was set".
    let special = |p: &ValueProfile| p.sample_of(|f| *f == ValueFormat::Special).cloned();
    let null = |p: &ValueProfile| p.sample_of(|f| *f == ValueFormat::NullLiteral).cloned();
    if let (None, Some(sb)) = (special(a), special(b)) {
        note(
            "special",
            "Unset-looking values (null, NaN)",
            format!("value is {} ({}×)", q(Some(&sb)), sb.count),
            a.any_sample(),
            Some(&sb),
        );
    }
    if let (None, Some(sb)) = (null(a), null(b)) {
        note(
            "special",
            "Unset-looking values (null, NaN)",
            format!("value is the literal {} ({}×)", q(Some(&sb)), sb.count),
            a.any_sample(),
            Some(&sb),
        );
    }
    if let (None, Some(sb)) = (&a.null_token, &b.null_token) {
        note(
            "special",
            "Unset-looking values (null, NaN)",
            format!("text contains an unset-looking word: {}", q(Some(sb))),
            a.any_sample(),
            Some(sb),
        );
    }

    let fa = a.families();
    let fb = b.families();
    if fa.is_empty() || fb.is_empty() {
        return out;
    }

    if fa == [Family::Number] && fb == [Family::Number] {
        let (na, nb) = (num_stats(a), num_stats(b));
        let is_num = |f: &ValueFormat| f.family() == Some(Family::Number);
        let grouped = |f: &ValueFormat| {
            matches!(
                f,
                ValueFormat::Integer { grouped: Some(_), .. }
                    | ValueFormat::Decimal { grouped: Some(_), .. }
            )
        };
        let large_plain = |f: &ValueFormat| {
            matches!(f, ValueFormat::Integer { grouped: None, digits } if *digits >= 4)
                || matches!(f, ValueFormat::Decimal { grouped: None, int_digits, .. } if *int_digits >= 4)
        };
        match (na.grouped, nb.grouped) {
            (Some(g), None) if nb.ungrouped_large => note(
                "num-grouping",
                "Thousands separator",
                format!(
                    "reference groups thousands with a {} ({}); candidate does not ({})",
                    group_label(g),
                    q(a.sample_of(grouped)),
                    q(b.sample_of(large_plain))
                ),
                a.sample_of(grouped),
                b.sample_of(large_plain),
            ),
            (None, Some(g)) => note(
                "num-grouping",
                "Thousands separator",
                format!(
                    "candidate groups thousands with a {} ({}); reference does not ({})",
                    group_label(g),
                    q(b.sample_of(grouped)),
                    q(a.sample_of(is_num))
                ),
                a.sample_of(is_num),
                b.sample_of(grouped),
            ),
            (Some(x), Some(y)) if x != y => note(
                "num-grouping",
                "Thousands separator",
                format!(
                    "different thousands separator: reference {}, candidate {}",
                    q(a.sample_of(grouped)),
                    q(b.sample_of(grouped))
                ),
                a.sample_of(grouped),
                b.sample_of(grouped),
            ),
            _ => {}
        }
        let dec = |f: &ValueFormat| matches!(f, ValueFormat::Decimal { .. });
        let int = |f: &ValueFormat| matches!(f, ValueFormat::Integer { .. });
        if na.has_dec && nb.has_dec && na.seps != nb.seps {
            note(
                "num-separator",
                "Decimal separator",
                format!(
                    "decimal separator differs: reference {}, candidate {}",
                    q(a.sample_of(dec)),
                    q(b.sample_of(dec))
                ),
                a.sample_of(dec),
                b.sample_of(dec),
            );
        }
        if nb.scientific && !na.scientific {
            let sci = |f: &ValueFormat| *f == ValueFormat::Scientific;
            note(
                "num-scientific",
                "Scientific notation",
                format!(
                    "candidate uses scientific notation ({}); reference does not ({})",
                    q(b.sample_of(sci)),
                    q(a.sample_of(is_num))
                ),
                a.sample_of(is_num),
                b.sample_of(sci),
            );
        }
        if !na.has_dec && nb.has_dec && !nb.has_int {
            note(
                "num-int-dec",
                "Whole number in reference, decimal in candidate",
                format!(
                    "reference writes whole numbers ({}); candidate always writes decimals ({})",
                    q(a.sample_of(int)),
                    q(b.sample_of(dec))
                ),
                a.sample_of(int),
                b.sample_of(dec),
            );
        } else if na.has_dec && nb.has_dec {
            let fixed_a = na.frac_min == na.frac_max && !na.has_int;
            let max_sample = |p: &ValueProfile, n: u8| {
                p.sample_of(|f| matches!(f, ValueFormat::Decimal { frac, .. } if *frac == n)).cloned()
            };
            let a_count: usize = a.formats.values().map(|s| s.count).sum();
            // Trailing zeros are often dropped, so one extra digit against a
            // handful of reference values says little.
            if nb.frac_max > na.frac_max && (nb.frac_max - na.frac_max >= 2 || a_count >= 3) {
                let (sa, sb) = (max_sample(a, na.frac_max), max_sample(b, nb.frac_max));
                note(
                    "num-precision",
                    "Number precision",
                    format!(
                        "candidate has more fraction digits: up to {} ({}) against up to {} ({})",
                        nb.frac_max,
                        q(sb.as_ref()),
                        na.frac_max,
                        q(sa.as_ref())
                    ),
                    sa.as_ref(),
                    sb.as_ref(),
                );
            } else if fixed_a && na.dec_formats == 1 && (nb.frac_min != na.frac_min || nb.has_int) {
                let other = |f: &ValueFormat| match f {
                    ValueFormat::Decimal { frac, .. } => *frac != na.frac_max,
                    ValueFormat::Integer { .. } => true,
                    _ => false,
                };
                // A few reference values are too little to call the width "fixed".
                if a_count >= 3 {
                    note(
                        "num-precision",
                        "Number precision",
                        format!(
                            "reference always writes {} fraction digits ({}); candidate varies ({})",
                            na.frac_max,
                            q(a.sample_of(dec)),
                            q(b.sample_of(other))
                        ),
                        a.sample_of(dec),
                        b.sample_of(other),
                    );
                }
            }
        }
        return out;
    }

    if fa == [Family::Date] && fb == [Family::Date] {
        let styles = |p: &ValueProfile| {
            let mut v: Vec<&'static str> = p
                .formats
                .keys()
                .filter_map(|f| if let ValueFormat::Date { style, .. } = f { Some(*style) } else { None })
                .collect();
            v.dedup();
            v
        };
        let tzs = |p: &ValueProfile| {
            let mut v: Vec<Tz> = p
                .formats
                .keys()
                .filter_map(|f| if let ValueFormat::Date { tz, .. } = f { Some(*tz) } else { None })
                .collect();
            v.sort();
            v.dedup();
            v
        };
        let (sa, sb) = (styles(a), styles(b));
        if sa != sb {
            note(
                "date-format",
                "Date format",
                format!(
                    "reference uses {} ({}); candidate uses {} ({})",
                    sa.join(" / "),
                    q(a.any_sample()),
                    sb.join(" / "),
                    q(b.any_sample())
                ),
                a.any_sample(),
                b.any_sample(),
            );
        }
        let (ta, tb) = (tzs(a), tzs(b));
        if ta != tb {
            let pick = |p: &ValueProfile, t: Tz| {
                p.sample_of(|f| matches!(f, ValueFormat::Date { tz, .. } if *tz == t)).cloned()
            };
            let (xa, xb) = (pick(a, *ta.last().unwrap()), pick(b, *tb.last().unwrap()));
            note(
                "timezone",
                "Time zone",
                format!(
                    "reference has {} ({}); candidate has {} ({})",
                    tz_label(*ta.last().unwrap()),
                    q(xa.as_ref()),
                    tz_label(*tb.last().unwrap()),
                    q(xb.as_ref())
                ),
                xa.as_ref(),
                xb.as_ref(),
            );
        }
        return out;
    }

    if fa == [Family::DateTime] && fb == [Family::DateTime] {
        let parts = |p: &ValueProfile| {
            let mut styles = Vec::new();
            let mut frac = 0u8;
            let mut tzs = Vec::new();
            for f in p.formats.keys() {
                if let ValueFormat::DateTime { style, frac: fr, tz } = f {
                    if !styles.contains(style) {
                        styles.push(*style);
                    }
                    frac = frac.max(*fr);
                    if !tzs.contains(tz) {
                        tzs.push(*tz);
                    }
                }
            }
            tzs.sort();
            (styles, frac, tzs)
        };
        let (sa, fra, ta) = parts(a);
        let (sb, frb, tb) = parts(b);
        if sa != sb {
            note(
                "date-format",
                "Date format",
                format!(
                    "reference uses {} ({}); candidate uses {} ({})",
                    sa.join(" / "),
                    q(a.any_sample()),
                    sb.join(" / "),
                    q(b.any_sample())
                ),
                a.any_sample(),
                b.any_sample(),
            );
        }
        if fra != frb {
            let pick = |p: &ValueProfile, n: u8| {
                p.sample_of(|f| matches!(f, ValueFormat::DateTime { frac, .. } if *frac == n)).cloned()
            };
            let (xa, xb) = (pick(a, fra), pick(b, frb));
            note(
                "time-precision",
                "Timestamp precision",
                format!(
                    "fractional seconds: reference {} digits ({}), candidate {} digits ({})",
                    fra,
                    q(xa.as_ref()),
                    frb,
                    q(xb.as_ref())
                ),
                xa.as_ref(),
                xb.as_ref(),
            );
        }
        if ta != tb {
            note(
                "timezone",
                "Time zone",
                format!(
                    "reference has {} ({}); candidate has {} ({})",
                    tz_label(*ta.last().unwrap()),
                    q(a.any_sample()),
                    tz_label(*tb.last().unwrap()),
                    q(b.any_sample())
                ),
                a.any_sample(),
                b.any_sample(),
            );
        }
        return out;
    }

    // Different kinds of value. Free text in the reference accepts anything,
    // so only report when the reference is clearly typed.
    if fa != fb && fa != [Family::Text] {
        let unexpected: Vec<Family> = fb.iter().copied().filter(|f| !fa.contains(f)).collect();
        if !unexpected.is_empty() {
            let sb = b.sample_of(|f| f.family().map(|x| unexpected.contains(&x)).unwrap_or(false));
            note(
                "type",
                "Value type",
                format!(
                    "reference is {} ({}); candidate has {} ({})",
                    a.describe(),
                    q(a.any_sample()),
                    unexpected.iter().map(|f| f.label()).collect::<Vec<_>>().join(" / "),
                    q(sb)
                ),
                a.any_sample(),
                sb,
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_numbers() {
        assert_eq!(classify("150"), ValueFormat::Integer { grouped: None, digits: 3 });
        assert_eq!(
            classify("52,164.00"),
            ValueFormat::Decimal { grouped: Some(','), sep: '.', frac: 2, int_digits: 5 }
        );
        assert_eq!(classify("50,550"), ValueFormat::Integer { grouped: Some(','), digits: 5 });
        assert_eq!(
            classify("72096.0"),
            ValueFormat::Decimal { grouped: None, sep: '.', frac: 1, int_digits: 5 }
        );
        assert_eq!(
            classify("1.234,50"),
            ValueFormat::Decimal { grouped: Some('.'), sep: ',', frac: 2, int_digits: 4 }
        );
        assert_eq!(classify("1.0E-4"), ValueFormat::Scientific);
        assert_eq!(classify("NaN"), ValueFormat::Special);
    }

    #[test]
    fn classifies_dates() {
        assert_eq!(classify("1990-01-01"), ValueFormat::Date { style: "yyyy-MM-dd", tz: Tz::None });
        assert_eq!(
            classify("1982-01-01+01:00"),
            ValueFormat::Date { style: "yyyy-MM-dd", tz: Tz::Offset }
        );
        assert_eq!(classify("02.01.2005"), ValueFormat::Date { style: "dd.MM.yyyy", tz: Tz::None });
        assert_eq!(
            classify("2026-11-11T12:10:51.926361900Z"),
            ValueFormat::DateTime { style: "yyyy-MM-dd'T'HH:mm:ss", frac: 9, tz: Tz::Utc }
        );
        assert_eq!(classify("13.0.2"), ValueFormat::Text);
    }

    fn profile(values: &[&str]) -> ValueProfile {
        let mut p = ValueProfile::default();
        for v in values {
            p.add(v, 1);
        }
        p
    }

    #[test]
    fn notes_date_and_precision() {
        let n = diff_profiles(&profile(&["1990-01-01"]), &profile(&["02.01.2005"]));
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].category, "date-format");

        let n = diff_profiles(
            &profile(&["2026-11-19T09:43:54.062Z"]),
            &profile(&["2026-11-11T12:10:51.926361900Z"]),
        );
        assert_eq!(n[0].category, "time-precision");

        let n = diff_profiles(&profile(&["1990-01-01"]), &profile(&["1982-01-01+01:00"]));
        assert_eq!(n[0].category, "timezone");

        let n = diff_profiles(&profile(&["52,164.00", "115"]), &profile(&["72096.0"]));
        assert!(n.iter().any(|x| x.category == "num-grouping"));

        let n = diff_profiles(&profile(&["150"]), &profile(&["0.0"]));
        assert_eq!(n[0].category, "num-int-dec");

        let n = diff_profiles(&profile(&["x, y"]), &profile(&["APD null,null"]));
        assert_eq!(n[0].category, "special");
    }
}
