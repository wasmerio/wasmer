//! Cron job commands.

use std::{
    io::{self, IsTerminal, Write},
    num::NonZeroU32,
};

use anyhow::{Context, bail};
use time::OffsetDateTime;

use crate::{
    commands::{AsyncCliCommand, app::AppIdentArgOpts},
    config::WasmerEnv,
    opts::{ItemFormatOpts, ListFormatOpts},
    utils::{render::ListFormat, timestamp::parse_timestamp_or_relative_time_negative_offset},
};

const DEFAULT_CRON_JOB_PAGE_SIZE: NonZeroU32 =
    match NonZeroU32::new(wasmer_backend_api::query::CRON_JOB_PAGE_SIZE as u32) {
        Some(value) => value,
        None => panic!("cron job page size must be non-zero"),
    };

/// Manage cron jobs for Wasmer Edge apps.
#[derive(clap::Subcommand, Debug)]
pub enum CmdCron {
    List(CmdCronList),
    Get(CmdCronGet),
    Invocations(CmdCronInvocations),
    Enable(CmdCronEnable),
    Disable(CmdCronDisable),
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdCron {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        match self {
            Self::List(cmd) => cmd.run_async().await,
            Self::Get(cmd) => cmd.run_async().await,
            Self::Invocations(cmd) => cmd.run_async().await,
            Self::Enable(cmd) => cmd.run_async().await,
            Self::Disable(cmd) => cmd.run_async().await,
        }
    }
}

/// List cron jobs for an app.
#[derive(clap::Parser, Debug)]
pub struct CmdCronList {
    #[clap(flatten)]
    fmt: ListFormatOpts,

    #[clap(flatten)]
    env: WasmerEnv,

    #[clap(flatten)]
    ident: AppIdentArgOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdCronList {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;
        let (_ident, app) = self.ident.to_opts().load_app(&client).await?;
        let cron_jobs = wasmer_backend_api::query::get_app_cron_jobs(
            &client,
            &app.owner.global_name,
            &app.name,
        )
        .await?;

        if cron_jobs.is_empty() {
            eprintln!("App {} has no cron jobs!", app.name);
        } else {
            println!("{}", self.fmt.format.render(cron_jobs.as_slice()));
        }

        Ok(())
    }
}

/// Get one cron job.
#[derive(clap::Parser, Debug)]
pub struct CmdCronGet {
    #[clap(flatten)]
    fmt: ItemFormatOpts,

    #[clap(flatten)]
    env: WasmerEnv,

    #[clap(flatten)]
    ident: AppIdentArgOpts,

    /// Cron job id or name.
    cron_job: String,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdCronGet {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;
        let (_ident, app) = self.ident.to_opts().load_app(&client).await?;
        let cron_job = find_app_cron_job(&client, &app, &self.cron_job).await?;

        println!("{}", self.fmt.get().render(&cron_job));
        Ok(())
    }
}

/// List invocations for one cron job.
#[derive(clap::Parser, Debug)]
pub struct CmdCronInvocations {
    #[clap(flatten)]
    fmt: ListFormatOpts,

    #[clap(flatten)]
    env: WasmerEnv,

    #[clap(flatten)]
    ident: AppIdentArgOpts,

    /// Cron job id or name.
    cron_job: String,

    /// The earliest invocation timestamp to include.
    ///
    /// Defaults to 31 days before --end, or 31 days before now.
    ///
    /// Accepts RFC 3339, RFC 2822, date, Unix timestamp, or relative time.
    #[clap(long = "start", alias = "from", value_parser = parse_timestamp_or_relative_time_negative_offset)]
    start: Option<OffsetDateTime>,

    /// The latest invocation timestamp to include.
    ///
    /// Defaults to now.
    ///
    /// Accepts RFC 3339, RFC 2822, date, Unix timestamp, or relative time.
    #[clap(long = "end", alias = "until", value_parser = parse_timestamp_or_relative_time_negative_offset)]
    end: Option<OffsetDateTime>,

    /// Number of invocations to fetch per page.
    #[clap(long, default_value_t = DEFAULT_CRON_JOB_PAGE_SIZE)]
    page_size: NonZeroU32,

    /// Fetch all invocation pages without prompting.
    #[clap(long)]
    all: bool,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdCronInvocations {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let invocation_first = invocation_page_size(self.page_size)?;

        if let (Some(start), Some(end)) = (self.start, self.end)
            && start > end
        {
            bail!("--start must be before or equal to --end");
        }

        let client = self.env.client()?;
        let (_ident, app) = self.ident.to_opts().load_app(&client).await?;
        let format = self.fmt.format;
        let interactive = io::stdin().is_terminal()
            && matches!(format, ListFormat::Table | ListFormat::ItemTable)
            && !self.all;
        let render_after_fetching =
            self.all || !matches!(format, ListFormat::Table | ListFormat::ItemTable);
        let mut invocation_after = None;
        let mut all_invocations = Vec::new();
        let mut saw_invocations = false;

        loop {
            let (_cron_job, page) = wasmer_backend_api::query::get_cron_job_invocations_page(
                &client,
                &app.owner.global_name,
                &app.name,
                &self.cron_job,
                invocation_after,
                Some(invocation_first),
                self.start,
                self.end,
            )
            .await?;

            saw_invocations |= !page.items.is_empty();
            invocation_after = page.next_cursor;

            if render_after_fetching {
                all_invocations.extend(page.items);
            } else if !page.items.is_empty() {
                println!("{}", format.render(page.items.as_slice()));
            }

            if invocation_after.is_none() {
                break;
            }

            if self.all {
                continue;
            }

            if !interactive {
                eprintln!("More invocations are available. Re-run with --all to fetch every page.");
                break;
            }

            if !prompt_next_invocation_page()? {
                break;
            }
        }

        if !saw_invocations {
            eprintln!("Cron job {} has no invocations!", self.cron_job);
        } else if render_after_fetching {
            println!("{}", format.render(all_invocations.as_slice()));
        }

        Ok(())
    }
}

fn prompt_next_invocation_page() -> Result<bool, anyhow::Error> {
    eprint!("Press Enter for the next page, or q to quit: ");
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let input = input.trim();
    if input.is_empty() {
        Ok(true)
    } else if input.eq_ignore_ascii_case("q") {
        Ok(false)
    } else {
        eprintln!("Unrecognized input; stopping.");
        Ok(false)
    }
}

/// Enable one cron job.
#[derive(clap::Parser, Debug)]
pub struct CmdCronEnable {
    #[clap(flatten)]
    env: WasmerEnv,

    #[clap(flatten)]
    ident: AppIdentArgOpts,

    /// Cron job id or name.
    cron_job: String,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdCronEnable {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        toggle_cron_job(self.env, self.ident, self.cron_job, true).await
    }
}

/// Disable one cron job.
#[derive(clap::Parser, Debug)]
pub struct CmdCronDisable {
    #[clap(flatten)]
    env: WasmerEnv,

    #[clap(flatten)]
    ident: AppIdentArgOpts,

    /// Cron job id or name.
    cron_job: String,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdCronDisable {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        toggle_cron_job(self.env, self.ident, self.cron_job, false).await
    }
}

async fn toggle_cron_job(
    env: WasmerEnv,
    ident: AppIdentArgOpts,
    cron_job: String,
    enabled: bool,
) -> Result<(), anyhow::Error> {
    let client = env.client()?;
    let (_ident, app) = ident.to_opts().load_app(&client).await?;
    let cron_job = find_app_cron_job(&client, &app, &cron_job).await?;

    let cron_job =
        wasmer_backend_api::query::toggle_cron_job(&client, cron_job.id.inner(), enabled).await?;
    let state = if cron_job.enabled {
        "enabled"
    } else {
        "disabled"
    };

    eprintln!("Cron job {} is now {}.", cron_job.name, state);
    Ok(())
}

async fn find_app_cron_job(
    client: &wasmer_backend_api::WasmerClient,
    app: &wasmer_backend_api::types::DeployApp,
    cron_job: &str,
) -> Result<wasmer_backend_api::types::CronJob, anyhow::Error> {
    wasmer_backend_api::query::get_app_cron_jobs(client, &app.owner.global_name, &app.name)
        .await?
        .into_iter()
        .find(|job| job.id.inner() == cron_job || job.name == cron_job)
        .with_context(|| format!("cron job '{cron_job}' not found"))
}

fn invocation_page_size(page_size: NonZeroU32) -> Result<i32, anyhow::Error> {
    i32::try_from(page_size.get()).context("--page-size must be less than or equal to 2147483647")
}
