use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use anyhow::{Result, ensure};
use wasmer_wasix::virtual_fs::{FsError, Metadata, OpenOptions, ReadDir};

use super::*;

#[derive(Debug, Default)]
struct UnsupportedSymlinkFileSystem {
    inner: mem_fs::FileSystem,
    symlink_requests: Mutex<Vec<(PathBuf, PathBuf)>>,
}

impl FileSystem for UnsupportedSymlinkFileSystem {
    fn readlink(&self, path: &Path) -> Result<PathBuf, FsError> {
        self.inner.readlink(path)
    }

    fn read_dir(&self, path: &Path) -> Result<ReadDir, FsError> {
        self.inner.read_dir(path)
    }

    fn create_dir(&self, path: &Path) -> Result<(), FsError> {
        self.inner.create_dir(path)
    }

    fn create_symlink(&self, source: &Path, target: &Path) -> Result<(), FsError> {
        self.symlink_requests
            .lock()
            .unwrap()
            .push((source.to_path_buf(), target.to_path_buf()));
        Err(FsError::Unsupported)
    }

    fn remove_dir(&self, path: &Path) -> Result<(), FsError> {
        self.inner.remove_dir(path)
    }

    fn rename<'a>(
        &'a self,
        from: &'a Path,
        to: &'a Path,
    ) -> Pin<Box<dyn Future<Output = Result<(), FsError>> + Send + 'a>> {
        self.inner.rename(from, to)
    }

    fn metadata(&self, path: &Path) -> Result<Metadata, FsError> {
        self.inner.metadata(path)
    }

    fn symlink_metadata(&self, path: &Path) -> Result<Metadata, FsError> {
        self.inner.symlink_metadata(path)
    }

    fn remove_file(&self, path: &Path) -> Result<(), FsError> {
        self.inner.remove_file(path)
    }

    fn new_open_options(&self) -> OpenOptions<'_> {
        self.inner.new_open_options()
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
    let backing = Arc::new(UnsupportedSymlinkFileSystem::default());

    let result = runner::run_wasm_with_runner_config(
        &wasm,
        &config.build_path(),
        config.engine,
        None,
        false,
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
    ensure!(
        *backing.symlink_requests.lock().unwrap()
            == [
                (PathBuf::from("target.txt"), PathBuf::from("/link")),
                (PathBuf::from("target.txt"), PathBuf::from("/second")),
                (PathBuf::from("absent.txt"), PathBuf::from("/dangling")),
            ],
        "guest symlink requests did not reach the unsupported backend"
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
