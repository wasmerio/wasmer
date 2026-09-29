use std::{
    borrow::Cow,
    fmt::{Debug, Display},
    str::FromStr,
    time::Duration,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::Error};

/// A duration parsed from a human-readable value.
///
/// Format:
/// ( [NUMBER][UNIT] [SPACE]? )+
///
/// Unit:
/// * Seconds: s|sec|secs|seconds
/// * Minutes: m|min|mins|minutes
/// * Hours:   h|hour|hours
/// * Days:    d|day|days
///
/// Examples: `30s`, `1m30s`, `1m 30s`, `2hours`, `7d`.
///
/// Resolution is one second; there are no sub-second units.
///
/// The spelling a value was parsed from is preserved, so a config that is read
/// and written back keeps the author's formatting. Values constructed in code
/// are formatted with the largest units that divide them evenly.
#[derive(Clone)]
pub struct PrettyDuration {
    /// The text this value was parsed from, or the formatting of a value
    /// constructed in code. Always re-parses to the same duration.
    text: String,
    duration: Duration,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum DurationUnit {
    Seconds,
    Minutes,
    Hours,
    Days,
}

impl DurationUnit {
    /// The number of seconds in one of this unit.
    const fn secs(self) -> u64 {
        match self {
            Self::Seconds => 1,
            Self::Minutes => 60,
            Self::Hours => 60 * 60,
            Self::Days => 24 * 60 * 60,
        }
    }
}

impl PrettyDuration {
    /// Sub-second precision is truncated, since no unit can express it and the
    /// spelling and the value would otherwise disagree.
    pub fn new(duration: Duration) -> Self {
        let duration = Duration::from_secs(duration.as_secs());
        Self {
            text: format_duration(duration),
            duration,
        }
    }

    pub fn as_duration(&self) -> Duration {
        self.duration
    }

    /// The spelling this value was parsed from or formatted as.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn from_secs(secs: u64) -> Self {
        Self::new(Duration::from_secs(secs))
    }

    /// Saturates at [`u64::MAX`] seconds, matching [`Self::from_hours`] and
    /// [`Self::from_days`]. Parsing rejects that magnitude instead, because
    /// there the amount comes from a config file rather than from a caller.
    pub fn from_mins(mins: u64) -> Self {
        Self::new(Duration::from_secs(mins.saturating_mul(60)))
    }

    /// Saturates at [`u64::MAX`] seconds. See [`Self::from_mins`].
    pub fn from_hours(hours: u64) -> Self {
        Self::new(Duration::from_secs(hours.saturating_mul(60 * 60)))
    }

    /// Saturates at [`u64::MAX`] seconds. See [`Self::from_mins`].
    pub fn from_days(days: u64) -> Self {
        Self::new(Duration::from_secs(days.saturating_mul(24 * 60 * 60)))
    }
}

impl From<Duration> for PrettyDuration {
    fn from(duration: Duration) -> Self {
        Self::new(duration)
    }
}

impl From<PrettyDuration> for Duration {
    fn from(duration: PrettyDuration) -> Self {
        duration.duration
    }
}

impl Default for PrettyDuration {
    fn default() -> Self {
        Self::new(Duration::ZERO)
    }
}

// Compare by the duration rather than the spelling, so `60s` and `1m` are equal
// and `Eq`/`Ord`/`Hash` stay consistent with each other.
impl PartialEq for PrettyDuration {
    fn eq(&self, other: &Self) -> bool {
        self.duration == other.duration
    }
}

impl Eq for PrettyDuration {}

impl std::hash::Hash for PrettyDuration {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.duration.hash(state);
    }
}

impl PartialOrd for PrettyDuration {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrettyDuration {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.duration.cmp(&other.duration)
    }
}

impl JsonSchema for PrettyDuration {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PrettyDuration")
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        String::json_schema(generator)
    }

    fn inline_schema() -> bool {
        false
    }

    fn schema_id() -> Cow<'static, str> {
        Self::schema_name()
    }
}

impl Display for DurationUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let unit = match self {
            DurationUnit::Seconds => "s",
            DurationUnit::Minutes => "m",
            DurationUnit::Hours => "h",
            DurationUnit::Days => "d",
        };
        f.write_str(unit)
    }
}

impl FromStr for DurationUnit {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "s" | "sec" | "secs" | "seconds" => Ok(Self::Seconds),
            "m" | "min" | "mins" | "minutes" => Ok(Self::Minutes),
            "h" | "hour" | "hours" => Ok(Self::Hours),
            "d" | "day" | "days" => Ok(Self::Days),
            _ => Err(()),
        }
    }
}

/// Render a duration as the largest units that divide it evenly, e.g.
/// `90s` as `1m30s`. Zero has no components, so it is spelled out explicitly.
fn format_duration(duration: Duration) -> String {
    if duration.is_zero() {
        return "0s".to_string();
    }

    let mut secs = duration.as_secs();
    let mut text = String::new();
    for unit in [
        DurationUnit::Days,
        DurationUnit::Hours,
        DurationUnit::Minutes,
        DurationUnit::Seconds,
    ] {
        let scale = unit.secs();
        let amount = secs / scale;
        if amount > 0 {
            text.push_str(&format!("{amount}{unit}"));
            secs %= scale;
        }
    }
    text
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrettyDurationParseError {
    value: String,
    message: String,
}

impl Display for PrettyDurationParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid duration '{}': {}. Expected a value such as '30s', '1m30s' or '2h'",
            self.value, self.message
        )
    }
}

impl std::error::Error for PrettyDurationParseError {}

impl Display for PrettyDuration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

impl Debug for PrettyDuration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <Self as Display>::fmt(self, f)
    }
}

impl FromStr for PrettyDuration {
    type Err = PrettyDurationParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let fail = |message: &str| PrettyDurationParseError {
            value: s.to_string(),
            message: message.to_string(),
        };

        let mut rest = s;
        let mut secs: u64 = 0;
        let mut components = 0;
        loop {
            // A single space may separate components, as in `1m 30s`.
            rest = rest.strip_prefix(' ').unwrap_or(rest);
            if rest.is_empty() {
                break;
            }

            let digits = rest.chars().take_while(char::is_ascii_digit).count();
            if digits == 0 {
                return Err(fail("every component must start with a number"));
            }
            let amount = rest[..digits]
                .parse::<u64>()
                .map_err(|_| fail("number is out of range"))?;
            rest = &rest[digits..];

            let letters = rest.chars().take_while(char::is_ascii_alphabetic).count();
            let unit = rest[..letters]
                .parse::<DurationUnit>()
                .map_err(|()| fail(&format!("unknown unit '{}'", &rest[..letters])))?;
            rest = &rest[letters..];

            secs = amount
                .checked_mul(unit.secs())
                .and_then(|component| secs.checked_add(component))
                .ok_or_else(|| fail("duration is too large"))?;
            components += 1;
        }

        if components == 0 {
            return Err(fail("must not be empty"));
        }

        Ok(Self {
            text: s.to_string(),
            duration: Duration::from_secs(secs),
        })
    }
}

impl Serialize for PrettyDuration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.text)
    }
}

impl<'de> Deserialize<'de> for PrettyDuration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let repr: Cow<'de, str> = Cow::deserialize(deserializer)?;
        repr.parse().map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn pretty_duration_serialize() {
        assert_eq!(PrettyDuration::from_secs(1234).to_string(), "20m34s");
        assert_eq!(PrettyDuration::from_mins(345).to_string(), "5h45m");
        assert_eq!(PrettyDuration::from_hours(56).to_string(), "2d8h");
        assert_eq!(PrettyDuration::from_days(7).to_string(), "7d");
        assert_eq!(PrettyDuration::default().to_string(), "0s");
    }

    #[test]
    pub fn pretty_duration_deserialize() {
        fn assert_deserializes_to(repr1: &str, repr2: &str, duration: Duration) {
            assert_eq!(
                repr1.parse::<PrettyDuration>().unwrap().as_duration(),
                duration
            );
            assert_eq!(
                repr2.parse::<PrettyDuration>().unwrap().as_duration(),
                duration
            );
        }

        assert_deserializes_to("12s", "12S", Duration::from_secs(12));
        assert_deserializes_to("34m", "34M", Duration::from_secs(34 * 60));
        assert_deserializes_to("56h", "56H", Duration::from_secs(56 * 60 * 60));
        assert_deserializes_to("7d", "7D", Duration::from_secs(7 * 24 * 60 * 60));
    }

    #[test]
    fn compound_and_long_form_units_are_accepted() {
        let cases = [
            ("30s", Duration::from_secs(30)),
            ("90seconds", Duration::from_secs(90)),
            ("5mins", Duration::from_secs(5 * 60)),
            ("2hours", Duration::from_secs(2 * 60 * 60)),
            ("1m30s", Duration::from_secs(90)),
            ("1m 30s", Duration::from_secs(90)),
            ("1h1m1s", Duration::from_secs(61 * 60 + 1)),
            ("1d 2h 30s", Duration::from_secs(26 * 60 * 60 + 30)),
        ];
        for (input, expected) in cases {
            let parsed = input.parse::<PrettyDuration>().unwrap();
            assert_eq!(parsed.as_duration(), expected, "parsing {input}");
        }
    }

    #[test]
    fn parsing_preserves_the_original_spelling() {
        // Config files are read and written back, so the author's formatting
        // must survive a round trip rather than being normalized.
        for input in ["120s", "0s", "1m 30s", "2hours", "12S"] {
            let parsed = input.parse::<PrettyDuration>().unwrap();
            assert_eq!(parsed.to_string(), input);
            assert_eq!(
                serde_json::to_value(&parsed).unwrap(),
                serde_json::json!(input)
            );
        }
    }

    #[test]
    fn formatted_durations_reparse_to_the_same_value() {
        for duration in [
            Duration::ZERO,
            Duration::from_secs(1),
            Duration::from_secs(90),
            Duration::from_secs(26 * 60 * 60 + 30),
            Duration::from_secs(7 * 24 * 60 * 60),
        ] {
            let formatted = PrettyDuration::new(duration);
            assert_eq!(
                formatted.to_string().parse::<PrettyDuration>().unwrap(),
                formatted
            );
        }
    }

    #[test]
    fn equal_durations_compare_equal_regardless_of_spelling() {
        assert_eq!(
            "60s".parse::<PrettyDuration>().unwrap(),
            "1m".parse::<PrettyDuration>().unwrap()
        );
        assert!("59s".parse::<PrettyDuration>().unwrap() < "1m".parse::<PrettyDuration>().unwrap());
    }

    #[test]
    #[should_panic]
    pub fn cant_parse_negative_duration() {
        _ = "-12s".parse::<PrettyDuration>().unwrap();
    }

    #[test]
    fn unit_constructors_saturate_instead_of_overflowing() {
        for duration in [
            PrettyDuration::from_mins(u64::MAX),
            PrettyDuration::from_hours(u64::MAX),
            PrettyDuration::from_days(u64::MAX),
        ] {
            assert_eq!(duration.as_duration(), Duration::from_secs(u64::MAX));
            assert_eq!(
                duration.to_string().parse::<PrettyDuration>().unwrap(),
                duration
            );
        }
    }

    #[test]
    fn sub_second_precision_is_truncated() {
        // The unit set bottoms out at seconds, so a constructed value drops
        // anything finer rather than formatting to a spelling it cannot parse.
        assert_eq!(
            PrettyDuration::new(Duration::from_millis(1500)).as_duration(),
            Duration::from_secs(1)
        );
        assert_eq!(
            PrettyDuration::new(Duration::from_millis(1500)).as_str(),
            "1s"
        );
    }

    #[test]
    fn invalid_durations_are_rejected() {
        for input in [
            "",
            " ",
            "5",
            "s",
            "5x",
            "500ms",
            "1ns",
            "1microsec",
            "5 s",
            "1m  30s",
            "not a duration",
            "18446744073709551616s",
            "18446744073709551615d",
        ] {
            assert!(
                input.parse::<PrettyDuration>().is_err(),
                "accepted invalid duration {input:?}"
            );
            assert!(serde_json::from_value::<PrettyDuration>(serde_json::json!(input)).is_err());
        }
    }
}
