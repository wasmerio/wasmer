use std::{
    borrow::{Borrow, BorrowMut, Cow},
    fmt::{self, Pointer},
    ops::{Deref, DerefMut},
};

use rkyv::{
    Archive, Archived,
    rancor::Fallible,
    ser::{Allocator, WriterExt},
    vec::{ArchivedVec, VecResolver},
};

/// An aligned COW vector of bytes which avoids copying data
/// when its constructed. The vector is aligned on the 16-byte
/// boundary
#[derive(Clone)]
pub struct AlignedCowVec<'a, T>
where
    [T]: ToOwned,
{
    inner: Cow<'a, [T]>,
}

impl<'a, T> AlignedCowVec<'a, T>
where
    T: 'a,
    [T]: ToOwned,
{
    /// The alignment of the vector
    pub const ALIGNMENT: usize = 16;

    pub fn into_inner(self) -> Cow<'a, [T]> {
        self.inner
    }

    #[inline]
    pub fn as_slice(&self) -> &[T] {
        self.inner.as_ref()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn len_with_padding(&self) -> usize {
        let mut ret = self.inner.len() * std::mem::size_of::<T>();
        let padding = ret % Self::ALIGNMENT;
        if padding != 0 {
            ret += Self::ALIGNMENT - padding;
        }
        ret
    }
}

impl<'a, T> Default for AlignedCowVec<'a, T>
where
    T: 'a + Clone,
    [T]: ToOwned,
{
    fn default() -> Self {
        Self {
            inner: Vec::new().into(),
        }
    }
}

impl<'a, T> fmt::Debug for AlignedCowVec<'a, T>
where
    T: 'a,
    [T]: ToOwned,
{
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_slice().fmt(f)
    }
}

impl<'a, T> From<Vec<T>> for AlignedCowVec<'a, T>
where
    T: 'a + Clone,
    [T]: ToOwned,
{
    fn from(value: Vec<T>) -> Self {
        Self {
            inner: value.into(),
        }
    }
}

#[allow(clippy::from_over_into)]
impl Into<Vec<u8>> for AlignedCowVec<'_, u8> {
    fn into(self) -> Vec<u8> {
        self.inner.into_owned()
    }
}

impl<'a, T> From<Cow<'a, [T]>> for AlignedCowVec<'a, T>
where
    T: 'a,
    [T]: ToOwned,
{
    fn from(value: Cow<'a, [T]>) -> Self {
        Self { inner: value }
    }
}

#[allow(clippy::from_over_into)]
impl<'a, T> Into<Cow<'a, [T]>> for AlignedCowVec<'a, T>
where
    T: 'a,
    [T]: ToOwned,
{
    fn into(self) -> Cow<'a, [T]> {
        self.inner
    }
}

impl<'a, T> Deref for AlignedCowVec<'a, T>
where
    T: 'a,
    [T]: ToOwned,
{
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<'a, T> DerefMut for AlignedCowVec<'a, T>
where
    T: 'a,
    [T]: ToOwned,
    <[T] as ToOwned>::Owned: BorrowMut<[T]>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.to_mut().borrow_mut()
    }
}

impl<'a, T> AsMut<[T]> for AlignedCowVec<'a, T>
where
    T: 'a,
    [T]: ToOwned,
    <[T] as ToOwned>::Owned: BorrowMut<[T]>,
{
    #[inline]
    fn as_mut(&mut self) -> &mut [T] {
        self.inner.to_mut().borrow_mut()
    }
}

impl<'a, T> AsRef<[T]> for AlignedCowVec<'a, T>
where
    T: 'a,
    [T]: ToOwned,
{
    #[inline]
    fn as_ref(&self) -> &[T] {
        self.inner.as_ref()
    }
}

impl<'a, T> Borrow<[T]> for AlignedCowVec<'a, T>
where
    T: 'a,
    [T]: ToOwned,
{
    #[inline]
    fn borrow(&self) -> &[T] {
        self.inner.borrow()
    }
}

impl<'a, T> BorrowMut<[T]> for AlignedCowVec<'a, T>
where
    T: 'a,
    [T]: ToOwned,
    <[T] as ToOwned>::Owned: BorrowMut<[T]>,
{
    #[inline]
    fn borrow_mut(&mut self) -> &mut [T] {
        self.inner.to_mut().borrow_mut()
    }
}

impl<'a, T> Archive for AlignedCowVec<'a, T>
where
    T: 'a,
    [T]: ToOwned,
{
    type Archived = ArchivedVec<T>;
    type Resolver = VecResolver;

    #[inline]
    fn resolve(&self, resolver: Self::Resolver, out: rkyv::Place<Self::Archived>) {
        ArchivedVec::resolve_from_len(self.len(), resolver, out);
    }
}

impl<S> rkyv::Serialize<S> for AlignedCowVec<'_, u8>
where
    S: Fallible + WriterExt<S::Error> + Allocator + ?Sized,
    S::Error: rkyv::rancor::Source,
{
    #[inline]
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        serializer.align(Self::ALIGNMENT)?;
        ArchivedVec::<Archived<u8>>::serialize_from_slice(self.as_slice(), serializer)
    }
}
