pub(crate) mod abstract_volume;
pub(crate) mod fs;
pub(crate) mod in_memory;

pub(crate) use abstract_volume::AbstractVolume;
use abstract_volume::Metadata;
use std::{fmt::Debug, sync::Arc};

use anyhow::Error;
use shared_buffer::OwnedBuffer;

use webc::{v3::write::Directory, PathSegment, PathSegments};

use super::Strictness;

/// An abstraction over concrete volumes implementation to be used in a Wasmer Package.
pub trait WasmerPackageVolume: Debug {
    /// Read a file.  
    fn read_file(&self, path: &PathSegments) -> Option<OwnedBuffer>;

    /// Read a directory.  
    fn read_dir(
        &self,
        path: &PathSegments,
    ) -> Option<Vec<(PathSegment, Option<[u8; 32]>, Metadata)>>;

    /// Get metadata.  
    fn metadata(&self, path: &PathSegments) -> Option<Metadata>;

    /// Serialize the volume as a [`webc::v3::write::Directory`].  
    fn as_directory_tree(&self, strictness: Strictness) -> Result<Directory<'_>, Error>;
}

impl<A: WasmerPackageVolume> AbstractVolume for A {
    fn metadata(&self, path: &PathSegments) -> Option<Metadata> {
        self.metadata(path)
    }

    fn read_dir(
        &self,
        path: &PathSegments,
    ) -> Option<Vec<(PathSegment, Option<[u8; 32]>, Metadata)>> {
        self.read_dir(path)
    }

    fn read_file(&self, path: &PathSegments) -> Option<(OwnedBuffer, Option<[u8; 32]>)> {
        self.read_file(path).map(|b| (b, None))
    }
}

impl AbstractVolume for Arc<dyn WasmerPackageVolume + Send + Sync + 'static> {
    fn metadata(&self, path: &PathSegments) -> Option<Metadata> {
        (**self).metadata(path)
    }

    fn read_dir(
        &self,
        path: &PathSegments,
    ) -> Option<Vec<(PathSegment, Option<[u8; 32]>, Metadata)>> {
        (**self).read_dir(path)
    }

    fn read_file(&self, path: &PathSegments) -> Option<(OwnedBuffer, Option<[u8; 32]>)> {
        (**self).read_file(path).map(|b| (b, None))
    }
}
