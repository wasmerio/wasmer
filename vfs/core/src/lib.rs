//! Semantic core and shared types for the new Wasmer/Wasix VFS.
//!
//! ## What lives in `vfs-core` (semantic source of truth)
//!
//! - Namespace + traversal semantics (mount-aware traversal, `..` mount boundaries, symlink rules).
//! - Open File Description (OFD) semantics and VFS-level handle types (offset, status flags, etc.).
//! - Stable operation contracts and option/flag types used by Wasix.
//! - Stable error kinds and capability vocabulary (errno/WASI mapping lives elsewhere).
//! - Policy hooks for confinement/permissions (semantics live here; implementations can live elsewhere).
//!
//! ## What does not live in `vfs-core`
//!
//! - OS/WASI translation (errno mapping, open flags translation): see `vfs-unix`.
//! - Host filesystem containment details (`openat`/Windows handles): see `vfs-host`.
//! - Async runtime coupling (`tokio`, spawn_blocking, block_on): see `vfs-rt`.
//! - Implementations of mount tables, path walker, overlays, etc. (later phases), but
//!   `vfs-core` defines the contracts and naming now to avoid semantic drift.
//!
//! ## Performance boundary
//!
//! Paths are byte-oriented (`VfsPath` / `VfsPathBuf`) and component iteration is allocation-free.
//! Hot paths avoid `String`, `PathBuf`, and `OsString` in the core contract.

pub mod capabilities;
pub mod context;
pub mod dir;
pub mod error;
pub mod flags;
pub mod file_type;
pub mod fs;
pub mod handle;
pub mod inode;
pub mod ids;
pub mod metadata;
pub mod mount;
pub mod path;
pub mod path_walker;
pub mod path_types;
pub mod node;
pub mod policy;
pub mod provider;
pub mod time;
pub mod vfs;

pub use error::{VfsError, VfsErrorKind, VfsResult};
pub use fs::Fs;
pub use path_types::{
    VfsComponent, VfsComponents, VfsName, VfsNameBuf, VfsPath, VfsPathBuf,
};

pub use capabilities::VfsCapabilities;
pub use context::{VfsConfig, VfsContext, VfsCred};
pub use dir::VfsDirEntry;
pub use flags::*;
pub use file_type::VfsFileType;
pub use handle::{DirStreamHandle, VfsDirHandle, VfsHandle};
pub use ids::{BackendInodeId, MountId, VfsHandleId, VfsInodeId};
pub use inode::{make_vfs_inode, split, InodeCache, NodeRef};
pub use path_walker::{PathWalker, ResolutionRequest, Resolved, ResolvedParent, WalkFlags};
pub use metadata::VfsMetadata;
pub use policy::{AllowAllPolicy, VfsOp, VfsPolicy};
pub use time::VfsTimespec;
pub use vfs::{Vfs, VfsBaseDir};
