use super::WasmerCmd;
use anyhow::Context;
use clap::CommandFactory;
use clap_mangen::generate_to;

#[derive(Debug, Clone, clap::Parser)]
pub struct CmdGenManPage {
    /// Where to store the generated file(s) to.
    #[clap(long)]
    pub out: Option<String>,
}

impl CmdGenManPage {
    pub fn execute(&self) -> anyhow::Result<()> {
        let cmd = WasmerCmd::command();
        let outpath = if let Some(out) = &self.out {
            out.clone()
        } else {
            "~/.local/share/man".to_string()
        };
        generate_to(cmd, &outpath).context(format!("While generating the man page(s) to {outpath}"))
    }
}
