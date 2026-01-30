/// A normalized classification for throttling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IoClass {
    /// Metadata operations (lookup, stat, chmod, utimens, etc.).
    Meta,
    /// Directory listing / enumeration.
    ReadDir,
    /// File reads (bytes count is meaningful).
    Read,
    /// File writes (bytes count is meaningful).
    Write,
    /// Open/close-like operations (optional; can map to Meta).
    OpenClose,
}

#[derive(Clone, Copy, Debug)]
pub struct IoCost {
    pub class: IoClass,
    /// “1” means one logical operation (for IOPS throttles).
    pub ops: u32,
    /// Bytes for bandwidth throttles. 0 for pure metadata ops.
    pub bytes: u64,
}
