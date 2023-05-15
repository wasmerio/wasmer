mod builtin_loader;

pub use self::builtin_loader::BuiltinLoader;

use std::{fmt::Debug, ops::Deref};

use anyhow::Error;
use webc::compat::Container;

use crate::runtime::resolver::Summary;

#[async_trait::async_trait]
pub trait PackageLoader: Debug {
    async fn load(&self, summary: &Summary) -> Result<Container, Error>;
}

#[async_trait::async_trait]
impl<D, P> PackageLoader for D
where
    D: Deref<Target = P> + Debug + Send + Sync,
    P: PackageLoader + Send + Sync + ?Sized + 'static,
{
    async fn load(&self, summary: &Summary) -> Result<Container, Error> {
        (**self).load(summary).await
    }
}
