use crate::ArgusConfig;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};

type WasmerVersion = String;
type EngineId = String;

/// The result of a test run
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TestResults {
    results: HashMap<WasmerVersion, HashMap<EngineId, TestReport>>,
}

impl TestResults {
    pub(crate) fn has(&self, config: &ArgusConfig) -> bool {
        let wasmer_version = config.wasmer_version();
        let engine_id = config.compiler_backend.to_string();

        if let Some(v) = self.results.get(&wasmer_version) {
            if let Some(v) = v.get(&engine_id) {
                return v.config.is_compatible(config);
            }
        }

        false
    }

    pub(crate) fn add(&mut self, report: TestReport) {
        let wasmer_version = report.config.wasmer_version();
        let engine_id = report.config.compiler_backend.to_string();

        match self.results.get_mut(&wasmer_version) {
            Some(v) => _ = v.insert(engine_id, report),
            None => {
                _ = self.results.insert(
                    wasmer_version,
                    HashMap::from_iter(vec![(engine_id, report)]),
                )
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestReport {
    /// The outcome of the test
    result: Result<(), String>,
    /// How long the test took
    time: Duration,
    /// The configuration of the test
    config: ArgusConfig,
}

impl TestReport {
    pub fn new(config: &ArgusConfig, result: Result<(), String>, time: Duration) -> Self {
        Self {
            result,
            time,
            config: config.clone(),
        }
    }
}
