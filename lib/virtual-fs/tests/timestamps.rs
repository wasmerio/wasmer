use std::{path::Path, sync::Arc};
use virtual_fs::{FileSystem, FsError, MountFileSystem, TmpFileSystem, mem_fs};

const ATIME: u64 = 1_700_000_123_456_789_000;
const MTIME: u64 = 1_700_000_234_567_890_000;

#[test]
fn directory_heartbeats_are_shared_through_mounts_and_preserve_omitted_times() {
    let backing = Arc::new(TmpFileSystem::new());
    backing.create_dir(Path::new("/settings.lock")).unwrap();
    let parent_before = backing.metadata(Path::new("/")).unwrap();
    let first = MountFileSystem::new();
    first.mount("/home", backing.clone()).unwrap();
    let second = MountFileSystem::new();
    second.mount("/home", backing.clone()).unwrap();

    first
        .set_times(
            Path::new("/home/settings.lock"),
            Some(ATIME),
            Some(MTIME),
            true,
        )
        .unwrap();
    let metadata = second.metadata(Path::new("/home/settings.lock")).unwrap();
    assert_eq!((metadata.accessed(), metadata.modified()), (ATIME, MTIME));
    second
        .set_times(
            Path::new("/home/settings.lock"),
            None,
            Some(MTIME + 1),
            true,
        )
        .unwrap();
    let metadata = first.metadata(Path::new("/home/settings.lock")).unwrap();
    assert_eq!(
        (metadata.accessed(), metadata.modified()),
        (ATIME, MTIME + 1)
    );
    let parent_after = backing.metadata(Path::new("/")).unwrap();
    assert_eq!(parent_before.modified(), parent_after.modified());
    assert!(
        backing
            .read_dir(Path::new("/settings.lock"))
            .unwrap()
            .next()
            .is_none()
    );
    assert_eq!(
        first.set_times(Path::new("/home/missing"), Some(ATIME), None, true),
        Err(FsError::EntryNotFound)
    );
}

#[test]
fn memory_symlink_updates_do_not_touch_target_unless_requested() {
    let fs = mem_fs::FileSystem::default();
    fs.create_dir(Path::new("/target")).unwrap();
    fs.create_symlink(Path::new("target"), Path::new("/link"))
        .unwrap();
    fs.set_times(Path::new("/target"), Some(ATIME), Some(MTIME), true)
        .unwrap();
    fs.set_times(Path::new("/link"), Some(ATIME + 1), Some(MTIME + 1), false)
        .unwrap();
    assert_eq!(fs.metadata(Path::new("/target")).unwrap().modified(), MTIME);
    assert_eq!(
        fs.symlink_metadata(Path::new("/link")).unwrap().modified(),
        MTIME + 1
    );
    fs.set_times(Path::new("/link"), None, Some(MTIME + 2), true)
        .unwrap();
    assert_eq!(
        fs.metadata(Path::new("/target")).unwrap().modified(),
        MTIME + 2
    );
    assert_eq!(
        fs.symlink_metadata(Path::new("/link")).unwrap().modified(),
        MTIME + 1
    );
}

#[test]
fn memory_mounted_root_metadata_tracks_the_shared_backing_directory() {
    let backing = Arc::new(mem_fs::FileSystem::default());
    backing.create_dir(Path::new("/source")).unwrap();
    let fs = mem_fs::FileSystem::default();
    let erased: Arc<dyn FileSystem + Send + Sync> = backing.clone();
    fs.insert_arc_directory_at("/mount".into(), erased, "/source".into())
        .unwrap();
    fs.set_times(Path::new("/mount"), Some(ATIME), Some(MTIME), true)
        .unwrap();
    assert_eq!(
        backing.metadata(Path::new("/source")).unwrap().modified(),
        MTIME
    );
    backing
        .set_times(Path::new("/source"), None, Some(MTIME + 1), true)
        .unwrap();
    assert_eq!(
        fs.metadata(Path::new("/mount")).unwrap().modified(),
        MTIME + 1
    );
}

#[cfg(feature = "host-fs")]
#[tokio::test]
async fn host_path_and_handle_timestamps_use_nanoseconds_and_survive_reopening() {
    let temp = tempfile::tempdir().unwrap();
    let fs = virtual_fs::host_fs::FileSystem::new(tokio::runtime::Handle::current(), temp.path())
        .unwrap();
    fs.create_dir(Path::new("/lock")).unwrap();
    fs.set_times(Path::new("/lock"), Some(ATIME), Some(MTIME), true)
        .unwrap();
    let other =
        virtual_fs::host_fs::FileSystem::new(tokio::runtime::Handle::current(), temp.path())
            .unwrap();
    let stat = other.metadata(Path::new("/lock")).unwrap();
    assert_eq!((stat.accessed(), stat.modified()), (ATIME, MTIME));
    let mut file = fs
        .new_open_options()
        .write(true)
        .create_new(true)
        .open("/file")
        .unwrap();
    file.set_times(Some(ATIME), Some(MTIME)).unwrap();
    drop(file);
    let mut file = other.new_open_options().read(true).open("/file").unwrap();
    assert_eq!((file.last_accessed(), file.last_modified()), (ATIME, MTIME));
    file.set_times(None, Some(MTIME + 1000)).unwrap();
    assert_eq!(
        (file.last_accessed(), file.last_modified()),
        (ATIME, MTIME + 1000)
    );
    assert_eq!(
        fs.set_times(Path::new("/missing"), Some(ATIME), None, true),
        Err(FsError::EntryNotFound)
    );
}

#[cfg(all(feature = "host-fs", unix))]
#[tokio::test]
async fn host_nofollow_updates_symlink_including_dangling_symlinks() {
    let temp = tempfile::tempdir().unwrap();
    let fs = virtual_fs::host_fs::FileSystem::new(tokio::runtime::Handle::current(), temp.path())
        .unwrap();
    fs.create_dir(Path::new("/target")).unwrap();
    std::os::unix::fs::symlink("target", temp.path().join("link")).unwrap();
    std::os::unix::fs::symlink("missing", temp.path().join("dangling")).unwrap();
    fs.set_times(Path::new("/target"), Some(ATIME), Some(MTIME), true)
        .unwrap();
    fs.set_times(
        Path::new("/link"),
        Some(ATIME + 1000),
        Some(MTIME + 1000),
        false,
    )
    .unwrap();
    assert_eq!(fs.metadata(Path::new("/target")).unwrap().modified(), MTIME);
    assert_eq!(
        fs.symlink_metadata(Path::new("/link")).unwrap().modified(),
        MTIME + 1000
    );
    fs.set_times(Path::new("/dangling"), Some(ATIME), Some(MTIME), false)
        .unwrap();
    assert_eq!(
        fs.symlink_metadata(Path::new("/dangling"))
            .unwrap()
            .modified(),
        MTIME
    );
    fs.set_times(Path::new("/link"), None, Some(MTIME + 2000), true)
        .unwrap();
    assert_eq!(
        fs.metadata(Path::new("/target")).unwrap().modified(),
        MTIME + 2000
    );
}

#[test]
fn arc_file_path_and_open_handle_share_timestamps_without_reopening_replaced_paths() {
    let backing = Arc::new(mem_fs::FileSystem::default());
    backing
        .new_open_options()
        .write(true)
        .create_new(true)
        .open("/source")
        .unwrap();
    let fs = mem_fs::FileSystem::default();
    let erased: Arc<dyn FileSystem + Send + Sync> = backing.clone();
    fs.insert_arc_file_at("/alias".into(), erased, "/source".into())
        .unwrap();
    let mut handle = fs
        .new_open_options()
        .read(true)
        .write(true)
        .open("/alias")
        .unwrap();
    fs.set_times(Path::new("/alias"), Some(ATIME), Some(MTIME), true)
        .unwrap();
    assert_eq!(
        (handle.last_accessed(), handle.last_modified()),
        (ATIME, MTIME)
    );
    handle.set_times(None, Some(MTIME + 1)).unwrap();
    assert_eq!(
        fs.metadata(Path::new("/alias")).unwrap().modified(),
        MTIME + 1
    );
    assert_eq!(
        backing.metadata(Path::new("/source")).unwrap().modified(),
        MTIME + 1
    );

    backing.remove_file(Path::new("/source")).unwrap();
    backing
        .new_open_options()
        .write(true)
        .create_new(true)
        .open("/source")
        .unwrap();
    backing
        .set_times(
            Path::new("/source"),
            Some(ATIME + 100),
            Some(MTIME + 100),
            true,
        )
        .unwrap();
    handle.set_times(None, Some(MTIME + 2)).unwrap();
    assert_eq!(handle.last_modified(), MTIME + 2);
    assert_eq!(
        fs.metadata(Path::new("/alias")).unwrap().modified(),
        MTIME + 100
    );
}
