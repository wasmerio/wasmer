//! Search for packages in the registry.

use wasmer_backend_api::types::{DateTime, SearchPackageVersion};
use wasmer_sdk::package::search::{
    CountComparison, CountFilter, PackageOrderBy, PackagesFilter, SearchOptions, SearchOrderSort,
    SearchPublishDate,
};

use crate::{
    commands::AsyncCliCommand, config::WasmerEnv, opts::ListFormatOpts, utils::render::ListFormat,
};

const NO_PACKAGES_FOUND_MESSAGE: &str = "No packages found.";

/// Field to order results by.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum OrderBy {
    Alphabetically,
    Size,
    Downloads,
    Published,
    Created,
    Likes,
}

impl From<OrderBy> for PackageOrderBy {
    fn from(value: OrderBy) -> Self {
        match value {
            OrderBy::Alphabetically => PackageOrderBy::Alphabetically,
            OrderBy::Size => PackageOrderBy::Size,
            OrderBy::Downloads => PackageOrderBy::TotalDownloads,
            OrderBy::Published => PackageOrderBy::PublishedDate,
            OrderBy::Created => PackageOrderBy::CreatedDate,
            OrderBy::Likes => PackageOrderBy::TotalLikes,
        }
    }
}

/// Sort direction.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum Sort {
    Asc,
    Desc,
}

impl From<Sort> for SearchOrderSort {
    fn from(value: Sort) -> Self {
        match value {
            Sort::Asc => SearchOrderSort::Asc,
            Sort::Desc => SearchOrderSort::Desc,
        }
    }
}

/// Relative window for the last-published date filter.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
#[allow(clippy::enum_variant_names)] // `last-day`/`last-week`/... are the desired CLI values
enum PublishedWithin {
    LastDay,
    LastWeek,
    LastMonth,
    LastYear,
}

impl From<PublishedWithin> for SearchPublishDate {
    fn from(value: PublishedWithin) -> Self {
        match value {
            PublishedWithin::LastDay => SearchPublishDate::LastDay,
            PublishedWithin::LastWeek => SearchPublishDate::LastWeek,
            PublishedWithin::LastMonth => SearchPublishDate::LastMonth,
            PublishedWithin::LastYear => SearchPublishDate::LastYear,
        }
    }
}

/// A `count >= n` filter (the common case for download/like/size thresholds).
fn at_least(count: i32) -> CountFilter {
    CountFilter {
        count: Some(count),
        comparison: Some(CountComparison::GreaterThanOrEqual),
    }
}

/// Validate an RFC3339 timestamp at parse time so a bad value is a clap error
/// rather than a cryptic backend scalar rejection.
fn parse_rfc3339(value: &str) -> Result<DateTime, String> {
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map(|_| DateTime(value.to_string()))
        .map_err(|e| {
            format!("`{value}` is not an RFC3339 timestamp (e.g. 2024-01-01T00:00:00Z): {e}")
        })
}

fn render_search_results(format: ListFormat, results: &[SearchPackageVersion]) -> String {
    if results.is_empty() && matches!(format, ListFormat::Table | ListFormat::ItemTable) {
        NO_PACKAGES_FOUND_MESSAGE.to_string()
    } else {
        format.render(results)
    }
}

/// Search for packages in the registry.
#[derive(clap::Parser, Debug)]
pub struct PackageSearch {
    #[clap(flatten)]
    fmt: ListFormatOpts,

    #[clap(flatten)]
    env: WasmerEnv,

    /// Only show packages owned by this user or namespace.
    #[clap(long)]
    owner: Option<String>,

    /// Only show packages published by this user.
    #[clap(long)]
    published_by: Option<String>,

    /// Filter by curated status. `--curated` for curated only, `--curated=false`
    /// for non-curated.
    #[clap(long, num_args = 0..=1, default_missing_value = "true", require_equals = true)]
    curated: Option<bool>,

    /// Filter by deployable status. `--deployable` for deployable only,
    /// `--deployable=false` for non-deployable.
    #[clap(long, num_args = 0..=1, default_missing_value = "true", require_equals = true)]
    deployable: Option<bool>,

    /// Only show packages whose latest version has bindings.
    #[clap(long)]
    has_bindings: bool,

    /// Only show packages whose latest version has commands.
    #[clap(long)]
    has_commands: bool,

    /// Only show standalone packages (implies --has-commands).
    #[clap(long)]
    standalone: bool,

    /// Only show packages exposing this interface (repeatable).
    #[clap(long = "interface", value_name = "NAME")]
    interfaces: Vec<String>,

    /// Only show packages whose latest version's license matches (substring).
    #[clap(long)]
    license: Option<String>,

    /// Only show packages with at least this many downloads.
    #[clap(long, value_parser = clap::value_parser!(i32).range(0..))]
    min_downloads: Option<i32>,

    /// Only show packages with at least this many likes.
    #[clap(long, value_parser = clap::value_parser!(i32).range(0..))]
    min_likes: Option<i32>,

    /// Only show packages whose latest version is at least this many bytes.
    #[clap(long, value_parser = clap::value_parser!(i32).range(0..))]
    min_size: Option<i32>,

    /// Only show packages created on or after this RFC3339 timestamp
    /// (e.g. 2024-01-01T00:00:00Z).
    #[clap(long, value_parser = parse_rfc3339)]
    created_after: Option<DateTime>,

    /// Only show packages created on or before this RFC3339 timestamp.
    #[clap(long, value_parser = parse_rfc3339)]
    created_before: Option<DateTime>,

    /// Only show packages last published on or after this RFC3339 timestamp.
    #[clap(long, value_parser = parse_rfc3339)]
    published_after: Option<DateTime>,

    /// Only show packages last published on or before this RFC3339 timestamp.
    #[clap(long, value_parser = parse_rfc3339)]
    published_before: Option<DateTime>,

    /// Only show packages published within the given window.
    #[clap(long, value_enum)]
    published_within: Option<PublishedWithin>,

    /// Field to order results by.
    #[clap(long, value_enum, default_value = "published")]
    order_by: OrderBy,

    /// Sort direction.
    #[clap(long, value_enum, default_value = "desc")]
    sort: Sort,

    /// Maximum number of results to display.
    #[clap(long, default_value = "50")]
    max: usize,

    /// The search query. Leave empty to list all (matching) packages.
    #[clap(default_value = "")]
    query: String,
}

#[async_trait::async_trait]
impl AsyncCliCommand for PackageSearch {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client_unauthennticated()?;

        let with_interfaces =
            (!self.interfaces.is_empty()).then(|| self.interfaces.into_iter().map(Some).collect());

        let filter = PackagesFilter {
            owner: self.owner,
            published_by: self.published_by,
            curated: self.curated,
            deployable: self.deployable,
            has_bindings: self.has_bindings.then_some(true),
            has_commands: self.has_commands.then_some(true),
            is_standalone: self.standalone.then_some(true),
            with_interfaces,
            license: self.license,
            downloads: self.min_downloads.map(at_least),
            likes: self.min_likes.map(at_least),
            size: self.min_size.map(at_least),
            created_after: self.created_after,
            created_before: self.created_before,
            last_published_after: self.published_after,
            last_published_before: self.published_before,
            publish_date: self.published_within.map(Into::into),
            order_by: Some(self.order_by.into()),
            sort_by: Some(self.sort.into()),
            ..Default::default()
        };

        let results = wasmer_sdk::package::search::search_packages(
            &client,
            SearchOptions {
                query: self.query,
                filter,
                limit: Some(self.max),
            },
        )
        .await?;

        println!(
            "{}",
            render_search_results(self.fmt.format, results.as_slice())
        );

        Ok(())
    }
}
