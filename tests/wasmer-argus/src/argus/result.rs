use crate::ArgusConfig;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};

/// The result of a test run
// [todo] This must support multiple test runs, so fields shall be serializable collections
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TestResults {
    // In order to avoid having complex mechanisms to serialize and deserialize
    // hashmaps with struct keys we use Engines' identifiers as keys
    results: HashMap<String, TestReport>,
}

impl TestResults {
    pub(crate) fn has(&self, config: &ArgusConfig) -> bool {
        match self.results.get(&config.compiler_backend.to_string()) {
            Some(prev_result) => {
                // Ideally we should test more differences between runs,
                // once this is the case this check can be moved to a function
                // in ArgusConfig::is_compatible(&self, other: Self) -> bool
                prev_result.config.is_compatible(config)
            }
            None => false,
        }
    }

    pub(crate) fn add(&mut self, report: TestReport) {
        self.results
            .insert(report.config.compiler_backend.to_string(), report);
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
