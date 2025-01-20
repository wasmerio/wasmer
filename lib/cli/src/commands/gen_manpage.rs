use super::WasmerCmd;
use anyhow::Context;
use clap::CommandFactory;
use clap_mangen::generate_to;
use std::path::PathBuf;
use std::sync::LazyLock;

static DEFAULT_MAN_DIR_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    dirs::data_dir()
        .unwrap_or_default()
        .join("man")
        .join("man1")
});

#[derive(Debug, Clone, clap::Parser)]
pub struct CmdGenManPage {
    /// Where to store the generated file(s) to.
    #[clap(long, default_value = DEFAULT_MAN_DIR_PATH.as_os_str())]
    pub out: PathBuf,
}

impl CmdGenManPage {
    pub fn execute(&self) -> anyhow::Result<()> {
        let cmd = WasmerCmd::command();
        generate_to(cmd, &self.out).context(format!(
            "While generating the man page(s) to {}",
            self.out.display()
        ))
    }
}
