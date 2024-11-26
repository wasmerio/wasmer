use super::WasmerCmd;
use clap::CommandFactory;
use clap_complete::{generate, Shell};
use std::fs::OpenOptions;

#[derive(Debug, Clone, clap::Parser)]
pub struct CmdGenCompletions {
    /// The shell to generate the autocompletions script for.
    pub shell: Shell,

    /// Where to store the generated file(s) to. Defaults to stdout.
    #[clap(long)]
    pub out: Option<String>,
}

impl CmdGenCompletions {
    pub fn execute(&self) -> anyhow::Result<()> {
        let mut cmd = WasmerCmd::command();

        let name = std::env::args().next().unwrap();
        if let Some(out) = &self.out {
            let mut f = OpenOptions::new()
                .truncate(true)
                .create(true)
                .write(true)
                .open(out)?;
            generate(self.shell, &mut cmd, name, &mut f);
            Ok(())
        } else {
            generate(self.shell, &mut cmd, name, &mut std::io::stdout());
            Ok(())
        }
    }
}
