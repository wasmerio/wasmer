use structopt::StructOpt;
use anyhow::{Context, Result};
use std::env;
use std::path::{PathBuf, Path};
use std::fs;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use Action::*;

#[derive(StructOpt)]
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
#[derive(StructOpt)]
pub struct Binfmt {
    // Might be better to traverse the mount list
    /// Mount point of binfmt_misc fs
    #[structopt(default_value = "/proc/sys/fs/binfmt_misc/")]
    binfmt_misc: PathBuf,

    #[structopt(subcommand)]
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
    anyhow::ensure!(m.mode() & 0o2 == 0 || m.mode() & 0o1000 != 0, "{} is world writeable and not sticky", path.to_string_lossy());
    Ok(())
}

impl Binfmt {
    /// execute [Binfmt]
    pub fn execute(&self) -> Result<()> {
        if !self.binfmt_misc.exists() {
            panic!("{} does not exist", self.binfmt_misc.to_string_lossy());
        }
        let temp_dir;
        let spec = match self.action {
            Register | Reregister => {
                temp_dir = tempfile::tempdir().context("Make temporary directory")?;
                seccheck(temp_dir.path())?;
                let bin_path_orig: PathBuf = env::args_os().nth(0).map(Into::into).filter(|p: &PathBuf| p.exists())
                    .context("Cannot get path to wasmer executable")?;
                let bin_path = temp_dir.path().join("wasmer-binfmt-interpreter");
                fs::copy(&bin_path_orig, &bin_path)
                    .context("Copy wasmer binary to temp folder")?;
                let bin_path = fs::canonicalize(&bin_path)
                    .with_context(|| format!("Couldn't get absolute path for {}", bin_path.to_string_lossy()))?;
                Some([b":wasm32:M::\\x00asm\\x01\\x00\\x00::".as_ref(), bin_path.as_os_str().as_bytes(), b":PFC"].concat())
            },
            _ => None
        };
        let registration = self.binfmt_misc.join("wasm32");
        let registration_exists = registration.exists();
        match self.action {
            Unregister if !registration_exists => {
                bail!("Cannot unregister binfmt, not registered");
            },
            Reregister | Unregister if registration.exists() => {
                let mut registration = fs::OpenOptions::new()
                    .write(true)
                    .open(registration).context("Open existing binfmt entry to remove")?;
                registration.write_all(b"-1").context("Couldn't write binfmt unregister request")?;
            },
            _ => (),
        };
        if cfg!(target_env = "gnu") {
            // Approximate. ELF parsing for a proper check feels like overkill here.
            eprintln!("Warning: wasmer has been compiled for glibc, and is thus likely dynamically linked. Invoking wasm binaries in chroots or mount namespaces (lxc, docker, ...) may not work.");
        }
        if let Some(spec) = spec {
            let register = self.binfmt_misc.join("register");
            let mut register = fs::OpenOptions::new()
                .write(true)
                .open(register).context("Open binfmt misc for registration")?;
            register.write_all(&spec).context("Couldn't register binfmt")?;
        }
        Ok(())
    }
}
