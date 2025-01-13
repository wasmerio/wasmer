use std::{
    borrow::Cow,
    fmt::{Debug, Display},
    str::FromStr,
    time::Duration,
};

use schemars::JsonSchema;
use serde::{de::Error, Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PrettyDuration {
    unit: DurationUnit,
    amount: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum DurationUnit {
    Seconds,
    Minutes,
    Hours,
    Days,
}

impl PrettyDuration {
    pub fn as_duration(&self) -> Duration {
        match self.unit {
            DurationUnit::Seconds => Duration::from_secs(self.amount),
            DurationUnit::Minutes => Duration::from_secs(self.amount * 60),
            DurationUnit::Hours => Duration::from_secs(self.amount * 60 * 60),
            DurationUnit::Days => Duration::from_secs(self.amount * 60 * 60 * 24),
        }
    }
}

impl Default for PrettyDuration {
    fn default() -> Self {
        Self {
            unit: DurationUnit::Seconds,
            amount: 0,
        }
    }
}

impl PartialOrd for PrettyDuration {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrettyDuration {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_duration().cmp(&other.as_duration())
    }
}

impl JsonSchema for PrettyDuration {
    fn schema_name() -> String {
        "PrettyDuration".to_owned()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        String::json_schema(gen)
    }
}

impl Display for DurationUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DurationUnit::Seconds => write!(f, "s"),
            DurationUnit::Minutes => write!(f, "m"),
            DurationUnit::Hours => write!(f, "h"),
            DurationUnit::Days => write!(f, "d"),
        }
    }
}

impl FromStr for DurationUnit {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "s" | "S" => Ok(Self::Seconds),
            "m" | "M" => Ok(Self::Minutes),
            "h" | "H" => Ok(Self::Hours),
            "d" | "D" => Ok(Self::Days),
            _ => Err(()),
        }
    }
}

impl Display for PrettyDuration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.amount, self.unit)
    }
}

impl Debug for PrettyDuration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <Self as Display>::fmt(self, f)
    }
}

impl FromStr for PrettyDuration {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (amount_str, unit_str) = s.split_at_checked(s.len() - 1).ok_or(())?;
        Ok(Self {
            unit: unit_str.parse()?,
            amount: amount_str.parse().map_err(|_| ())?,
        })
    }
}

impl Serialize for PrettyDuration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for PrettyDuration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let repr: Cow<'de, str> = Cow::deserialize(deserializer)?;
        repr.parse()
            .map_err(|()| D::Error::custom("Failed to parse value as a duration"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn pretty_duration_serialize() {
        assert_eq!(
            PrettyDuration {
                unit: DurationUnit::Seconds,
                amount: 1234
            }
            .to_string(),
            "1234s"
        );
        assert_eq!(
            PrettyDuration {
                unit: DurationUnit::Minutes,
                amount: 345
            }
            .to_string(),
            "345m"
        );
        assert_eq!(
            PrettyDuration {
                unit: DurationUnit::Hours,
                amount: 56
            }
            .to_string(),
            "56h"
        );
        assert_eq!(
            PrettyDuration {
                unit: DurationUnit::Days,
                amount: 7
            }
            .to_string(),
            "7d"
        );
    }

    #[test]
    pub fn pretty_duration_deserialize() {
        fn assert_deserializes_to(repr1: &str, repr2: &str, unit: DurationUnit, amount: u64) {
            let duration = PrettyDuration { unit, amount };
            assert_eq!(duration, repr1.parse().unwrap());
            assert_eq!(duration, repr2.parse().unwrap());
        }

        assert_deserializes_to("12s", "12S", DurationUnit::Seconds, 12);
        assert_deserializes_to("34m", "34M", DurationUnit::Minutes, 34);
        assert_deserializes_to("56h", "56H", DurationUnit::Hours, 56);
        assert_deserializes_to("7d", "7D", DurationUnit::Days, 7);
    }

    #[test]
    #[should_panic]
    pub fn cant_parse_nagative_duration() {
        _ = "-12s".parse::<PrettyDuration>().unwrap();
    }
}
