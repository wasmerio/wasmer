mod api;
pub use api::*;
mod utils;
pub use utils::*;

use crate::KeyType;
use crate::{DirEntry, FsError, Metadata, OpenOptions, OpenOptionsConfig, ReadDir, VirtualFile};
use libc::{ino_t, mode_t, uid_t};
use rusqlite::named_params;
use rusqlite::Connection;

use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::info;

type Result<T> = std::result::Result<T, FsError>;

impl crate::FileSystem for SqliteFs {
    fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        let uid = 0;
        let guid = 0;
        let mut lck = self.inner.lock().unwrap();
        let tx = lck.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut tx = Transaction(tx);
        match tx.key_is_dir(path.into())? {
            None => return Err(Error::DoesNotExist(path.into()).into()),
            Some(false) => return Err(Error::NotADirectory(path.into()).into()),
            Some(true) => {}
        }

        tx.check_parent_access(path.into(), uid, guid)?;
        tx.check_dir_read(path.into(), uid, guid)?;
        let mut ret = vec![];
        {
            let mut stmt = tx.0.prepare_cached("SELECT key, mode, uid, gid, atime, mtime, ctime, size, inode, type FROM meta_data WHERE key glob :pattern;")?;

            let mut path_s = path.to_str().expect("Non-utf8 path").to_string();
            while path_s.ends_with('/') {
                path_s.pop();
            }
            let pattern = format!("{}/*", path_s);
            let iter = stmt.query_map(named_params! { ":pattern": pattern }, |row| {
                let key: String = row.get(0)?;
                let metadata = Metadata {
                    mode: row.get(1)?,
                    uid: row.get(2)?,
                    gid: row.get(3)?,
                    atime: row.get(4)?,
                    mtime: row.get(5)?,
                    ctime: row.get(6)?,
                    size: row.get(7)?,
                    inode: row.get(8)?,
                    type_: row.get(9)?,
                };
                Ok((key, metadata))
            })?;

            for result in iter {
                let (key, metadata) = result?;
                let entry_path = Path::new(&key);
                let entry_path = entry_path.strip_prefix(&path).unwrap();
                if entry_path.as_os_str().is_empty() {
                    continue;
                }
                match entry_path.parent() {
                    Some(p) if p.as_os_str().is_empty() => {}
                    Some(_) => {
                        continue;
                    }
                    None => {}
                }
                ret.push(DirEntry {
                    path: entry_path.into(),
                    full_path: PathBuf::from(key),
                    metadata: Ok(metadata),
                });
            }
        }
        tx.0.commit()?;

        Ok(ReadDir::new(ret))
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        let mut lck = self.inner.lock().unwrap();
        let tx = lck.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut tx = Transaction(tx);
        let _ret = tx.mkdir(
            path.into(),
            600, /*mode*/
            0,   /*uid*/
            0,   /*guid*/
        )?;
        tx.0.commit()?;
        Ok(())
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        let mut lck = self.inner.lock().unwrap();
        let tx = lck.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut tx = Transaction(tx);
        tx.rmdir(path.into(), 0 /*uid*/, 0 /*guid*/)?;
        tx.0.commit()?;
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let mut lck = self.inner.lock().unwrap();
        let tx = lck.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut tx = Transaction(tx);
        tx.rename(from.into(), to.into(), 0 /*uid*/, 0 /*guid*/)?;
        tx.0.commit()?;
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        let mut lck = self.inner.lock().unwrap();
        let tx = lck.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut tx = Transaction(tx);
        tx.unlink(path.into(), 0 /*uid*/, 0 /*guid*/)?;
        tx.0.commit()?;
        Ok(())
    }

    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(Box::new(self.clone()))
    }

    fn metadata(&self, path: &Path) -> Result<Metadata> {
        let uid = 0;
        let guid = 0;
        let mut lck = self.inner.lock().unwrap();
        let tx = lck.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut tx = Transaction(tx);
        tx.check_parent_access(path.into(), uid, guid)?;
        tx.check_read(path.into(), uid, guid, None)?;

        if tx.key_exists(path.into())?.is_none() {
            return Err(Error::DoesNotExist(path.into()).into());
        };
        let metadatas = tx.getmetadata(path.into())?;
        tx.0.commit()?;
        Ok(metadatas)
    }
}

impl crate::FileOpener for SqliteFs {
    fn open(
        &mut self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync + 'static>> {
        let uid = 0;
        let guid = 0;
        let default_mode = self.default_mode;
        let mut lck = self.inner.lock().unwrap();
        let tx = lck.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut tx = Transaction(tx);
        if conf.create() {
            tx.check_parent_write(path.into(), uid, guid)?;
        } else if conf.write() {
            tx.check_parent_access(path.into(), uid, guid)?;
            tx.check_write(path.into(), uid, guid)?;
        } else {
            tx.check_parent_access(path.into(), uid, guid)?;
            tx.check_read(path.into(), uid, guid, None)?;
        }
        let exists = tx.key_exists(path.into())?;
        if !(conf.create() || conf.create_new()) && exists.is_none() {
            return Err(Error::DoesNotExist(path.into()).into());
        }

        if conf.read() || conf.write() {
            match tx.key_is_dir(path.into())? {
                Some(true) => return Err(Error::IsDirectory(path.into()).into()),
                Some(false) => {}
                None => {}
            }
        }

        if exists.is_some() && conf.truncate() && conf.write() {
            tx.truncate(path.into(), 0)?;
        }
        let metadata = if exists.is_none() && conf.create() {
            let metadata = Metadata {
                mode: default_mode,
                uid: uid as _,
                gid: guid as _,
                inode: tx.get_new_inode()?,
                atime: 0,
                mtime: 0,
                ctime: 0,
                size: 0,
                type_: KeyType::Blob,
            };
            tx.createmetadata(path.into(), metadata)?;
            tx.0.commit()?;
            metadata
        } else {
            let ret = tx.getmetadata(path.into())?;
            tx.0.commit()?;
            ret
        };
        Ok(Box::new(File {
            metadata,
            offset: 0,
            inner: self.inner.clone(),
            host_path: path.to_path_buf(),
            _open_flags: conf.clone(),
        }))
    }
}

#[derive(Debug)]
pub struct File {
    pub metadata: Metadata,
    offset: libc::off_t,
    pub host_path: PathBuf,
    inner: Arc<Mutex<Connection>>,
    _open_flags: OpenOptionsConfig,
}

impl VirtualFile for File {
    fn last_accessed(&self) -> u64 {
        self.metadata.atime as _
    }

    fn last_modified(&self) -> u64 {
        self.metadata.mtime as _
    }

    fn created_time(&self) -> u64 {
        self.metadata.ctime as _
    }

    fn size(&self) -> u64 {
        self.metadata.size as _
    }

    fn set_len(&mut self, new_size: u64) -> Result<()> {
        let mut metadata = self.metadata;
        metadata.size = new_size as _;
        Transaction(
            self.inner
                .lock()
                .unwrap()
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?,
        )
        .setmetadata(self.host_path.as_path().into(), metadata)?;

        Ok(())
    }

    fn unlink(&mut self) -> Result<()> {
        Transaction(
            self.inner
                .lock()
                .unwrap()
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?,
        )
        .unlink(self.host_path.as_path().into(), 0, 0)?;
        Ok(())
    }

    fn bytes_available(&self) -> Result<usize> {
        Ok(0)
    }

    fn get_fd(&self) -> Option<crate::FileDescriptor> {
        Some(crate::FileDescriptor(self.metadata.inode as _))
    }
}

impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let uid = 0;
        let guid = 0;
        let mut lck = self.inner.lock().unwrap();
        let tx = lck
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        let mut tx = Transaction(tx);
        let ret = tx
            .read(
                self.host_path.as_path().into(),
                buf,
                self.offset,
                libc::O_RDONLY,
                uid,
                guid,
            )
            .unwrap();
        tx.0.commit().unwrap();
        self.offset += ret as i64;
        Ok(ret)
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        self.read(buf)
    }

    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        // SAFETY: `String::as_mut_vec` cannot check that modifcations
        // of the `Vec` will produce a valid UTF-8 string. In our
        // case, we use `str::from_utf8` to ensure that the UTF-8
        // constraint still hold before returning.
        let bytes_buffer = unsafe { buf.as_mut_vec() };
        bytes_buffer.clear();
        let read = self.read_to_end(bytes_buffer)?;

        if std::str::from_utf8(bytes_buffer).is_err() {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "buffer did not contain valid UTF-8",
            ))
        } else {
            Ok(read)
        }
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.read(buf)?;
        Ok(())
    }
}

impl Seek for File {
    fn seek(&mut self, _position: io::SeekFrom) -> io::Result<u64> {
        // In `append` mode, it's not possible to seek in the file. In
        // [`open(2)`](https://man7.org/linux/man-pages/man2/open.2.html),
        // the `O_APPEND` option describes this behavior well:
        //
        // > Before each write(2), the file offset is positioned at
        // > the end of the file, as if with lseek(2).  The
        // > modification of the file offset and the write operation
        // > are performed as a single atomic step.
        // >
        // > O_APPEND may lead to corrupted files on NFS filesystems
        // > if more than one process appends data to a file at once.
        // > This is because NFS does not support appending to a file,
        // > so the client kernel has to simulate it, which can't be
        // > done without a race condition.
        Ok(0)
    }
}

impl Write for File {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let default_mode = DEFAULT_MODE;
        let uid = 0;
        let guid = 0;
        let mut lck = self.inner.lock().unwrap();
        let tx = lck
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        let mut tx = Transaction(tx);
        let ret = tx
            .write(
                self.host_path.as_path().into(),
                buf,
                self.offset,
                libc::O_WRONLY,
                default_mode,
                uid,
                guid,
            )
            .unwrap();
        tx.0.commit().unwrap();
        Ok(ret)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    #[allow(clippy::unused_io_amount)]
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.write(buf)?;
        Ok(())
    }
}
/* schema from https://github.com/guardianproject/libsqlfs */
pub const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS meta_data(key text, type text, inode integer, uid integer,
                        gid integer, mode integer, acl text,
                        metadataibute text, atime integer, mtime integer,
                        ctime integer, size integer, block_size integer,
                        primary key (key), unique(key));

 CREATE TABLE IF NOT EXISTS value_data (key text, block_no integer, data_block blob, unique(key, block_no));
 CREATE INDEX IF NOT EXISTS meta_index ON meta_data (key);
 CREATE INDEX IF NOT EXISTS value_index ON value_data (key, block_no);";

/// Handle to a database connection.
#[derive(Debug, Clone)]
pub struct SqliteFs {
    inner: Arc<Mutex<Connection>>,
    max_inode: Option<ino_t>,
    default_mode: mode_t,
}

/// Handle to a database transaction.
pub struct Transaction<'tx>(pub rusqlite::Transaction<'tx>);

const DEFAULT_MODE: mode_t = 0o700;

impl SqliteFs {
    pub fn init<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let sqlite_version = rusqlite::version();
        info!("Using sqlite version {}", sqlite_version);
        let conn = Connection::open(db_path.as_ref())?;
        conn.execute("PRAGMA foreign_keys = true;", named_params! {})?;
        conn.execute("PRAGMA journal_mode = WAL;", named_params! {})?;
        conn.execute("PRAGMA encoding = 'UTF-8';", named_params! {})?;
        let mut ret = Self {
            inner: Arc::new(Mutex::new(conn)),
            max_inode: None,
            default_mode: 0o777,
        };
        ret.update_max_inode()?;
        ret.ensure_existence(Path::new("/"), KeyType::Dir, 0, 0)?;
        ret.default_mode = DEFAULT_MODE;
        Ok(ret)
    }

    /// Sets the default mode for newly created files.
    pub fn set_default_mode(&mut self, new_mode: mode_t) {
        self.default_mode = new_mode;
    }

    /// Returns the default mode for newly created files.
    pub fn default_mode(&self) -> mode_t {
        self.default_mode
    }

    /// Get currently cached maximum inode. To ensure the value is up to date, call
    /// [`Connection::update_max_inode`] first.
    pub fn max_inode(&self) -> Option<ino_t> {
        self.max_inode
    }

    /// Fetches the maximum inode from the database and updates the cached value.
    pub fn update_max_inode(&mut self) -> Result<Option<ino_t>> {
        let max = {
            let mut lck = self.inner.lock().unwrap();
            let tx = lck.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let mut tx = Transaction(tx);
            let max = tx.get_max_inode()?;
            tx.0.commit().map_err(|err| {
                let e: Error = err.into();
                e
            })?;
            max
        };
        self.max_inode = max;
        Ok(max)
    }

    /// Ensure the existence of a path. If it does not exist, it is created.
    pub fn ensure_existence(
        &mut self,
        path: &Path,
        key_type: KeyType,
        uid: uid_t,
        guid: uid_t,
    ) -> Result<()> {
        let default_mode = self.default_mode;
        let mut lck = self.inner.lock().unwrap();
        let tx = lck.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut tx = Transaction(tx);
        let mut path_iter = Some(path);
        while let Some(path) = path_iter {
            if tx.key_exists(path.into())?.is_none() {
                let mut mode = default_mode; /* use default mode */
                if key_type == KeyType::Dir {
                    mode |= libc::S_IFDIR; // Set directory type bit
                }
                let metadatas = Metadata {
                    mode: mode as _,
                    uid: uid as _,
                    gid: guid as _,
                    inode: tx.get_new_inode()?,
                    atime: 0,
                    mtime: 0,
                    ctime: 0,
                    size: 0,
                    type_: key_type,
                };
                tx.createmetadata(path.into(), metadatas)?;
            };
            // if `path` is a nested directory? All the components of the hierarchy must be
            // created as well.
            path_iter = path.parent();
        }
        tx.0.commit()?;
        Ok(())
    }
}
