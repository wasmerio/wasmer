//! `vfs-core` defines the semantic core and shared types for the new VFS.
//!
//! This crate is intentionally runtime-agnostic and platform-agnostic:
//! - No `tokio`/async runtime coupling (see `vfs-rt` for runtime glue).
//! - No `std::path` usage; paths are raw bytes (`VfsPath` / `VfsPathBuf`).
//!
//! Higher-level semantics (mounts, path walking, OFD behavior, etc.) are implemented
//! in later phases, but all shared core types live here.

pub mod capabilities;
pub mod dir;
pub mod error;
pub mod file_type;
pub mod fs;
pub mod ids;
pub mod metadata;
pub mod path;
pub mod policy;
pub mod provider;
pub mod time;

pub use error::{VfsError, VfsResult};
pub use fs::Fs;
pub use path::{VfsPath, VfsPathBuf};

pub use capabilities::VfsCapabilities;
pub use dir::VfsDirEntry;
pub use file_type::VfsFileType;
pub use ids::{MountId, VfsHandleId, VfsInodeId};
pub use metadata::VfsMetadata;
pub use policy::{AllowAllPolicy, VfsOperation, VfsPolicy};
pub use time::VfsTimespec;
