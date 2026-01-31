use std::sync::Arc;

use vfs_core::flags::{OpenFlags, OpenOptions, ResolveFlags};
use vfs_core::node::{CreateFile, FsNode, MkdirOptions, RenameOptions, UnlinkOptions};
use vfs_core::path_types::VfsName;
use vfs_core::{Fs, VfsErrorKind, VfsResult};

use vfs_mem::MemFs;

use crate::config::{OverlayBuilder, OverlayOptions};

fn ensure_dir(root: Arc<dyn FsNode>, path: &str) -> VfsResult<Arc<dyn FsNode>> {
    let mut cur = root;
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        let name = VfsName::new(seg.as_bytes())?;
        match cur.lookup(&name) {
            Ok(node) => {
                cur = node;
            }
            Err(err) if err.kind() == VfsErrorKind::NotFound => {
                cur = cur.mkdir(&name, MkdirOptions { mode: None })?;
            }
            Err(err) => return Err(err),
        }
    }
    Ok(cur)
}

fn lookup_path(root: Arc<dyn FsNode>, path: &str) -> VfsResult<Arc<dyn FsNode>> {
    let mut cur = root;
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        let name = VfsName::new(seg.as_bytes())?;
        cur = cur.lookup(&name)?;
    }
    Ok(cur)
}

fn create_file_with_contents(root: Arc<dyn FsNode>, path: &str, data: &[u8]) -> VfsResult<()> {
    let (parent_path, name) = match path.rsplit_once('/') {
        Some((parent, name)) => (parent, name),
        None => ("", path),
    };
    let parent = ensure_dir(root, parent_path)?;
    let name = VfsName::new(name.as_bytes())?;
    let node = parent.create_file(
        &name,
        CreateFile {
            mode: None,
            truncate: true,
            exclusive: false,
        },
    )?;
    let handle = node.open(OpenOptions {
        flags: OpenFlags::WRITE | OpenFlags::TRUNC,
        mode: None,
        resolve: ResolveFlags::empty(),
    })?;
    handle.write_at(0, data)?;
    Ok(())
}

fn read_file(root: Arc<dyn FsNode>, path: &str) -> VfsResult<Vec<u8>> {
    let node = lookup_path(root, path)?;
    let meta = node.metadata()?;
    let handle = node.open(OpenOptions {
        flags: OpenFlags::READ,
        mode: None,
        resolve: ResolveFlags::empty(),
    })?;
    let mut buf = vec![0u8; meta.size as usize];
    let read = handle.read_at(0, &mut buf)?;
    buf.truncate(read);
    Ok(buf)
}

#[test]
fn upper_shadows_lower() {
    let upper = Arc::new(MemFs::new());
    let lower = Arc::new(MemFs::new());
    create_file_with_contents(lower.root(), "/a", b"lower").unwrap();
    create_file_with_contents(upper.root(), "/a", b"upper").unwrap();

    let overlay = OverlayBuilder::new(upper.clone(), vec![lower.clone()])
        .build()
        .unwrap();
    let data = read_file(overlay.root(), "/a").unwrap();
    assert_eq!(data, b"upper");
}

#[test]
fn merged_readdir() {
    let upper = Arc::new(MemFs::new());
    let lower = Arc::new(MemFs::new());
    create_file_with_contents(upper.root(), "/d/a", b"1").unwrap();
    create_file_with_contents(upper.root(), "/d/c", b"1").unwrap();
    create_file_with_contents(lower.root(), "/d/b", b"1").unwrap();
    create_file_with_contents(lower.root(), "/d/c", b"1").unwrap();

    let overlay = OverlayBuilder::new(upper, vec![lower]).build().unwrap();
    let dir = lookup_path(overlay.root(), "/d").unwrap();
    let batch = dir.read_dir(None, 16).unwrap();
    let names: Vec<_> = batch
        .entries
        .iter()
        .map(|e| String::from_utf8_lossy(e.name.as_bytes()).to_string())
        .collect();
    assert_eq!(
        names,
        vec!["a".to_string(), "c".to_string(), "b".to_string()]
    );
}

#[test]
fn whiteout_hides_lower() {
    let upper = Arc::new(MemFs::new());
    let lower = Arc::new(MemFs::new());
    create_file_with_contents(lower.root(), "/x", b"lower").unwrap();

    let overlay = OverlayBuilder::new(upper.clone(), vec![lower.clone()])
        .build()
        .unwrap();
    let root = overlay.root();
    root.unlink(
        &VfsName::new(b"x").unwrap(),
        UnlinkOptions { must_be_dir: false },
    )
    .unwrap();

    let err = lookup_path(root.clone(), "/x")
        .err()
        .expect("expected not found");
    assert_eq!(err.kind(), VfsErrorKind::NotFound);

    let batch = root.read_dir(None, 16).unwrap();
    let names: Vec<_> = batch
        .entries
        .iter()
        .map(|e| String::from_utf8_lossy(e.name.as_bytes()).to_string())
        .collect();
    assert!(!names.contains(&"x".to_string()));
}

#[test]
fn opaque_dir_hides_lower() {
    let upper = Arc::new(MemFs::new());
    let lower = Arc::new(MemFs::new());
    create_file_with_contents(lower.root(), "/d/file", b"lower").unwrap();
    ensure_dir(upper.root(), "/d").unwrap();
    create_file_with_contents(upper.root(), "/d/.wasmer_overlay.opaque", b"").unwrap();

    let overlay = OverlayBuilder::new(upper, vec![lower]).build().unwrap();
    let err = lookup_path(overlay.root(), "/d/file")
        .err()
        .expect("expected not found");
    assert_eq!(err.kind(), VfsErrorKind::NotFound);
}

#[test]
fn copy_up_on_open_for_write() {
    let upper = Arc::new(MemFs::new());
    let lower = Arc::new(MemFs::new());
    create_file_with_contents(lower.root(), "/f", b"lower").unwrap();

    let overlay = OverlayBuilder::new(upper.clone(), vec![lower.clone()])
        .build()
        .unwrap();
    let node = lookup_path(overlay.root(), "/f").unwrap();
    let handle = node
        .open(OpenOptions {
            flags: OpenFlags::WRITE,
            mode: None,
            resolve: ResolveFlags::empty(),
        })
        .unwrap();
    handle.write_at(0, b"upper").unwrap();

    let upper_data = read_file(upper.root(), "/f").unwrap();
    let lower_data = read_file(lower.root(), "/f").unwrap();
    assert_eq!(upper_data, b"upper");
    assert_eq!(lower_data, b"lower");
}

#[test]
fn rename_lower_only_file_supported() {
    let upper = Arc::new(MemFs::new());
    let lower = Arc::new(MemFs::new());
    create_file_with_contents(lower.root(), "/f", b"lower").unwrap();

    let overlay = OverlayBuilder::new(upper.clone(), vec![lower.clone()])
        .build()
        .unwrap();
    let root = overlay.root();
    root.rename(
        &VfsName::new(b"f").unwrap(),
        &*root,
        &VfsName::new(b"g").unwrap(),
        RenameOptions {
            noreplace: false,
            exchange: false,
        },
    )
    .unwrap();

    assert!(lookup_path(root.clone(), "/g").is_ok());
    assert_eq!(
        lookup_path(root, "/f")
            .err()
            .expect("expected not found")
            .kind(),
        VfsErrorKind::NotFound
    );
    assert!(lookup_path(upper.root(), "/g").is_ok());
}

#[test]
fn rename_lower_only_dir_enotsup() {
    let upper = Arc::new(MemFs::new());
    let lower = Arc::new(MemFs::new());
    ensure_dir(lower.root(), "/dir").unwrap();

    let overlay = OverlayBuilder::new(upper, vec![lower]).build().unwrap();
    let root = overlay.root();
    let err = root
        .rename(
            &VfsName::new(b"dir").unwrap(),
            &*root,
            &VfsName::new(b"dir2").unwrap(),
            RenameOptions {
                noreplace: false,
                exchange: false,
            },
        )
        .unwrap_err();
    assert_eq!(err.kind(), VfsErrorKind::NotSupported);
}

#[test]
fn inode_stable_across_copy_up() {
    let upper = Arc::new(MemFs::new());
    let lower = Arc::new(MemFs::new());
    ensure_dir(lower.root(), "/mp").unwrap();

    let overlay = OverlayBuilder::new(upper.clone(), vec![lower])
        .build()
        .unwrap();
    let root = overlay.root();
    let mp = lookup_path(root.clone(), "/mp").unwrap();
    let inode_before = mp.inode();

    create_file_with_contents(root.clone(), "/mp/file", b"x").unwrap();
    let mp_after = lookup_path(root, "/mp").unwrap();
    let inode_after = mp_after.inode();

    assert_eq!(inode_before, inode_after);
}

#[test]
fn reserved_names_hidden() {
    let upper = Arc::new(MemFs::new());
    let lower = Arc::new(MemFs::new());
    create_file_with_contents(upper.root(), "/.wasmer_overlay.opaque", b"").unwrap();
    create_file_with_contents(upper.root(), "/.wasmer_overlay.wh.hidden", b"").unwrap();
    create_file_with_contents(lower.root(), "/visible", b"ok").unwrap();

    let overlay = OverlayBuilder::new(upper, vec![lower]).build().unwrap();
    let root = overlay.root();

    let err = root
        .lookup(&VfsName::new(b".wasmer_overlay.opaque").unwrap())
        .err()
        .expect("expected error");
    assert_eq!(err.kind(), VfsErrorKind::NotFound);
    let err = root
        .lookup(&VfsName::new(b".wasmer_overlay.wh.hidden").unwrap())
        .err()
        .expect("expected error");
    assert_eq!(err.kind(), VfsErrorKind::NotFound);

    let batch = root.read_dir(None, 16).unwrap();
    let names: Vec<_> = batch
        .entries
        .iter()
        .map(|e| String::from_utf8_lossy(e.name.as_bytes()).to_string())
        .collect();
    assert!(names.contains(&"visible".to_string()));
    assert!(
        names
            .iter()
            .all(|name| !name.starts_with(".wasmer_overlay."))
    );
}

#[test]
fn deny_reserved_names_on_create() {
    let upper = Arc::new(MemFs::new());
    let lower = Arc::new(MemFs::new());
    let overlay = OverlayBuilder::new(upper, vec![lower])
        .with_options(OverlayOptions {
            deny_reserved_names: true,
            ..OverlayOptions::default()
        })
        .build()
        .unwrap();
    let root = overlay.root();
    let err = root
        .create_file(
            &VfsName::new(b".wasmer_overlay.opaque").unwrap(),
            CreateFile {
                mode: None,
                truncate: true,
                exclusive: false,
            },
        )
        .err()
        .expect("expected error");
    assert_eq!(err.kind(), VfsErrorKind::PermissionDenied);
}

#[test]
fn multi_lower_layering_order_and_lookup() {
    let upper = Arc::new(MemFs::new());
    let lower0 = Arc::new(MemFs::new());
    let lower1 = Arc::new(MemFs::new());

    create_file_with_contents(upper.root(), "/d/upper_a", b"u").unwrap();
    create_file_with_contents(upper.root(), "/d/upper_c", b"u").unwrap();
    create_file_with_contents(lower0.root(), "/d/lower_b", b"l0").unwrap();
    create_file_with_contents(lower0.root(), "/d/lower_d", b"l0").unwrap();
    create_file_with_contents(lower0.root(), "/d/shared", b"lower0").unwrap();
    create_file_with_contents(lower1.root(), "/d/lower_e", b"l1").unwrap();
    create_file_with_contents(lower1.root(), "/d/lower_f", b"l1").unwrap();
    create_file_with_contents(lower1.root(), "/d/shared", b"lower1").unwrap();

    let overlay = OverlayBuilder::new(upper, vec![lower0, lower1])
        .build()
        .unwrap();
    let dir = lookup_path(overlay.root(), "/d").unwrap();
    let batch = dir.read_dir(None, 16).unwrap();
    let names: Vec<_> = batch
        .entries
        .iter()
        .map(|e| String::from_utf8_lossy(e.name.as_bytes()).to_string())
        .collect();

    assert!(names.contains(&"upper_a".to_string()));
    assert!(names.contains(&"upper_c".to_string()));
    assert!(names.contains(&"lower_b".to_string()));
    assert!(names.contains(&"lower_d".to_string()));
    assert!(names.contains(&"lower_e".to_string()));
    assert!(names.contains(&"lower_f".to_string()));
    assert!(names.contains(&"shared".to_string()));
    assert_eq!(names.iter().filter(|name| *name == "shared").count(), 1);

    let pos = |name: &str| names.iter().position(|n| n == name).unwrap();
    assert!(pos("upper_a") < pos("lower_b"));
    assert!(pos("upper_c") < pos("lower_b"));
    assert!(pos("lower_b") < pos("lower_e"));
    assert!(pos("lower_d") < pos("lower_e"));

    let shared = read_file(overlay.root(), "/d/shared").unwrap();
    assert_eq!(shared, b"lower0");
}
