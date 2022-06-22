use super::*;

pub type Result<T> = std::result::Result<T, FsError>;

#[derive(Debug)]
#[repr(transparent)]
pub struct FileDescriptor(pub usize);

impl From<u32> for FileDescriptor {
    fn from(a: u32) -> Self {
        Self(a as usize)
    }
}

impl From<FileDescriptor> for u32 {
    fn from(a: FileDescriptor) -> u32 {
        a.0 as u32
    }
}

#[derive(Debug, Clone)]
pub struct OpenOptionsConfig {
    read: bool,
    write: bool,
    create_new: bool,
    create: bool,
    append: bool,
    truncate: bool,
}

impl OpenOptionsConfig {
    pub const fn read(&self) -> bool {
        self.read
    }

    pub const fn write(&self) -> bool {
        self.write
    }

    pub const fn create_new(&self) -> bool {
        self.create_new
    }

    pub const fn create(&self) -> bool {
        self.create
    }

    pub const fn append(&self) -> bool {
        self.append
    }

    pub const fn truncate(&self) -> bool {
        self.truncate
    }
}

// TODO: manually implement debug

pub struct OpenOptions {
    opener: Box<dyn FileOpener>,
    conf: OpenOptionsConfig,
}

impl OpenOptions {
    pub fn new(opener: Box<dyn FileOpener>) -> Self {
        Self {
            opener,
            conf: OpenOptionsConfig {
                read: false,
                write: false,
                create_new: false,
                create: false,
                append: false,
                truncate: false,
            },
        }
    }
    pub fn options(&mut self, options: OpenOptionsConfig) -> &mut Self {
        self.conf = options;
        self
    }

    pub fn read(&mut self, read: bool) -> &mut Self {
        self.conf.read = read;
        self
    }

    pub fn write(&mut self, write: bool) -> &mut Self {
        self.conf.write = write;
        self
    }

    pub fn append(&mut self, append: bool) -> &mut Self {
        self.conf.append = append;
        self
    }

    pub fn truncate(&mut self, truncate: bool) -> &mut Self {
        self.conf.truncate = truncate;
        self
    }

    pub fn create(&mut self, create: bool) -> &mut Self {
        self.conf.create = create;
        self
    }

    pub fn create_new(&mut self, create_new: bool) -> &mut Self {
        self.conf.create_new = create_new;
        self
    }

    pub fn open<P: AsRef<Path>>(
        &mut self,
        path: P,
    ) -> Result<Box<dyn VirtualFile + Send + Sync + 'static>> {
        self.opener.open(path.as_ref(), &self.conf)
    }
}

/// This trait relies on your file closing when it goes out of scope via `Drop`
#[cfg_attr(feature = "enable-serde", typetag::serde)]
pub trait VirtualFile: fmt::Debug + Write + Read + Seek + Upcastable {
    /// the last time the file was accessed in nanoseconds as a UNIX timestamp
    fn last_accessed(&self) -> u64;

    /// the last time the file was modified in nanoseconds as a UNIX timestamp
    fn last_modified(&self) -> u64;

    /// the time at which the file was created in nanoseconds as a UNIX timestamp
    fn created_time(&self) -> u64;

    /// the size of the file in bytes
    fn size(&self) -> u64;

    /// Change the size of the file, if the `new_size` is greater than the current size
    /// the extra bytes will be allocated and zeroed
    fn set_len(&mut self, new_size: u64) -> Result<()>;

    /// Request deletion of the file
    fn unlink(&mut self) -> Result<()>;

    /// Store file contents and metadata to disk
    /// Default implementation returns `Ok(())`.  You should implement this method if you care
    /// about flushing your cache to permanent storage
    fn sync_to_disk(&self) -> Result<()> {
        Ok(())
    }

    /// Returns the number of bytes available.  This function must not block
    fn bytes_available(&self) -> Result<usize> {
        Ok(self.bytes_available_read()?.unwrap_or(0usize)
            + self.bytes_available_write()?.unwrap_or(0usize))
    }

    /// Returns the number of bytes available.  This function must not block
    /// Defaults to `None` which means the number of bytes is unknown
    fn bytes_available_read(&self) -> Result<Option<usize>> {
        Ok(None)
    }

    /// Returns the number of bytes available.  This function must not block
    /// Defaults to `None` which means the number of bytes is unknown
    fn bytes_available_write(&self) -> Result<Option<usize>> {
        Ok(None)
    }

    /// Indicates if the file is opened or closed. This function must not block
    /// Defaults to a status of being constantly open
    fn is_open(&self) -> bool {
        true
    }

    /// Used for polling.  Default returns `None` because this method cannot be implemented for most types
    /// Returns the underlying host fd
    fn get_fd(&self) -> Option<FileDescriptor> {
        None
    }
}

// Implementation of `Upcastable` taken from https://users.rust-lang.org/t/why-does-downcasting-not-work-for-subtraits/33286/7 .
/// Trait needed to get downcasting from `VirtualFile` to work.
pub trait Upcastable {
    fn upcast_any_ref(&'_ self) -> &'_ dyn Any;
    fn upcast_any_mut(&'_ mut self) -> &'_ mut dyn Any;
    fn upcast_any_box(self: Box<Self>) -> Box<dyn Any>;
}

impl<T: Any + fmt::Debug + 'static> Upcastable for T {
    #[inline]
    fn upcast_any_ref(&'_ self) -> &'_ dyn Any {
        self
    }
    #[inline]
    fn upcast_any_mut(&'_ mut self) -> &'_ mut dyn Any {
        self
    }
    #[inline]
    fn upcast_any_box(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

/// Determines the mode that stdio handlers will operate in
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StdioMode {
    /// Stdio will be piped to a file descriptor
    Piped,
    /// Stdio will inherit the file handlers of its parent
    Inherit,
    /// Stdio will be dropped
    Null,
    /// Stdio will be sent to the log handler
    Log,
}

#[derive(Debug)]
pub struct ReadDir {
    // TODO: to do this properly we need some kind of callback to the core FS abstraction
    data: Vec<DirEntry>,
    index: usize,
}

impl ReadDir {
    pub fn new(data: Vec<DirEntry>) -> Self {
        Self { data, index: 0 }
    }
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub path: PathBuf,
    // weird hack, to fix this we probably need an internal trait object or callbacks or something
    /// The full path of the entry.
    pub full_path: PathBuf,
    pub metadata: Result<Metadata>,
}

impl DirEntry {
    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }

    pub fn metadata(&self) -> Result<Metadata> {
        self.metadata
    }

    pub fn file_type(&self) -> Result<FileType> {
        let metadata = self.metadata?;
        Ok(metadata.file_type())
    }

    pub fn file_name(&self) -> OsString {
        self.path
            .file_name()
            .unwrap_or(self.path.as_os_str())
            .to_owned()
    }
}

// The type for a path (key) entry in the database.
#[derive(Debug, Copy, PartialEq, Clone)]
#[repr(u8)]
pub enum KeyType {
    /// `TYPE_NULL`
    Null = 0,
    /// `TYPE_DIR`
    Dir,
    /// `TYPE_SYM_LINK`
    SymLink,
    /// `TYPE_BLOB`
    Blob,
    CharDevice,
    BlockDevice,
    Socket,
    Fifo,
}

impl Default for KeyType {
    fn default() -> Self {
        Self::Blob
    }
}

impl Into<FileType> for KeyType {
    fn into(self) -> FileType {
        match self {
            KeyType::Null => FileType::default(),
            KeyType::Dir => FileType {
                dir: true,
                ..Default::default()
            },
            KeyType::SymLink => FileType {
                symlink: true,
                ..Default::default()
            },
            KeyType::Blob => FileType {
                file: true,
                ..Default::default()
            },
            KeyType::CharDevice => FileType {
                char_device: true,
                ..Default::default()
            },
            KeyType::BlockDevice => FileType {
                block_device: true,
                ..Default::default()
            },
            KeyType::Socket => FileType {
                socket: true,
                ..Default::default()
            },
            KeyType::Fifo => FileType {
                fifo: true,
                ..Default::default()
            },
        }
    }
}

impl KeyType {
    pub fn is_file(self) -> bool {
        self == Self::Blob
    }

    pub fn is_dir(self) -> bool {
        self == Self::Dir
    }
    pub fn is_symlink(self) -> bool {
        self == Self::SymLink
    }
    pub fn is_char_device(self) -> bool {
        self == Self::CharDevice
    }
    pub fn is_block_device(self) -> bool {
        self == Self::BlockDevice
    }
    pub fn is_socket(self) -> bool {
        self == Self::Socket
    }
    pub fn is_fifo(self) -> bool {
        self == Self::Fifo
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Metadata {
    /// Mode of file.
    pub mode: u16,
    /// User id of file.
    pub uid: u32,
    /// Group id of file.
    pub gid: u32,
    /// Access time of file.
    pub atime: u64,
    /// Last modification time of file.
    pub mtime: u64,
    /// Last change time of file.
    pub ctime: u64,
    /// Size of file in bytes.
    pub size: u64,
    /// Assigned inode of file.
    pub inode: u64,
    /// Type of file.
    pub type_: KeyType,
}

impl Metadata {
    pub fn is_file(&self) -> bool {
        self.type_ == KeyType::Blob
    }

    pub fn is_dir(&self) -> bool {
        self.type_ == KeyType::Dir
    }

    pub fn accessed(&self) -> u64 {
        self.atime as _
    }

    pub fn created(&self) -> u64 {
        self.ctime as _
    }

    pub fn modified(&self) -> u64 {
        self.mtime as _
    }

    pub fn file_type(&self) -> FileType {
        self.type_.into()
    }

    pub fn len(&self) -> u64 {
        self.size as _
    }
}

// TODO: review this, proper solution would probably use a trait object internally
#[derive(Clone, Debug, Default)]
pub struct FileType {
    pub dir: bool,
    pub file: bool,
    pub symlink: bool,
    // TODO: the following 3 only exist on unix in the standard FS API.
    // We should mirror that API and extend with that trait too.
    pub char_device: bool,
    pub block_device: bool,
    pub socket: bool,
    pub fifo: bool,
}

impl FileType {
    pub fn is_dir(&self) -> bool {
        self.dir
    }
    pub fn is_file(&self) -> bool {
        self.file
    }
    pub fn is_symlink(&self) -> bool {
        self.symlink
    }
    pub fn is_char_device(&self) -> bool {
        self.char_device
    }
    pub fn is_block_device(&self) -> bool {
        self.block_device
    }
    pub fn is_socket(&self) -> bool {
        self.socket
    }
    pub fn is_fifo(&self) -> bool {
        self.fifo
    }
}

impl Iterator for ReadDir {
    type Item = Result<DirEntry>;

    fn next(&mut self) -> Option<Result<DirEntry>> {
        if let Some(v) = self.data.get(self.index).cloned() {
            self.index += 1;
            return Some(Ok(v));
        }
        None
    }
}
