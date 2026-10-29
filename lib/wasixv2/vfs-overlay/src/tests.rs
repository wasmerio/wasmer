use futures::executor::block_on;
use wasixv2_vfs::{Clock, Timestamp};
use wasixv2_vfs_mem::MemFs;

use super::*;

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        Timestamp::new(1_700_000_000, 0)
    }
}

fn mem() -> Arc<dyn Filesystem> {
    Arc::new(MemFs::new(Arc::new(FixedClock), 0o755, 1, 1))
}

fn new(kind: NewKind<'_>) -> NewNode<'_> {
    NewNode {
        kind,
        mode: 0o644,
        uid: 1,
        gid: 1,
        task: 0,
        quota_exempt: false,
    }
}

fn write(dir: &Arc<dyn Node>, name: &[u8], data: &[u8]) -> Arc<dyn Node> {
    let file = block_on(dir.create(name, &new(NewKind::Regular))).unwrap();
    let handle = block_on(file.open(OpenOptions {
        write: true,
        ..OpenOptions::default()
    }))
    .unwrap();
    block_on(handle.write_at(data, 0)).unwrap();
    file
}

fn mkdir(dir: &Arc<dyn Node>, name: &[u8]) -> Arc<dyn Node> {
    block_on(dir.create(name, &new(NewKind::Directory))).unwrap()
}

fn lookup(dir: &Arc<dyn Node>, name: &[u8]) -> FsResult<Arc<dyn Node>> {
    block_on(dir.lookup(name))
}

fn read(node: &Arc<dyn Node>) -> Vec<u8> {
    let handle = block_on(node.open(OpenOptions {
        read: true,
        ..OpenOptions::default()
    }))
    .unwrap();
    let mut buf = vec![0; 64];
    let n = block_on(handle.read_at(&mut buf, 0)).unwrap();
    buf.truncate(n);
    buf
}

struct Names(Vec<(Vec<u8>, u64, u64)>, usize);

impl DirSink for Names {
    fn push(&mut self, entry: DirEntry<'_>) -> bool {
        if self.0.len() == self.1 {
            return false;
        }
        self.0.push((entry.name.to_vec(), entry.ino, entry.next));
        true
    }
}

fn list(dir: &Arc<dyn Node>) -> Vec<Vec<u8>> {
    let handle = block_on(dir.open(OpenOptions::default())).unwrap();
    let mut names = Names(Vec::new(), usize::MAX);
    block_on(handle.readdir(0, &mut names)).unwrap();
    let mut names: Vec<_> = names.0.into_iter().map(|(name, _, _)| name).collect();
    names.sort();
    names
}

/// A lower layer with `d/f` = "hello" carrying xattr `user.x`, under an empty upper layer.
fn setup() -> (
    Arc<dyn Filesystem>,
    Arc<dyn Filesystem>,
    Arc<dyn Filesystem>,
) {
    let lower = mem();
    let d = mkdir(&lower.root(), b"d");
    let f = write(&d, b"f", b"hello");
    block_on(f.setxattr(b"user.x", b"1", XattrSet::Any)).unwrap();
    let upper = mem();
    let overlay = OverlayFs::new(Some(upper.clone()), vec![lower.clone()]);
    (overlay, upper, lower)
}

#[test]
fn write_copies_up_file_and_parents_keeping_ino() {
    let (overlay, upper, lower) = setup();
    let d = lookup(&overlay.root(), b"d").unwrap();
    let f = lookup(&d, b"f").unwrap();
    let ino = f.ino();
    let reader = block_on(f.open(OpenOptions {
        read: true,
        ..OpenOptions::default()
    }))
    .unwrap();

    let writer = block_on(f.open(OpenOptions {
        write: true,
        ..OpenOptions::default()
    }))
    .unwrap();
    block_on(writer.write_at(b"J", 0)).unwrap();

    let upper_f = lookup(&lookup(&upper.root(), b"d").unwrap(), b"f").unwrap();
    assert_eq!(read(&upper_f), b"Jello");
    assert_eq!(block_on(upper_f.getxattr(b"user.x")), Ok(b"1".to_vec()));
    let lower_f = lookup(&lookup(&lower.root(), b"d").unwrap(), b"f").unwrap();
    assert_eq!(read(&lower_f), b"hello");
    // A reader opened before the copy-up sees the copy.
    let mut buf = [0; 5];
    assert_eq!(block_on(reader.read_at(&mut buf, 0)), Ok(5));
    assert_eq!(&buf, b"Jello");
    let again = lookup(&d, b"f").unwrap();
    assert_eq!(
        (again.ino(), block_on(again.getattr()).unwrap().ino),
        (ino, ino)
    );
}

#[test]
fn lower_hard_links_are_separate_objects_with_their_own_inos() {
    let (overlay, _upper, lower) = setup();
    let lower_d = lookup(&lower.root(), b"d").unwrap();
    let lower_f = lookup(&lower_d, b"f").unwrap();
    block_on(lower_d.link(b"g", &lower_f)).unwrap();

    let d = lookup(&overlay.root(), b"d").unwrap();
    let (f, g) = (lookup(&d, b"f").unwrap(), lookup(&d, b"g").unwrap());
    assert_ne!(f.ino(), g.ino());
    let ino = f.ino();
    block_on(f.setattr(&SetAttr::default())).unwrap();
    assert_eq!(lookup(&d, b"f").unwrap().ino(), ino);
    assert_eq!(lookup(&d, b"g").unwrap().ino(), g.ino());
    assert_ne!(ino, g.ino());
}

#[test]
fn setattr_and_xattr_copy_up() {
    let (overlay, upper, _) = setup();
    let f = lookup(&lookup(&overlay.root(), b"d").unwrap(), b"f").unwrap();
    let attr = block_on(f.setattr(&SetAttr {
        size: Some(2),
        ..SetAttr::default()
    }))
    .unwrap();
    assert_eq!(attr.size, 2);
    block_on(f.setxattr(b"user.y", b"2", XattrSet::Any)).unwrap();
    let upper_f = lookup(&lookup(&upper.root(), b"d").unwrap(), b"f").unwrap();
    assert_eq!(read(&upper_f), b"he");
    assert_eq!(block_on(upper_f.getxattr(b"user.y")), Ok(b"2".to_vec()));
}

#[test]
fn unlink_of_lower_file_leaves_whiteout() {
    let (overlay, upper, lower) = setup();
    let d = lookup(&overlay.root(), b"d").unwrap();

    block_on(d.unlink(b"f")).unwrap();

    assert_eq!(lookup(&d, b"f").err(), Some(Errno::ENOENT));
    assert!(list(&d).is_empty());
    let whiteout = lookup(&lookup(&upper.root(), b"d").unwrap(), b"f").unwrap();
    assert_eq!(whiteout.file_type(), FileType::CharDevice);
    assert!(lookup(&lookup(&lower.root(), b"d").unwrap(), b"f").is_ok());
    // Creating the name again replaces the whiteout.
    let f = write(&d, b"f", b"new");
    assert_eq!(read(&f), b"new");
    assert_eq!(list(&d), vec![b"f".to_vec()]);
}

#[test]
fn rmdir_needs_emptiness_across_layers() {
    let (overlay, _, _) = setup();
    let root = overlay.root();
    let d = lookup(&root, b"d").unwrap();
    write(&d, b"g", b"upper");

    assert_eq!(block_on(root.rmdir(b"d")), Err(Errno::ENOTEMPTY));
    block_on(d.unlink(b"g")).unwrap();
    assert_eq!(block_on(root.rmdir(b"d")), Err(Errno::ENOTEMPTY));
    block_on(d.unlink(b"f")).unwrap();
    block_on(root.rmdir(b"d")).unwrap();

    assert_eq!(lookup(&root, b"d").err(), Some(Errno::ENOENT));
    assert!(list(&root).is_empty());
}

#[test]
fn directory_recreated_over_whiteout_is_opaque() {
    let (overlay, upper, _) = setup();
    let root = overlay.root();
    block_on(lookup(&root, b"d").unwrap().unlink(b"f")).unwrap();
    block_on(root.rmdir(b"d")).unwrap();

    let d = mkdir(&root, b"d");

    assert!(list(&d).is_empty());
    assert_eq!(lookup(&d, b"f").err(), Some(Errno::ENOENT));
    let upper_d = lookup(&upper.root(), b"d").unwrap();
    assert_eq!(
        block_on(upper_d.getxattr(b"trusted.overlay.opaque")),
        Ok(b"y".to_vec())
    );
    // Overlay xattrs are hidden.
    assert_eq!(
        block_on(d.getxattr(b"trusted.overlay.opaque")),
        Err(Errno::EOPNOTSUPP)
    );
    assert!(block_on(d.listxattr()).unwrap().is_empty());
}

#[test]
fn readdir_merges_layers_with_matching_inos_and_cookies() {
    let top = mem();
    write(&top.root(), b"a", b"");
    write(&top.root(), b"b", b"top");
    mkdir(&top.root(), b"sub");
    let bottom = mem();
    write(&bottom.root(), b"b", b"bottom");
    write(&bottom.root(), b"c", b"");
    write(&mkdir(&bottom.root(), b"sub"), b"x", b"");
    let upper = mem();
    mkdir(&upper.root(), b"sub");
    write(&upper.root(), b"u", b"");
    let overlay = OverlayFs::new(Some(upper), vec![top, bottom]);
    let root = overlay.root();

    let handle = block_on(root.open(OpenOptions::default())).unwrap();
    let mut first = Names(Vec::new(), 2);
    block_on(handle.readdir(0, &mut first)).unwrap();
    let mut rest = Names(Vec::new(), usize::MAX);
    block_on(handle.readdir(first.0[1].2, &mut rest)).unwrap();
    let mut all: Vec<_> = first.0.into_iter().chain(rest.0).collect();
    all.sort();

    let names: Vec<&[u8]> = all.iter().map(|(name, _, _)| name.as_slice()).collect();
    assert_eq!(names, [&b"a"[..], b"b", b"c", b"sub", b"u"]);
    for (name, ino, _) in &all {
        assert_eq!(lookup(&root, name).unwrap().ino(), *ino);
    }
    assert_eq!(read(&lookup(&root, b"b").unwrap()), b"top");
    let sub = lookup(&root, b"sub").unwrap();
    assert_eq!(list(&sub), vec![b"x".to_vec()]);
    assert_eq!(block_on(sub.getattr()).unwrap().nlink, 1);
}

#[test]
fn renaming_lower_directory_is_exdev_and_files_move() {
    let (overlay, _, _) = setup();
    let root = overlay.root();
    assert_eq!(
        block_on(root.rename(b"d", &root, b"e", RenameMode::Replace)),
        Err(Errno::EXDEV)
    );

    let d = lookup(&root, b"d").unwrap();
    block_on(d.rename(b"f", &root, b"g", RenameMode::Replace)).unwrap();

    assert_eq!(read(&lookup(&root, b"g").unwrap()), b"hello");
    assert_eq!(lookup(&d, b"f").err(), Some(Errno::ENOENT));
}

#[test]
fn read_only_overlay_refuses_changes() {
    let (_, _, lower) = setup();
    let overlay = OverlayFs::new(None, vec![lower]);
    let root = overlay.root();
    let f = lookup(&lookup(&root, b"d").unwrap(), b"f").unwrap();

    assert!(overlay.info().read_only);
    assert_eq!(read(&f), b"hello");
    assert_eq!(
        block_on(root.create(b"n", &new(NewKind::Regular))).err(),
        Some(Errno::EROFS)
    );
    assert_eq!(
        block_on(f.setattr(&SetAttr::default())).err(),
        Some(Errno::EROFS)
    );
    let write = OpenOptions {
        write: true,
        ..OpenOptions::default()
    };
    assert_eq!(block_on(f.open(write)).err(), Some(Errno::EROFS));
    assert_eq!(block_on(root.rmdir(b"d")), Err(Errno::ENOTEMPTY));
}

#[test]
fn copy_up_of_an_unlinked_open_file_stays_unnamed() {
    let (overlay, upper, _lower) = setup();
    let d = lookup(&overlay.root(), b"d").unwrap();
    let f = lookup(&d, b"f").unwrap();
    let reader = block_on(f.open(OpenOptions {
        read: true,
        ..OpenOptions::default()
    }))
    .unwrap();
    block_on(d.unlink(b"f")).unwrap();

    // `fchmod` on the still-open file copies it up without giving it its old name back.
    let chmod = SetAttr {
        mode: Some(0o600),
        ..SetAttr::default()
    };
    assert_eq!(block_on(f.setattr(&chmod)).unwrap().mode, 0o600);
    assert_eq!(lookup(&d, b"f").err(), Some(Errno::ENOENT));
    assert!(list(&d).is_empty());
    assert_eq!(list(&lookup(&upper.root(), b"d").unwrap()), [b"f".to_vec()]);
    let mut buf = [0; 5];
    assert_eq!(block_on(reader.read_at(&mut buf, 0)), Ok(5));
    assert_eq!(&buf, b"hello");
}

#[test]
fn opening_for_truncation_copies_up_no_data() {
    let (overlay, upper, _lower) = setup();
    let f = lookup(&lookup(&overlay.root(), b"d").unwrap(), b"f").unwrap();

    let writer = block_on(f.open(OpenOptions {
        write: true,
        truncate: true,
        ..OpenOptions::default()
    }))
    .unwrap();
    block_on(writer.write_at(b"new", 0)).unwrap();

    let upper_f = lookup(&lookup(&upper.root(), b"d").unwrap(), b"f").unwrap();
    assert_eq!(read(&upper_f), b"new");
    // The rest of the copy is metadata: xattrs, mode and times.
    assert_eq!(block_on(upper_f.getxattr(b"user.x")), Ok(b"1".to_vec()));
    assert_eq!(block_on(upper_f.getattr()).unwrap().mode, 0o644);
}

#[test]
fn whiteouts_replacing_upper_entries_leave_no_temporary_names() {
    let (overlay, upper, _lower) = setup();
    let d = lookup(&overlay.root(), b"d").unwrap();
    let f = lookup(&d, b"f").unwrap();
    block_on(f.setattr(&SetAttr {
        mode: Some(0o600),
        ..SetAttr::default()
    }))
    .unwrap();

    block_on(d.unlink(b"f")).unwrap();
    assert_eq!(lookup(&d, b"f").err(), Some(Errno::ENOENT));
    assert!(list(&d).is_empty());
    // The upper directory holds only the whiteout, under the removed name.
    assert_eq!(list(&lookup(&upper.root(), b"d").unwrap()), [b"f".to_vec()]);
}
