use crate::{FileSystem, VirtualFile};
use std::path::{Path, PathBuf};
use tracing::*;
use wasmer_wasi_types::types::{__WASI_STDERR_FILENO, __WASI_STDIN_FILENO, __WASI_STDOUT_FILENO};

use super::{NullFile, SpecialFile};
use super::{ZeroFile};
use crate::tmp_fs::TmpFileSystem;

pub struct RootFileSystemBuilder {
    default_root_dirs: bool,
    default_dev_files: bool,
    add_wasmer_command: bool,
    stdin: Option<Box<dyn VirtualFile + Send + Sync>>,
    stdout: Option<Box<dyn VirtualFile + Send + Sync>>,
    stderr: Option<Box<dyn VirtualFile + Send + Sync>>,
    tty: Option<Box<dyn VirtualFile + Send + Sync>>,
}

impl Default for RootFileSystemBuilder {
    fn default() -> Self {
        Self {
            default_root_dirs: true,
            default_dev_files: true,
            add_wasmer_command: true,
            stdin: None,
            stdout: None,
            stderr: None,
            tty: None,
        }
    }
}

impl RootFileSystemBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_stdin(mut self, file: Box<dyn VirtualFile + Send + Sync>) -> Self {
        self.stdin.replace(file);
        self
    }

    pub fn with_stdout(mut self, file: Box<dyn VirtualFile + Send + Sync>) -> Self {
        self.stdout.replace(file);
        self
    }

    pub fn with_stderr(mut self, file: Box<dyn VirtualFile + Send + Sync>) -> Self {
        self.stderr.replace(file);
        self
    }

    pub fn with_tty(mut self, file: Box<dyn VirtualFile + Send + Sync>) -> Self {
        self.tty.replace(file);
        self
    }

    pub fn default_root_dirs(mut self, val: bool) -> Self {
        self.default_root_dirs = val;
        self
    }

    pub fn build(self) -> TmpFileSystem {
        let tmp = TmpFileSystem::new();
        if self.default_root_dirs {
            for root_dir in &["/.app", "/.private", "/bin", "/dev", "/etc", "/tmp"] {
                if let Err(err) = tmp.create_dir(Path::new(root_dir)) {
                    debug!("failed to create dir [{}] - {}", root_dir, err);
                }
            }
        }
        if self.add_wasmer_command {
            let _ = tmp
                .new_open_options_ext()
                .insert_custom_file(PathBuf::from("/bin/wasmer"), Box::new(NullFile::default()));
        }
        if self.default_dev_files {
            let _ = tmp
                .new_open_options_ext()
                .insert_custom_file(PathBuf::from("/dev/null"), Box::new(NullFile::default()));
            let _ = tmp
                .new_open_options_ext()
                .insert_custom_file(PathBuf::from("/dev/zero"), Box::new(ZeroFile::default()));
            let _ = tmp.new_open_options_ext().insert_custom_file(
                PathBuf::from("/dev/stdin"),
                self.stdin
                    .unwrap_or_else(|| Box::new(SpecialFile::new(__WASI_STDIN_FILENO))),
            );
            let _ = tmp.new_open_options_ext().insert_custom_file(
                PathBuf::from("/dev/stdout"),
                self.stdout
                    .unwrap_or_else(|| Box::new(SpecialFile::new(__WASI_STDOUT_FILENO))),
            );
            let _ = tmp.new_open_options_ext().insert_custom_file(
                PathBuf::from("/dev/stderr"),
                self.stderr
                    .unwrap_or_else(|| Box::new(SpecialFile::new(__WASI_STDERR_FILENO))),
            );
            let _ = tmp.new_open_options_ext().insert_custom_file(
                PathBuf::from("/dev/tty"),
                self.tty.unwrap_or_else(|| Box::new(NullFile::default())),
            );
        }
        tmp
    }
}

#[test]
fn test_root_file_system() {
    let root_fs = RootFileSystemBuilder::new().build();
    let mut dev_null = root_fs
        .new_open_options()
        .read(true)
        .write(true)
        .open("/dev/null")
        .unwrap();
    assert_eq!(dev_null.write(b"hello").unwrap(), 5);
    let mut buf = Vec::new();
    dev_null.read_to_end(&mut buf);
    assert!(buf.is_empty());
    assert!(dev_null.get_special_fd().is_none());

    let mut dev_zero = root_fs
        .new_open_options()
        .read(true)
        .write(true)
        .open("/dev/zero")
        .unwrap();
    assert_eq!(dev_zero.write(b"hello").unwrap(), 5);
    let mut buf = vec![1; 10];
    dev_zero.read(&mut buf[..]).unwrap();
    assert_eq!(buf, vec![0; 10]);
    assert!(dev_zero.get_special_fd().is_none());

    let mut dev_tty = root_fs
        .new_open_options()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .unwrap();
    assert_eq!(dev_tty.write(b"hello").unwrap(), 5);
    let mut buf = Vec::new();
    dev_tty.read_to_end(&mut buf);
    assert!(buf.is_empty());
    assert!(dev_tty.get_special_fd().is_none());

    root_fs
        .new_open_options()
        .read(true)
        .open("/bin/wasmer")
        .unwrap();

    let dev_stdin = root_fs
        .new_open_options()
        .read(true)
        .write(true)
        .open("/dev/stdin")
        .unwrap();
    assert_eq!(dev_stdin.get_special_fd().unwrap(), 0);
    let dev_stdout = root_fs
        .new_open_options()
        .read(true)
        .write(true)
        .open("/dev/stdout")
        .unwrap();
    assert_eq!(dev_stdout.get_special_fd().unwrap(), 1);
    let dev_stderr = root_fs
        .new_open_options()
        .read(true)
        .write(true)
        .open("/dev/stderr")
        .unwrap();
    assert_eq!(dev_stderr.get_special_fd().unwrap(), 2);
}
