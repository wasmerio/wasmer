use std::{
    env, fs,
    io::Write,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use clap::Parser;
use Action::*;

#[derive(Debug, Parser, Clone, Copy)]
enum Action {
    /// Register wasmer as binfmt interpreter
    Register,
    /// Unregister a binfmt interpreter for wasm32
    Unregister,
    /// Soft unregister, and register
    Reregister,
}

/// Unregister and/or register wasmer as binfmt interpreter
///
/// Check the wasmer repository for a systemd service definition example
/// to automate the process at start-up.
#[derive(Debug, Parser)]
pub struct Binfmt {
    // Might be better to traverse the mount list
    /// Mount point of binfmt_misc fs
    #[clap(long, default_value = "/proc/sys/fs/binfmt_misc/")]
    binfmt_misc: PathBuf,

    #[clap(subcommand)]
    action: Action,
}

// Quick safety check:
// This folder isn't world writeable (or else its sticky bit is set), and neither are its parents.
//
// If somebody mounted /tmp wrong, this might result in a TOCTOU problem.
fn seccheck(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        seccheck(parent)?;
    }
    let m = std::fs::metadata(path)
        .with_context(|| format!("Can't check permissions of {}", path.to_string_lossy()))?;
    use unix_mode::*;
    anyhow::ensure!(
        !is_allowed(Accessor::Other, Access::Write, m.mode()) || is_sticky(m.mode()),
        "{} is world writeable and not sticky",
        path.to_string_lossy()
    );
    Ok(())
}

impl Binfmt {
    /// The filename used to register the wasmer CLI as a binfmt interpreter.
    pub const FILENAME: &'static str = "wasmer-binfmt-interpreter";

    /// execute [Binfmt]
    pub fn execute(&self) -> Result<()> {
        if !self.binfmt_misc.exists() {
            panic!("{} does not exist", self.binfmt_misc.to_string_lossy());
        }
        let temp_dir;
        let specs = match self.action {
            Register | Reregister => {
                temp_dir = tempfile::tempdir().context("Make temporary directory")?;
                seccheck(temp_dir.path())?;
                let bin_path_orig: PathBuf = env::current_exe()
                    .and_then(|p| p.canonicalize())
                    .context("Cannot get path to wasmer executable")?;
                let bin_path = temp_dir.path().join(Binfmt::FILENAME);
                fs::copy(bin_path_orig, &bin_path).context("Copy wasmer binary to temp folder")?;
                let bin_path = fs::canonicalize(&bin_path).with_context(|| {
                    format!(
                        "Couldn't get absolute path for {}",
                        bin_path.to_string_lossy()
                    )
                })?;
                Some([
                    [
                        b":wasm32:M::\\x00asm\\x01\\x00\\x00::".as_ref(),
                        bin_path.as_os_str().as_bytes(),
                        b":PFC",
                    ]
                    .concat(),
                    [
                        b":wasm32-wat:E::wat::".as_ref(),
                        bin_path.as_os_str().as_bytes(),
                        b":PFC",
                    ]
                    .concat(),
                ])
            }
            _ => None,
        };
        let wasm_registration = self.binfmt_misc.join("wasm32");
        let wat_registration = self.binfmt_misc.join("wasm32-wat");
        match self.action {
            Reregister | Unregister => {
                let unregister = [wasm_registration, wat_registration]
                    .iter()
                    .map(|registration| {
                        if registration.exists() {
                            let mut registration = fs::OpenOptions::new()
                                .write(true)
                                .open(registration)
                                .context("Open existing binfmt entry to remove")?;
                            registration
                                .write_all(b"-1")
                                .context("Couldn't write binfmt unregister request")?;
                            Ok(true)
                        } else {
                            eprintln!(
                                "Warning: {} does not exist, not unregistered.",
                                registration.to_string_lossy()
                            );
                            Ok(false)
                        }
                    })
                    .collect::<Vec<_>>()
                    .into_iter()
                    .collect::<Result<Vec<_>>>()?;
                if let (Unregister, false) = (self.action, unregister.into_iter().any(|b| b)) {
                    bail!("Nothing unregistered");
                }
            }
            _ => (),
        };
        if let Some(specs) = specs {
            if cfg!(target_env = "gnu") {
                // Approximate. ELF parsing for a proper check feels like overkill here.
                eprintln!("Warning: wasmer has been compiled for glibc, and is thus likely dynamically linked. Invoking wasm binaries in chroots or mount namespaces (lxc, docker, ...) may not work.");
            }
            specs
                .iter()
                .map(|spec| {
                    let register = self.binfmt_misc.join("register");
                    let mut register = fs::OpenOptions::new()
                        .write(true)
                        .open(register)
                        .context("Open binfmt misc for registration")?;
                    register
                        .write_all(spec)
                        .context("Couldn't register binfmt")?;
                    Ok(())
                })
                .collect::<Vec<_>>()
                .into_iter()
                .collect::<Result<Vec<_>>>()?;
        }
        Ok(())
    }
}
