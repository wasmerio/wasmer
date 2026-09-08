use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{Result, ensure};
use wasmer_wasix::virtual_fs::{FsError, Metadata, OpenOptions, ReadDir};

use super::*;

#[derive(Debug)]
struct UnsupportedSymlinkFileSystem(mem_fs::FileSystem);

impl FileSystem for UnsupportedSymlinkFileSystem {
    fn readlink(&self, path: &Path) -> Result<PathBuf, FsError> {
        self.0.readlink(path)
    }

    fn read_dir(&self, path: &Path) -> Result<ReadDir, FsError> {
        self.0.read_dir(path)
    }

    fn create_dir(&self, path: &Path) -> Result<(), FsError> {
        self.0.create_dir(path)
    }

    fn remove_dir(&self, path: &Path) -> Result<(), FsError> {
        self.0.remove_dir(path)
    }

    fn rename<'a>(
        &'a self,
        from: &'a Path,
        to: &'a Path,
    ) -> Pin<Box<dyn Future<Output = Result<(), FsError>> + Send + 'a>> {
        self.0.rename(from, to)
    }

    fn metadata(&self, path: &Path) -> Result<Metadata, FsError> {
        self.0.metadata(path)
    }

    fn symlink_metadata(&self, path: &Path) -> Result<Metadata, FsError> {
        self.0.symlink_metadata(path)
    }

    fn remove_file(&self, path: &Path) -> Result<(), FsError> {
        self.0.remove_file(path)
    }

    fn new_open_options(&self) -> OpenOptions<'_> {
        self.0.new_open_options()
    }
}

pub(super) fn fallback_metadata_config(
    tests_dir: &Path,
    build_root: &Path,
    sysroot: Option<&'static str>,
) -> Result<Config> {
    let mut config = Config::new(
        PrimarySource::CSourceFile("main.c".to_owned()),
        tests_dir.join("path/ephemeral-symlink-metadata"),
        build_root.to_path_buf(),
        "path/ephemeral-symlink-metadata-fallback".to_owned(),
    );
    if let Some(sysroot) = sysroot {
        config.set_sysroot(sysroot)?;
    }
    Ok(config)
}

pub(super) fn run_fallback_metadata_test(config: Config) -> Result<()> {
    let wasm = run_build_script(&config)?;
    let backing = Arc::new(UnsupportedSymlinkFileSystem(mem_fs::FileSystem::default()));
    ensure!(matches!(
        backing.create_symlink(Path::new("target.txt"), Path::new("/probe")),
        Err(FsError::Unsupported)
    ));

    let result = runner::run_wasm_with_runner_config(
        &wasm,
        &config.build_path(),
        config.engine,
        None,
        true,
        |runner| {
            runner.with_mount("/ephemeral".to_owned(), backing.clone());
            runner.with_current_dir("/ephemeral".to_owned());
            Ok(())
        },
    )?;

    ensure!(
        result.exit_code == 0,
        "ephemeral symlink fallback fixture failed\n{}",
        runner::format_captured_output(&result)
    );
    ensure!(
        String::from_utf8_lossy(&result.stdout).trim() == "ephemeral symlink metadata passed",
        "unexpected fixture output\n{}",
        runner::format_captured_output(&result)
    );
    ensure!(backing.metadata(Path::new("/target.txt"))?.is_file());
    ensure!(matches!(
        backing.symlink_metadata(Path::new("/link")),
        Err(FsError::EntryNotFound)
    ));
    ensure!(matches!(
        backing.symlink_metadata(Path::new("/second")),
        Err(FsError::EntryNotFound)
    ));
    ensure!(matches!(
        backing.symlink_metadata(Path::new("/dangling")),
        Err(FsError::EntryNotFound)
    ));

    Ok(())
}
