#[derive(Clone, Debug)]
pub struct MemFsConfig {
    /// Optional max bytes allowed for all file data in this FS instance.
    pub max_bytes: Option<u64>,
    /// Optional max inode count allowed in this FS instance.
    pub max_inodes: Option<u64>,
    /// If true, `read_dir` ordering is stable/deterministic.
    pub deterministic_readdir: bool,
}

impl Default for MemFsConfig {
    fn default() -> Self {
        Self {
            max_bytes: None,
            max_inodes: None,
            deterministic_readdir: true,
        }
    }
}
