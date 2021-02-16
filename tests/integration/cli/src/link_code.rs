use crate::assets::*;
use anyhow::bail;
use std::path::PathBuf;
use std::process::Command;

/// Data used to run a linking command for generated artifacts.
#[derive(Debug)]
pub struct LinkCode {
    /// The directory to operate in.
    pub current_dir: PathBuf,
    /// Path to the linker used to run the linking command.
    pub linker_path: PathBuf,
    /// String used as an optimization flag.
    pub optimization_flag: String,
    /// Paths of objects to link.
    pub object_paths: Vec<PathBuf>,
    /// Path to the output target.
    pub output_path: PathBuf,
    /// Path to the static libwasmer library.
    pub libwasmer_path: PathBuf,
}

impl Default for LinkCode {
    fn default() -> Self {
        #[cfg(not(windows))]
        let linker = "cc";
        #[cfg(windows)]
        let linker = "clang";
        Self {
            current_dir: std::env::current_dir().unwrap(),
            linker_path: PathBuf::from(linker),
            optimization_flag: String::from("-O2"),
            object_paths: vec![],
            output_path: PathBuf::from("a.out"),
            libwasmer_path: get_libwasmer_path(),
        }
    }
}

impl LinkCode {
    pub fn run(&self) -> anyhow::Result<()> {
        let mut command = Command::new(&self.linker_path);
        let command = command
            .current_dir(&self.current_dir)
            .arg(&self.optimization_flag)
            .args(
                self.object_paths
                    .iter()
                    .map(|path| path.canonicalize().unwrap()),
            )
            .arg(&self.libwasmer_path.canonicalize()?);
        #[cfg(windows)]
        let command = command
            .arg("-luserenv")
            .arg("-lWs2_32")
            .arg("-ladvapi32")
            .arg("-lbcrypt");
        #[cfg(not(windows))]
        let command = command.arg("-ldl").arg("-lm").arg("-pthread");
        let output = command.arg("-o").arg(&self.output_path).output()?;

        if !output.status.success() {
            bail!(
                "linking failed with: stdout: {}\n\nstderr: {}",
                std::str::from_utf8(&output.stdout)
                    .expect("stdout is not utf8! need to handle arbitrary bytes"),
                std::str::from_utf8(&output.stderr)
                    .expect("stderr is not utf8! need to handle arbitrary bytes")
            );
        }
        Ok(())
    }
}
