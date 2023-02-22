use std::{convert::Infallible, net::SocketAddr, path::PathBuf};

use anyhow::{Context, Error};
use clap::Parser;
use tracing_subscriber::fmt::format::FmtSpan;
use wcgi_runner::Runner;

fn main() -> Result<(), Error> {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "wcgi_runner=trace,info");
    }
    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .init();

    let Args {
        address,
        input,
        env_all,
        mapped_dirs,
    } = Args::parse();

    // Hack to initialize the global shared tokio task manager handle.
    // Prevents "cannot drop runtime in async context" errors, because
    // the default will be initialized to the current tokio context.
    let rt = wasmer_wasi::runtime::task_manager::tokio::TokioTaskManager::default();

    let mut builder = Runner::builder()
        .map_dirs(mapped_dirs)
        .tokio_handle(rt.runtime_handle());

    if env_all {
        builder = builder.forward_host_env();
    }

    let runner = builder
        .watch(input)
        .context("Unable to create the runner")?;

    let make_service = hyper::service::make_service_fn(move |_| {
        let runner = runner.clone();
        async move { Ok::<_, Infallible>(runner) }
    });

    tracing::info!(%address, "Started the server");
    rt.runtime_handle()
        .block_on(async { hyper::Server::bind(&address).serve(make_service).await })
        .context("Unable to start the server")?;

    Ok(())
}

#[derive(Debug, Clone, Parser)]
#[clap(about, version, author)]
struct Args {
    /// Server address.
    #[clap(long, short, env, default_value_t = ([127, 0, 0, 1], 8000).into())]
    address: SocketAddr,

    /// Map a host directory to a different location for the Wasm module
    ///
    /// Example:
    ///
    /// --map-dir /www:./my-website
    ///   => will make the ./my-website directory available for wazsm at /www
    #[clap(
        long = "mapdir",
        name = "GUEST_DIR:HOST_DIR",
        value_parser = parse_mapdir,
    )]
    mapped_dirs: Vec<(String, PathBuf)>,

    /// Forward all host env variables to the wcgi task.
    #[clap(long)]
    env_all: bool,

    /// A WCGI program.
    input: PathBuf,
}

/// Parses a mapdir from a string
// NOTE: copied from wasmerio/wasmer lib/cli/src/utils.rs.
pub fn parse_mapdir(entry: &str) -> Result<(String, PathBuf), anyhow::Error> {
    fn retrieve_alias_pathbuf(
        alias: &str,
        real_dir: &str,
    ) -> Result<(String, PathBuf), anyhow::Error> {
        let pb = PathBuf::from(&real_dir);
        if let Ok(pb_metadata) = pb.metadata() {
            if !pb_metadata.is_dir() {
                anyhow::bail!("\"{real_dir}\" exists, but it is not a directory");
            }
        } else {
            anyhow::bail!("Directory \"{real_dir}\" does not exist");
        }
        Ok((alias.to_string(), pb))
    }

    // We try first splitting by `::`
    if let Some((alias, real_dir)) = entry.split_once("::") {
        retrieve_alias_pathbuf(alias, real_dir)
    }
    // And then we try splitting by `:` (for compatibility with previous API)
    else if let Some((alias, real_dir)) = entry.split_once(':') {
        retrieve_alias_pathbuf(alias, real_dir)
    } else {
        anyhow::bail!(
            "Directory mappings must consist of two paths separate by a `::` or `:`. Found {entry}",
        )
    }
}
