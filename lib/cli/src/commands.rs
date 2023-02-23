//! The commands available in the Wasmer binary.
mod add;
#[cfg(target_os = "linux")]
mod binfmt;
mod cache;
#[cfg(feature = "compiler")]
mod compile;
mod config;
#[cfg(any(feature = "static-artifact-create", feature = "wasmer-artifact-create"))]
mod create_exe;
#[cfg(feature = "static-artifact-create")]
mod create_obj;
#[cfg(feature = "static-artifact-create")]
mod gen_c_header;
mod init;
mod inspect;
mod list;
mod login;
mod logout;
mod publish;
mod publish_website;
mod run;
mod self_update;
mod validate;
#[cfg(feature = "wast")]
mod wast;
mod whoami;

#[cfg(target_os = "linux")]
pub use binfmt::*;
#[cfg(feature = "compiler")]
pub use compile::*;
#[cfg(any(feature = "static-artifact-create", feature = "wasmer-artifact-create"))]
pub use create_exe::*;
use serde::{Deserialize, Serialize};
#[cfg(feature = "wast")]
pub use wast::*;
pub use {
    add::*, cache::*, config::*, init::*, inspect::*, list::*, login::*, logout::*, publish::*,
    publish_website::*, run::*, self_update::*, validate::*, whoami::*,
};
#[cfg(feature = "static-artifact-create")]
pub use {create_obj::*, gen_c_header::*};

/// The kind of object format to emit.
#[derive(Debug, Copy, Clone, PartialEq, Eq, clap::Parser, Serialize, Deserialize)]
pub enum ObjectFormat {
    /// Serialize the entire module into an object file.
    Serialized,
    /// Serialize only the module metadata into an object file and emit functions as symbols.
    Symbols,
}

impl Default for ObjectFormat {
    fn default() -> Self {
        ObjectFormat::Symbols
    }
}

#[cfg(any(feature = "static-artifact-create", feature = "wasmer-artifact-create"))]
impl std::str::FromStr for ObjectFormat {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "serialized" => Ok(Self::Serialized),
            "symbols" => Ok(Self::Symbols),
            _ => Err("must be one of two options: `serialized` or `symbols`."),
        }
    }
}
