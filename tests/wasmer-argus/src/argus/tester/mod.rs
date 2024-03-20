use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use wasmer_api::types::PackageVersionWithPackage;

use super::ArgusConfig;

pub(crate) mod cli_tester;

#[cfg(feature = "wasmer_lib")]
pub(crate) mod lib_tester;

#[async_trait::async_trait]
pub(crate) trait Tester: Send + Sync {
    async fn is_to_test(&self) -> bool;
    async fn run_test(&self) -> anyhow::Result<TestReport>;
}

/// The result of a test run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestReport {
    pub package_namespace: String,
    pub package_name: String,
    pub package_version: String,

    /// The unique identifier of the test runner.
    ///
    /// In practice, it will be one of `wasmer_cli`
    /// or `wasmer_lib`.
    pub runner_id: String,
    pub runner_version: String,

    /// The unique identifier of the compiler backend used to perform the test.
    pub compiler_backend: String,

    pub time: Duration,
    pub outcome: Result<String, String>,
}

impl TestReport {
    pub fn new(
        package: &PackageVersionWithPackage,
        runner_id: String,
        runner_version: String,
        compiler_backend: String,
        time: Duration,
        outcome: Result<String, String>,
    ) -> Self {
        Self {
            package_namespace: match &package.package.namespace {
                Some(ns) => ns.clone(),
                None => String::from("unknown_namespace"),
            },
            package_name: package.package.package_name.clone(),
            package_version: package.version.clone(),
            runner_id,
            runner_version,
            compiler_backend,
            time,
            outcome,
        }
    }

    pub fn to_test(&self, _config: Arc<ArgusConfig>) -> bool {
        // In time we will have more checks to add here.
        true
    }
}
