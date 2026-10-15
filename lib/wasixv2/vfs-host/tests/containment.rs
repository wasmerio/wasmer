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
