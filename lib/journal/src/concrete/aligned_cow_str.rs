use std::{borrow::Cow, ops::Deref};

use rkyv::{
    Archive, Archived,
    rancor::Fallible,
    ser::{Allocator, WriterExt},
    vec::{ArchivedVec, VecResolver},
};

#[derive(Clone)]
pub struct AlignedCowStr<'a> {
    inner: Cow<'a, str>,
}

impl<'a> AlignedCowStr<'a> {
    pub const ALIGNMENT: usize = 16;

    pub fn into_inner(self) -> Cow<'a, str> {
        self.inner
    }

    #[inline]
    pub fn as_slice(&self) -> &str {
        self.inner.as_ref()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl Default for AlignedCowStr<'_> {
    fn default() -> Self {
        Self {
            inner: String::new().into(),
        }
    }
}

impl std::fmt::Debug for AlignedCowStr<'_> {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

impl From<String> for AlignedCowStr<'_> {
    fn from(value: String) -> Self {
        Self {
            inner: value.into(),
        }
    }
}

#[allow(clippy::from_over_into)]
impl Into<String> for AlignedCowStr<'_> {
    fn into(self) -> String {
        self.inner.into_owned()
    }
}

impl<'a> From<Cow<'a, str>> for AlignedCowStr<'a> {
    fn from(value: Cow<'a, str>) -> Self {
        Self { inner: value }
    }
}

#[allow(clippy::from_over_into)]
impl<'a> Into<Cow<'a, str>> for AlignedCowStr<'a> {
    fn into(self) -> Cow<'a, str> {
        self.inner
    }
}

impl Deref for AlignedCowStr<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl AsRef<str> for AlignedCowStr<'_> {
    #[inline]
    fn as_ref(&self) -> &str {
        self.inner.as_ref()
    }
}

impl Archive for AlignedCowStr<'_> {
    type Archived = ArchivedVec<u8>;
    type Resolver = VecResolver;

    #[inline]
    fn resolve(&self, resolver: Self::Resolver, out: rkyv::Place<Self::Archived>) {
        ArchivedVec::resolve_from_len(self.inner.len(), resolver, out);
    }
}

impl<S> rkyv::Serialize<S> for AlignedCowStr<'_>
where
    S: Fallible + WriterExt<S::Error> + Allocator + ?Sized,
    S::Error: rkyv::rancor::Source,
{
    #[inline]
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        serializer.align(Self::ALIGNMENT)?;
        ArchivedVec::<Archived<u8>>::serialize_from_slice(self.inner.as_bytes(), serializer)
    }
}
