use libc::size_t;
use std::ffi::CString;
use std::ffi::OsStr;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::{DirEntry, FsError, KeyType};

/// The crate's error type

/// Key type
#[derive(Debug, Copy, Clone)]
pub struct Key<'k>(&'k Path);

impl<'k, P: AsRef<Path>> From<&'k P> for Key<'k> {
    fn from(val: &'k P) -> Key<'k> {
        let p: &Path = val.as_ref();
        let b = p.as_os_str().as_bytes();
        if b.ends_with(b"/") && b != b"/" {
            Key(Path::new(OsStr::from_bytes(&b[..b.len() - 1])))
        } else {
            Key(p)
        }
    }
}
impl<'k> From<&'k Path> for Key<'k> {
    fn from(p: &'k Path) -> Key<'k> {
        let b = p.as_os_str().as_bytes();
        if b.ends_with(b"/") && b != b"/" {
            Key(Path::new(OsStr::from_bytes(&b[..b.len() - 1])))
        } else {
            Key(p)
        }
    }
}

impl Into<PathBuf> for Key<'_> {
    fn into(self) -> PathBuf {
        self.0.into()
    }
}

impl AsRef<Path> for Key<'_> {
    fn as_ref(&self) -> &Path {
        self.0
    }
}

impl rusqlite::types::ToSql for Key<'_> {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Borrowed(
            rusqlite::types::ValueRef::Text(self.0.as_os_str().as_bytes()),
        ))
    }
}

impl From<DirEntry> for libc::dirent {
    fn from(ent: DirEntry) -> Self {
        let mut maybe_uninit = MaybeUninit::<libc::dirent>::zeroed();
        let dir_entry_pointer = maybe_uninit.as_mut_ptr();

        let name_cstring =
            CString::new(ent.path.as_os_str().as_bytes()).expect("path to be valid CString");
        let name_len = name_cstring.to_bytes_with_nul().len();
        let copy_size = std::cmp::min(name_len, 200); // 200 is the size defined for the dirent name buffer

        unsafe {
            std::ptr::copy(
                name_cstring.as_ptr(),
                (*dir_entry_pointer).d_name.as_mut_ptr(),
                copy_size,
            )
        };
        if let Ok(metadata) = ent.metadata.as_ref() {
            unsafe {
                (*dir_entry_pointer).d_ino = metadata.inode;
            }
        }
        unsafe {
            (*dir_entry_pointer).d_reclen = std::mem::size_of::<libc::dirent>() as _;
        }

        unsafe { maybe_uninit.assume_init() }
    }
}

/// The size of a data block for the table `value_data`. See
/// [`CREATE_STATEMENTS`][`crate::CREATE_STATEMENTS`] for the
/// SQL schema.
pub const BLOCK_SIZE: size_t = 8192;

/// String constant used in the database to denote an entry's type.
pub const TYPE_NULL: &str = "null";
/// String constant used in the database to denote an entry's type.
pub const TYPE_DIR: &str = "dir";
/// String constant used in the database to denote an entry's type.
pub const TYPE_SYM_LINK: &str = "sym link";
/// String constant used in the database to denote an entry's type.
pub const TYPE_BLOB: &str = "blob";
/// String constant used in the database to denote an entry's type.
pub const TYPE_CHAR_DEVICE: &str = "char device";
/// String constant used in the database to denote an entry's type.
pub const TYPE_BLOCK_DEV: &str = "block device";
/// String constant used in the database to denote an entry's type.
pub const TYPE_SOCKET: &str = "socket";
/// String constant used in the database to denote an entry's type.
pub const TYPE_FIFO: &str = "fifo";

/// Library error type.
#[derive(Debug)]
pub enum Error {
    /// Depends on value of wrapped error, but in general: EIO 5 Input/output error
    SqlError(rusqlite::Error),
    /// EIO 5 Input/output error
    Io(std::io::Error, PathBuf),
    /// ENOTDIR 20 Not a directory
    NotADirectory(PathBuf),
    /// ENOTEMPTY 39 Directory not empty
    DirectoryNotEmpty(PathBuf),
    /// EISDIR 21 Is a directory
    IsDirectory(PathBuf),
    /// ENOENT 2 No such file or directory
    DoesNotExist(PathBuf),
    /// EEXIST 17 File exists
    AlreadyExists(PathBuf),
    /// EACCES 13 Permission denied
    PermissionDenied(PathBuf),
    /// EINVAL 22 Invalid argument
    BadOpenFlags,
    /// EINVAL 22 Invalid argument
    InvalidArgument,
    /// EINVAL 22 Invalid argument
    IntegerOverflow(std::num::TryFromIntError),
}

impl From<Error> for i32 {
    fn from(err: Error) -> Self {
        match err {
            Error::SqlError(_) => libc::EIO,
            Error::NotADirectory(_) => libc::ENOTDIR,
            Error::DirectoryNotEmpty(_) => libc::ENOTEMPTY,
            Error::IsDirectory(_) => libc::EISDIR,
            Error::DoesNotExist(_) => libc::ENOENT,
            Error::AlreadyExists(_) => libc::EEXIST,
            Error::PermissionDenied(_) => libc::EACCES,
            Error::BadOpenFlags => libc::EINVAL,
            Error::InvalidArgument => libc::EINVAL,
            Error::IntegerOverflow(_) => libc::EINVAL,
            Error::Io(_, _) => libc::EIO,
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(val: rusqlite::Error) -> Error {
        if val == rusqlite::Error::QueryReturnedNoRows {
            Error::DoesNotExist(PathBuf::new())
        } else {
            Error::SqlError(val)
        }
    }
}

impl From<std::num::TryFromIntError> for Error {
    fn from(val: std::num::TryFromIntError) -> Error {
        Error::IntegerOverflow(val)
    }
}

impl From<(std::io::Error, PathBuf)> for Error {
    fn from(val: (std::io::Error, PathBuf)) -> Error {
        Error::Io(val.0, val.1)
    }
}

impl From<rusqlite::Error> for FsError {
    fn from(val: rusqlite::Error) -> FsError {
        if val == rusqlite::Error::QueryReturnedNoRows {
            FsError::EntityNotFound
        } else {
            FsError::IOError
        }
    }
}

impl From<std::num::TryFromIntError> for FsError {
    fn from(_val: std::num::TryFromIntError) -> FsError {
        FsError::InvalidData
    }
}

impl From<(std::io::Error, PathBuf)> for FsError {
    fn from(_: (std::io::Error, PathBuf)) -> FsError {
        FsError::IOError
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(fmt, "{:?}", self)
    }
}

impl From<Error> for FsError {
    fn from(val: Error) -> FsError {
        match val {
            Error::SqlError(_) => FsError::IOError,
            Error::NotADirectory(_) => FsError::BaseNotDirectory,
            Error::DirectoryNotEmpty(_) => FsError::DirectoryNotEmpty,
            Error::IsDirectory(_) => FsError::IsDirectory,
            Error::DoesNotExist(_) => FsError::EntityNotFound,
            Error::AlreadyExists(_) => FsError::AlreadyExists,
            Error::PermissionDenied(_) => FsError::PermissionDenied,
            Error::BadOpenFlags => FsError::InvalidInput,
            Error::InvalidArgument => FsError::InvalidInput,
            Error::IntegerOverflow(_) => FsError::UnknownError,
            Error::Io(_, _) => FsError::IOError,
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Error::SqlError(ref source) = self {
            Some(source)
        } else if let Error::IntegerOverflow(ref source) = self {
            Some(source)
        } else if let Error::Io(ref source, _) = self {
            Some(source)
        } else {
            None
        }
    }
}

impl rusqlite::types::FromSql for KeyType {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        Ok(if let Some(s) = value.as_str_or_null()? {
            match s {
                self::TYPE_NULL => KeyType::Null,
                self::TYPE_DIR => KeyType::Dir,
                self::TYPE_SYM_LINK => KeyType::SymLink,
                self::TYPE_BLOB => KeyType::Blob,
                _ => KeyType::default(),
            }
        } else {
            KeyType::default()
        })
    }
}

impl rusqlite::types::ToSql for KeyType {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        match self {
            KeyType::Null => Ok(TYPE_NULL.into()),
            KeyType::Dir => Ok(TYPE_DIR.into()),
            KeyType::SymLink => Ok(TYPE_SYM_LINK.into()),
            KeyType::Blob => Ok(TYPE_BLOB.into()),
            KeyType::CharDevice => Ok(TYPE_CHAR_DEVICE.into()),
            KeyType::BlockDevice => Ok(TYPE_BLOCK_DEV.into()),
            KeyType::Socket => Ok(TYPE_SOCKET.into()),
            KeyType::Fifo => Ok(TYPE_FIFO.into()),
        }
    }
}
