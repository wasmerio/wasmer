//! Time-related core types.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VfsTimespec {
    /// Seconds since the Unix epoch.
    pub secs: i64,
    /// Nanoseconds fraction.
    pub nanos: u32,
}
