//! User-facing app.yaml file config: [`AppConfigV1`].

mod healthcheck;

pub use self::healthcheck::{HealthCheckHttpV1, HealthCheckV1};

use std::collections::HashMap;

use anyhow::{bail, Context};
use bytesize::ByteSize;

use crate::package::PackageSource;

/// Header added to Edge app HTTP responses.
/// The value contains the app version ID that generated the response.
///
// This is used by the CLI to determine when a new version was successfully
// released.
#[allow(clippy::declare_interior_mutable_const)]
pub const HEADER_APP_VERSION_ID: &str = "x-edge-app-version-id";

/// User-facing app.yaml config file for apps.
///
/// NOTE: only used by the backend, Edge itself does not use this format, and
/// uses [`super::AppVersionV1Spec`] instead.
#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub struct AppConfigV1 {
    /// Name of the app.
    pub name: String,

    /// App id assigned by the backend.
    ///
    /// This will get populated once the app has been deployed.
    ///
    /// This id is also used to map to the existing app during deployments.
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,

    /// Owner of the app.
    ///
    /// This is either a username or a namespace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,

    /// The package to execute.
    pub package: PackageSource,

    /// Domains for the app.
    ///
    /// This can include both provider-supplied
    /// alias domains and custom domains.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domains: Option<Vec<String>>,

    /// Environment variables.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,

    // CLI arguments passed to the runner.
    /// Only applicable for runners that accept CLI arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_args: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<AppConfigCapabilityMapV1>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_tasks: Option<Vec<AppScheduledTask>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub volumes: Option<Vec<AppVolume>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_checks: Option<Vec<HealthCheckV1>>,

    /// Enable debug mode, which will show detailed error pages in the web gateway.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scaling: Option<AppScalingConfigV1>,

    /// Capture extra fields for forwards compatibility.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub struct AppScalingConfigV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<AppScalingModeV1>,
}

#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub enum AppScalingModeV1 {
    #[serde(rename = "single_concurrency")]
    SingleConcurrency,
}

#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub struct AppVolume {
    pub name: String,
    pub mounts: Vec<AppVolumeMount>,
}

#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub struct AppVolumeMount {
    /// Path to mount the volume at.
    pub mount_path: String,
    /// Sub-path within the volume to mount.
    pub sub_path: Option<String>,
}

#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub struct AppScheduledTask {
    pub name: String,
    // #[serde(flatten)]
    // pub spec: CronJobSpecV1,
}

impl AppConfigV1 {
    pub const KIND: &'static str = "wasmer.io/App.v0";
    pub const CANONICAL_FILE_NAME: &'static str = "app.yaml";

    pub fn to_yaml_value(self) -> Result<serde_yaml::Value, serde_yaml::Error> {
        // Need to do an annoying type dance to both insert the kind field
        // and also insert kind at the top.
        let obj = match serde_yaml::to_value(self)? {
            serde_yaml::Value::Mapping(m) => m,
            _ => unreachable!(),
        };
        let mut m = serde_yaml::Mapping::new();
        m.insert("kind".into(), Self::KIND.into());
        for (k, v) in obj.into_iter() {
            m.insert(k, v);
        }
        Ok(m.into())
    }

    pub fn to_yaml(self) -> Result<String, serde_yaml::Error> {
        serde_yaml::to_string(&self.to_yaml_value()?)
    }

    pub fn parse_yaml(value: &str) -> Result<Self, anyhow::Error> {
        let raw = serde_yaml::from_str::<serde_yaml::Value>(value).context("invalid yaml")?;
        let kind = raw
            .get("kind")
            .context("invalid app config: no 'kind' field found")?
            .as_str()
            .context("invalid app config: 'kind' field is not a string")?;
        match kind {
            Self::KIND => {}
            other => {
                bail!(
                    "invalid app config: unspported kind '{}', expected {}",
                    other,
                    Self::KIND
                );
            }
        }

        let data = serde_yaml::from_value(raw).context("could not deserialize app config")?;
        Ok(data)
    }
}

/// Restricted version of [`super::CapabilityMapV1`], with only a select subset
/// of settings.
#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub struct AppConfigCapabilityMapV1 {
    /// Instance memory settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<AppConfigCapabilityMemoryV1>,
}

/// Memory capability settings.
///
/// NOTE: this is kept separate from the [`super::CapabilityMemoryV1`] struct
/// to have separation between the high-level app.yaml and the more internal
/// App entity.
#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub struct AppConfigCapabilityMemoryV1 {
    /// Memory limit for an instance.
    ///
    /// Format: [digit][unit], where unit is Mb/Gb/MiB/GiB,...
    #[schemars(with = "Option<String>")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<ByteSize>,
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_app_config_v1_deser() {
        let config = r#"
kind: wasmer.io/App.v0
name: test
package: ns/name@0.1.0
debug: true
env:
  e1: v1
  E2: V2
cli_args:
  - arg1
  - arg2
scheduled_tasks:
  - name: backup
    schedule: 1day
    max_retries: 3
    timeout: 10m
    invoke:
      fetch:
        url: /api/do-backup
        headers:
          h1: v1
        success_status_codes: [200, 201]
        "#;

        let parsed = AppConfigV1::parse_yaml(config).unwrap();

        assert_eq!(
            parsed,
            AppConfigV1 {
                name: "test".to_string(),
                app_id: None,
                package: "ns/name@0.1.0".parse().unwrap(),
                owner: None,
                domains: None,
                env: [
                    ("e1".to_string(), "v1".to_string()),
                    ("E2".to_string(), "V2".to_string())
                ]
                .into_iter()
                .collect(),
                volumes: None,
                cli_args: Some(vec!["arg1".to_string(), "arg2".to_string()]),
                capabilities: None,
                scaling: None,
                scheduled_tasks: Some(vec![AppScheduledTask {
                    name: "backup".to_string(),
                    // spec: CronJobSpecV1 {
                    //     schedule: "1day".to_string(),
                    //     max_schedule_drift: None,
                    //     job: crate::schema::JobDefinition {
                    //         max_retries: Some(3),
                    //         timeout: Some(std::time::Duration::from_secs(10 * 60).into()),
                    //         invoke: crate::schema::JobInvoke::Fetch(
                    //             crate::schema::JobInvokeFetch {
                    //                 url: "/api/do-backup".parse().unwrap(),
                    //                 headers: Some(
                    //                     [("h1".to_string(), "v1".to_string())]
                    //                         .into_iter()
                    //                         .collect()
                    //                 ),
                    //                 success_status_codes: Some(vec![200, 201]),
                    //                 method: None,
                    //             }
                    //         )
                    //     },
                    // }
                }]),
                health_checks: None,
                extra: [(
                    "kind".to_string(),
                    serde_json::Value::from("wasmer.io/App.v0")
                ),]
                .into_iter()
                .collect(),
                debug: Some(true),
            }
        );
    }
}
