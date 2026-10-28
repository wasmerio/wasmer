use futures::executor::block_on;

use super::*;

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        Timestamp::new(1_700_000_000, 0)
    }
}

fn fs() -> MemFs {
    MemFs::new(Arc::new(FixedClock), 0o755, 1, 1)
}

fn new(kind: NewKind<'_>) -> NewNode<'_> {
    NewNode {
        kind,
        mode: 0o644,
        uid: 1,
        gid: 1,
    }
}

struct Names(Vec<(Vec<u8>, u64)>, usize);

impl DirSink for Names {
    fn push(&mut self, entry: DirEntry<'_>) -> bool {
        if self.0.len() == self.1 {
            return false;
        }
        self.0.push((entry.name.to_vec(), entry.next));
        true
    }
}

fn read_dir(dir: &dyn Handle, cookie: u64, max: usize) -> Vec<(Vec<u8>, u64)> {
    let mut names = Names(Vec::new(), max);
    block_on(dir.readdir(cookie, &mut names)).unwrap();
    names.0
}

#[test]
fn sparse_file_reads_zero_holes_and_seeks_data() {
    let fs = fs();
    let file = block_on(fs.root().create(b"f", &new(NewKind::Regular))).unwrap();
    let handle = block_on(file.open(OpenOptions::default())).unwrap();
    let far = 1 << 33;

    block_on(handle.write_at(b"hello", 0)).unwrap();
    block_on(handle.write_at(b"X", far)).unwrap();
    let mut buf = [0xff; 8];
    let n = block_on(handle.read_at(&mut buf, 3)).unwrap();

    assert_eq!((n, &buf), (8, b"lo\0\0\0\0\0\0"));
    let size = far + 1;
    assert_eq!(
        block_on(handle.seek_hole(0, size, SeekHole::Hole)),
        Ok(BLOCK)
    );
    assert_eq!(
        block_on(handle.seek_hole(BLOCK, size, SeekHole::Data)),
        Ok(far / BLOCK * BLOCK)
    );
    assert_eq!(
        block_on(handle.seek_hole(size, size, SeekHole::Data)),
        Err(Errno::ENXIO)
    );
    assert_eq!(block_on(file.getattr()).unwrap().blocks, 2 * BLOCK / 512);
}

#[test]
fn punch_hole_frees_whole_blocks_and_keeps_size() {
    let fs = fs();
    let file = block_on(fs.root().create(b"f", &new(NewKind::Regular))).unwrap();
    let handle = block_on(file.open(OpenOptions::default())).unwrap();
    block_on(handle.write_at(&[1; 3 * BLOCK as usize], 0)).unwrap();

    block_on(handle.allocate(AllocMode::PunchHole, 10, 2 * BLOCK)).unwrap();
    let attr = block_on(file.getattr()).unwrap();
    let mut buf = [9; 4];
    block_on(handle.read_at(&mut buf, BLOCK)).unwrap();

    assert_eq!(attr.size, 3 * BLOCK);
    assert_eq!(attr.blocks, 2 * BLOCK / 512);
    assert_eq!(buf, [0; 4]);
}

#[test]
fn readdir_cookies_survive_removal_and_insertion() {
    let fs = fs();
    let root = fs.root();
    for name in [b"a", b"b", b"c"] {
        block_on(root.create(name, &new(NewKind::Regular))).unwrap();
    }
    let dir = block_on(root.open(OpenOptions::default())).unwrap();
    let first = read_dir(&*dir, 0, 1);
    assert_eq!(first[0].0, b"a");

    // "b" goes away and "d" arrives while the reader holds a cookie.
    block_on(root.unlink(b"b")).unwrap();
    block_on(root.create(b"d", &new(NewKind::Regular))).unwrap();
    let rest: Vec<_> = read_dir(&*dir, first[0].1, 10)
        .into_iter()
        .map(|(name, _)| name)
        .collect();

    assert_eq!(rest, [b"c".to_vec(), b"d".to_vec()]);
}

#[test]
fn rename_keeps_link_counts_right() {
    let fs = fs();
    let root = fs.root();
    let a = block_on(root.create(b"a", &new(NewKind::Directory))).unwrap();
    let b = block_on(root.create(b"b", &new(NewKind::Directory))).unwrap();
    block_on(a.create(b"sub", &new(NewKind::Directory))).unwrap();
    let file = block_on(root.create(b"f", &new(NewKind::Regular))).unwrap();
    block_on(root.link(b"g", &file)).unwrap();

    block_on(a.rename(b"sub", &b, b"sub", RenameMode::Replace)).unwrap();
    block_on(root.rename(b"f", &root, b"g", RenameMode::Replace)).unwrap();
    let nlink = |node: &Arc<dyn Node>| block_on(node.getattr()).unwrap().nlink;

    assert_eq!((nlink(&a), nlink(&b), nlink(&root)), (2, 3, 4));
    // Renaming one link of a file onto another of the same file does nothing (POSIX).
    assert_eq!(nlink(&file), 2);
    assert!(block_on(root.lookup(b"f")).is_ok());
    assert_eq!(block_on(root.rmdir(b"b")).err(), Some(Errno::ENOTEMPTY));
}

#[test]
fn exchange_swaps_entries_and_dir_link_counts() {
    let fs = fs();
    let root = fs.root();
    let a = block_on(root.create(b"a", &new(NewKind::Directory))).unwrap();
    let b = block_on(root.create(b"b", &new(NewKind::Directory))).unwrap();
    let d = block_on(a.create(b"d", &new(NewKind::Directory))).unwrap();
    let f = block_on(b.create(b"f", &new(NewKind::Regular))).unwrap();

    block_on(a.rename(b"d", &b, b"f", RenameMode::Exchange)).unwrap();

    assert_eq!(block_on(a.lookup(b"d")).unwrap().ino(), f.ino());
    assert_eq!(block_on(b.lookup(b"f")).unwrap().ino(), d.ino());
    assert_eq!(block_on(a.getattr()).unwrap().nlink, 2);
    assert_eq!(block_on(b.getattr()).unwrap().nlink, 3);
}

#[test]
fn xattrs_honour_create_and_replace() {
    let fs = fs();
    let root = fs.root();

    assert_eq!(
        block_on(root.setxattr(b"user.k", b"v", XattrSet::Replace)),
        Err(Errno::ENODATA)
    );
    block_on(root.setxattr(b"user.k", b"v", XattrSet::Create)).unwrap();
    assert_eq!(
        block_on(root.setxattr(b"user.k", b"w", XattrSet::Create)),
        Err(Errno::EEXIST)
    );
    assert_eq!(block_on(root.getxattr(b"user.k")), Ok(b"v".to_vec()));
    assert_eq!(block_on(root.listxattr()), Ok(vec![b"user.k".to_vec()]));
}

fn limited(limits: Limits) -> MemFs {
    MemFs::with_limits(Arc::new(FixedClock), 0o755, 1, 1, limits)
}

fn open_file(fs: &MemFs, name: &[u8]) -> (Arc<dyn Node>, Box<dyn Handle>) {
    let options = OpenOptions {
        read: true,
        write: true,
        ..OpenOptions::default()
    };
    block_on(fs.root().create_open(name, &new(NewKind::Regular), options)).unwrap()
}

#[test]
fn fallocate_past_the_size_limit_fails_at_once() {
    let fs = fs();
    let (file, handle) = open_file(&fs, b"f");

    // Far more than the default limit (half of 4 GiB): refused up front, nothing charged.
    assert_eq!(
        block_on(handle.allocate(AllocMode::Allocate, 0, 1 << 40)),
        Err(Errno::ENOSPC)
    );
    let attr = block_on(file.getattr()).unwrap();
    assert_eq!((attr.size, attr.blocks), (0, 0));
    let statfs = block_on(fs.statfs()).unwrap();
    assert_eq!(statfs.blocks, (1 << 31) / BLOCK);
    assert_eq!(statfs.blocks_free, statfs.blocks);
}

#[test]
fn huge_allocations_and_punches_cost_ranges_not_blocks() {
    let fs = limited(Limits::UNLIMITED);
    let (file, handle) = open_file(&fs, b"f");

    // A terabyte of allocation is one range: no memory and no per-block loop.
    block_on(handle.allocate(AllocMode::AllocateKeepSize, 0, 1 << 40)).unwrap();
    block_on(handle.write_at(b"data", 1 << 30)).unwrap();
    let attr = block_on(file.getattr()).unwrap();
    assert_eq!(attr.size, (1 << 30) + 4);
    assert_eq!(attr.blocks, (1 << 40) / 512);
    // Allocated but unwritten blocks are holes to SEEK_DATA, as tmpfs's.
    assert_eq!(
        block_on(handle.seek_hole(0, attr.size, SeekHole::Data)),
        Ok(1 << 30)
    );

    // A file made enormous by truncation, then punched across its whole length.
    block_on(handle.set_size(1 << 62)).unwrap();
    block_on(handle.allocate(AllocMode::PunchHole, 0, 1 << 62)).unwrap();
    let attr = block_on(file.getattr()).unwrap();
    assert_eq!((attr.size, attr.blocks), (1 << 62, 0));
    let mut buf = [1; 4];
    block_on(handle.read_at(&mut buf, 1 << 30)).unwrap();
    assert_eq!(buf, [0; 4]);
}

#[test]
fn writes_stop_at_the_size_limit() {
    let fs = limited(Limits {
        max_bytes: Some(4 * BLOCK),
        max_inodes: None,
    });
    let (file, handle) = open_file(&fs, b"f");

    let data = vec![7; 5 * BLOCK as usize];
    assert_eq!(block_on(handle.write_at(&data, 0)), Ok(4 * BLOCK as usize));
    assert_eq!(
        block_on(handle.write_at(&data, 4 * BLOCK)),
        Err(Errno::ENOSPC)
    );
    assert_eq!(block_on(fs.statfs()).unwrap().blocks_free, 0);
    // Rewriting allocated blocks needs no space; freeing gives it back.
    assert_eq!(block_on(handle.write_at(b"x", 0)), Ok(1));
    block_on(handle.set_size(BLOCK)).unwrap();
    assert_eq!(block_on(fs.statfs()).unwrap().blocks_free, 3);
    drop((file, handle));
    block_on(fs.root().unlink(b"f")).unwrap();
    assert_eq!(block_on(fs.statfs()).unwrap().blocks_free, 4);
}

#[test]
fn inodes_and_extra_links_are_limited() {
    // The root and two more.
    let fs = limited(Limits {
        max_bytes: None,
        max_inodes: Some(3),
    });
    let root = fs.root();
    let a = block_on(root.create(b"a", &new(NewKind::Regular))).unwrap();
    assert_eq!(block_on(fs.statfs()).unwrap().files_free, 1);
    block_on(root.link(b"b", &a)).unwrap();

    assert_eq!(
        block_on(root.create(b"c", &new(NewKind::Regular))).err(),
        Some(Errno::ENOSPC)
    );
    assert_eq!(block_on(root.link(b"c", &a)), Err(Errno::ENOSPC));
    block_on(root.unlink(b"b")).unwrap();
    block_on(root.create(b"c", &new(NewKind::Directory))).unwrap();
}

#[test]
fn a_remount_changes_the_limits_within_what_is_used() {
    let fs = limited(Limits {
        max_bytes: Some(4 * BLOCK),
        max_inodes: Some(3),
    });
    let (_file, handle) = open_file(&fs, b"f");
    block_on(handle.write_at(&vec![7; 2 * BLOCK as usize], 0)).unwrap();
    let limits = |max_bytes, max_inodes| Limits {
        max_bytes,
        max_inodes,
    };

    assert_eq!(fs.set_limits(limits(Some(BLOCK), None)), Err(Errno::EINVAL));
    assert_eq!(fs.set_limits(limits(None, Some(1))), Err(Errno::EINVAL));
    assert_eq!(fs.set_limits(limits(Some(2 * BLOCK + 1), Some(2))), Ok(()));
    let stat = block_on(fs.statfs()).unwrap();
    assert_eq!((stat.blocks, stat.blocks_free, stat.files), (3, 1, 2));
    // Nothing named, nothing changes; 0 lifts a limit, which cannot come back.
    assert_eq!(fs.set_limits(limits(None, None)), Ok(()));
    assert_eq!(fs.set_limits(limits(Some(0), None)), Ok(()));
    assert_eq!(block_on(fs.statfs()).unwrap().blocks, 0);
    assert_eq!(
        fs.set_limits(limits(Some(8 * BLOCK), None)),
        Err(Errno::EINVAL)
    );
}

#[test]
fn options_show_what_is_not_tmpfs_s_default() {
    let options = |fs: &MemFs| {
        let mut out = Vec::new();
        fs.show_options(&mut out);
        String::from_utf8(out).unwrap()
    };
    let defaults = MemFs::new(Arc::new(FixedClock), 0o1777, 0, 0);
    assert_eq!(options(&defaults), "");
    let fs = limited(Limits {
        max_bytes: Some(4 * BLOCK),
        max_inodes: Some(3),
    });
    assert_eq!(options(&fs), ",size=16k,nr_inodes=3,mode=755,uid=1,gid=1");
    fs.set_limits(Limits {
        max_bytes: Some(0),
        max_inodes: None,
    })
    .unwrap();
    assert!(options(&fs).starts_with(",size=0k,nr_inodes=3,"));
}

#[test]
fn an_empty_write_past_the_end_does_not_extend() {
    let fs = fs();
    let (file, handle) = open_file(&fs, b"f");

    assert_eq!(block_on(handle.write_at(b"", 1 << 20)), Ok(0));
    assert_eq!(block_on(file.getattr()).unwrap().size, 0);
}

#[test]
fn readdir_resumes_across_batches() {
    let fs = fs();
    let root = fs.root();
    let names: Vec<Vec<u8>> = (0..200).map(|i| format!("f{i:03}").into_bytes()).collect();
    for name in &names {
        block_on(root.create(name, &new(NewKind::Regular))).unwrap();
    }
    let dir = block_on(root.open(OpenOptions::default())).unwrap();

    let mut seen = Vec::new();
    let mut cookie = 0;
    loop {
        let chunk = read_dir(&*dir, cookie, 37);
        let Some((_, next)) = chunk.last() else {
            break;
        };
        cookie = *next;
        seen.extend(chunk.into_iter().map(|(name, _)| name));
    }
    assert_eq!(seen, names);
}

/// Files kept in host shared memory objects, as the memfd and `/dev/shm` filesystems keep them.
#[cfg(any(target_os = "linux", target_os = "android"))]
mod shared {
    use std::io;

    use wasixv2_pages::{Memfd, Provider, SharedPages};

    use super::*;

    struct Memfds;

    impl Provider for Memfds {
        fn shared_pages(&self, size: u64) -> io::Result<Arc<dyn SharedPages>> {
            Ok(Arc::new(Memfd::new(size)?))
        }
    }

    struct Unsupported;

    impl Provider for Unsupported {
        fn shared_pages(&self, _: u64) -> io::Result<Arc<dyn SharedPages>> {
            Err(io::ErrorKind::Unsupported.into())
        }
    }

    fn shared_fs() -> MemFs {
        fs().with_shared_pages(Arc::new(Memfds))
    }

    fn object(file: &Arc<dyn Node>, end: u64) -> Arc<dyn SharedPages> {
        file.shared_pages(end)
            .unwrap()
            .expect("a file in shared memory")
    }

    #[test]
    fn writes_and_mapped_stores_are_the_same_bytes() {
        let fs = shared_fs();
        let (file, handle) = open_file(&fs, b"f");
        block_on(handle.write_at(b"written", 10)).unwrap();

        let pages = object(&file, 2 * BLOCK);
        let mut buf = [0; 7];
        pages.read_at(&mut buf, 10).unwrap();
        assert_eq!(&buf, b"written");
        // A store through a mapping is what read() returns, with no copy in between.
        pages.write_at(b"mapped", 2).unwrap();
        let mut buf = [0; 6];
        block_on(handle.read_at(&mut buf, 2)).unwrap();
        assert_eq!(&buf, b"mapped");
        // The same object every time.
        assert_eq!(object(&file, 0).id(), pages.id());
    }

    #[test]
    fn the_object_covers_every_mapping_whatever_the_size() {
        let fs = shared_fs();
        let (file, handle) = open_file(&fs, b"f");
        block_on(handle.write_at(&[7; 3 * BLOCK as usize], 0)).unwrap();
        let pages = object(&file, 16 * BLOCK);
        assert_eq!(pages.size(), 16 * BLOCK);

        // Shrinking frees the pages past the new size but keeps the object's length, so a
        // mapping of all 16 blocks stays backed on the host.
        block_on(handle.set_size(1)).unwrap();
        assert_eq!(pages.size(), 16 * BLOCK);
        let mut buf = [1; 2];
        pages.read_at(&mut buf, 0).unwrap();
        assert_eq!(buf, [7, 0]);
        let attr = block_on(file.getattr()).unwrap();
        assert_eq!((attr.size, attr.blocks), (1, BLOCK / 512));
    }

    #[test]
    fn growing_clears_what_a_mapping_stored_past_the_end() {
        let fs = shared_fs();
        let (file, handle) = open_file(&fs, b"f");
        block_on(handle.write_at(b"ab", 0)).unwrap();
        let pages = object(&file, 4 * BLOCK);
        // Past the end of the file, where Linux would raise SIGBUS.
        pages.write_at(b"stale", 2 * BLOCK).unwrap();

        block_on(handle.set_size(3 * BLOCK)).unwrap();
        let mut buf = [1; 5];
        block_on(handle.read_at(&mut buf, 2 * BLOCK)).unwrap();
        assert_eq!(buf, [0; 5]);
        block_on(handle.read_at(&mut buf[..2], 0)).unwrap();
        assert_eq!(&buf[..2], b"ab");
    }

    #[test]
    fn holes_and_data_come_from_the_object() {
        let fs = shared_fs();
        let (file, handle) = open_file(&fs, b"f");
        let far = 1 << 33;
        block_on(handle.write_at(b"hello", 0)).unwrap();
        block_on(handle.write_at(b"X", far)).unwrap();
        let size = far + 1;

        assert_eq!(
            block_on(handle.seek_hole(0, size, SeekHole::Hole)),
            Ok(BLOCK)
        );
        assert_eq!(
            block_on(handle.seek_hole(BLOCK, size, SeekHole::Data)),
            Ok(far / BLOCK * BLOCK)
        );
        assert_eq!(
            block_on(handle.seek_hole(far, size, SeekHole::Hole)),
            Ok(size)
        );
        // Punched blocks are holes again; allocated ones never were data.
        block_on(handle.allocate(AllocMode::PunchHole, far / BLOCK * BLOCK, BLOCK)).unwrap();
        assert_eq!(
            block_on(handle.seek_hole(BLOCK, size, SeekHole::Data)),
            Err(Errno::ENXIO)
        );
        block_on(handle.allocate(AllocMode::AllocateKeepSize, BLOCK, BLOCK)).unwrap();
        assert_eq!(
            block_on(handle.seek_hole(BLOCK, size, SeekHole::Hole)),
            Ok(BLOCK)
        );
        assert_eq!(block_on(file.getattr()).unwrap().blocks, 2 * BLOCK / 512);
    }

    #[test]
    fn writes_stop_at_the_size_limit() {
        let fs = limited(Limits {
            max_bytes: Some(2 * BLOCK),
            max_inodes: None,
        })
        .with_shared_pages(Arc::new(Memfds));
        let (_file, handle) = open_file(&fs, b"f");

        let data = vec![7; 3 * BLOCK as usize];
        assert_eq!(
            block_on(handle.write_at(&data, 10)),
            Ok(2 * BLOCK as usize - 10)
        );
        assert_eq!(
            block_on(handle.write_at(&data, 2 * BLOCK)),
            Err(Errno::ENOSPC)
        );
        let mut buf = [0; 2];
        block_on(handle.read_at(&mut buf, 2 * BLOCK - 11)).unwrap();
        assert_eq!(buf, [7, 7]);
    }

    #[test]
    fn a_file_without_an_object_stays_on_the_heap() {
        let fs = fs().with_shared_pages(Arc::new(Unsupported));
        let (file, handle) = open_file(&fs, b"f");
        block_on(handle.write_at(b"heap", 0)).unwrap();

        assert!(file.shared_pages(BLOCK).unwrap().is_none());
        let mut buf = [0; 4];
        block_on(handle.read_at(&mut buf, 0)).unwrap();
        assert_eq!(&buf, b"heap");
    }

    #[test]
    fn only_regular_files_have_objects() {
        let fs = shared_fs();
        let dir = block_on(fs.root().create(b"d", &new(NewKind::Directory))).unwrap();
        assert!(dir.shared_pages(BLOCK).unwrap().is_none());
        // Nor do files of a filesystem without shared memory.
        let (file, _) = open_file(&super::fs(), b"f");
        assert!(file.shared_pages(BLOCK).unwrap().is_none());
    }
}
