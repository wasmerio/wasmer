use std::{collections::HashMap, ffi::OsString, path::PathBuf, sync::Arc};

use anyhow::{Context, Result, bail};
use clap::Parser;
use indexmap::IndexMap;
use itertools::Itertools;
use url::Url;
use wasmer::Engine;
#[cfg(feature = "journal")]
use wasmer_wasix::journal::SnapshotTrigger;
use wasmer_wasix::{
    PluggableRuntime, Runtime,
    http::HttpClient,
    runners::{MAPPED_CURRENT_DIR_DEFAULT_PATH, MappedDirectory},
    runtime::{
        module_cache::{FileSystemCache, ModuleCache},
        package_loader::BuiltinPackageLoader,
        resolver::{
            BackendSource, FileSystemSource, InMemorySource, LocalRegistrySource, MultiSource,
            WebSource,
        },
        task_manager::tokio::{RuntimeOrHandle, TokioTaskManager},
    },
};

use crate::{
    config::WasmerEnv,
    utils::{
        GuestUser, PublishedPort, WAPM_SOURCE_CACHE_TIMEOUT, parse_envvar, parse_mapdir,
        parse_publish, parse_user, parse_volume, registry_query_cache_dir,
    },
};

#[derive(Debug, Parser, Clone, Default)]
/// WASI Options
pub struct Wasi {
    /// Map a host directory to a different location for the Wasm module
    #[clap(
        long = "volume",
        name = "[HOST_DIR:]GUEST_DIR",
        value_parser = parse_volume,
    )]
    pub(crate) volumes: Vec<MappedDirectory>,

    // Legacy option
    #[clap(long = "dir", group = "wasi", hide = true)]
    pub(crate) pre_opened_directories: Vec<PathBuf>,

    // Legacy option
    #[clap(
        long = "mapdir",
        value_parser = parse_mapdir,
        hide = true
     )]
    pub(crate) mapped_dirs: Vec<MappedDirectory>,

    /// Set the program's initial working directory to this absolute guest path.
    #[clap(long = "cwd")]
    pub(crate) cwd: Option<PathBuf>,

    /// Pass custom environment variables
    #[clap(
        long = "env",
        name = "KEY=VALUE",
        value_parser=parse_envvar,
    )]
    pub(crate) env_vars: Vec<(String, String)>,

    /// Load environment variables from a dotenv file.
    #[clap(long = "env-file", name = "PATH")]
    pub(crate) env_file: Option<PathBuf>,

    /// Forward all host env variables to guest
    #[clap(long, env)]
    pub(crate) forward_host_env: bool,

    /// A package whose commands go in /bin and /usr/bin and whose volumes are mounted, under
    /// the program's own (repeatable; earlier ones win)
    #[clap(long = "use", name = "USE")]
    pub(crate) uses: Vec<String>,

    /// Webc packages that are explicitly included for execution, taking
    /// precedence over the registry ones: either a `*.webc` file, or a
    /// directory laid out `<namespace>/<name>/<version>.webc` and queried on
    /// demand like a registry. Resolves named dependencies from local files,
    /// offline.
    #[clap(long = "include-webc", name = "WEBC")]
    pub(super) include_webcs: Vec<PathBuf>,

    /// Resolve only from local sources (`--include-webc`, filesystem paths), never
    /// the registry. Needed offline, where a registry query would fail resolution.
    #[clap(long = "offline")]
    pub(super) offline: bool,

    /// List of injected atoms
    #[clap(long = "map-command", name = "MAPCMD")]
    pub(super) map_commands: Vec<String>,

    /// Enable networking with the host network.
    ///
    /// Programs on the WASIX v2 kernel always reach the host's network for outgoing
    /// connections, so plain `--net` changes nothing; a guest listener is reached from the host
    /// through `--publish`. Network rulesets (`--net=<RULES>`) are not supported there yet.
    #[clap(long = "net", require_equals = true)]
    // Note that when --net is passed to the cli, the first Option will be initialized: Some(None)
    // and when --net=<ruleset> is specified, the inner Option will be initialized: Some(Some(ruleset))
    pub networking: Option<Option<String>>,

    /// Forward connections to a host port to a port the guest listens on, as
    /// `[HOST_IP:]HOST_PORT:GUEST_PORT`, or with `/udp` datagrams to the UDP socket the guest
    /// binds there, whose replies leave from the host port. The host address defaults to
    /// 127.0.0.1.
    #[clap(
        long = "publish",
        name = "[HOST_IP:]HOST_PORT:GUEST_PORT[/udp]",
        value_parser = parse_publish
    )]
    pub(crate) publish: Vec<PublishedPort>,

    /// Run the guest as `UID[:GID]` (numeric; the gid defaults to the uid), which also owns
    /// the guest's filesystems. uid 0 is root and holds every capability, so permission checks
    /// that would stop other users pass. The default is 1:1.
    #[clap(long = "user", name = "UID[:GID]", value_parser = parse_user)]
    pub(crate) user: Option<GuestUser>,

    /// The host name the guest sees (`uname -n`).
    #[clap(long = "hostname")]
    pub(crate) hostname: Option<String>,

    /// The NIS domain name the guest sees (`getdomainname`).
    #[clap(long = "domainname")]
    pub(crate) domainname: Option<String>,

    /// The kernel release the guest sees (`uname -r`), e.g. for programs that insist on a
    /// Linux version. The default is the WASIX v2 ABI version.
    #[clap(long = "uname-release")]
    pub(crate) uname_release: Option<String>,

    /// The kernel version the guest sees (`uname -v`).
    #[clap(long = "uname-version")]
    pub(crate) uname_version: Option<String>,

    /// The machine the guest sees (`uname -m`); the default is `wasm32`.
    #[clap(long = "uname-machine")]
    pub(crate) uname_machine: Option<String>,

    /// Boot the machine without an init: the program runs as pid 1 with no parent, and orphans
    /// are reaped as they exit. By default a built-in init is pid 1 and the program its child.
    #[clap(long = "no-init")]
    pub(crate) no_init: bool,

    /// Keeps standard input, output and error plain streams even on a terminal. By default a
    /// terminal on standard input becomes the guest's console: the terminal is in raw mode while
    /// the guest runs, and the guest's line discipline does echo, line editing and `^C`.
    #[clap(long = "no-tty")]
    pub no_tty: bool,

    /// Enables or disables asynchronous threading.
    ///
    /// If omitted, the runtime default is used.
    #[clap(
        long = "enable-async-threads",
        require_equals = true,
        default_missing_value = "true",
        num_args = 0..=1,
        action = clap::ArgAction::Set
    )]
    pub enable_async_threads: Option<bool>,

    /// Enables an exponential backoff (measured in milli-seconds) of
    /// the process CPU usage when there are no active run tokens (when set
    /// holds the maximum amount of time that it will pause the CPU)
    /// (default = off)
    #[clap(long = "enable-cpu-backoff")]
    pub enable_cpu_backoff: Option<u64>,

    /// Specifies one or more journal files that Wasmer will use to restore
    /// the state of the WASM process as it executes.
    ///
    /// The state of the WASM process and its sandbox will be reapplied using
    /// the journals in the order that you specify here.
    #[cfg(feature = "journal")]
    #[clap(long = "journal")]
    pub read_only_journals: Vec<PathBuf>,

    /// Specifies one or more journal files that Wasmer will use to restore
    /// and save the state of the WASM process as it executes.
    ///
    /// The state of the WASM process and its sandbox will be reapplied using
    /// the journals in the order that you specify here.
    ///
    /// The last journal file specified will be created if it does not exist
    /// and opened for read and write. New journal events will be written to this
    /// file
    #[cfg(feature = "journal")]
    #[clap(long = "journal-writable")]
    pub writable_journals: Vec<PathBuf>,

    /// Flag that indicates if the journal will be automatically compacted
    /// as it fills up and when the process exits
    #[cfg(feature = "journal")]
    #[clap(long = "enable-compaction")]
    pub enable_compaction: bool,

    /// Tells the compactor not to compact when the journal log file is closed
    #[cfg(feature = "journal")]
    #[clap(long = "without-compact-on-drop")]
    pub without_compact_on_drop: bool,

    /// Tells the compactor to compact when it grows by a certain factor of
    /// its original size. (i.e. '0.2' would be it compacts after the journal
    /// has grown by 20 percent)
    ///
    /// Default is to compact on growth that exceeds 15%
    #[cfg(feature = "journal")]
    #[clap(long = "with-compact-on-growth", default_value = "0.15")]
    pub with_compact_on_growth: f32,

    /// Indicates what events will cause a snapshot to be taken
    /// and written to the journal file.
    ///
    /// If not specified, the default is to snapshot when the process idles, when
    /// the process exits or periodically if an interval argument is also supplied,
    /// as well as when the process requests a snapshot explicitly.
    ///
    /// Additionally if the snapshot-on is not specified it will also take a snapshot
    /// on the first stdin, environ or socket listen - this can be used to accelerate
    /// the boot up time of WASM processes.
    #[cfg(feature = "journal")]
    #[clap(long = "snapshot-on")]
    pub snapshot_on: Vec<SnapshotTrigger>,

    /// Adds a periodic interval (measured in milli-seconds) that the runtime will automatically
    /// take snapshots of the running process and write them to the journal. When specifying
    /// this parameter it implies that `--snapshot-on interval` has also been specified.
    #[cfg(feature = "journal")]
    #[clap(long = "snapshot-period")]
    pub snapshot_interval: Option<u64>,

    /// If specified, the runtime will stop executing the WASM module after the first snapshot
    /// is taken.
    #[cfg(feature = "journal")]
    #[clap(long = "stop-after-snapshot")]
    pub stop_after_snapshot: bool,

    /// Skip writes to stdout and stderr when replying journal events to bootstrap a module.
    #[cfg(feature = "journal")]
    #[clap(long = "skip-journal-stdio")]
    pub skip_stdio_during_bootstrap: bool,

    /// Allow instances to send http requests.
    ///
    /// Access to domains is granted by default.
    #[clap(long)]
    pub http_client: bool,

    /// Require WASI modules to only import 1 version of WASI.
    #[clap(long = "deny-multiple-wasi-versions")]
    pub deny_multiple_wasi_versions: bool,

    /// Disable the cache for the compiled modules.
    ///
    /// Cache is used to speed up the loading of modules, as the
    /// generated artifacts are cached.
    #[clap(long = "disable-cache")]
    disable_cache: bool,
}

/// Environment variables are arbitrary byte strings on unix, but `Wasi` stores
/// them as `String`, so reject the ones that cannot be represented.
pub(super) fn utf8_env_part(part: OsString) -> Result<String> {
    part.into_string()
        .map_err(|part| anyhow::anyhow!("environment variable is not valid UTF-8: {part:?}"))
}

impl Wasi {
    pub(crate) fn resolved_env_vars(&self) -> Result<Vec<(String, String)>> {
        let mut env = IndexMap::new();
        if let Some(path) = &self.env_file {
            let entries = dotenvy::from_path_iter(path)
                .with_context(|| format!("Could not read env file '{}'", path.display()))?;
            for entry in entries {
                let (key, value) = entry
                    .with_context(|| format!("Could not parse env file '{}'", path.display()))?;
                env.insert(key, value);
            }
        }
        for (key, value) in &self.env_vars {
            env.insert(key.clone(), value.clone());
        }
        Ok(env.into_iter().collect())
    }

    pub(crate) fn all_volumes(&self) -> Vec<MappedDirectory> {
        self.volumes
            .iter()
            .cloned()
            .chain(self.pre_opened_directories.iter().map(|d| MappedDirectory {
                host: d.clone(),
                guest: d.to_str().expect("must be a valid path string").to_string(),
            }))
            .chain(self.mapped_dirs.iter().cloned())
            .collect_vec()
    }

    /// Every `--volume` (and legacy `--dir` and `--mapdir`) with its host directory resolved,
    /// and whether one maps the current directory, which goes to `/home`.
    pub fn build_mapped_directories(&self) -> Result<(bool, Vec<MappedDirectory>), anyhow::Error> {
        let mut mapped_dirs = Vec::new();

        // Process the --volume flag.
        let mut have_current_dir = false;
        for MappedDirectory { host, guest } in &self.all_volumes() {
            let resolved_host = host.canonicalize().with_context(|| {
                format!(
                    "could not canonicalize path for argument '--volume {}:{}'",
                    host.display(),
                    guest,
                )
            })?;

            let mapping = if guest == "." {
                if have_current_dir {
                    bail!(
                        "Cannot pre-open the current directory twice: '--volume=.' must only be specified once"
                    );
                }
                have_current_dir = true;
                MappedDirectory {
                    host: resolved_host,
                    guest: MAPPED_CURRENT_DIR_DEFAULT_PATH.to_string(),
                }
            } else {
                MappedDirectory {
                    host: resolved_host,
                    guest: guest.clone(),
                }
            };
            mapped_dirs.push(mapping);
        }

        Ok((have_current_dir, mapped_dirs))
    }

    /// The runtime `wasmer run` resolves, downloads and compiles packages with: the registry
    /// and local sources, the package cache and the compiled-module cache.
    pub fn prepare_runtime<I>(
        &self,
        engine: Engine,
        env: &WasmerEnv,
        rt_or_handle: I,
        preferred_webc_version: webc::Version,
        compiler_debug_dir_used: bool,
    ) -> Result<impl Runtime + Send + Sync + use<I>>
    where
        I: Into<RuntimeOrHandle>,
    {
        let tokio_task_manager = Arc::new(TokioTaskManager::new(rt_or_handle.into()));
        let mut rt = PluggableRuntime::new(tokio_task_manager.clone());

        let client =
            wasmer_wasix::http::default_http_client().context("No HTTP client available")?;
        let client = Arc::new(client);

        let package_loader = self
            .prepare_package_loader(env, client.clone())
            .context("Unable to prepare the package loader")?;

        let registry = self.prepare_source(env, client, preferred_webc_version)?;

        if !self.disable_cache && !compiler_debug_dir_used {
            let cache_dir = env.cache_dir().join("compiled");
            let module_cache = wasmer_wasix::runtime::module_cache::in_memory()
                .with_fallback(FileSystemCache::new(cache_dir, tokio_task_manager));
            rt.set_module_cache(module_cache);
        }

        rt.set_package_loader(package_loader)
            .set_source(registry)
            .set_engine(engine);

        Ok(rt)
    }

    fn prepare_package_loader(
        &self,
        env: &WasmerEnv,
        client: Arc<dyn HttpClient + Send + Sync>,
    ) -> Result<BuiltinPackageLoader> {
        let checkout_dir = env.cache_dir().join("checkouts");
        let tokens = tokens_by_authority(env)?;

        let loader = BuiltinPackageLoader::new()
            .with_cache_dir(checkout_dir)
            .with_shared_http_client(client)
            .with_tokens(tokens);

        Ok(loader)
    }

    fn prepare_source(
        &self,
        env: &WasmerEnv,
        client: Arc<dyn HttpClient + Send + Sync>,
        preferred_webc_version: webc::Version,
    ) -> Result<MultiSource> {
        let mut source = MultiSource::default();

        // Local packages go first so they override the registry. A directory
        // is served on demand as a local registry; a single file is loaded
        // eagerly under its manifest id.
        let mut preloaded = InMemorySource::new();
        for path in &self.include_webcs {
            if path.is_dir() {
                source.add_source(LocalRegistrySource::new(path)?);
            } else {
                preloaded
                    .add_webc(path)
                    .with_context(|| format!("Unable to load \"{}\"", path.display()))?;
            }
        }
        source.add_source(preloaded);

        // Drop the registry/web sources offline. Their network errors bubble out
        // of the merging MultiSource and abort resolution even when a local
        // source already matched.
        if !self.offline {
            let graphql_endpoint = self.graphql_endpoint(env)?;
            let cache_dir = registry_query_cache_dir(env.cache_dir(), &graphql_endpoint);
            let mut wapm_source = BackendSource::new(graphql_endpoint, Arc::clone(&client))
                .with_local_cache(cache_dir, WAPM_SOURCE_CACHE_TIMEOUT)
                .with_preferred_webc_version(preferred_webc_version);
            if let Some(token) = env
                .config()?
                .registry
                .get_login_token_for_registry(wapm_source.registry_endpoint().as_str())
            {
                wapm_source = wapm_source.with_auth_token(token);
            }
            source.add_source(wapm_source);

            let cache_dir = env.cache_dir().join("downloads");
            source.add_source(WebSource::new(cache_dir, client));
        }

        source.add_source(FileSystemSource::default());

        Ok(source)
    }

    fn graphql_endpoint(&self, env: &WasmerEnv) -> Result<Url> {
        if let Ok(endpoint) = env.registry_endpoint() {
            return Ok(endpoint);
        }

        let config = env.config()?;
        let graphql_endpoint = config.registry.get_graphql_url();
        let graphql_endpoint = graphql_endpoint
            .parse()
            .with_context(|| format!("Unable to parse \"{graphql_endpoint}\" as a URL"))?;

        Ok(graphql_endpoint)
    }
}

fn tokens_by_authority(env: &WasmerEnv) -> Result<HashMap<String, String>> {
    let mut tokens = HashMap::new();
    let config = env.config()?;

    for credentials in config.registry.tokens {
        if let Ok(url) = Url::parse(&credentials.registry)
            && url.has_authority()
        {
            tokens.insert(url.authority().to_string(), credentials.token);
        }
    }

    if let (Ok(current_registry), Some(token)) = (env.registry_endpoint(), env.token())
        && current_registry.has_authority()
    {
        tokens.insert(current_registry.authority().to_string(), token);
    }

    // Note: The global wasmer.toml config file stores URLs for the GraphQL
    // endpoint, however that's often on the backend (i.e.
    // https://registry.wasmer.io/graphql) and we also want to use the same API
    // token when sending requests to the frontend (e.g. downloading a package
    // using the `Accept: application/webc` header).
    //
    // As a workaround to avoid needing to query *all* backends to find out
    // their frontend URL every time the `wasmer` CLI runs, we'll assume that
    // when a backend is called something like `registry.wasmer.io`, the
    // frontend will be at `wasmer.io`. This works everywhere except for people
    // developing the backend locally... Sorry, Ayush.

    let mut frontend_tokens = HashMap::new();
    for (hostname, token) in &tokens {
        if let Some(frontend_url) = hostname.strip_prefix("registry.")
            && !tokens.contains_key(frontend_url)
        {
            frontend_tokens.insert(frontend_url.to_string(), token.clone());
        }
    }
    tokens.extend(frontend_tokens);

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_file_is_merged_and_explicit_env_wins() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("runtime.env");
        std::fs::write(&path, "FROM_FILE=yes\nSHARED=file\n").unwrap();
        let wasi = Wasi {
            env_file: Some(path),
            env_vars: vec![("SHARED".to_owned(), "explicit".to_owned())],
            ..Default::default()
        };

        assert_eq!(
            wasi.resolved_env_vars().unwrap(),
            [
                ("FROM_FILE".to_owned(), "yes".to_owned()),
                ("SHARED".to_owned(), "explicit".to_owned()),
            ]
        );
    }
}
