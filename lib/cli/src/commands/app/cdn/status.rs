use bytesize::ByteSize;
use comfy_table::{Cell, Table};
use time::{Duration, OffsetDateTime};

use super::{AppIdentOpts, WasmerEnv};
use crate::{
    commands::AsyncCliCommand,
    opts::ItemFormatOpts,
    utils::render::{CliRender, ListFormat},
};

/// Show CDN cache status for an app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppCdnStatus {
    #[clap(flatten)]
    pub env: WasmerEnv,

    #[clap(flatten)]
    pub fmt: ItemFormatOpts,

    #[clap(flatten)]
    pub ident: AppIdentOpts,

    /// Include CDN cache metrics for the last 30 days.
    #[clap(long)]
    pub with_metrics: bool,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppCdnStatus {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;
        let cdn_status = wasmer_backend_api::query::app_cdn_cache_status(
            &client,
            wasmer_backend_api::types::GetAppCdnCacheStatusVars {
                app: app.id.clone(),
            },
        )
        .await?;

        let metrics = if self.with_metrics {
            let end_at = OffsetDateTime::now_utc();
            let start_at = end_at - Duration::days(30);
            let metrics = wasmer_backend_api::query::app_cdn_cache_metrics(
                &client,
                wasmer_backend_api::types::GetAppCdnCacheMetricsVars {
                    app: app.id.clone(),
                    start_at: start_at.try_into()?,
                    end_at: end_at.try_into()?,
                    grouped_by: wasmer_backend_api::types::MetricGrouping::ByDay,
                },
            )
            .await?;
            Some(CdnCacheMetricsStatus::from(metrics))
        } else {
            None
        };

        let status = CdnCacheStatus {
            app: app.name,
            owner: app.owner.global_name,
            enabled: cdn_status.cdn_cache_enabled,
            last_purged_at: cdn_status.cdn_cache_purged_at.map(|dt| dt.0),
            metrics,
        };

        println!("{}", self.fmt.get().render(&status));

        Ok(())
    }
}

#[derive(Debug, serde::Serialize)]
struct CdnCacheStatus {
    app: String,
    owner: String,
    enabled: bool,
    last_purged_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metrics: Option<CdnCacheMetricsStatus>,
}

#[derive(Debug, serde::Serialize)]
struct CdnCacheMetricsStatus {
    cached_requests: i64,
    total_requests: i64,
    cached_network_egress_bytes: i64,
    total_network_egress_bytes: i64,
}

impl From<wasmer_backend_api::types::AppCdnCacheMetrics> for CdnCacheMetricsStatus {
    fn from(value: wasmer_backend_api::types::AppCdnCacheMetrics) -> Self {
        let requests = value.grouped_metrics.totals.requests;
        Self {
            cached_requests: requests.cached_requests.0,
            total_requests: requests.total_requests.0,
            cached_network_egress_bytes: requests.data_cached_bytes.0,
            total_network_egress_bytes: requests.data_served_bytes.0,
        }
    }
}

impl CliRender for CdnCacheStatus {
    fn render_item_table(&self) -> String {
        let mut table = Table::new();
        table.load_preset(comfy_table::presets::NOTHING);
        table.set_content_arrangement(comfy_table::ContentArrangement::Dynamic);
        table.set_header(vec![
            Cell::new("App"),
            Cell::new("Owner"),
            Cell::new("Status"),
            Cell::new("Last purge"),
            Cell::new("Cached requests"),
            Cell::new("Cached egress"),
        ]);

        let (cached_requests, cached_egress) = match &self.metrics {
            Some(metrics) => (
                format!("{} / {}", metrics.cached_requests, metrics.total_requests),
                format!(
                    "{} / {}",
                    ByteSize(metrics.cached_network_egress_bytes as u64),
                    ByteSize(metrics.total_network_egress_bytes as u64)
                ),
            ),
            None => ("-".to_string(), "-".to_string()),
        };

        table.add_row(vec![
            Cell::new(&self.app),
            Cell::new(&self.owner),
            Cell::new(if self.enabled { "enabled" } else { "disabled" }),
            Cell::new(self.last_purged_at.as_deref().unwrap_or("-")),
            Cell::new(cached_requests),
            Cell::new(cached_egress),
        ]);

        table.to_string()
    }

    fn render_list(_items: &[Self], _format: ListFormat) -> String {
        unreachable!("CDN cache status is rendered as a single item")
    }

    fn render_list_table(items: &[Self]) -> String {
        items
            .iter()
            .map(Self::render_item_table)
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}
