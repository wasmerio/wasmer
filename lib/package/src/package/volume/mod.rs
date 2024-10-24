pub(crate) mod fs;
pub(crate) mod in_memory;

use std::{fmt::Debug, sync::Arc};

use anyhow::Error;

use webc::{v3::write::Directory, AbstractVolume};

use super::Strictness;

pub trait IntoSuper<Super: ?Sized> {
    fn into_super(self: Arc<Self>) -> Arc<Super>;
}

impl<T: AbstractVolume + Send + Sync + 'static>
    IntoSuper<dyn AbstractVolume + Send + Sync + 'static> for T
{
    fn into_super(self: Arc<Self>) -> Arc<dyn AbstractVolume + Send + Sync + 'static> {
        self
    }
}

/// An abstraction over concrete volumes implementation to be used in a Wasmer Package.
pub trait WasmerPackageVolume:
    AbstractVolume
    + Send
    + Sync
    + 'static
    + Debug
    + IntoSuper<dyn AbstractVolume + Send + Sync + 'static>
{
    fn as_volume(self: Arc<Self>) -> Arc<dyn AbstractVolume + Send + Sync + 'static> {
        self.into_super()
    }

    /// Serialize the volume as a [`webc::v3::write::Directory`].  
    fn as_directory_tree(&self, strictness: Strictness) -> Result<Directory<'_>, Error>;
}
