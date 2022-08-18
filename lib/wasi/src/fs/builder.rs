use std::path::{Path, PathBuf};
use tracing::*;
use wasmer_vfs::{FileSystem, VirtualFile};
use wasmer_wasi_types::{__WASI_STDIN_FILENO, __WASI_STDOUT_FILENO, __WASI_STDERR_FILENO};

use super::{TmpFileSystem, ZeroFile};
use super::{
    NullFile,
    SpecialFile
};

pub struct RootFileSystemBuilder
{
    default_root_dirs: bool,
    default_dev_files: bool,
    add_wasmer_command: bool,
    stdin: Option<Box<dyn VirtualFile + Send + Sync>>,
    stdout: Option<Box<dyn VirtualFile + Send + Sync>>,
    stderr: Option<Box<dyn VirtualFile + Send + Sync>>,
    tty: Option<Box<dyn VirtualFile + Send + Sync>>,
}

impl RootFileSystemBuilder
{
    pub fn new() -> Self {
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
            for root_dir in vec![
                "/.app",
                "/.private",
                "/bin",
                "/dev",
                "/etc",
                "/tmp"
            ] {
                if let Err(err) = tmp.create_dir(&Path::new(root_dir)) {
                    debug!("failed to create dir [{}] - {}", root_dir, err);
                }
            }
        }
        if self.add_wasmer_command {
            let _ = tmp.new_open_options_ext()
                .insert_custom_file(PathBuf::from("/bin/wasmer"), Box::new(NullFile::default()));
        }
        if self.default_dev_files {
            let _ = tmp.new_open_options_ext()
                .insert_custom_file(PathBuf::from("/dev/null"), Box::new(NullFile::default()));
            let _ = tmp.new_open_options_ext()
                .insert_custom_file(PathBuf::from("/dev/zero"), Box::new(ZeroFile::default()));
            let _ = tmp.new_open_options_ext()
                .insert_custom_file(PathBuf::from("/dev/stdin"), self.stdin.unwrap_or_else(|| Box::new(SpecialFile::new(__WASI_STDIN_FILENO))));
            let _ = tmp.new_open_options_ext()
                .insert_custom_file(PathBuf::from("/dev/stdout"), self.stdout.unwrap_or_else(|| Box::new(SpecialFile::new(__WASI_STDOUT_FILENO))));
            let _ = tmp.new_open_options_ext()
                .insert_custom_file(PathBuf::from("/dev/stderr"), self.stderr.unwrap_or_else(|| Box::new(SpecialFile::new(__WASI_STDERR_FILENO))));
            let _ = tmp.new_open_options_ext()
                .insert_custom_file(PathBuf::from("/dev/tty"), self.tty.unwrap_or_else(|| Box::new(NullFile::default())));
        }
        tmp
    }
}