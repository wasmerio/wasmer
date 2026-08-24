use std::sync::Arc;

use wasmer_config::package::PackageSource;

use crate::runtime::resolver::{PackageSummary, QueryError, Source};

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

impl Default for MultiSource {
    fn default() -> Self {
        Self::new()
    }
}

impl MultiSource {
    pub fn new() -> Self {
        MultiSource {
            sources: Vec::new(),
            strategy: MultiSourceStrategy::default(),
        }
    }

    pub fn add_source(&mut self, source: impl Source + Send + 'static) -> &mut Self {
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

/// Keep whichever skipped error explains the overall failure better, so a bare
/// `NotFound` from one source cannot mask another source's all-matches-yanked
/// `NoMatches`.
fn keep_most_informative(slot: &mut Option<QueryError>, error: QueryError) {
    fn rank(error: &QueryError) -> u8 {
        match error {
            QueryError::Unsupported { .. } => 0,
            QueryError::NotFound { .. } => 1,
            QueryError::NoMatches {
                yanked_versions, ..
            } => 2 + u8::from(!yanked_versions.is_empty()),
            _ => 4,
        }
    }

    if slot.as_ref().is_none_or(|kept| rank(kept) < rank(&error)) {
        *slot = Some(error);
    }
}

#[async_trait::async_trait]
impl Source for MultiSource {
    #[tracing::instrument(level = "debug", skip_all, fields(%package))]
    async fn query(&self, package: &PackageSource) -> Result<Vec<PackageSummary>, QueryError> {
        let mut output = Vec::<PackageSummary>::new();
        let mut skipped_error: Option<QueryError> = None;

        for source in &self.sources {
            match source.query(package).await {
                Ok(mut summaries) => {
                    if self.strategy.merge_results {
                        // Extend matches, but skip already found versions.
                        summaries.retain(|new| {
                            !output.iter().any(|existing| new.pkg.id == existing.pkg.id)
                        });
                        output.extend(summaries);
                    } else {
                        return Ok(summaries);
                    }
                }
                Err(error @ QueryError::Unsupported { .. })
                    if self.strategy.continue_if_unsupported || self.strategy.merge_results =>
                {
                    keep_most_informative(&mut skipped_error, error);
                    continue;
                }
                Err(error @ QueryError::NotFound { .. })
                    if self.strategy.continue_if_not_found || self.strategy.merge_results =>
                {
                    keep_most_informative(&mut skipped_error, error);
                    continue;
                }
                Err(error @ QueryError::NoMatches { .. })
                    if self.strategy.continue_if_no_matches || self.strategy.merge_results =>
                {
                    keep_most_informative(&mut skipped_error, error);
                    continue;
                }
                // Generic errors do not respect the `merge_results` strategy
                // flag, because unexpected errors should be bubbled to the
                // caller.
                Err(e) => return Err(e),
            }
        }

        if !output.is_empty() {
            output.sort_by(|a, b| a.pkg.id.cmp(&b.pkg.id));

            Ok(output)
        } else {
            Err(skipped_error.unwrap_or_else(|| QueryError::NotFound {
                query: package.clone(),
            }))
        }
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

    /// Merge results from all sources into a single result.
    ///
    /// True by default.
    pub merge_results: bool,
}

impl Default for MultiSourceStrategy {
    fn default() -> Self {
        MultiSourceStrategy {
            continue_if_unsupported: true,
            continue_if_not_found: true,
            continue_if_no_matches: true,
            merge_results: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use wasmer_config::package::PackageId;

    use super::super::{DistributionInfo, InMemorySource, PackageInfo, WebcHash};
    use super::*;

    /// Test that the `MultiSource` can merge results from multiple sources.
    #[tokio::test]
    async fn test_multi_source_merge() {
        let id1 = PackageId::new_named("ns/pkg", "0.0.1".parse().unwrap());
        let pkg1 = PackageSummary {
            pkg: PackageInfo {
                id: id1.clone(),
                commands: Vec::new(),
                entrypoint: None,
                dependencies: Vec::new(),
                filesystem: Vec::new(),
            },
            dist: DistributionInfo {
                webc: "https://example.com/ns/pkg/0.0.1".parse().unwrap(),
                webc_sha256: WebcHash([0u8; 32]),
            },
        };

        let id2 = PackageId::new_named("ns/pkg", "0.0.2".parse().unwrap());
        let mut pkg2 = pkg1.clone();
        pkg2.pkg.id = id2.clone();

        let mut mem1 = InMemorySource::new();
        mem1.add(pkg1);

        let mut mem2 = InMemorySource::new();
        mem2.add(pkg2);

        let mut multi = MultiSource::new();
        multi.add_source(mem1);
        multi.add_source(mem2);

        let summaries = multi.query(&"ns/pkg".parse().unwrap()).await.unwrap();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].pkg.id, id1);
        assert_eq!(summaries[1].pkg.id, id2);
    }

    #[derive(Debug)]
    struct FixedErrorSource(QueryError);

    #[async_trait::async_trait]
    impl Source for FixedErrorSource {
        async fn query(&self, _: &PackageSource) -> Result<Vec<PackageSummary>, QueryError> {
            Err(self.0.clone())
        }
    }

    /// A source that only skipped yanked versions must win over one that never
    /// had the package, so the user learns why nothing matched.
    #[tokio::test]
    async fn all_yanked_no_matches_is_not_masked_by_not_found() {
        let package: PackageSource = "ns/pkg@^1".parse().unwrap();

        let mut multi = MultiSource::new();
        multi.add_source(FixedErrorSource(QueryError::NotFound {
            query: package.clone(),
        }));
        multi.add_source(FixedErrorSource(QueryError::NoMatches {
            query: package.clone(),
            yanked_versions: vec!["1.2.3".parse().unwrap()],
        }));

        let err = multi.query(&package).await.unwrap_err();
        let QueryError::NoMatches {
            yanked_versions, ..
        } = err
        else {
            panic!("expected NoMatches, got {err:?}");
        };
        assert_eq!(yanked_versions.len(), 1);
        assert_eq!(yanked_versions[0].to_string(), "1.2.3");
    }
}
