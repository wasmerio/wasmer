use std::path::{Path, PathBuf};

use anyhow::Error;
use url::Url;

use crate::runtime::resolver::{PackageSpecifier, Source, SourceId, SourceKind, Summary};

/// A [`Source`] which uses the `*.webc` files in a particular directory to
/// resolve dependencies.
///
/// This is typically used during testing to inject well-known packages into the
/// dependency resolution process.
#[derive(Debug, Clone)]
pub struct DirectorySource {
    path: PathBuf,
}

impl DirectorySource {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        DirectorySource { path: dir.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[async_trait::async_trait]
impl Source for DirectorySource {
    fn id(&self) -> SourceId {
        SourceId::new(
            SourceKind::LocalRegistry,
            Url::from_directory_path(&self.path).unwrap(),
        )
    }

    async fn query(&self, _package: &PackageSpecifier) -> Result<Vec<Summary>, Error> {
        todo!();
    }
}
