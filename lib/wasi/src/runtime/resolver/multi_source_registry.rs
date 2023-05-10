use std::sync::Arc;

use anyhow::Error;

use crate::runtime::resolver::{PackageSpecifier, Registry, Source, Summary};

/// A registry that works by querying multiple [`Source`]s in succession.
#[derive(Debug, Clone)]
pub struct MultiSourceRegistry {
    sources: Vec<Arc<dyn Source + Send + Sync>>,
}

impl MultiSourceRegistry {
    pub const fn new() -> Self {
        MultiSourceRegistry {
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
impl Registry for MultiSourceRegistry {
    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<Summary>, Error> {
        for source in &self.sources {
            let result = source.query(package).await?;
            if !result.is_empty() {
                return Ok(result);
            }
        }

        anyhow::bail!("Unable to find any packages that satisfy the query")
    }
}
