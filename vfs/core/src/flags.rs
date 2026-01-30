use bitflags::bitflags;

bitflags! {
    /// Open semantics flags.
    ///
    /// These are internal VFS flags (not WASI flags). Translation happens in `vfs-unix`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct OpenFlags: u32 {
        const READ = 1 << 0;
        const WRITE = 1 << 1;
        const APPEND = 1 << 2;
        const TRUNC = 1 << 3;
        const CREATE = 1 << 4;
        const EXCL = 1 << 5;

        const DIRECTORY = 1 << 6;
        const NOFOLLOW = 1 << 7;

        /// Per-file-status flag (best-effort, backend-dependent).
        const NONBLOCK = 1 << 8;
        const SYNC = 1 << 9;
        const DSYNC = 1 << 10;

        /// Per-FD flag (lives in Wasix resource table). This is present here only for
        /// translation convenience; core will treat it as metadata, not OFD state.
        const CLOEXEC = 1 << 11;
    }
}

impl OpenFlags {
    pub const STATUS_MASK: OpenFlags = OpenFlags::from_bits_truncate(
        OpenFlags::APPEND.bits()
            | OpenFlags::NONBLOCK.bits()
            | OpenFlags::SYNC.bits()
            | OpenFlags::DSYNC.bits(),
    );

    pub fn status_flags(self) -> HandleStatusFlags {
        HandleStatusFlags::from_bits_truncate((self & OpenFlags::STATUS_MASK).bits())
    }
}

bitflags! {
    /// OFD-scoped status flags.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct HandleStatusFlags: u32 {
        const APPEND = 1 << 2;
        const NONBLOCK = 1 << 8;
        const SYNC = 1 << 9;
        const DSYNC = 1 << 10;
    }
}

bitflags! {
    /// Path resolution flags.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct ResolveFlags: u32 {
        const NO_SYMLINK_FOLLOW = 1 << 0;
        const BENEATH = 1 << 1;
        const IN_ROOT = 1 << 2;
        const NO_MAGICLINKS = 1 << 3;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenOptions {
    pub flags: OpenFlags,
    pub mode: Option<u32>,
    pub resolve: ResolveFlags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatOptions {
    pub resolve: ResolveFlags,
    pub follow: bool,
    pub require_dir_if_trailing_slash: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MkdirOptions {
    pub mode: Option<u32>,
    pub resolve: ResolveFlags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnlinkOptions {
    pub resolve: ResolveFlags,
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct RenameFlags: u32 {
        const NOREPLACE = 1 << 0;
        const EXCHANGE = 1 << 1;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenameOptions {
    pub flags: RenameFlags,
    pub resolve: ResolveFlags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadlinkOptions {
    pub resolve: ResolveFlags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SymlinkOptions {
    pub resolve: ResolveFlags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadDirOptions;
