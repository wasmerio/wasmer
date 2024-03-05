//! Show logs for an Edge app.

use comfy_table::Table;
use edge_schema::pretty_duration::parse_timestamp_or_relative_time;
use futures::StreamExt;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use wasmer_api::types::{Log, LogStream};

use crate::{
    opts::{ApiOpts, ListFormatOpts},
    utils::{render::CliRender, Identifier},
};

#[derive(Debug, PartialEq, Eq, Clone, Copy, clap::ValueEnum)]
pub enum LogStreamArg {
    Stdout,
    Stderr,
}

/// Show an app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppLogs {
    #[clap(flatten)]
    api: ApiOpts,
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
    #[clap(long, value_parser = parse_timestamp_or_relative_time)]
    from: Option<OffsetDateTime>,

    /// The date of the latest log entry.
    ///
    /// Format:
    /// * RFC 3339 (`2006-01-02T03:04:05-07:00`)
    /// * RFC 2822 (`Mon, 02 Jan 2006 03:04:05 MST`)
    /// * Simple date (`2022-11-11`)
    /// * Unix timestamp (`1136196245`)
    /// * Relative time (`10m` / `1h`, `1d1h30s`)
    #[clap(long, value_parser = parse_timestamp_or_relative_time)]
    until: Option<OffsetDateTime>,

    /// Maximum log lines to fetch.
    /// Defaults to 1000.
    #[clap(long, default_value = "1000")]
    max: usize,

    #[clap(long, default_value = "false")]
    watch: bool,

    /// The name of the app.
    ///
    /// Eg:
    /// - name (assumes current user)
    /// - namespace/name
    /// - namespace/name@version
    ident: Identifier,

    /// Streams of logs to display
    #[clap(long, value_delimiter = ',', value_enum)]
    streams: Option<Vec<LogStreamArg>>,
}

#[async_trait::async_trait]
impl crate::commands::AsyncCliCommand for CmdAppLogs {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.api.client()?;

        let Identifier {
            name,
            owner,
            version,
        } = &self.ident;

        let owner = match owner {
            Some(owner) => owner.to_string(),
            None => {
                let user = wasmer_api::query::current_user_with_namespaces(&client, None).await?;
                user.username
            }
        };

        let from = self
            .from
            .unwrap_or_else(|| OffsetDateTime::now_utc() - time::Duration::minutes(10));

        tracing::info!(
            package.name=%self.ident.name,
            package.owner=%owner,
            package.version=self.ident.version.as_deref(),
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

        let logs_stream = wasmer_api::query::get_app_logs_paginated(
            &client,
            name.clone(),
            owner.to_string(),
            version.clone(),
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

        Ok(())
    }
}

impl CliRender for Log {
    fn render_item_table(&self) -> String {
        let mut table = Table::new();

        let Log { message, timestamp } = self;

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
        table.set_header(vec!["Timestamp".to_string(), "Message".to_string()]);

        for item in items {
            table.add_row([
                datetime_from_unix(item.timestamp).format(&Rfc3339).unwrap(),
                item.message.clone(),
            ]);
        }
        table.to_string()
    }
}

fn datetime_from_unix(timestamp: f64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos(timestamp as i128)
        .expect("Timestamp should always be valid")
}
