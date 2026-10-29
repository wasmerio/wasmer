//! A host directory mounted into a memory root: data passes through, and nothing outside the
//! mapped directory is reachable, whatever symlinks or `..` the guest uses.
#![cfg(target_os = "linux")]

use std::{fs, os::unix::fs::symlink, sync::Arc, time::Duration};

use futures::executor::block_on;
use wasixv2_abi::OpenFlags;
use wasixv2_host::{Executor, Host, LocalFuture};
use wasixv2_vfs::{Clock, Cred, Ctx, Errno, LookupFlags, MountFlags, OpenHow, Timestamp, Vfs};
use wasixv2_vfs_mem::MemFs;

struct InlineHost;

impl Host for InlineHost {
    fn realtime(&self) -> Duration {
        Duration::ZERO
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
    fn sleep(&self, _: Duration) -> LocalFuture<'_, ()> {
        Box::pin(async {})
    }
    fn random(&self, buf: &mut [u8]) {
        buf.fill(0);
    }
    fn executor(&self) -> &dyn Executor {
        self
    }
}

impl Executor for InlineHost {
    fn spawn_local(&self, _: LocalFuture<'static, ()>) {
        unimplemented!()
    }
    fn spawn_blocking(&self, work: Box<dyn FnOnce() + Send>) -> LocalFuture<'static, ()> {
        Box::pin(async move { work() })
    }
}

struct Epoch;

impl Clock for Epoch {
    fn now(&self) -> Timestamp {
        Timestamp::default()
    }
}

struct Setup {
    vfs: Arc<Vfs>,
    _dir: tempfile::TempDir,
    outside: std::path::PathBuf,
}

/// `<tmp>/inside` mounted at `/mnt`, next to `<tmp>/outside/secret`.
fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let inside = dir.path().join("inside");
    let outside = dir.path().join("outside");
    fs::create_dir_all(&inside).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret"), b"secret").unwrap();
    fs::write(inside.join("data"), b"inside").unwrap();
    symlink("../outside/secret", inside.join("up")).unwrap();
    symlink(outside.join("secret"), inside.join("abs")).unwrap();
    symlink("/", inside.join("root")).unwrap();

    let vfs = Vfs::new(Arc::new(MemFs::new(Arc::new(Epoch), 0o755, 1, 1)));
    let host = wasixv2_vfs_host::open(&inside, Arc::new(InlineHost), Default::default()).unwrap();
    block_on(vfs.mount(b"/mnt", host, MountFlags::default(), true)).unwrap();
    Setup {
        vfs,
        _dir: dir,
        outside,
    }
}

fn read(setup: &Setup, path: &[u8]) -> Result<Vec<u8>, Errno> {
    let cred = Cred::user(1, 1);
    let root = setup.vfs.root();
    let cx = Ctx {
        cred: &cred,
        root: &root,
        umask: 0o022,
        task: 0,
        leases: None,
    };
    let how = OpenHow {
        flags: OpenFlags::O_RDONLY,
        mode: 0,
        lookup: LookupFlags::follow(),
    };
    let file = block_on(setup.vfs.open(&cx, &root, path, &how))?;
    let mut buf = vec![0; 64];
    let n = block_on(file.read_at(&mut buf, 0))?;
    buf.truncate(n);
    Ok(buf)
}

#[test]
fn files_pass_through() {
    let setup = setup();
    assert_eq!(read(&setup, b"/mnt/data"), Ok(b"inside".to_vec()));
}

#[test]
fn dotdot_and_symlinks_stay_in_the_guest_tree() {
    let setup = setup();

    // `..` from the mount root is the guest's `/`, which has no `outside`.
    assert_eq!(read(&setup, b"/mnt/../outside/secret"), Err(Errno::ENOENT));
    assert_eq!(read(&setup, b"/mnt/up"), Err(Errno::ENOENT));
    // Absolute targets resolve against the guest root, not the host's.
    let host_path = setup.outside.join("secret");
    assert!(read(&setup, host_path.as_os_str().as_encoded_bytes()).is_err());
    assert_eq!(read(&setup, b"/mnt/abs"), Err(Errno::ENOENT));
    assert_eq!(read(&setup, b"/mnt/root/mnt/data"), Ok(b"inside".to_vec()));
}

#[test]
fn guest_changes_reach_the_host() {
    let setup = setup();
    let cred = Cred::user(1, 1);
    let root = setup.vfs.root();
    let cx = Ctx {
        cred: &cred,
        root: &root,
        umask: 0o022,
        task: 0,
        leases: None,
    };

    block_on(setup.vfs.mkdir(&cx, &root, b"/mnt/new", 0o750)).unwrap();
    block_on(setup.vfs.rename(
        &cx,
        &root,
        b"/mnt/data",
        &root,
        b"/mnt/new/moved",
        wasixv2_vfs::RenameMode::Replace,
    ))
    .unwrap();

    let inside = setup.outside.parent().unwrap().join("inside");
    assert_eq!(fs::read(inside.join("new/moved")).unwrap(), b"inside");
    let attr = block_on(
        setup
            .vfs
            .lookup(&cx, &root, b"/mnt/new", LookupFlags::follow()),
    )
    .map(|path| block_on(setup.vfs.stat(&path)).unwrap())
    .unwrap();
    assert_eq!((attr.mode, attr.uid), (0o750, 1));
}

fn lookup(setup: &Setup, path: &[u8]) -> Result<wasixv2_vfs::Path, Errno> {
    let cred = Cred::user(1, 1);
    let root = setup.vfs.root();
    let cx = Ctx {
        cred: &cred,
        root: &root,
        umask: 0o022,
        task: 0,
        leases: None,
    };
    block_on(setup.vfs.lookup(&cx, &root, path, LookupFlags::default()))
}

#[test]
fn directories_renamed_on_the_host_keep_their_one_dentry() {
    let setup = setup();
    let inside = setup.outside.parent().unwrap().join("inside");
    fs::create_dir(inside.join("old")).unwrap();
    let cwd = lookup(&setup, b"/mnt/old").unwrap();

    fs::rename(inside.join("old"), inside.join("new")).unwrap();
    let found = lookup(&setup, b"/mnt/new").unwrap();

    assert!(Arc::ptr_eq(&cwd.dentry, &found.dentry));
    assert_eq!(setup.vfs.d_path(&cwd, &setup.vfs.root()), b"/mnt/new");

    fs::remove_dir(inside.join("new")).unwrap();
    assert_eq!(lookup(&setup, b"/mnt/new").err(), Some(Errno::ENOENT));
    assert_eq!(
        setup.vfs.d_path(&cwd, &setup.vfs.root()),
        b"/mnt/new (deleted)"
    );
}

#[test]
fn host_device_numbers_stay_on_the_host() {
    use std::os::linux::fs::MetadataExt;

    let setup = setup();
    let stat = |path: &[u8]| block_on(setup.vfs.stat(&lookup(&setup, path).unwrap())).unwrap();
    let (root, mnt, data) = (stat(b"/"), stat(b"/mnt"), stat(b"/mnt/data"));
    let inside = setup.outside.parent().unwrap().join("inside");
    let host = fs::metadata(inside.join("data")).unwrap();

    assert_eq!(data.ino, host.st_ino());
    assert_eq!(mnt.dev, data.dev);
    assert_ne!(root.dev, mnt.dev);
    assert_eq!(mnt.dev.unwrap().major, 0);
}

fn open(
    setup: &Setup,
    path: &[u8],
    flags: OpenFlags,
    mode: u32,
) -> Result<wasixv2_vfs::File, Errno> {
    let cred = Cred::user(1, 1);
    let root = setup.vfs.root();
    let cx = Ctx {
        cred: &cred,
        root: &root,
        umask: 0o022,
        task: 0,
        leases: None,
    };
    let how = OpenHow {
        flags,
        mode,
        lookup: LookupFlags::follow(),
    };
    block_on(setup.vfs.open(&cx, &root, path, &how))
}

/// What `open(O_CREAT)` grants is the access asked for, whatever the new file's mode (git's
/// read-only objects, `tar x` and `install -m 0444`); the host must not check the mode again.
#[test]
fn a_file_created_read_only_is_writable_through_its_creating_open() {
    use std::os::unix::fs::PermissionsExt;

    let setup = setup();
    let flags = OpenFlags::O_CREAT | OpenFlags::O_EXCL | OpenFlags::O_RDWR;
    let file = open(&setup, b"/mnt/obj", flags, 0o444).unwrap();
    assert_eq!(block_on(file.write_at(b"object", 0)), Ok(6));
    assert_eq!(block_on(file.append(b"!")), Ok((7, 1)));
    // ftruncate needs the open file's write access, not the mode bits.
    assert_eq!(block_on(file.set_size(3)), Ok(()));
    drop(file);

    let host = setup.outside.parent().unwrap().join("inside/obj");
    assert_eq!(fs::read(&host).unwrap(), b"obj");
    assert_eq!(
        fs::metadata(&host).unwrap().permissions().mode() & 0o777,
        0o444
    );
    // Reopening is checked against the mode, by the VFS and the host alike.
    assert_eq!(
        open(&setup, b"/mnt/obj", OpenFlags::O_WRONLY, 0).err(),
        Some(Errno::EACCES)
    );
}

#[test]
fn a_writable_open_survives_chmod_to_read_only() {
    let setup = setup();
    let file = open(&setup, b"/mnt/data", OpenFlags::O_RDWR, 0).unwrap();
    let cred = Cred::user(1, 1);
    let root = setup.vfs.root();
    let cx = Ctx {
        cred: &cred,
        root: &root,
        umask: 0o022,
        task: 0,
        leases: None,
    };
    block_on(setup.vfs.chmod(&cx, &file.path, 0o444)).unwrap();

    assert_eq!(block_on(file.set_size(0)), Ok(()));
    assert_eq!(block_on(file.append(b"new")), Ok((3, 3)));
    assert_eq!(read(&setup, b"/mnt/data"), Ok(b"new".to_vec()));
}

#[test]
fn an_unnamed_read_only_file_is_writable_and_linkable() {
    let setup = setup();
    let flags = OpenFlags::__O_TMPFILE | OpenFlags::O_RDWR;
    let file = open(&setup, b"/mnt", flags, 0o400).unwrap();
    assert_eq!(block_on(file.write_at(b"tmp", 0)), Ok(3));

    let cred = Cred::user(1, 1);
    let root = setup.vfs.root();
    let cx = Ctx {
        cred: &cred,
        root: &root,
        umask: 0o022,
        task: 0,
        leases: None,
    };
    let empty = LookupFlags {
        empty: true,
        ..LookupFlags::default()
    };
    block_on(
        setup
            .vfs
            .link(&cx, &file.path, b"", empty, &root, b"/mnt/named"),
    )
    .unwrap();
    let host = setup.outside.parent().unwrap().join("inside/named");
    assert_eq!(fs::read(host).unwrap(), b"tmp");
}

#[test]
fn host_file_handles_use_native_exports_without_pinning_exported_files() {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use wasixv2_vfs::HandleKind;
    let s = setup();
    let real = s._dir.path().join("inside/data");
    let name = std::ffi::CString::new(real.as_os_str().as_encoded_bytes()).unwrap();
    // Probe host support independently so a missing driver implementation cannot make
    // this regression test skip itself. Some hosts mount filesystems without export ops.
    // SAFETY: NUL-terminated path and an aligned file_handle header plus 128 data bytes.
    let raw = unsafe { libc::open(name.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
    assert!(raw >= 0);
    // SAFETY: open returned an owned descriptor.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let mut native = [0u64; 17];
    native[0] = 128;
    let mut mount = 0i32;
    // SAFETY: initialized buffers have the full UAPI sizes.
    let result = unsafe {
        libc::syscall(
            libc::SYS_name_to_handle_at,
            fd.as_raw_fd(),
            c"".as_ptr(),
            native.as_mut_ptr(),
            &mut mount,
            libc::AT_EMPTY_PATH,
        )
    };
    if result < 0 {
        let err = std::io::Error::last_os_error().raw_os_error().unwrap();
        assert!(
            [libc::EOPNOTSUPP, libc::ENOSYS].contains(&err),
            "native export failed: {err}"
        );
        return;
    }
    let root = s.vfs.root();
    let cred = Cred::root();
    let cx = Ctx {
        cred: &cred,
        root: &root,
        umask: 0,
        task: 0,
        leases: None,
    };
    let path = block_on(
        s.vfs
            .lookup(&cx, &root, b"/mnt/data", LookupFlags::follow()),
    )
    .unwrap();
    let handle = s.vfs.encode_fh(&path, HandleKind::Decodable).unwrap();
    assert!(handle.data.len() <= wasixv2_vfs::MAX_HANDLE_SZ);
    let fs = &path.dentry.sb().fs;
    match block_on(fs.fh_to_node(&handle)) {
        Ok(decoded) => assert_eq!(decoded.ino(), path.dentry.node().ino()),
        Err(Errno::EPERM) => {} // Native decoding requires the host's CAP_DAC_READ_SEARCH.
        Err(err) => panic!("native decode failed: {err:?}"),
    }
    // Only mount roots are retained by the export machinery, not this ordinary inode.
    let weak = Arc::downgrade(path.dentry.node());
    drop(path);
    assert!(weak.upgrade().is_none());
}
