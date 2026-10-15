//! In-memory filesystem driver, the default writable root.
//!
//! File data is stored sparsely in fixed-size blocks, so a write far past the end costs one block
//! and the hole reads as zeroes.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
};

use wasixv2_abi::Errno;
use wasixv2_vfs::{FileType, Inode, LocalFuture};

const BLOCK: u64 = 4096;
/// Largest file size, as Linux's `MAX_LFS_FILESIZE` on 64-bit.
const MAX_SIZE: u64 = i64::MAX as u64;

/// A directory. [`MemDir::root`] creates an empty filesystem.
pub struct MemDir {
    this: Weak<MemDir>,
    parent: Option<Weak<MemDir>>,
    entries: Mutex<BTreeMap<Vec<u8>, Entry>>,
}

#[derive(Clone)]
enum Entry {
    File(Arc<MemFile>),
    Dir(Arc<MemDir>),
}

impl Entry {
    fn inode(&self) -> Arc<dyn Inode> {
        match self {
            Entry::File(file) => file.clone(),
            Entry::Dir(dir) => dir.clone(),
        }
    }

    fn file_type(&self) -> FileType {
        match self {
            Entry::File(_) => FileType::Regular,
            Entry::Dir(_) => FileType::Directory,
        }
    }
}

impl MemDir {
    /// The root directory of a new, empty filesystem.
    pub fn root() -> Arc<Self> {
        Self::new(None)
    }

    fn new(parent: Option<Weak<MemDir>>) -> Arc<Self> {
        Arc::new_cyclic(|this| Self {
            this: this.clone(),
            parent,
            entries: Mutex::default(),
        })
    }
}

impl Inode for MemDir {
    fn file_type(&self) -> FileType {
        FileType::Directory
    }

    fn parent(&self) -> Option<Arc<dyn Inode>> {
        let parent: Arc<dyn Inode> = self.parent.as_ref()?.upgrade()?;
        Some(parent)
    }

    fn lookup(&self, name: &[u8]) -> Result<Arc<dyn Inode>, Errno> {
        let entries = self.entries.lock().unwrap();
        entries.get(name).map(Entry::inode).ok_or(Errno::ENOENT)
    }

    fn create(&self, name: &[u8], file_type: FileType) -> Result<Arc<dyn Inode>, Errno> {
        let mut entries = self.entries.lock().unwrap();
        if entries.contains_key(name) {
            return Err(Errno::EEXIST);
        }
        let entry = match file_type {
            FileType::Regular => Entry::File(Arc::default()),
            FileType::Directory => Entry::Dir(MemDir::new(Some(self.this.clone()))),
        };
        let inode = entry.inode();
        entries.insert(name.to_vec(), entry);
        Ok(inode)
    }

    fn remove(&self, name: &[u8], file_type: FileType) -> Result<(), Errno> {
        let mut entries = self.entries.lock().unwrap();
        let entry = entries.get(name).ok_or(Errno::ENOENT)?;
        match (entry, file_type) {
            (Entry::Dir(_), FileType::Regular) => return Err(Errno::EISDIR),
            (Entry::File(_), FileType::Directory) => return Err(Errno::ENOTDIR),
            (Entry::Dir(dir), FileType::Directory) if !dir.entries.lock().unwrap().is_empty() => {
                return Err(Errno::ENOTEMPTY);
            }
            _ => {}
        }
        debug_assert_eq!(entry.file_type(), file_type);
        entries.remove(name);
        Ok(())
    }
}

/// A regular file. Open descriptors keep it alive after it is removed, as on Linux.
#[derive(Default)]
pub struct MemFile {
    data: Mutex<Blocks>,
}

#[derive(Default)]
struct Blocks {
    size: u64,
    blocks: BTreeMap<u64, Box<[u8]>>,
}

impl Blocks {
    fn read(&self, buf: &mut [u8], offset: u64) -> usize {
        let end = self.size.min(offset.saturating_add(buf.len() as u64));
        let mut pos = offset;
        while pos < end {
            let (index, start) = (pos / BLOCK, (pos % BLOCK) as usize);
            let len = ((BLOCK - start as u64).min(end - pos)) as usize;
            let out = &mut buf[(pos - offset) as usize..][..len];
            match self.blocks.get(&index) {
                Some(block) => out.copy_from_slice(&block[start..start + len]),
                None => out.fill(0),
            }
            pos += len as u64;
        }
        end.saturating_sub(offset) as usize
    }

    fn write(&mut self, data: &[u8], offset: u64) -> Result<usize, Errno> {
        let end = offset
            .checked_add(data.len() as u64)
            .filter(|&end| end <= MAX_SIZE)
            .ok_or(Errno::EFBIG)?;
        let mut pos = offset;
        while pos < end {
            let (index, start) = (pos / BLOCK, (pos % BLOCK) as usize);
            let len = ((BLOCK - start as u64).min(end - pos)) as usize;
            let block = self
                .blocks
                .entry(index)
                .or_insert_with(|| vec![0; BLOCK as usize].into_boxed_slice());
            block[start..start + len].copy_from_slice(&data[(pos - offset) as usize..][..len]);
            pos += len as u64;
        }
        self.size = self.size.max(end);
        Ok(data.len())
    }

    fn truncate(&mut self, size: u64) {
        let first_gone = size.div_ceil(BLOCK);
        self.blocks.split_off(&first_gone);
        // Zero the tail of the last block so a later extension reads zeroes there.
        if let Some(block) = self.blocks.get_mut(&(size / BLOCK)) {
            block[(size % BLOCK) as usize..].fill(0);
        }
        self.size = size;
    }
}

impl Inode for MemFile {
    fn file_type(&self) -> FileType {
        FileType::Regular
    }

    fn size(&self) -> u64 {
        self.data.lock().unwrap().size
    }

    fn read_at<'a>(
        &'a self,
        buf: &'a mut [u8],
        offset: u64,
    ) -> LocalFuture<'a, Result<usize, Errno>> {
        let n = self.data.lock().unwrap().read(buf, offset);
        Box::pin(async move { Ok(n) })
    }

    fn write_at<'a>(
        &'a self,
        data: &'a [u8],
        offset: u64,
    ) -> LocalFuture<'a, Result<usize, Errno>> {
        let result = self.data.lock().unwrap().write(data, offset);
        Box::pin(async move { result })
    }

    fn truncate(&self, size: u64) -> Result<(), Errno> {
        if size > MAX_SIZE {
            return Err(Errno::EFBIG);
        }
        self.data.lock().unwrap().truncate(size);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;
    use wasixv2_vfs::walk;

    use super::*;

    #[test]
    fn sparse_write_reads_back_with_a_zero_hole() {
        let file = MemFile::default();
        let far = 1 << 33;

        assert_eq!(block_on(file.write_at(b"hello", 0)), Ok(5));
        assert_eq!(block_on(file.write_at(b"X", far)), Ok(1));
        let mut buf = [0xff; 8];
        let n = block_on(file.read_at(&mut buf, 3));

        assert_eq!(file.size(), far + 1);
        assert_eq!(n, Ok(8));
        assert_eq!(&buf, b"lo\0\0\0\0\0\0");
        assert_eq!(file.data.lock().unwrap().blocks.len(), 2);
    }

    #[test]
    fn truncate_then_extend_reads_zeroes() {
        let file = MemFile::default();
        block_on(file.write_at(b"abcdef", 0)).unwrap();

        file.truncate(2).unwrap();
        file.truncate(6).unwrap();
        let mut buf = [0xff; 8];
        let n = block_on(file.read_at(&mut buf, 0));

        assert_eq!(n, Ok(6));
        assert_eq!(&buf[..6], b"ab\0\0\0\0");
    }

    #[test]
    fn paths_resolve_through_dot_dot_and_removal_follows_linux_errors() {
        let root: Arc<dyn Inode> = MemDir::root();
        let tmp = root.create(b"tmp", FileType::Directory).unwrap();
        tmp.create(b"f", FileType::Regular).unwrap();

        let found = walk(&root, &tmp, b"../tmp/./f").unwrap();

        assert_eq!(found.file_type(), FileType::Regular);
        assert_eq!(walk(&root, &root, b"/tmp/f/x").err(), Some(Errno::ENOTDIR));
        assert_eq!(walk(&root, &root, b"/nope").err(), Some(Errno::ENOENT));
        assert_eq!(
            root.remove(b"tmp", FileType::Directory),
            Err(Errno::ENOTEMPTY)
        );
        assert_eq!(tmp.remove(b"f", FileType::Directory), Err(Errno::ENOTDIR));
        assert_eq!(root.remove(b"tmp", FileType::Regular), Err(Errno::EISDIR));
        assert_eq!(tmp.remove(b"f", FileType::Regular), Ok(()));
        assert_eq!(root.remove(b"tmp", FileType::Directory), Ok(()));
    }
}
