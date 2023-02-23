use clap::Parser;
use wasmer_registry::WasmerConfig;

/// Subcommand for logging out of the registry
#[derive(Debug, Clone, Parser)]
pub struct Logout {}

impl Logout {
    /// execute [Logout]
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        let wasmer_dir =
            WasmerConfig::get_wasmer_dir().map_err(|e| anyhow::anyhow!("no wasmer dir: {e}"))?;
        let path = WasmerConfig::get_file_location(wasmer_dir.as_path());
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}

#[test]
fn test_logout() {
    Logout {}.execute().unwrap();
}
