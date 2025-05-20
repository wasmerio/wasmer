//! User-facing app.yaml file config: [`AppConfigV1`].

mod healthcheck;
mod http;
mod job;
mod pretty_duration;
mod snapshot_trigger;

pub use self::{healthcheck::*, http::*, job::*, pretty_duration::*, snapshot_trigger::*};

use anyhow::{bail, Context};
use bytesize::ByteSize;
use indexmap::IndexMap;

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
    pub name: Option<String>,

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

    /// Location-related configuration for the app.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locality: Option<Locality>,

    /// Environment variables.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub env: IndexMap<String, String>,

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

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redirect: Option<Redirect>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub jobs: Option<Vec<Job>>,

    /// Capture extra fields for forwards compatibility.
    #[serde(flatten)]
    pub extra: IndexMap<String, serde_json::Value>,
}

#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub struct Locality {
    pub regions: Vec<String>,
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
    pub mount: String,
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

    /// Runtime settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<AppConfigCapabilityRuntimeV1>,

    /// Enables app bootstrapping with startup snapshots.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instaboot: Option<AppConfigCapabilityInstaBootV1>,

    /// Additional unknown capabilities.
    ///
    /// This provides a small bit of forwards compatibility for newly added
    /// capabilities.
    #[serde(flatten)]
    pub other: IndexMap<String, serde_json::Value>,
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

/// Runtime capability settings.
#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub struct AppConfigCapabilityRuntimeV1 {
    /// Engine to use for an instance, e.g. wasmer_cranelift, wasmer_llvm, etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    /// Whether to enable asynchronous threads/deep sleeping.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub async_threads: Option<bool>,
}

/// Enables accelerated instance boot times with startup snapshots.
///
/// How it works:
/// The Edge runtime will create a pre-initialized snapshot of apps that is
/// ready to serve requests
/// Your app will then restore from the generated snapshot, which has the
/// potential to significantly speed up cold starts.
///
/// To drive the initialization, multiple http requests can be specified.
/// All the specified requests will be sent to the app before the snapshot is
/// created, allowing the app to pre-load files, pre initialize caches, ...
#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub struct AppConfigCapabilityInstaBootV1 {
    /// The method to use to generate the instaboot snapshot for the instance.
    #[serde(default)]
    pub mode: Option<InstabootSnapshotModeV1>,

    /// HTTP requests to perform during startup snapshot creation.
    /// Apps can perform all the appropriate warmup logic in these requests.
    ///
    /// NOTE: if no requests are configured, then a single HTTP
    /// request to '/' will be performed instead.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requests: Vec<HttpRequest>,

    /// Maximum age of snapshots.
    ///
    /// Format: 5m, 1h, 2d, ...
    ///
    /// After the specified time new snapshots will be created, and the old
    /// ones discarded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_age: Option<PrettyDuration>,
}

/// How will an instance be bootstrapped?
#[derive(
    serde::Serialize,
    serde::Deserialize,
    PartialEq,
    Eq,
    Hash,
    Clone,
    Debug,
    schemars::JsonSchema,
    Default,
)]
#[serde(rename_all = "snake_case")]
pub enum InstabootSnapshotModeV1 {
    /// Start the instance without any snapshot triggers. Once the requests are done,
    /// use [`snapshot_and_stop`](wasmer_wasix::WasiProcess::snapshot_and_stop) to
    /// capture a snapshot and shut the instance down.
    #[default]
    Bootstrap,

    /// Explicitly enable the given snapshot triggers before starting the instance.
    /// The instance's process will have its stop_running_after_checkpoint flag set,
    /// so the first snapshot will cause the instance to shut down.
    // FIXME: make this strongly typed
    Triggers(Vec<SnapshotTrigger>),
}

/// App redirect configuration.
#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Eq,
)]
pub struct Redirect {
    /// Force https by redirecting http requests to https automatically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_https: Option<bool>,
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
locality: 
  regions: 
    - eu-rome
redirect:
  force_https: true
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
                name: Some("test".to_string()),
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
                }]),
                health_checks: None,
                extra: [(
                    "kind".to_string(),
                    serde_json::Value::from("wasmer.io/App.v0")
                ),]
                .into_iter()
                .collect(),
                debug: Some(true),
                redirect: Some(Redirect {
                    force_https: Some(true)
                }),
                locality: Some(Locality {
                    regions: vec!["eu-rome".to_string()]
                }),
                jobs: None,
            }
        );
    }

    #[test]
    fn test_app_config_v1_volumes() {
        let config = r#"
kind: wasmer.io/App.v0
name: test
package: ns/name@0.1.0
volumes:
  - name: vol1
    mount: /vol1
  - name: vol2
    mount: /vol2

"#;

        let parsed = AppConfigV1::parse_yaml(config).unwrap();
        let expected_volumes = vec![
            AppVolume {
                name: "vol1".to_string(),
                mount: "/vol1".to_string(),
            },
            AppVolume {
                name: "vol2".to_string(),
                mount: "/vol2".to_string(),
            },
        ];
        if let Some(actual_volumes) = parsed.volumes {
            assert_eq!(actual_volumes, expected_volumes);
        } else {
            panic!("Parsed volumes are None, expected Some({expected_volumes:?})");
        }
    }
}
