use crate::error::GrowError;

pub trait Grow<T> {
    fn grow(&self, delta: T) -> Result<T, GrowError>;
}
