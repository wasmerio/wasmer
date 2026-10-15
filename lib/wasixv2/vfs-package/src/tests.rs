use std::collections::BTreeMap;

use futures::executor::block_on;
use webc::v3::{
    ChecksumAlgorithm, SignatureAlgorithm, Timestamps,
    write::{DirEntry as WEntry, Directory as WDir, FileEntry, SymlinkEntry, Writer},
};

use super::*;

fn dir(children: Vec<(&str, WEntry<'static>)>) -> WDir<'static> {
    let children: BTreeMap<_, _> = children
        .into_iter()
        .map(|(name, entry)| (name.parse().unwrap(), entry))
        .collect();
    WDir::new(children, Timestamps::default())
}

fn file(data: &'static [u8]) -> WEntry<'static> {
    WEntry::File(FileEntry::borrowed(data, Timestamps::default()))
}

/// `/a.txt`, `/bin/{tool,sh -> tool}`, `/empty/`, `/z/`.
fn fs() -> PackageFs {
    PackageFs::new(&reader(), "atom").unwrap()
}

fn reader() -> OwnedReader {
    let volume = dir(vec![
        ("z", WEntry::Dir(dir(vec![]))),
        ("a.txt", file(b"hello world")),
        (
            "bin",
            WEntry::Dir(dir(vec![
                ("tool", file(b"\0asm")),
                (
                    "sh",
                    WEntry::Symlink(SymlinkEntry::borrowed("tool", Timestamps::default())),
                ),
            ])),
        ),
        ("empty", WEntry::Dir(dir(vec![]))),
    ]);
    let mut writer = Writer::new(ChecksumAlgorithm::Sha256)
        .write_manifest(&Default::default())
        .unwrap()
        .write_atoms(BTreeMap::new())
        .unwrap();
    writer.write_volume("atom", volume).unwrap();
    let webc = writer.finish(SignatureAlgorithm::None).unwrap();
    OwnedReader::parse(webc).unwrap()
}

fn lookup(fs: &PackageFs, path: &str) -> Arc<dyn Node> {
    let mut node = fs.root();
    for name in path.split('/').filter(|n| !n.is_empty()) {
        node = block_on(node.lookup(name.as_bytes())).unwrap();
    }
    node
}

struct Names(Vec<(Vec<u8>, u64, FileType, u64)>, usize);

impl DirSink for Names {
    fn push(&mut self, entry: DirEntry<'_>) -> bool {
        if self.0.len() == self.1 {
            return false;
        }
        self.0
            .push((entry.name.to_vec(), entry.ino, entry.file_type, entry.next));
        true
    }
}

fn read_dir(node: &dyn Node, cookie: u64, max: usize) -> Vec<(Vec<u8>, u64, FileType, u64)> {
    let handle = block_on(node.open(OpenOptions::default())).unwrap();
    let mut names = Names(Vec::new(), max);
    block_on(handle.readdir(cookie, &mut names)).unwrap();
    names.0
}

#[test]
fn lookup_finds_entries_and_misses_with_enoent() {
    let fs = fs();
    assert_eq!(lookup(&fs, "bin/tool").file_type(), FileType::Regular);
    assert_eq!(lookup(&fs, "bin/sh").file_type(), FileType::Symlink);
    assert_eq!(lookup(&fs, "empty").file_type(), FileType::Directory);
    assert_eq!(
        block_on(fs.root().lookup(b"missing")).err(),
        Some(Errno::ENOENT)
    );
    assert_eq!(
        block_on(lookup(&fs, "a.txt").lookup(b"x")).err(),
        Some(Errno::ENOTDIR)
    );
}

#[test]
fn inode_numbers_are_unique_stable_and_match_readdir() {
    let fs = fs();
    let ino = lookup(&fs, "bin/tool").ino();
    assert_eq!(ino, lookup(&fs, "bin/tool").ino());
    assert_eq!(fs.root().ino(), ROOT_INO);

    let mut all = vec![ROOT_INO];
    for path in ["a.txt", "bin", "bin/tool", "bin/sh", "empty", "z"] {
        all.push(lookup(&fs, path).ino());
    }
    let count = all.len();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), count);

    let entries = read_dir(&*lookup(&fs, "bin"), 0, usize::MAX);
    assert!(entries.iter().any(|e| e.0 == b"tool" && e.1 == ino));
}

#[test]
fn readdir_is_sorted_and_resumes_at_cookies() {
    let fs = fs();
    let root = fs.root();
    let all = read_dir(&*root, 0, usize::MAX);
    let names: Vec<_> = all.iter().map(|e| e.0.as_slice()).collect();
    assert_eq!(names, [&b"a.txt"[..], b"bin", b"empty", b"z"]);
    assert_eq!(all[1].2, FileType::Directory);

    let first = read_dir(&*root, 0, 2);
    let rest = read_dir(&*root, first[1].3, usize::MAX);
    assert_eq!(rest, all[2..]);
    assert!(read_dir(&*root, all[3].3, usize::MAX).is_empty());
}

#[test]
fn read_at_serves_data_and_eof() {
    let fs = fs();
    let handle = block_on(lookup(&fs, "a.txt").open(OpenOptions {
        read: true,
        ..Default::default()
    }))
    .unwrap();
    let mut buf = [0; 8];
    assert_eq!(block_on(handle.read_at(&mut buf, 6)), Ok(5));
    assert_eq!(&buf[..5], b"world");
    assert_eq!(block_on(handle.read_at(&mut buf, 11)), Ok(0));
    assert_eq!(block_on(handle.read_at(&mut buf, u64::MAX)), Ok(0));
}

#[test]
fn getattr_reports_a_root_owned_read_only_image() {
    let fs = fs();
    let file = block_on(lookup(&fs, "a.txt").getattr()).unwrap();
    assert_eq!(
        (file.mode, file.uid, file.gid, file.nlink, file.size),
        (0o755, 0, 0, 1, 11)
    );
    let root = block_on(fs.root().getattr()).unwrap();
    // `.`, the parent's entry, and one `..` each from bin, empty and z.
    assert_eq!((root.mode, root.nlink), (0o755, 5));
    let link = lookup(&fs, "bin/sh");
    assert_eq!(block_on(link.readlink()), Ok(b"tool".to_vec()));
    assert_eq!(block_on(link.getattr()).unwrap().size, 4);
    assert!(fs.info().read_only);
}

#[test]
fn with_owner_sets_every_objects_owner() {
    let fs = PackageFs::with_owner(&reader(), "atom", 1, 2).unwrap();
    for path in ["", "a.txt", "bin", "bin/sh"] {
        let attr = block_on(lookup(&fs, path).getattr()).unwrap();
        assert_eq!((attr.uid, attr.gid), (1, 2), "{path}");
    }
}

#[test]
fn mutations_fail_with_erofs() {
    let fs = fs();
    let root = fs.root();
    let file = lookup(&fs, "a.txt");
    let new = NewNode {
        kind: wasixv2_vfs::NewKind::Regular,
        mode: 0o644,
        uid: 0,
        gid: 0,
    };
    assert_eq!(block_on(root.create(b"n", &new)).err(), Some(Errno::EROFS));
    assert_eq!(block_on(root.unlink(b"a.txt")), Err(Errno::EROFS));
    assert_eq!(block_on(root.rmdir(b"empty")), Err(Errno::EROFS));
    assert_eq!(
        block_on(file.setattr(&SetAttr::default())).err(),
        Some(Errno::EROFS)
    );
    let write = OpenOptions {
        write: true,
        ..Default::default()
    };
    assert_eq!(block_on(file.open(write)).err(), Some(Errno::EROFS));
    assert_eq!(block_on(file.getxattr(b"user.x")), Err(Errno::ENODATA));
    assert_eq!(block_on(file.listxattr()), Ok(vec![]));
}
