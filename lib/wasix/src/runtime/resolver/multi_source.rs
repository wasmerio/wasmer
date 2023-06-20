use std::sync::Arc;

use crate::runtime::resolver::{PackageSpecifier, PackageSummary, QueryError, Source};

/// A [`Source`] that works by querying multiple [`Source`]s in succession.
///
/// # Error Handling
///
/// A [`Source`] implementation can return certain non-fatal errors and,
/// depending on the [`MultiSourceStrategy`], the [`MultiSource`] can choose to
/// deal with it in different ways. Sometimes
///
///
/// The first [`Source`] to return one or more [`Summaries`][PackageSummary]
/// will be treated as the canonical source for that [`Dependency`][dep] and no
/// further [`Source`]s will be queried.
///
/// [dep]: crate::runtime::resolver::Dependency
#[derive(Debug, Clone)]
pub struct MultiSource {
    sources: Vec<Arc<dyn Source + Send + Sync>>,
    strategy: MultiSourceStrategy,
}

impl MultiSource {
    pub const fn new() -> Self {
        MultiSource {
            sources: Vec::new(),
            strategy: MultiSourceStrategy::default(),
        }
    }

    pub fn add_source(&mut self, source: impl Source + Send + Sync + 'static) -> &mut Self {
        self.add_shared_source(Arc::new(source))
    }

    pub fn add_shared_source(&mut self, source: Arc<dyn Source + Send + Sync>) -> &mut Self {
        self.sources.push(source);
        self
    }

    /// Override the strategy used when a [`Source`] returns a non-fatal error.
    pub fn with_strategy(self, strategy: MultiSourceStrategy) -> Self {
        MultiSource { strategy, ..self }
    }
}

#[async_trait::async_trait]
impl Source for MultiSource {
    #[tracing::instrument(level = "debug", skip_all, fields(%package))]
    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<PackageSummary>, QueryError> {
        for source in &self.sources {
            match source.query(package).await {
                Ok(summaries) => return Ok(summaries),
                Err(QueryError::Unsupported) if self.strategy.continue_if_unsupported => continue,
                Err(QueryError::NotFound) if self.strategy.continue_if_not_found => continue,
                Err(QueryError::NoMatches { .. }) if self.strategy.continue_if_no_matches => {
                    continue
                }
                Err(e) => return Err(e),
            }
        }

        Err(QueryError::NotFound)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MultiSourceStrategy {
    /// If encountered, treat [`QueryError::Unsupported`] as a non-fatal error
    /// and query the next [`Source`] in turn.
    ///
    /// This flag is **enabled** by default.
    pub continue_if_unsupported: bool,
    /// If encountered, treat [`QueryError::NotFound`] as a non-fatal error and
    /// query the next [`Source`] in turn.
    ///
    /// This flag is **enabled** by default and can be used to let earlier
    /// [`Source`]s "override" later ones.
    pub continue_if_not_found: bool,
    /// If encountered, treat [`QueryError::NoMatches`] as a non-fatal error and
    /// query the next [`Source`] in turn.
    ///
    /// This flag is **disabled** by default.
    pub continue_if_no_matches: bool,
}

impl MultiSourceStrategy {
    pub const fn default() -> Self {
        MultiSourceStrategy {
            continue_if_unsupported: true,
            continue_if_not_found: true,
            continue_if_no_matches: true,
        }
    }
}

impl Default for MultiSourceStrategy {
    fn default() -> Self {
        MultiSourceStrategy::default()
    }
}
