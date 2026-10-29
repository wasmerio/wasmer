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
        task: 0,
        quota_exempt: false,
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

/// The bytes of a package whose volume `atom` holds `children`.
fn webc_bytes(children: Vec<(&str, WEntry<'static>)>) -> Vec<u8> {
    let mut writer = Writer::new(ChecksumAlgorithm::Sha256)
        .write_manifest(&Default::default())
        .unwrap()
        .write_atoms(BTreeMap::new())
        .unwrap();
    writer.write_volume("atom", dir(children)).unwrap();
    writer.finish(SignatureAlgorithm::None).unwrap().to_vec()
}

fn mount(bytes: Vec<u8>) -> Result<PackageFs, PackageFsError> {
    PackageFs::new(&OwnedReader::parse(bytes).unwrap(), "atom")
}

fn find(bytes: &[u8], what: &[u8]) -> usize {
    let at = bytes.windows(what.len()).position(|w| w == what);
    let at = at.expect("pattern in the image");
    assert_eq!(
        bytes[at + 1..].windows(what.len()).position(|w| w == what),
        None
    );
    at
}

#[test]
fn a_file_whose_data_ends_before_it_starts_is_rejected_at_mount() {
    let mut bytes = webc_bytes(vec![("f", file(&[0xab; 1000]))]);
    // The file's header entry: its tag, then its data's start and end offsets.
    let at = (0..bytes.len() - 17)
        .find(|&i| {
            let word = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
            bytes[i] == 31 && word(i + 9).checked_sub(word(i + 1)) == Some(1000)
        })
        .expect("the file's entry");
    let (start, end) = (at + 1, at + 9);
    let (a, b) = (bytes[start..end].to_vec(), bytes[end..end + 8].to_vec());
    bytes[start..end].copy_from_slice(&b);
    bytes[end..end + 8].copy_from_slice(&a);

    assert!(matches!(mount(bytes), Err(PackageFsError::Malformed(_))));
}

#[test]
fn names_that_are_not_file_names_are_rejected_at_mount() {
    for bad in [&b".."[..], b"a/", b"a\0"] {
        let mut bytes = webc_bytes(vec![("ab", file(b"x"))]);
        // A directory entry: the name's length, then the name.
        let at = find(&bytes, b"\x02\0\0\0\0\0\0\0ab") + 8;
        bytes[at..at + 2].copy_from_slice(bad);
        assert!(
            matches!(mount(bytes), Err(PackageFsError::Malformed(_))),
            "{bad:?}"
        );
    }
}

#[test]
fn a_directory_that_contains_itself_is_rejected_at_mount() {
    let mut bytes = webc_bytes(vec![("d", WEntry::Dir(dir(vec![])))]);
    // The root's entry for `d`: its header offset, hash, name length and name. Offset 0 is the
    // root itself.
    let at = find(&bytes, b"\x01\0\0\0\0\0\0\0d") - 40;
    bytes[at..at + 8].copy_from_slice(&0u64.to_le_bytes());

    assert!(matches!(mount(bytes), Err(PackageFsError::Malformed(_))));
}

fn files(paths: &[(&str, &'static [u8])]) -> Result<PackageFs, PackageFsError> {
    let files = paths
        .iter()
        .map(|(path, data)| (path.to_string(), OwnedBuffer::from_static(data)));
    PackageFs::from_files(files, 1, 2)
}

#[test]
fn from_files_builds_the_directories_the_paths_imply() {
    let fs = files(&[
        ("bin/ls", b"\0asm ls"),
        ("usr/bin/ls", b"\0asm ls"),
        ("x", b""),
    ])
    .unwrap();
    let names: Vec<_> = read_dir(&*fs.root(), 0, usize::MAX)
        .into_iter()
        .map(|e| (e.0, e.2))
        .collect();
    assert_eq!(
        names,
        [
            (b"bin".to_vec(), FileType::Directory),
            (b"usr".to_vec(), FileType::Directory),
            (b"x".to_vec(), FileType::Regular),
        ]
    );
    let ls = lookup(&fs, "usr/bin/ls");
    let attr = block_on(ls.getattr()).unwrap();
    assert_eq!(
        (attr.file_type, attr.mode, attr.uid, attr.gid, attr.size),
        (FileType::Regular, 0o755, 1, 2, 7)
    );
    let handle = block_on(ls.open(OpenOptions {
        read: true,
        ..Default::default()
    }))
    .unwrap();
    let mut buf = [0; 16];
    assert_eq!(block_on(handle.read_at(&mut buf, 0)), Ok(7));
    assert_eq!(&buf[..7], b"\0asm ls");
    // `.`, the parent's entry and `..` of `bin`.
    assert_eq!(block_on(lookup(&fs, "usr").getattr()).unwrap().nlink, 3);
    assert!(fs.info().read_only);
    assert_eq!(block_on(fs.root().unlink(b"x")), Err(Errno::EROFS));
}

#[test]
fn from_files_numbers_every_object_once() {
    let fs = files(&[("bin/a", b"a"), ("bin/b", b"b"), ("usr/bin/a", b"a")]).unwrap();
    let mut all = vec![fs.root().ino()];
    for path in ["bin", "bin/a", "bin/b", "usr", "usr/bin", "usr/bin/a"] {
        let ino = lookup(&fs, path).ino();
        assert_eq!(ino, lookup(&fs, path).ino(), "{path}");
        all.push(ino);
    }
    let count = all.len();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), count);
    let entries = read_dir(&*lookup(&fs, "bin"), 0, usize::MAX);
    assert_eq!(entries[1].1, lookup(&fs, "bin/b").ino());
}

#[test]
fn from_files_rejects_bad_and_clashing_paths() {
    for path in [
        "", "/bin/a", "bin/", "bin//a", "bin/./a", "bin/../a", "a\0b",
    ] {
        assert!(
            matches!(files(&[(path, b"")]), Err(PackageFsError::InvalidPath(_))),
            "{path:?}"
        );
    }
    let long = "n".repeat(NAME_MAX + 1);
    assert!(matches!(
        files(&[(long.as_str(), b"")]).err(),
        Some(PackageFsError::InvalidPath(_))
    ));
    for clash in [
        &[("bin/a", &b""[..]), ("bin/a", b"")][..],
        &[("bin", b""), ("bin/a", b"")],
        &[("bin/a", b""), ("bin", b"")],
    ] {
        assert!(
            matches!(files(clash), Err(PackageFsError::Conflict(_))),
            "{clash:?}"
        );
    }
}

/// A path's length bounds the tree's depth: the deepest path that fits `PATH_MAX` builds,
/// works and drops on a test thread's stack, and a deeper one is refused before anything
/// recurses through it.
#[test]
fn from_files_bounds_the_depth() {
    let deepest = format!("{}x", "d/".repeat((PATH_MAX - 2) / 2));
    assert_eq!(deepest.len(), PATH_MAX - 1);
    let fs = files(&[(deepest.as_str(), b"deep")]).unwrap();
    let attr = block_on(lookup(&fs, &deepest).getattr()).unwrap();
    assert_eq!(attr.size, 4);
    drop(fs);

    let hostile = format!("{}x", "d/".repeat(1 << 20));
    assert!(matches!(
        files(&[(hostile.as_str(), b"")]),
        Err(PackageFsError::PathTooLong(len)) if len == hostile.len()
    ));
}
