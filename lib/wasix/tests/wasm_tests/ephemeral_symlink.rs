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
    create_on_lookup: Mutex<Option<PathBuf>>,
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
        let metadata = self.inner.symlink_metadata(path);
        let mut pending = self.create_on_lookup.lock().unwrap();
        if pending.as_deref() == Some(path) && matches!(metadata, Err(FsError::EntryNotFound)) {
            self.inner
                .new_open_options()
                .write(true)
                .create_new(true)
                .open(path)?;
            *pending = None;
        }
        metadata
    }

    fn remove_file(&self, path: &Path) -> Result<(), FsError> {
        self.inner.remove_file(path)
    }

    fn new_open_options(&self) -> OpenOptions<'_> {
        self.inner.new_open_options()
    }
}

pub(super) fn fallback_config(
    fixture: &str,
    tests_dir: &Path,
    build_root: &Path,
    sysroot: Option<&'static str>,
) -> Result<Config> {
    let mut config = Config::new(
        PrimarySource::CSourceFile("main.c".to_owned()),
        tests_dir.join("path").join(fixture),
        build_root.to_path_buf(),
        format!("path/{fixture}-fallback"),
    );
    if let Some(sysroot) = sysroot {
        config.set_sysroot(sysroot)?;
    }
    Ok(config)
}

fn run_on_mount(
    config: &Config,
    backing: Arc<dyn FileSystem + Send + Sync>,
) -> Result<runner::WasmRunResult> {
    let wasm = run_build_script(config)?;
    let result = runner::run_wasm_with_runner_config(
        &wasm,
        &config.build_path(),
        config.engine,
        None,
        false,
        |runner| {
            runner.with_mount("/ephemeral".to_owned(), backing);
            runner.with_current_dir("/ephemeral".to_owned());
            Ok(())
        },
    )?;

    ensure!(
        result.exit_code == 0,
        "fallback fixture failed\n{}",
        runner::format_captured_output(&result)
    );
    Ok(result)
}

pub(super) fn run_whiteout_test(config: Config) -> Result<()> {
    let lower = mem_fs::FileSystem::default();
    lower
        .new_open_options()
        .write(true)
        .create_new(true)
        .open("/notes")?;
    let overlay = wasmer_wasix::virtual_fs::OverlayFileSystem::new(
        UnsupportedSymlinkFileSystem::default(),
        [lower],
    );
    let result = run_on_mount(&config, Arc::new(overlay))?;
    ensure!(String::from_utf8_lossy(&result.stdout).trim() == "ok");
    Ok(())
}

pub(super) fn run_fallback_test(config: Config, exclusive_create: bool) -> Result<()> {
    let backing = Arc::new(UnsupportedSymlinkFileSystem::default());
    if exclusive_create {
        *backing.create_on_lookup.lock().unwrap() = Some(PathBuf::from("/occupied"));
    }

    let result = run_on_mount(&config, backing.clone())?;
    if exclusive_create {
        ensure!(
            backing.create_on_lookup.lock().unwrap().is_none(),
            "the lookup did not trigger the backing create"
        );
        ensure!(String::from_utf8_lossy(&result.stdout).trim() == "ok");
        return Ok(());
    }
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
