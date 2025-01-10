use std::{borrow::Cow, collections::HashMap, fmt::Display, str::FromStr};

use serde::{de::Error, Deserialize, Serialize};

use crate::package::PackageSource;

use super::{AppConfigCapabilityMemoryV1, AppVolume, HttpRequest};

/// Job configuration.
#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub struct Job {
    name: String,
    trigger: JobTrigger,
    #[serde(skip_serializing_if = "Option::is_none")]
    fetch: Option<HttpRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    execute: Option<ExecutableJob>,
}

#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub enum CronType {
    Daily,
    Hourly,
    Weekly,
    CronExpression(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobTrigger {
    PreDeployment,
    PostDeployment,
    Cron(CronType),
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
    pub env: Option<HashMap<String, String>>,

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
    pub other: HashMap<String, serde_json::Value>,
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
            Self::Cron(CronType::Hourly) => write!(f, "@hourly"),
            Self::Cron(CronType::Daily) => write!(f, "@daily"),
            Self::Cron(CronType::Weekly) => write!(f, "@weekly"),
            Self::Cron(CronType::CronExpression(sched)) => write!(f, "{}", sched),
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
        } else if let Some(predefined_sched) = s.strip_prefix('@') {
            match predefined_sched {
                "hourly" => Ok(Self::Cron(CronType::Hourly)),
                "daily" => Ok(Self::Cron(CronType::Daily)),
                "weekly" => Ok(Self::Cron(CronType::Weekly)),
                _ => Err(format!("Invalid cron expression {s}").into()),
            }
        } else {
            // Let's make sure the input string is valid...
            match cron::Schedule::from_str(s) {
                Ok(sched) => Ok(Self::Cron(CronType::CronExpression(
                    sched.source().to_owned(),
                ))),
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
    use crate::app::JobTrigger;

    use super::Job;

    #[test]
    pub fn job_trigger_serialization_roundtrip() {
        fn assert_roundtrip(serialized: &str, value: JobTrigger) {
            assert_eq!(&value.to_string(), serialized);
            assert_eq!(serialized.parse::<JobTrigger>().unwrap(), value);
        }

        assert_roundtrip("pre-deployment", JobTrigger::PreDeployment);
        assert_roundtrip("post-deployment", JobTrigger::PostDeployment);

        assert_roundtrip("@hourly", JobTrigger::Cron(crate::app::CronType::Hourly));
        assert_roundtrip("@daily", JobTrigger::Cron(crate::app::CronType::Daily));
        assert_roundtrip("@weekly", JobTrigger::Cron(crate::app::CronType::Weekly));

        // Note: the parsing code should keep the formatting of the source string.
        // This is tested in assert_roundtrip.
        assert_roundtrip(
            "0 0/2 12 ? JAN-APR 2",
            JobTrigger::Cron(crate::app::CronType::CronExpression(
                "0 0/2 12 ? JAN-APR 2".to_owned(),
            )),
        );
    }

    #[test]
    pub fn job_serialization_roundtrip() {
        let job = Job {
            name: "my-job".to_owned(),
            trigger: JobTrigger::Cron(super::CronType::CronExpression(
                "0 0/2 12 ? JAN-APR 2".to_owned(),
            )),
            fetch: None,
            execute: Some(super::ExecutableJob {
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
        };

        let serialized = r#"
name: my-job
trigger: '0 0/2 12 ? JAN-APR 2'
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
