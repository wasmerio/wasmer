//! The commands available in the Wasmer binary.
mod add;
#[cfg(target_os = "linux")]
mod binfmt;
mod cache;
#[cfg(feature = "compiler")]
mod compile;
mod config;
mod container;
#[cfg(any(feature = "static-artifact-create", feature = "wasmer-artifact-create"))]
mod create_exe;
#[cfg(feature = "static-artifact-create")]
mod create_obj;
#[cfg(feature = "static-artifact-create")]
mod gen_c_header;
mod init;
mod inspect;
#[cfg(feature = "journal")]
mod journal;
mod login;
mod package;
mod publish;
mod run;
mod self_update;
mod validate;
#[cfg(feature = "wast")]
mod wast;
mod whoami;

pub use self::{
    add::*, cache::*, config::*, container::*, init::*, inspect::*, journal::*, login::*,
    package::*, publish::*, run::Run, self_update::*, validate::*, whoami::*,
};

#[cfg(target_os = "linux")]
pub use binfmt::*;
#[cfg(feature = "compiler")]
pub use compile::*;
#[cfg(any(feature = "static-artifact-create", feature = "wasmer-artifact-create"))]
pub use create_exe::*;
#[cfg(feature = "wast")]
pub use wast::*;
#[cfg(feature = "static-artifact-create")]
pub use {create_obj::*, gen_c_header::*};
