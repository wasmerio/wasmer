use std::{borrow::Cow, fmt::Display, str::FromStr};

use serde::{de::Error, Deserialize, Serialize};

use indexmap::IndexMap;

use crate::package::PackageSource;

use super::{pretty_duration::PrettyDuration, AppConfigCapabilityMemoryV1, AppVolume, HttpRequest};

/// Job configuration.
#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub struct Job {
    name: String,
    trigger: JobTrigger,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<PrettyDuration>,

    /// Don't start job if past the due time by this amount,
    /// instead opting to wait for the next instance of it
    /// to be triggered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_schedule_drift: Option<PrettyDuration>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub retries: Option<u32>,

    action: JobAction,
}

// We need this wrapper struct to enable this formatting:
// job:
//   action:
//     execute: ...
#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub struct JobAction {
    #[serde(flatten)]
    action: JobActionCase,
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
}

#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub struct ExecutableJob {
    /// The package that contains the command to run. Defaults to the app config's package.
    #[serde(skip_serializing_if = "Option::is_none")]
    package: Option<PackageSource>,

    /// The command to run. Defaults to the package's entrypoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,

    /// CLI arguments passed to the runner.
    /// Only applicable for runners that accept CLI arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    cli_args: Option<Vec<String>>,

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
        }
    }
}

impl FromStr for JobTrigger {
    type Err = Box<dyn std::error::Error + Send + Sync>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "pre-deployment" {
            Ok(Self::PreDeployment)
        } else if s == "post-deployment" {
            Ok(Self::PostDeployment)
        } else {
            let expr = s.parse::<CronExpression>()?;
            Ok(Self::Cron(expr))
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
    fn schema_name() -> String {
        "JobTrigger".to_owned()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        String::json_schema(gen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn job_trigger_serialization_roundtrip() {
        fn assert_roundtrip(serialized: &str, description: Option<&str>) {
            let parsed = serialized.parse::<JobTrigger>().unwrap();
            assert_eq!(&parsed.to_string(), serialized);

            if let JobTrigger::Cron(expr) = parsed {
                assert_eq!(
                    &expr
                        .cron
                        .describe(saffron::parse::English::default())
                        .to_string(),
                    description.unwrap()
                );
            } else {
                assert!(description.is_none())
            }
        }

        assert_roundtrip("pre-deployment", None);
        assert_roundtrip("post-deployment", None);

        assert_roundtrip("@hourly", Some("Every hour"));
        assert_roundtrip("@daily", Some("At 12:00 AM"));
        assert_roundtrip("@weekly", Some("At 12:00 AM on Sunday"));
        assert_roundtrip("@monthly", Some("At 12:00 AM on the 1st of every month"));
        assert_roundtrip("@yearly", Some("At 12:00 AM on the 1st of January"));

        // Note: the parsing code should keep the formatting of the source string.
        // This is tested in assert_roundtrip.
        assert_roundtrip("0/2 12 * JAN-APR 2", Some("At every 2nd minute from 0 through 59 minutes past the hour, \
                                                    between 12:00 PM and 12:59 PM on Monday of January to April"));
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
                            limit: Some(bytesize::ByteSize::gb(1)),
                        }),
                        other: Default::default(),
                    }),
                    volumes: Some(vec![crate::app::AppVolume {
                        name: "vol".to_owned(),
                        mount: "/path/to/volume".to_owned(),
                    }]),
                }),
            },
        };

        let serialized = r#"
name: my-job
trigger: '0/2 12 * JAN-APR 2'
timeout: '1m'
max_schedule_drift: '2h'
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
        limit: '1000.0 MB'
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
