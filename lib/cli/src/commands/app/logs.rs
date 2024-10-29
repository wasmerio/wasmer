//! Show logs for an Edge app.

use crate::utils::timestamp::parse_timestamp_or_relative_time_negative_offset;
use colored::Colorize;
use comfy_table::{Cell, Table};
use futures::StreamExt;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use wasmer_backend_api::types::{Log, LogStream};

use crate::{config::WasmerEnv, opts::ListFormatOpts, utils::render::CliRender};

use super::util::AppIdentOpts;

#[derive(Debug, PartialEq, Eq, Clone, Copy, clap::ValueEnum)]
pub enum LogStreamArg {
    Stdout,
    Stderr,
}

/// Retrieve the logs of an app
#[derive(clap::Parser, Debug)]
pub struct CmdAppLogs {
    #[clap(flatten)]
    env: WasmerEnv,

    #[clap(flatten)]
    fmt: ListFormatOpts,

    /// The date of the earliest log entry.
    ///
    /// Defaults to the last 10 minutes.
    ///
    /// Format:
    /// * RFC 3339 (`2006-01-02T03:04:05-07:00`)
    /// * RFC 2822 (`Mon, 02 Jan 2006 03:04:05 MST`)
    /// * Simple date (`2022-11-11`)
    /// * Unix timestamp (`1136196245`)
    /// * Relative time (`10m` / `-1h`, `1d1h30s`)
    // TODO: should default to trailing logs once trailing is implemented.
    #[clap(long, value_parser = parse_timestamp_or_relative_time_negative_offset, conflicts_with = "request_id")]
    from: Option<OffsetDateTime>,

    /// The date of the latest log entry.
    ///
    /// Format:
    /// * RFC 3339 (`2006-01-02T03:04:05-07:00`)
    /// * RFC 2822 (`Mon, 02 Jan 2006 03:04:05 MST`)
    /// * Simple date (`2022-11-11`)
    /// * Unix timestamp (`1136196245`)
    /// * Relative time (`10m` / `1h`, `1d1h30s`)
    #[clap(long, value_parser = parse_timestamp_or_relative_time_negative_offset, conflicts_with = "request_id")]
    until: Option<OffsetDateTime>,

    /// Maximum log lines to fetch.
    /// Defaults to 1000.
    #[clap(long, default_value = "1000")]
    max: usize,

    /// Continuously watch for new logs and display them in real-time.
    #[clap(long, default_value = "false")]
    watch: bool,

    /// Streams of logs to display
    #[clap(long, value_delimiter = ',', value_enum)]
    streams: Option<Vec<LogStreamArg>>,

    #[clap(flatten)]
    #[allow(missing_docs)]
    pub ident: AppIdentOpts,

    /// The identifier of the request to show logs related to
    #[clap(long)]
    pub request_id: Option<String>,

    /// The identifier of the app instance to show logs related to
    #[clap(long, conflicts_with = "request_id", value_delimiter = ' ', num_args = 1..)]
    pub instance_id: Option<Vec<String>>,
}

#[async_trait::async_trait]
impl crate::commands::AsyncCliCommand for CmdAppLogs {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;

        let (_ident, app) = self.ident.load_app(&client).await?;

        let from = self
            .from
            .unwrap_or_else(|| OffsetDateTime::now_utc() - time::Duration::minutes(10));

        let version = app.active_version.as_ref().map_or("n/a", |v| &v.version);

        tracing::info!(
            app.name=%app.name,
            app.owner=%app.owner.global_name,
            app.version=version,
            range.start=%from,
            range.end=self.until.map(|ts| ts.to_string()),
            "Fetching logs",
        );

        let (stdout, stderr) = self
            .streams
            .map(|s| {
                let mut stdout = false;
                let mut stderr = false;

                for stream in s {
                    if matches!(stream, LogStreamArg::Stdout) {
                        stdout = true;
                    } else if matches!(stream, LogStreamArg::Stderr) {
                        stderr = true;
                    }
                }

                (stdout, stderr)
            })
            .unwrap_or_default();

        let streams = Vec::from(match (stdout, stderr) {
            (true, true) | (false, false) => &[LogStream::Stdout, LogStream::Stderr][..],
            (true, false) => &[LogStream::Stdout][..],
            (false, true) => &[LogStream::Stderr][..],
        });

        // Code duplication to avoid a dependency to `OR` streams.
        if let Some(instance_id) = &self.instance_id {
            let logs_stream = wasmer_backend_api::query::get_app_logs_paginated_filter_instance(
                &client,
                app.name.clone(),
                app.owner.global_name.to_string(),
                None, // keep version None since we want logs from all versions atm
                from,
                self.until,
                self.watch,
                Some(streams),
                instance_id.clone(),
            )
            .await;

            let mut logs_stream = std::pin::pin!(logs_stream);
            let mut rem = self.max;

            while let Some(logs) = logs_stream.next().await {
                let mut logs = logs?;

                let limit = std::cmp::min(logs.len(), rem);

                let logs: Vec<_> = logs.drain(..limit).collect();

                if !logs.is_empty() {
                    let rendered = self.fmt.format.render(&logs);
                    println!("{rendered}");

                    rem -= limit;
                }

                if !self.watch || rem == 0 {
                    break;
                }
            }
        } else if let Some(request_id) = &self.request_id {
            let logs_stream = wasmer_backend_api::query::get_app_logs_paginated_filter_request(
                &client,
                app.name.clone(),
                app.owner.global_name.to_string(),
                None, // keep version None since we want logs from all versions atm
                from,
                self.until,
                self.watch,
                Some(streams),
                request_id.clone(),
            )
            .await;

            let mut logs_stream = std::pin::pin!(logs_stream);
            let mut rem = self.max;

            while let Some(logs) = logs_stream.next().await {
                let mut logs = logs?;

                let limit = std::cmp::min(logs.len(), rem);

                let logs: Vec<_> = logs.drain(..limit).collect();

                if !logs.is_empty() {
                    let rendered = self.fmt.format.render(&logs);
                    println!("{rendered}");

                    rem -= limit;
                }

                if !self.watch || rem == 0 {
                    break;
                }
            }
        } else {
            let logs_stream = wasmer_backend_api::query::get_app_logs_paginated(
                &client,
                app.name.clone(),
                app.owner.global_name.to_string(),
                None, // keep version None since we want logs from all versions atm
                from,
                self.until,
                self.watch,
                Some(streams),
            )
            .await;

            let mut logs_stream = std::pin::pin!(logs_stream);
            let mut rem = self.max;

            while let Some(logs) = logs_stream.next().await {
                let mut logs = logs?;

                let limit = std::cmp::min(logs.len(), rem);

                let logs: Vec<_> = logs.drain(..limit).collect();

                if !logs.is_empty() {
                    let rendered = self.fmt.format.render(&logs);
                    println!("{rendered}");

                    rem -= limit;
                }

                if !self.watch || rem == 0 {
                    break;
                }
            }
        }

        Ok(())
    }
}

impl CliRender for Log {
    fn render_item_table(&self) -> String {
        let mut table = Table::new();
        // remove all borders from the table
        let Log {
            message, timestamp, ..
        }: &Log = self;

        table.add_rows([
            vec![
                "Timestamp".to_string(),
                datetime_from_unix(*timestamp).format(&Rfc3339).unwrap(),
            ],
            vec!["Message".to_string(), message.to_string()],
        ]);
        table.to_string()
    }

    fn render_list_table(items: &[Self]) -> String {
        let mut table = Table::new();
        // table.set_header(vec!["Timestamp".to_string(), "Message".to_string()]);
        table.load_preset(comfy_table::presets::NOTHING);
        table.set_content_arrangement(comfy_table::ContentArrangement::Dynamic);

        for item in items {
            let mut message = item.message.clone().bold();
            if let Some(stream) = item.stream {
                message = match stream {
                    LogStream::Stdout => message,
                    LogStream::Stderr => message.yellow(),
                    LogStream::Runtime => message.cyan(),
                };
            }
            table.add_row([
                Cell::new(format!(
                    "[{}]",
                    datetime_from_unix(item.timestamp).format(&Rfc3339).unwrap()
                ))
                .set_alignment(comfy_table::CellAlignment::Right),
                Cell::new(message),
            ]);
        }
        table.to_string()
    }
}

fn datetime_from_unix(timestamp: f64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos(timestamp as i128)
        .expect("Timestamp should always be valid")
}
