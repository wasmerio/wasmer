use std::sync::Arc;

use anyhow::Error;

use crate::runtime::resolver::{PackageSpecifier, PackageSummary, Source};

/// A [`Source`] that works by querying multiple [`Source`]s in succession.
///
/// The first [`Source`] to return one or more [`Summaries`][PackageSummary]
/// will be treated as the canonical source for that [`Dependency`][dep] and no
/// further [`Source`]s will be queried.
///
/// [dep]: crate::runtime::resolver::Dependency
#[derive(Debug, Clone)]
pub struct MultiSource {
    sources: Vec<Arc<dyn Source + Send + Sync>>,
}

impl MultiSource {
    pub const fn new() -> Self {
        MultiSource {
            sources: Vec::new(),
        }
    }

    pub fn add_source(&mut self, source: impl Source + Send + Sync + 'static) -> &mut Self {
        self.add_shared_source(Arc::new(source));
        self
    }

    pub fn add_shared_source(&mut self, source: Arc<dyn Source + Send + Sync>) -> &mut Self {
        self.sources.push(source);
        self
    }
}

#[async_trait::async_trait]
impl Source for MultiSource {
    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<PackageSummary>, Error> {
        for source in &self.sources {
            let result = source.query(package).await?;
            if !result.is_empty() {
                return Ok(result);
            }
        }

        anyhow::bail!("Unable to find any packages that satisfy the query")
    }
}
