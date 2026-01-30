mod config;
mod copy_up;
mod fs;
mod handle;
mod inodes;
mod node;
mod whiteout;

#[cfg(test)]
mod tests;

pub use config::{FsSpec, OverlayBuilder, OverlayConfig, OverlayOptions, OverlayProvider};
pub use fs::OverlayFs;
