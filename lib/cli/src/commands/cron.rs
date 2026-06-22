//! Cron job commands.

use std::io::{self, IsTerminal, Write};

use anyhow::{Context, bail};
use time::OffsetDateTime;

use crate::{
    commands::{AsyncCliCommand, app::AppIdentArgOpts},
    config::WasmerEnv,
    opts::{ItemFormatOpts, ListFormatOpts},
    utils::{render::ListFormat, timestamp::parse_timestamp_or_relative_time_negative_offset},
};

/// Manage cron jobs for Wasmer Edge apps.
#[derive(clap::Subcommand, Debug)]
pub enum CmdCron {
    List(CmdCronList),
    Show(CmdCronShow),
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
            Self::Show(cmd) => cmd.run_async().await,
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

/// Show one cron job.
#[derive(clap::Parser, Debug)]
pub struct CmdCronShow {
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
impl AsyncCliCommand for CmdCronShow {
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
        let cron_job = cron_jobs
            .into_iter()
            .find(|job| job.id.inner() == self.cron_job || job.name == self.cron_job)
            .with_context(|| format!("cron job '{}' not found", self.cron_job))?;

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
    /// Accepts RFC 3339, RFC 2822, date, Unix timestamp, or relative time.
    #[clap(long = "start", alias = "from", value_parser = parse_timestamp_or_relative_time_negative_offset)]
    start: Option<OffsetDateTime>,

    /// The latest invocation timestamp to include.
    ///
    /// Accepts RFC 3339, RFC 2822, date, Unix timestamp, or relative time.
    #[clap(long = "end", alias = "until", value_parser = parse_timestamp_or_relative_time_negative_offset)]
    end: Option<OffsetDateTime>,

    /// Number of invocations to fetch per page.
    #[clap(long, default_value = "100")]
    page_size: i32,

    /// Fetch all invocation pages without prompting.
    #[clap(long)]
    all: bool,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdCronInvocations {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        if self.page_size <= 0 {
            bail!("--page-size must be greater than 0");
        }
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
                Some(self.page_size),
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
        let _ = (self.env, self.ident, self.cron_job);
        bail!("enabling cron jobs is not supported by the current backend API yet")
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
        let _ = (self.env, self.ident, self.cron_job);
        bail!("disabling cron jobs is not supported by the current backend API yet")
    }
}
