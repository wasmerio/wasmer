use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct DcgiMetadata {
    /// Shard associated with this WCGI
    pub shard: String,
    /// This master lock prevents multiple writable instances
    /// from running at the same time. It is held for the duration
    /// of the instance running until it returns to the factory
    /// or its dropped, for example if an error occurs
    pub master_lock: Option<Arc<tokio::sync::OwnedMutexGuard<()>>>,
}

impl Default for DcgiMetadata {
    fn default() -> Self {
        DcgiMetadata {
            shard: Default::default(),
            master_lock: None,
        }
    }
}
