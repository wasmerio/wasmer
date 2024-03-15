use super::result::TestReport;
use crate::ArgusConfig;
use indicatif::ProgressBar;
use std::{path::PathBuf, sync::Arc};

pub(crate) mod cli_tester;

#[cfg(feature = "wasmer_lib")]
pub(crate) mod lib_tester;

#[async_trait::async_trait]
pub(crate) trait Tester {
    async fn run_test(
        test_id: u64,
        config: Arc<ArgusConfig>,
        p: &ProgressBar,
        webc_path: PathBuf,
        package_name: &str,
    ) -> anyhow::Result<TestReport>;
}
