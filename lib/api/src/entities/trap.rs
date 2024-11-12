use std::any::Any;

/// The trait that every concrete trap must implement.
pub trait TrapLike: std::error::Error {
    fn as_any(&self) -> &dyn Any;
}
