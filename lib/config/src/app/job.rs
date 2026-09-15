use std::{borrow::Cow, fmt::Display, str::FromStr};

use anyhow::anyhow;
use serde::{Deserialize, Serialize, de::Error};

use indexmap::IndexMap;

use crate::package::PackageSource;

use super::{AppConfigCapabilityMemoryV1, AppVolume, HttpRequest, pretty_duration::PrettyDuration};

/// Job configuration.
#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub struct Job {
    pub name: String,
    pub trigger: JobTrigger,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<PrettyDuration>,

    /// Don't start job if past the due time by this amount,
    /// instead opting to wait for the next instance of it
    /// to be triggered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_schedule_drift: Option<PrettyDuration>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub retries: Option<u32>,

    /// Maximum percent of "jitter" to introduce between invocations.
    ///
    /// Value range: 0-100
    ///
    /// Jitter is used to spread out jobs over time.
    /// The calculation works by multiplying the time between invocations
    /// by a random amount, and taking the percentage of that random amount.
    ///
    /// See also [`Self::jitter_percent_min`] to set a minimum jitter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jitter_percent_max: Option<u8>,

    /// Minimum "jitter" to introduce between invocations.
    ///
    /// Value range: 0-100
    ///
    /// Jitter is used to spread out jobs over time.
    /// The calculation works by multiplying the time between invocations
    /// by a random amount, and taking the percentage of that random amount.
    ///
    /// If not specified while `jitter_percent_max` is, it will default to 10%.
    ///
    /// See also [`Self::jitter_percent_max`] to set a maximum jitter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jitter_percent_min: Option<u8>,

    pub action: JobAction,

    /// Additional unknown fields.
    ///
    /// Exists for forward compatibility for newly added fields.
    #[serde(flatten)]
    pub other: IndexMap<String, serde_json::Value>,
}

// We need this wrapper struct to enable this formatting:
// job:
//   action:
//     execute: ...
#[derive(serde::Serialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct JobAction {
    #[serde(flatten)]
    pub action: JobActionCase,
}

impl From<JobActionCase> for JobAction {
    fn from(action: JobActionCase) -> Self {
        Self { action }
    }
}

impl<'de> Deserialize<'de> for JobAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Repr {
            #[serde(default)]
            fetch: Option<HttpRequest>,
            #[serde(default)]
            execute: Option<ExecutableJob>,
        }

        match Repr::deserialize(deserializer)? {
            Repr {
                fetch: Some(_),
                execute: Some(_),
            } => Err(D::Error::custom(
                "job action must set exactly one of 'fetch' or 'execute', not both",
            )),
            Repr {
                fetch: Some(fetch),
                execute: None,
            } => Ok(JobActionCase::Fetch(fetch).into()),
            Repr {
                fetch: None,
                execute: Some(execute),
            } => Ok(JobActionCase::Execute(execute).into()),
            Repr {
                fetch: None,
                execute: None,
            } => Err(D::Error::custom(
                "job action must set one of 'fetch' or 'execute'",
            )),
        }
    }
}

#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
#[serde(rename_all = "lowercase")]
pub enum JobActionCase {
    Fetch(HttpRequest),
    Execute(ExecutableJob),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CronExpression {
    pub cron: saffron::parse::CronExpr,
    // Keep the original string form around for serialization purposes.
    pub parsed_from: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobTrigger {
    PreDeployment,
    PostDeployment,
    Cron(CronExpression),
    Duration(PrettyDuration),
}

#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub struct ExecutableJob {
    /// The package that contains the command to run. Defaults to the app config's package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<PackageSource>,

    /// The command to run. Defaults to the package's entrypoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// CLI arguments passed to the runner.
    /// Only applicable for runners that accept CLI arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_args: Option<Vec<String>>,

    /// Environment variables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<IndexMap<String, String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ExecutableJobCompatibilityMapV1>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub volumes: Option<Vec<AppVolume>>,
}

#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub struct ExecutableJobCompatibilityMapV1 {
    /// Instance memory settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<AppConfigCapabilityMemoryV1>,

    /// Additional unknown capabilities.
    ///
    /// This provides a small bit of forwards compatibility for newly added
    /// capabilities.
    #[serde(flatten)]
    pub other: IndexMap<String, serde_json::Value>,
}

impl Serialize for JobTrigger {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for JobTrigger {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let repr: Cow<'de, str> = Cow::deserialize(deserializer)?;
        repr.parse().map_err(D::Error::custom)
    }
}

impl Display for JobTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PreDeployment => write!(f, "pre-deployment"),
            Self::PostDeployment => write!(f, "post-deployment"),
            Self::Cron(cron) => write!(f, "{}", cron.parsed_from),
            Self::Duration(duration) => write!(f, "{duration}"),
        }
    }
}

impl FromStr for JobTrigger {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "pre-deployment" {
            Ok(Self::PreDeployment)
        } else if s == "post-deployment" {
            Ok(Self::PostDeployment)
        } else {
            match s.parse::<CronExpression>() {
                Ok(expr) => Ok(Self::Cron(expr)),
                _ => {
                    if let Ok(duration) = s.parse::<PrettyDuration>() {
                        Ok(Self::Duration(duration))
                    } else {
                        Err(anyhow!(
                            "Invalid job trigger '{s}'. Must be 'pre-deployment', 'post-deployment', \
                a valid cron expression such as '0 */5 * * *' or a duration such as '15m'.",
                        ))
                    }
                }
            }
        }
    }
}

impl FromStr for CronExpression {
    type Err = Box<dyn std::error::Error + Send + Sync>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(predefined_sched) = s.strip_prefix('@') {
            match predefined_sched {
                "hourly" => Ok(Self {
                    cron: "0 * * * *".parse().unwrap(),
                    parsed_from: s.to_owned(),
                }),
                "daily" => Ok(Self {
                    cron: "0 0 * * *".parse().unwrap(),
                    parsed_from: s.to_owned(),
                }),
                "weekly" => Ok(Self {
                    cron: "0 0 * * 1".parse().unwrap(),
                    parsed_from: s.to_owned(),
                }),
                "monthly" => Ok(Self {
                    cron: "0 0 1 * *".parse().unwrap(),
                    parsed_from: s.to_owned(),
                }),
                "yearly" => Ok(Self {
                    cron: "0 0 1 1 *".parse().unwrap(),
                    parsed_from: s.to_owned(),
                }),
                _ => Err(format!("Invalid cron expression {s}").into()),
            }
        } else {
            // Let's make sure the input string is valid...
            match s.parse() {
                Ok(expr) => Ok(Self {
                    cron: expr,
                    parsed_from: s.to_owned(),
                }),
                Err(_) => Err(format!("Invalid cron expression {s}").into()),
            }
        }
    }
}

impl schemars::JsonSchema for JobTrigger {
    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("JobTrigger")
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        String::json_schema(generator)
    }

    fn schema_name() -> Cow<'static, str> {
        Self::schema_id()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn job_action_rejects_anything_but_exactly_one_action() {
        for yaml in ["execute: {}\nfetch:\n  path: /", "{}", "sleep: {}"] {
            assert!(
                serde_yaml::from_str::<JobAction>(yaml).is_err(),
                "accepted invalid action {yaml:?}"
            );
            let json: serde_json::Value = serde_yaml::from_str(yaml).unwrap();
            assert!(serde_json::from_value::<JobAction>(json).is_err());
        }
    }

    #[test]
    fn job_action_serialization_roundtrip() {
        for yaml in [
            "execute:\n  command: php",
            "fetch:\n  path: /\n  timeout: 30s",
        ] {
            let action: JobAction = serde_yaml::from_str(yaml).unwrap();
            assert_eq!(serde_yaml::to_string(&action).unwrap().trim(), yaml);

            let json: serde_json::Value = serde_yaml::from_str(yaml).unwrap();
            assert_eq!(
                serde_json::from_value::<JobAction>(json.clone()).unwrap(),
                action
            );
            assert_eq!(serde_json::to_value(action).unwrap(), json);
        }
    }

    #[test]
    pub fn job_trigger_serialization_roundtrip() {
        fn assert_roundtrip(
            serialized: &str,
            description: Option<&str>,
            duration: Option<Duration>,
        ) {
            let parsed = serialized.parse::<JobTrigger>().unwrap();
            assert_eq!(&parsed.to_string(), serialized);

            if let JobTrigger::Cron(expr) = &parsed {
                assert_eq!(
                    &expr
                        .cron
                        .describe(saffron::parse::English::default())
                        .to_string(),
                    description.unwrap()
                );
            } else {
                assert!(description.is_none());
            }

            if let JobTrigger::Duration(d) = &parsed {
                assert_eq!(d.as_duration(), duration.unwrap());
            } else {
                assert!(duration.is_none());
            }
        }

        assert_roundtrip("pre-deployment", None, None);
        assert_roundtrip("post-deployment", None, None);

        assert_roundtrip("@hourly", Some("Every hour"), None);
        assert_roundtrip("@daily", Some("At 12:00 AM"), None);
        assert_roundtrip("@weekly", Some("At 12:00 AM on Sunday"), None);
        assert_roundtrip(
            "@monthly",
            Some("At 12:00 AM on the 1st of every month"),
            None,
        );
        assert_roundtrip("@yearly", Some("At 12:00 AM on the 1st of January"), None);

        // Note: the parsing code should keep the formatting of the source string.
        // This is tested in assert_roundtrip.
        assert_roundtrip(
            "0/2 12 * JAN-APR 2",
            Some(
                "At every 2nd minute from 0 through 59 minutes past the hour, \
                between 12:00 PM and 12:59 PM on Monday of January to April",
            ),
            None,
        );

        assert_roundtrip("10s", None, Some(Duration::from_secs(10)));
        assert_roundtrip("15m", None, Some(Duration::from_secs(15 * 60)));
        assert_roundtrip("20h", None, Some(Duration::from_secs(20 * 60 * 60)));
        assert_roundtrip("2d", None, Some(Duration::from_secs(2 * 60 * 60 * 24)));
    }

    #[test]
    pub fn job_serialization_roundtrip() {
        fn parse_cron(expr: &str) -> CronExpression {
            CronExpression {
                cron: expr.parse().unwrap(),
                parsed_from: expr.to_owned(),
            }
        }

        let job = Job {
            name: "my-job".to_owned(),
            trigger: JobTrigger::Cron(parse_cron("0/2 12 * JAN-APR 2")),
            timeout: Some("1m".parse().unwrap()),
            max_schedule_drift: Some("2h".parse().unwrap()),
            jitter_percent_max: None,
            jitter_percent_min: None,
            retries: None,
            action: JobAction {
                action: JobActionCase::Execute(super::ExecutableJob {
                    package: Some(crate::package::PackageSource::Ident(
                        crate::package::PackageIdent::Named(crate::package::NamedPackageIdent {
                            registry: None,
                            namespace: Some("ns".to_owned()),
                            name: "pkg".to_owned(),
                            tag: None,
                        }),
                    )),
                    command: Some("cmd".to_owned()),
                    cli_args: Some(vec!["arg-1".to_owned(), "arg-2".to_owned()]),
                    env: Some([("VAR1".to_owned(), "Value".to_owned())].into()),
                    capabilities: Some(super::ExecutableJobCompatibilityMapV1 {
                        memory: Some(crate::app::AppConfigCapabilityMemoryV1 {
                            limit: Some(bytesize::ByteSize::gib(1)),
                        }),
                        other: Default::default(),
                    }),
                    volumes: Some(vec![crate::app::AppVolume {
                        name: "vol".to_owned(),
                        mount: "/path/to/volume".to_owned(),
                    }]),
                }),
            },
            other: Default::default(),
        };

        let serialized = r#"
name: my-job
trigger: 0/2 12 * JAN-APR 2
timeout: 1m
max_schedule_drift: 2h
action:
  execute:
    package: ns/pkg
    command: cmd
    cli_args:
    - arg-1
    - arg-2
    env:
      VAR1: Value
    capabilities:
      memory:
        limit: 1.0 GiB
    volumes:
    - name: vol
      mount: /path/to/volume"#;

        assert_eq!(
            serialized.trim(),
            serde_yaml::to_string(&job).unwrap().trim()
        );
        assert_eq!(job, serde_yaml::from_str(serialized).unwrap());
    }
}
