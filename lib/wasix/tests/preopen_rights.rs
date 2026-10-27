//! Per-preopen `read`/`write`/`create` settings from `PreopenDirBuilder` must
//! hold no matter how a guest reaches the directory: through the virtual root
//! fd, an absolute path, or `..` from a sibling preopen.
#![cfg(all(feature = "host-fs", not(target_arch = "wasm32")))]

use std::path::PathBuf;
use std::sync::Arc;

use wasmer::{Instance, Memory, Module, Store, Value};
use wasmer_wasix::WasiEnvBuilder;
use wasmer_wasix::virtual_fs::{FileSystem, host_fs};
use wasmer_wasix::wasmer_wasix_types::types::__WASI_LOOKUP_SYMLINK_FOLLOW;
use wasmer_wasix::wasmer_wasix_types::wasi::{Errno, Fstflags, Oflags, Rights};

const VIRTUAL_ROOT_FD: i32 = 3;
const TARGET_CONTENTS: &[u8] = b"read-only input contents";
const SECRET_CONTENTS: &[u8] = b"TOP-SECRET";

const PATH_A: u32 = 1024;
const PATH_B: u32 = 2048;
const OUT: u32 = 4096;
const BUF: u32 = 8192;
const BUF_LEN: u32 = 4096;

const GUEST: &str = r#"
(module
  (import "wasi_snapshot_preview1" "fd_prestat_get" (func $fd_prestat_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_prestat_dir_name" (func $fd_prestat_dir_name (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_read" (func $fd_read (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_readdir" (func $fd_readdir (param i32 i32 i32 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_open" (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_create_directory" (func $path_create_directory (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_remove_directory" (func $path_remove_directory (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_unlink_file" (func $path_unlink_file (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_rename" (func $path_rename (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_symlink" (func $path_symlink (param i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_link" (func $path_link (param i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_filestat_get" (func $path_filestat_get (param i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_filestat_set_times" (func $path_filestat_set_times (param i32 i32 i32 i32 i64 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_readlink" (func $path_readlink (param i32 i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (export "fd_prestat_get" (func $fd_prestat_get))
  (export "fd_prestat_dir_name" (func $fd_prestat_dir_name))
  (export "fd_read" (func $fd_read))
  (export "fd_readdir" (func $fd_readdir))
  (export "path_open" (func $path_open))
  (export "path_create_directory" (func $path_create_directory))
  (export "path_remove_directory" (func $path_remove_directory))
  (export "path_unlink_file" (func $path_unlink_file))
  (export "path_rename" (func $path_rename))
  (export "path_symlink" (func $path_symlink))
  (export "path_link" (func $path_link))
  (export "path_filestat_get" (func $path_filestat_get))
  (export "path_filestat_set_times" (func $path_filestat_set_times))
  (export "path_readlink" (func $path_readlink))
)
"#;

/// A guest with three preopens: `input` is read-only, `private` is
/// write-only without create, and `output` has every permission.
struct Guest {
    runtime: tokio::runtime::Runtime,
    root: tempfile::TempDir,
    store: Store,
    instance: Instance,
    memory: Memory,
}

impl Guest {
    fn new() -> Self {
        Self::with_preopens(|builder| {
            builder
                .preopen_build(|p| p.directory("/input").alias("input").read(true))
                .unwrap()
                .preopen_build(|p| p.directory("/private").alias("private").write(true))
                .unwrap()
                .preopen_build(|p| {
                    p.directory("/output")
                        .alias("output")
                        .read(true)
                        .write(true)
                        .create(true)
                })
                .unwrap()
        })
    }

    /// Builds the host layout of [`Guest::new`] with different preopens.
    fn with_preopens(preopens: impl FnOnce(WasiEnvBuilder) -> WasiEnvBuilder) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = runtime.enter();

        let root = tempfile::tempdir().unwrap();
        let host = root.path();
        for dir in ["input/sub", "private", "output"] {
            std::fs::create_dir_all(host.join(dir)).unwrap();
        }
        std::fs::write(host.join("input/target.txt"), TARGET_CONTENTS).unwrap();
        std::fs::write(host.join("private/secret.txt"), SECRET_CONTENTS).unwrap();
        std::fs::write(host.join("output/mine.txt"), b"mine").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("secret.txt", host.join("private/link")).unwrap();

        let fs = host_fs::FileSystem::new(runtime.handle().clone(), host).unwrap();
        let mut store = Store::default();
        let module = Module::new(&store, GUEST).unwrap();
        let builder = WasiEnvBuilder::new("preopen-rights")
            .engine(store.engine().clone())
            .fs(Arc::new(fs) as Arc<dyn FileSystem + Send + Sync>);
        let (instance, _env) = preopens(builder).instantiate(module, &mut store).unwrap();
        let memory = instance.exports.get_memory("memory").unwrap().clone();

        Self {
            runtime,
            root,
            store,
            instance,
            memory,
        }
    }

    fn host(&self, path: &str) -> PathBuf {
        self.root.path().join(path)
    }

    fn call(&mut self, name: &str, args: &[Value]) -> Errno {
        let _guard = self.runtime.enter();
        let func = self.instance.exports.get_function(name).unwrap().clone();
        let result = func.call(&mut self.store, args).unwrap();
        Errno::try_from(result[0].unwrap_i32() as u16).unwrap()
    }

    fn write(&self, offset: u32, bytes: &[u8]) -> i32 {
        self.memory
            .view(&self.store)
            .write(offset as u64, bytes)
            .unwrap();
        bytes.len() as i32
    }

    fn read_u32(&self, offset: u32) -> u32 {
        let mut bytes = [0u8; 4];
        self.memory
            .view(&self.store)
            .read(offset as u64, &mut bytes)
            .unwrap();
        u32::from_le_bytes(bytes)
    }

    /// Finds a preopen by name the way wasi-libc does, via `fd_prestat_*`.
    fn preopen(&mut self, name: &str) -> i32 {
        for fd in 3..32 {
            if self.call("fd_prestat_get", &[Value::I32(fd), Value::I32(OUT as i32)])
                != Errno::Success
            {
                continue;
            }
            let len = self.read_u32(OUT + 4);
            assert_eq!(
                self.call(
                    "fd_prestat_dir_name",
                    &[
                        Value::I32(fd),
                        Value::I32(BUF as i32),
                        Value::I32(len as i32)
                    ]
                ),
                Errno::Success
            );
            let mut dir_name = vec![0u8; len as usize];
            self.memory
                .view(&self.store)
                .read(BUF as u64, &mut dir_name)
                .unwrap();
            if String::from_utf8_lossy(&dir_name).trim_matches('/') == name {
                return fd;
            }
        }
        panic!("no preopen named {name}");
    }

    fn path_open(&mut self, fd: i32, path: &str, oflags: Oflags, rights: Rights) -> Errno {
        let len = self.write(PATH_A, path.as_bytes());
        self.call(
            "path_open",
            &[
                Value::I32(fd),
                Value::I32(__WASI_LOOKUP_SYMLINK_FOLLOW as i32),
                Value::I32(PATH_A as i32),
                Value::I32(len),
                Value::I32(oflags.bits() as i32),
                Value::I64(rights.bits() as i64),
                Value::I64(0i64),
                Value::I32(0i32),
                Value::I32(OUT as i32),
            ],
        )
    }

    fn opened_fd(&self) -> i32 {
        self.read_u32(OUT) as i32
    }

    fn fd_read(&mut self, fd: i32) -> Errno {
        let mut iovec = BUF.to_le_bytes().to_vec();
        iovec.extend_from_slice(&BUF_LEN.to_le_bytes());
        self.write(OUT + 16, &iovec);
        self.call(
            "fd_read",
            &[
                Value::I32(fd),
                Value::I32((OUT + 16) as i32),
                Value::I32(1),
                Value::I32(OUT as i32),
            ],
        )
    }

    fn fd_readdir(&mut self, fd: i32) -> Errno {
        self.call(
            "fd_readdir",
            &[
                Value::I32(fd),
                Value::I32(BUF as i32),
                Value::I32(BUF_LEN as i32),
                Value::I64(0i64),
                Value::I32(OUT as i32),
            ],
        )
    }

    fn path_call(&mut self, name: &str, fd: i32, path: &str) -> Errno {
        let len = self.write(PATH_A, path.as_bytes());
        self.call(
            name,
            &[Value::I32(fd), Value::I32(PATH_A as i32), Value::I32(len)],
        )
    }

    fn path_rename(&mut self, old: (i32, &str), new: (i32, &str)) -> Errno {
        let old_len = self.write(PATH_A, old.1.as_bytes());
        let new_len = self.write(PATH_B, new.1.as_bytes());
        self.call(
            "path_rename",
            &[
                Value::I32(old.0),
                Value::I32(PATH_A as i32),
                Value::I32(old_len),
                Value::I32(new.0),
                Value::I32(PATH_B as i32),
                Value::I32(new_len),
            ],
        )
    }

    fn path_symlink(&mut self, target: &str, at: (i32, &str)) -> Errno {
        let target_len = self.write(PATH_A, target.as_bytes());
        let at_len = self.write(PATH_B, at.1.as_bytes());
        self.call(
            "path_symlink",
            &[
                Value::I32(PATH_A as i32),
                Value::I32(target_len),
                Value::I32(at.0),
                Value::I32(PATH_B as i32),
                Value::I32(at_len),
            ],
        )
    }

    fn path_link(&mut self, old: (i32, &str), new: (i32, &str)) -> Errno {
        let old_len = self.write(PATH_A, old.1.as_bytes());
        let new_len = self.write(PATH_B, new.1.as_bytes());
        self.call(
            "path_link",
            &[
                Value::I32(old.0),
                Value::I32(0i32),
                Value::I32(PATH_A as i32),
                Value::I32(old_len),
                Value::I32(new.0),
                Value::I32(PATH_B as i32),
                Value::I32(new_len),
            ],
        )
    }

    fn path_filestat_get(&mut self, fd: i32, path: &str) -> Errno {
        let len = self.write(PATH_A, path.as_bytes());
        self.call(
            "path_filestat_get",
            &[
                Value::I32(fd),
                Value::I32(0i32),
                Value::I32(PATH_A as i32),
                Value::I32(len),
                Value::I32(BUF as i32),
            ],
        )
    }

    fn path_filestat_set_mtime(&mut self, fd: i32, path: &str, mtime_ns: u64) -> Errno {
        let len = self.write(PATH_A, path.as_bytes());
        self.call(
            "path_filestat_set_times",
            &[
                Value::I32(fd),
                Value::I32(0i32),
                Value::I32(PATH_A as i32),
                Value::I32(len),
                Value::I64(0i64),
                Value::I64(mtime_ns as i64),
                Value::I32(Fstflags::SET_MTIM.bits() as i32),
            ],
        )
    }

    fn path_readlink(&mut self, fd: i32, path: &str) -> Errno {
        let len = self.write(PATH_A, path.as_bytes());
        self.call(
            "path_readlink",
            &[
                Value::I32(fd),
                Value::I32(PATH_A as i32),
                Value::I32(len),
                Value::I32(BUF as i32),
                Value::I32(BUF_LEN as i32),
                Value::I32(OUT as i32),
            ],
        )
    }
}

/// The three ways to name a path inside another preopen.
#[derive(Clone, Copy)]
enum Via {
    VirtualRoot,
    AbsolutePath,
    ParentTraversal,
}

impl Via {
    /// Names `path`, given relative to the virtual root, from the writable
    /// `output` preopen or the virtual root fd.
    fn at(self, guest: &mut Guest, path: &str) -> (i32, String) {
        match self {
            Via::VirtualRoot => (VIRTUAL_ROOT_FD, path.to_owned()),
            Via::AbsolutePath => (guest.preopen("output"), format!("/{path}")),
            Via::ParentTraversal => (guest.preopen("output"), format!("../{path}")),
        }
    }
}

/// Generates one test per [`Via`] from a `|guest, via|` body.
macro_rules! via_tests {
    ($name:ident, |$guest:ident, $via:ident| $body:block) => {
        mod $name {
            use super::*;

            fn check($via: Via) {
                let mut $guest = Guest::new();
                let $guest = &mut $guest;
                $body
            }

            #[test]
            fn via_virtual_root() {
                check(Via::VirtualRoot);
            }

            #[test]
            fn via_absolute_path() {
                check(Via::AbsolutePath);
            }

            #[test]
            fn via_parent_traversal() {
                check(Via::ParentTraversal);
            }
        }
    };
}

/// `path_remove_directory` only sees directories already in the inode cache,
/// so resolve `input/sub` through the permitted `input` fd first.
fn cache_input_sub(guest: &mut Guest) {
    let input = guest.preopen("input");
    assert_eq!(guest.path_filestat_get(input, "sub"), Errno::Success);
}

/// Denials must come from a rights check, not an unrelated resolution failure.
fn assert_denied(errno: Errno, what: &str) {
    assert!(
        matches!(errno, Errno::Access | Errno::Xdev),
        "{what} returned {errno:?}, expected Access or Xdev"
    );
}

fn assert_target_untouched(guest: &Guest) {
    assert_eq!(
        std::fs::read(guest.host("input/target.txt")).unwrap(),
        TARGET_CONTENTS
    );
}

fn assert_absent(guest: &Guest, path: &str) {
    assert!(
        std::fs::symlink_metadata(guest.host(path)).is_err(),
        "{path} exists on the host"
    );
}

via_tests!(path_open_truncate_read_only, |guest, via| {
    let (fd, path) = via.at(guest, "input/target.txt");
    let errno = guest.path_open(fd, &path, Oflags::TRUNC, Rights::FD_WRITE);
    assert_denied(errno, "truncating input/target.txt");
    assert_target_untouched(guest);
});

via_tests!(path_open_create_in_read_only, |guest, via| {
    let (fd, path) = via.at(guest, "input/new.txt");
    let errno = guest.path_open(fd, &path, Oflags::CREATE, Rights::FD_WRITE);
    assert_denied(errno, "creating input/new.txt");
    assert_absent(guest, "input/new.txt");
});

via_tests!(path_open_read_write_only, |guest, via| {
    let (fd, path) = via.at(guest, "private/secret.txt");
    // Either the open is refused, or it yields an fd that cannot read.
    match guest.path_open(fd, &path, Oflags::empty(), Rights::FD_READ) {
        Errno::Success => {
            let opened = guest.opened_fd();
            assert_denied(guest.fd_read(opened), "reading private/secret.txt");
        }
        errno => assert_denied(errno, "opening private/secret.txt for reading"),
    }
});

via_tests!(fd_readdir_write_only, |guest, via| {
    let (fd, path) = via.at(guest, "private");
    // Either the open is refused, or it yields an fd that cannot list.
    match guest.path_open(fd, &path, Oflags::DIRECTORY, Rights::FD_READDIR) {
        Errno::Success => {
            let opened = guest.opened_fd();
            assert_denied(guest.fd_readdir(opened), "listing private");
        }
        errno => assert_denied(errno, "opening private for listing"),
    }
});

via_tests!(path_create_directory_in_read_only, |guest, via| {
    let (fd, path) = via.at(guest, "input/newdir");
    let errno = guest.path_call("path_create_directory", fd, &path);
    assert_denied(errno, "creating input/newdir");
    assert_absent(guest, "input/newdir");
});

via_tests!(path_remove_directory_in_read_only, |guest, via| {
    cache_input_sub(guest);
    let (fd, path) = via.at(guest, "input/sub");
    let errno = guest.path_call("path_remove_directory", fd, &path);
    assert_denied(errno, "removing input/sub");
    assert!(guest.host("input/sub").is_dir());
});

via_tests!(path_unlink_file_in_read_only, |guest, via| {
    let (fd, path) = via.at(guest, "input/target.txt");
    let errno = guest.path_call("path_unlink_file", fd, &path);
    assert_denied(errno, "unlinking input/target.txt");
    assert_target_untouched(guest);
});

via_tests!(path_rename_within_read_only, |guest, via| {
    let (old_fd, old_path) = via.at(guest, "input/target.txt");
    let (new_fd, new_path) = via.at(guest, "input/renamed.txt");
    let errno = guest.path_rename((old_fd, &old_path), (new_fd, &new_path));
    assert_denied(errno, "renaming inside input");
    assert_target_untouched(guest);
    assert_absent(guest, "input/renamed.txt");
});

via_tests!(path_rename_into_read_only, |guest, via| {
    let output = guest.preopen("output");
    let (new_fd, new_path) = via.at(guest, "input/planted.txt");
    let errno = guest.path_rename((output, "mine.txt"), (new_fd, &new_path));
    assert_denied(errno, "renaming output/mine.txt into input");
    assert_absent(guest, "input/planted.txt");
});

via_tests!(path_rename_out_of_write_only, |guest, via| {
    let output = guest.preopen("output");
    let (old_fd, old_path) = via.at(guest, "private/secret.txt");
    let errno = guest.path_rename((old_fd, &old_path), (output, "leak.txt"));
    assert_denied(errno, "moving private/secret.txt into output");
    assert_eq!(
        std::fs::read(guest.host("private/secret.txt")).unwrap(),
        SECRET_CONTENTS
    );
    assert_absent(guest, "output/leak.txt");
});

via_tests!(path_symlink_in_read_only, |guest, via| {
    let (fd, path) = via.at(guest, "input/link");
    let errno = guest.path_symlink("target.txt", (fd, &path));
    assert_denied(errno, "creating symlink input/link");
    assert_absent(guest, "input/link");
});

via_tests!(path_link_into_read_only, |guest, via| {
    let output = guest.preopen("output");
    let (new_fd, new_path) = via.at(guest, "input/hard");
    let errno = guest.path_link((output, "mine.txt"), (new_fd, &new_path));
    // `host_fs` has no hard links, so a granted link exists only in the
    // guest's namespace and the host cannot be checked.
    assert_denied(errno, "hard-linking into input");
});

via_tests!(path_link_out_of_write_only, |guest, via| {
    let output = guest.preopen("output");
    let (old_fd, old_path) = via.at(guest, "private/secret.txt");
    let errno = guest.path_link((old_fd, &old_path), (output, "leak.txt"));
    assert_denied(errno, "hard-linking private/secret.txt into output");
});

via_tests!(path_filestat_set_times_in_read_only, |guest, via| {
    let before = std::fs::metadata(guest.host("input/target.txt"))
        .unwrap()
        .modified()
        .unwrap();
    let (fd, path) = via.at(guest, "input/target.txt");
    let errno = guest.path_filestat_set_mtime(fd, &path, 1_000_000_000_000_000_000);
    assert_denied(errno, "setting mtime on input/target.txt");
    let after = std::fs::metadata(guest.host("input/target.txt"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(before, after);
});

via_tests!(path_filestat_get_in_write_only, |guest, via| {
    let (fd, path) = via.at(guest, "private/secret.txt");
    let errno = guest.path_filestat_get(fd, &path);
    assert_denied(errno, "stat of private/secret.txt");
});

#[cfg(unix)]
via_tests!(path_readlink_in_write_only, |guest, via| {
    let (fd, path) = via.at(guest, "private/link");
    let errno = guest.path_readlink(fd, &path);
    assert_denied(errno, "readlink of private/link");
});

/// A read-only preopen's own fd must not be able to remove its contents.
#[test]
fn path_remove_directory_through_read_only_preopen() {
    let mut guest = Guest::new();
    cache_input_sub(&mut guest);
    let input = guest.preopen("input");
    let errno = guest.path_call("path_remove_directory", input, "sub");
    assert_denied(errno, "removing input/sub");
    assert!(guest.host("input/sub").is_dir());
}

/// A read-only preopen's own fd must not be able to move files out of it.
#[test]
fn path_rename_out_of_read_only_preopen() {
    let mut guest = Guest::new();
    let input = guest.preopen("input");
    let output = guest.preopen("output");
    let errno = guest.path_rename((input, "target.txt"), (output, "moved.txt"));
    assert_denied(errno, "moving input/target.txt to output");
    assert_target_untouched(&guest);
}

/// A symlink in a writable preopen must not lend its rights to its target.
#[cfg(unix)]
#[test]
fn path_open_truncate_through_symlink_into_read_only() {
    let mut guest = Guest::new();
    std::os::unix::fs::symlink("../input/target.txt", guest.host("output/link")).unwrap();
    let output = guest.preopen("output");
    let errno = guest.path_open(output, "link", Oflags::TRUNC, Rights::FD_WRITE);
    assert_denied(errno, "truncating input/target.txt via output/link");
    assert_target_untouched(&guest);
}

/// Two preopens of one host directory keep separate ceilings, even though
/// they resolve to the same backing path.
#[test]
fn aliased_preopens_keep_their_own_rights() {
    let mut guest = Guest::with_preopens(|builder| {
        builder
            .preopen_build(|p| p.directory("/input").alias("ro").read(true))
            .unwrap()
            .preopen_build(|p| {
                p.directory("/input")
                    .alias("rw")
                    .read(true)
                    .write(true)
                    .create(true)
            })
            .unwrap()
    });

    let errno = guest.path_open(
        VIRTUAL_ROOT_FD,
        "ro/target.txt",
        Oflags::TRUNC,
        Rights::FD_WRITE,
    );
    assert_denied(errno, "truncating ro/target.txt");
    assert_target_untouched(&guest);

    let errno = guest.path_open(
        VIRTUAL_ROOT_FD,
        "rw/target.txt",
        Oflags::TRUNC,
        Rights::FD_WRITE,
    );
    assert_eq!(errno, Errno::Success);
}

/// Creating through a dangling symlink is checked against the target's
/// parent, not the preopen holding the link.
#[cfg(unix)]
#[test]
fn path_open_create_through_dangling_symlink_into_read_only() {
    let mut guest = Guest::new();
    std::os::unix::fs::symlink("../input/new.txt", guest.host("output/dangling")).unwrap();
    let output = guest.preopen("output");
    let errno = guest.path_open(output, "dangling", Oflags::CREATE, Rights::FD_WRITE);
    assert_denied(errno, "creating input/new.txt via output/dangling");
    assert_absent(&guest, "input/new.txt");
}

/// A preopen with `write` but not `create` must not create files.
#[test]
fn path_open_create_through_preopen_without_create() {
    let mut guest = Guest::new();
    let private = guest.preopen("private");
    let errno = guest.path_open(private, "new.txt", Oflags::CREATE, Rights::FD_WRITE);
    assert_denied(errno, "creating private/new.txt");
    assert_absent(&guest, "private/new.txt");
}

/// Every operation denied above works on the preopen that grants it, so the
/// denials are not failing for unrelated reasons.
#[test]
fn permitted_operations_succeed() {
    let mut guest = Guest::new();
    let input = guest.preopen("input");
    let private = guest.preopen("private");
    let output = guest.preopen("output");

    assert_eq!(
        guest.path_open(input, "target.txt", Oflags::empty(), Rights::FD_READ),
        Errno::Success
    );
    let opened = guest.opened_fd();
    assert_eq!(guest.fd_read(opened), Errno::Success);
    assert_eq!(
        guest.path_open(input, ".", Oflags::DIRECTORY, Rights::FD_READDIR),
        Errno::Success
    );
    let opened = guest.opened_fd();
    assert_eq!(guest.fd_readdir(opened), Errno::Success);
    assert_eq!(guest.path_filestat_get(input, "target.txt"), Errno::Success);
    assert_eq!(
        guest.path_open(private, "secret.txt", Oflags::TRUNC, Rights::FD_WRITE),
        Errno::Success
    );

    assert_eq!(
        guest.path_open(output, "new.txt", Oflags::CREATE, Rights::FD_WRITE),
        Errno::Success
    );
    assert_eq!(
        guest.path_call("path_create_directory", output, "newdir"),
        Errno::Success
    );
    assert_eq!(
        guest.path_call("path_remove_directory", output, "newdir"),
        Errno::Success
    );
    assert_eq!(
        guest.path_rename((output, "mine.txt"), (output, "renamed.txt")),
        Errno::Success
    );
    assert_eq!(
        guest.path_symlink("renamed.txt", (output, "link")),
        Errno::Success
    );
    assert_eq!(guest.path_readlink(output, "link"), Errno::Success);
    assert_eq!(
        guest.path_link((output, "renamed.txt"), (output, "hard")),
        Errno::Success
    );
    assert_eq!(
        guest.path_filestat_set_mtime(output, "hard", 1_000_000_000_000_000_000),
        Errno::Success
    );
    assert_eq!(
        guest.path_call("path_unlink_file", output, "new.txt"),
        Errno::Success
    );

    assert_eq!(guest.path_filestat_get(output, "hard"), Errno::Success);
    assert!(!guest.host("output/new.txt").exists());
}
