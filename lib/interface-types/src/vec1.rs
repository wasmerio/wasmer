//! `Vec1<T>` represents a non-empty `Vec<T>`.

use std::{
    error,
    fmt::{self, Debug},
    ops,
};

/// `Vec1<T>` represents a non-empty `Vec<T>`. It derefs to `Vec<T>`
/// directly.
#[derive(Clone, PartialEq)]
pub struct Vec1<T>(Vec<T>)
where
    T: Debug;

/// Represents the only error that can be emitted by `Vec1`, i.e. when
/// the number of items is zero.
#[derive(Debug)]
pub struct EmptyVec;

impl error::Error for EmptyVec {}

impl fmt::Display for EmptyVec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Vec1 must as least contain one item, zero given")
    }
}

impl<T> Vec1<T>
where
    T: Debug,
{
    /// Creates a new non-empty vector, based on an inner `Vec<T>`. If
    /// the inner vector is empty, a `EmptyVec` error is returned.
    pub fn new(items: Vec<T>) -> Result<Self, EmptyVec> {
        if items.len() == 0 {
            Err(EmptyVec)
        } else {
            Ok(Self(items))
        }
    }
}

impl<T> fmt::Debug for Vec1<T>
where
    T: Debug,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}", self.0)
    }
}

impl<T> ops::Deref for Vec1<T>
where
    T: Debug,
{
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
