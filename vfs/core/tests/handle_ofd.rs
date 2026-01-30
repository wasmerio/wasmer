use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

use vfs_core::flags::HandleStatusFlags;
use vfs_core::inode::make_vfs_inode;
use vfs_core::node::{FsHandle, FsNode};
use vfs_core::{
    BackendInodeId, MountId, VfsError, VfsErrorKind, VfsFileMode, VfsFileType, VfsHandle,
    VfsHandleId, VfsMetadata, VfsResult, VfsTimespec,
};
use vfs_ratelimit::LimiterChain;

struct MemHandle {
    inode: BackendInodeId,
    file_type: VfsFileType,
    data: Mutex<Vec<u8>>,
}

impl MemHandle {
    fn new(inode: BackendInodeId, file_type: VfsFileType) -> Self {
        Self {
            inode,
            file_type,
            data: Mutex::new(Vec::new()),
        }
    }
}

impl FsHandle for MemHandle {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let data = self.data.lock().unwrap();
        let start = offset as usize;
        if start >= data.len() {
            return Ok(0);
        }
        let end = usize::min(data.len(), start + buf.len());
        let count = end - start;
        buf[..count].copy_from_slice(&data[start..end]);
        Ok(count)
    }

    fn write_at(&self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        let mut data = self.data.lock().unwrap();
        let start = offset as usize;
        if start > data.len() {
            data.resize(start, 0);
        }
        let end = start + buf.len();
        if end > data.len() {
            data.resize(end, 0);
        }
        data[start..end].copy_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&self) -> VfsResult<()> {
        Ok(())
    }

    fn fsync(&self) -> VfsResult<()> {
        Ok(())
    }

    fn get_metadata(&self) -> VfsResult<VfsMetadata> {
        let size = self.data.lock().unwrap().len() as u64;
        Ok(VfsMetadata {
            inode: make_vfs_inode(MountId::from_index(0), self.inode),
            file_type: self.file_type,
            mode: VfsFileMode(0o666),
            nlink: 1,
            uid: 0,
            gid: 0,
            size,
            atime: VfsTimespec { secs: 0, nanos: 0 },
            mtime: VfsTimespec { secs: 0, nanos: 0 },
            ctime: VfsTimespec { secs: 0, nanos: 0 },
            rdev_major: 0,
            rdev_minor: 0,
        })
    }

    fn set_len(&self, len: u64) -> VfsResult<()> {
        let mut data = self.data.lock().unwrap();
        data.resize(len as usize, 0);
        Ok(())
    }
}

struct DummyNode;

impl DummyNode {
    fn unsupported<T>(&self, op: &'static str) -> VfsResult<T> {
        Err(VfsError::new(VfsErrorKind::NotSupported, op))
    }
}

impl FsNode for DummyNode {
    fn inode(&self) -> BackendInodeId {
        BackendInodeId::new(1).expect("non-zero inode")
    }

    fn file_type(&self) -> VfsFileType {
        VfsFileType::Directory
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(VfsMetadata {
            inode: make_vfs_inode(MountId::from_index(0), self.inode()),
            file_type: self.file_type(),
            mode: VfsFileMode(0o755),
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 0,
            atime: VfsTimespec { secs: 0, nanos: 0 },
            mtime: VfsTimespec { secs: 0, nanos: 0 },
            ctime: VfsTimespec { secs: 0, nanos: 0 },
            rdev_major: 0,
            rdev_minor: 0,
        })
    }

    fn set_metadata(&self, _set: vfs_core::VfsSetMetadata) -> VfsResult<()> {
        self.unsupported("dummy.set_metadata")
    }

    fn lookup(&self, _name: &vfs_core::VfsName) -> VfsResult<Arc<dyn FsNode>> {
        self.unsupported("dummy.lookup")
    }

    fn create_file(
        &self,
        _name: &vfs_core::VfsName,
        _opts: vfs_core::node::CreateFile,
    ) -> VfsResult<Arc<dyn FsNode>> {
        self.unsupported("dummy.create_file")
    }

    fn mkdir(
        &self,
        _name: &vfs_core::VfsName,
        _opts: vfs_core::node::MkdirOptions,
    ) -> VfsResult<Arc<dyn FsNode>> {
        self.unsupported("dummy.mkdir")
    }

    fn unlink(
        &self,
        _name: &vfs_core::VfsName,
        _opts: vfs_core::node::UnlinkOptions,
    ) -> VfsResult<()> {
        self.unsupported("dummy.unlink")
    }

    fn rmdir(&self, _name: &vfs_core::VfsName) -> VfsResult<()> {
        self.unsupported("dummy.rmdir")
    }

    fn read_dir(
        &self,
        _cursor: Option<vfs_core::node::DirCursor>,
        _max: usize,
    ) -> VfsResult<vfs_core::node::ReadDirBatch> {
        self.unsupported("dummy.read_dir")
    }

    fn rename(
        &self,
        _old_name: &vfs_core::VfsName,
        _new_parent: &dyn FsNode,
        _new_name: &vfs_core::VfsName,
        _opts: vfs_core::node::RenameOptions,
    ) -> VfsResult<()> {
        self.unsupported("dummy.rename")
    }

    fn open(&self, _opts: vfs_core::flags::OpenOptions) -> VfsResult<Arc<dyn FsHandle>> {
        self.unsupported("dummy.open")
    }

    fn link(&self, _existing: &dyn FsNode, _new_name: &vfs_core::VfsName) -> VfsResult<()> {
        self.unsupported("dummy.link")
    }

    fn symlink(&self, _new_name: &vfs_core::VfsName, _target: &vfs_core::VfsPath) -> VfsResult<()> {
        self.unsupported("dummy.symlink")
    }

    fn readlink(&self) -> VfsResult<vfs_core::VfsPathBuf> {
        self.unsupported("dummy.readlink")
    }
}

struct DummyFs {
    root: Arc<dyn FsNode>,
}

impl DummyFs {
    fn new() -> Self {
        Self {
            root: Arc::new(DummyNode),
        }
    }
}

impl vfs_core::Fs for DummyFs {
    fn provider_name(&self) -> &'static str {
        "dummy"
    }

    fn capabilities(&self) -> vfs_core::VfsCapabilities {
        vfs_core::VfsCapabilities::NONE
    }

    fn root(&self) -> Arc<dyn FsNode> {
        self.root.clone()
    }
}

fn make_handle(handle_id: u64, open_flags: vfs_core::OpenFlags) -> VfsHandle {
    let fs: Arc<dyn vfs_core::Fs> = Arc::new(DummyFs::new());
    let mount_table = vfs_core::mount::MountTable::new(fs).expect("mount table");
    let guard = mount_table
        .guard(MountId::from_index(0))
        .expect("mount guard");

    let inode = BackendInodeId::new(10).expect("non-zero inode");
    let vfs_inode = make_vfs_inode(MountId::from_index(0), inode);
    let backend = Arc::new(MemHandle::new(inode, VfsFileType::RegularFile));

    VfsHandle::new(
        VfsHandleId(handle_id),
        guard,
        vfs_inode,
        VfsFileType::RegularFile,
        backend,
        open_flags,
        LimiterChain::default(),
    )
}

#[test]
fn dup_shares_offset() {
    let handle = make_handle(1, vfs_core::OpenFlags::READ | vfs_core::OpenFlags::WRITE);
    handle.write(b"hello").expect("write");
    handle.seek(std::io::SeekFrom::Start(0)).expect("seek");

    let handle_b = handle.clone();
    let mut buf = [0u8; 2];
    handle.read(&mut buf).expect("read a");
    assert_eq!(&buf, b"he");

    let mut buf2 = [0u8; 2];
    handle_b.read(&mut buf2).expect("read b");
    assert_eq!(&buf2, b"ll");
}

#[test]
fn pread_does_not_change_offset() {
    let handle = make_handle(2, vfs_core::OpenFlags::READ | vfs_core::OpenFlags::WRITE);
    handle.write(b"hello").expect("write");
    handle.seek(std::io::SeekFrom::Start(4)).expect("seek");

    let mut buf = [0u8; 2];
    handle.pread_at(0, &mut buf).expect("pread");
    assert_eq!(&buf, b"he");
    assert_eq!(handle.tell(), 4);
}

#[test]
fn append_is_atomic_within_ofd() {
    let handle = Arc::new(make_handle(
        3,
        vfs_core::OpenFlags::READ | vfs_core::OpenFlags::WRITE | vfs_core::OpenFlags::APPEND,
    ));
    handle
        .set_status_flags(HandleStatusFlags::APPEND)
        .expect("set append");

    let thread_count = 4usize;
    let writes_per_thread = 50usize;
    let chunk_len = 4usize;

    let mut handles = Vec::new();
    for tid in 0..thread_count {
        let handle = handle.clone();
        handles.push(thread::spawn(move || {
            let chunk = vec![tid as u8; chunk_len];
            for _ in 0..writes_per_thread {
                handle.write(&chunk).expect("append write");
            }
            chunk
        }));
    }

    let mut chunks = Vec::new();
    for join in handles {
        chunks.push(join.join().expect("thread join"));
    }

    let total_len = thread_count * writes_per_thread * chunk_len;
    let mut buf = vec![0u8; total_len];
    let read = handle.pread_at(0, &mut buf).expect("read back");
    assert_eq!(read, total_len);

    let mut counts: HashMap<Vec<u8>, usize> = HashMap::new();
    for chunk in buf.chunks_exact(chunk_len) {
        *counts.entry(chunk.to_vec()).or_insert(0) += 1;
    }

    for chunk in chunks {
        assert_eq!(counts.get(&chunk).copied().unwrap_or(0), writes_per_thread);
    }
}

#[test]
fn seek_end_works() {
    let handle = make_handle(4, vfs_core::OpenFlags::READ | vfs_core::OpenFlags::WRITE);
    handle.write(b"abcdef").expect("write");
    handle.seek(std::io::SeekFrom::End(-2)).expect("seek end");

    let mut buf = [0u8; 2];
    handle.read(&mut buf).expect("read");
    assert_eq!(&buf, b"ef");
}
