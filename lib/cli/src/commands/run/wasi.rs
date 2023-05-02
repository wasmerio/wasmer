use crate::utils::{parse_envvar, parse_mapdir};
use anyhow::{Context, Result};
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};
use virtual_fs::{DeviceFile, FileSystem, PassthruFileSystem, RootFileSystemBuilder};
use wasmer::{AsStoreMut, Instance, Module, RuntimeError, Value};
use wasmer_registry::WasmerConfig;
use wasmer_wasix::{
    bin_factory::BinaryPackage,
    default_fs_backing, get_wasi_versions,
    os::{tty_sys::SysTty, TtyBridge},
    runners::MappedDirectory,
    runtime::{
        resolver::{PackageResolver, RegistryResolver},
        task_manager::tokio::TokioTaskManager,
    },
    types::__WASI_STDIN_FILENO,
    PluggableRuntime, WasiEnv, WasiEnvBuilder, WasiError, WasiFunctionEnv, WasiVersion,
};

use clap::Parser;

#[derive(Debug, Parser, Clone, Default)]
/// WASI Options
pub struct Wasi {
    /// WASI pre-opened directory
    #[clap(long = "dir", name = "DIR", group = "wasi")]
    pub(crate) pre_opened_directories: Vec<PathBuf>,

    /// Map a host directory to a different location for the Wasm module
    #[clap(
        long = "mapdir",
        name = "GUEST_DIR:HOST_DIR",
        parse(try_from_str = parse_mapdir),
    )]
    pub(crate) mapped_dirs: Vec<MappedDirectory>,

    /// Pass custom environment variables
    #[clap(
        long = "env",
        name = "KEY=VALUE",
        parse(try_from_str = parse_envvar),
    )]
    pub(crate) env_vars: Vec<(String, String)>,

    /// Forward all host env variables to the wcgi task.
    #[clap(long, env)]
    pub(crate) forward_host_env: bool,

    /// List of other containers this module depends on
    #[clap(long = "use", name = "USE")]
    uses: Vec<String>,

    /// List of webc packages that are explicitly included for execution
    /// Note: these packages will be used instead of those in the registry
    #[clap(long = "include-webc", name = "WEBC")]
    include_webcs: Vec<PathBuf>,

    /// List of injected atoms
    #[clap(long = "map-command", name = "MAPCMD")]
    map_commands: Vec<String>,

    /// Enable experimental IO devices
    #[cfg(feature = "experimental-io-devices")]
    #[cfg_attr(
        feature = "experimental-io-devices",
        clap(long = "enable-experimental-io-devices")
    )]
    enable_experimental_io_devices: bool,

    /// Enable networking with the host network.
    ///
    /// Allows WASI modules to open TCP and UDP connections, create sockets, ...
    #[clap(long = "net")]
    pub networking: bool,

    /// Disables the TTY bridge
    #[clap(long = "no-tty")]
    pub no_tty: bool,

    /// Allow instances to send http requests.
    ///
    /// Access to domains is granted by default.
    #[clap(long)]
    pub http_client: bool,

    /// Allow WASI modules to import multiple versions of WASI without a warning.
    #[clap(long = "allow-multiple-wasi-versions")]
    pub allow_multiple_wasi_versions: bool,

    /// Require WASI modules to only import 1 version of WASI.
    #[clap(long = "deny-multiple-wasi-versions")]
    pub deny_multiple_wasi_versions: bool,
}

#[allow(dead_code)]
impl Wasi {
    pub fn map_dir(&mut self, alias: &str, target_on_disk: PathBuf) {
        self.mapped_dirs.push(MappedDirectory {
            guest: alias.to_string(),
            host: target_on_disk,
        });
    }

    pub fn set_env(&mut self, key: &str, value: &str) {
        self.env_vars.push((key.to_string(), value.to_string()));
    }

    /// Gets the WASI version (if any) for the provided module
    pub fn get_versions(module: &Module) -> Option<BTreeSet<WasiVersion>> {
        // Get the wasi version in non-strict mode, so multiple wasi versions
        // are potentially allowed.
        //
        // Checking for multiple wasi versions is handled outside this function.
        get_wasi_versions(module, false)
    }

    /// Checks if a given module has any WASI imports at all.
    pub fn has_wasi_imports(module: &Module) -> bool {
        // Get the wasi version in non-strict mode, so no other imports
        // are allowed
        get_wasi_versions(module, false).is_some()
    }

    pub fn prepare(
        &self,
        store: &mut impl AsStoreMut,
        module: &Module,
        program_name: String,
        args: Vec<String>,
    ) -> Result<WasiEnvBuilder> {
        let args = args.into_iter().map(|arg| arg.into_bytes());

        let map_commands = self
            .map_commands
            .iter()
            .map(|map| map.split_once('=').unwrap())
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect::<HashMap<_, _>>();

        let rt = self
            .prepare_runtime(store)
            .context("Unable to prepare the wasi runtime")?;

        let builder = WasiEnv::builder(program_name)
            .runtime(Arc::new(rt))
            .args(args)
            .envs(self.env_vars.clone())
            .uses(self.uses.clone())
            .map_commands(map_commands);

        let mut builder = if wasmer_wasix::is_wasix_module(module) {
            // If we preopen anything from the host then shallow copy it over
            let root_fs = RootFileSystemBuilder::new()
                .with_tty(Box::new(DeviceFile::new(__WASI_STDIN_FILENO)))
                .build();
            if !self.mapped_dirs.is_empty() {
                let fs_backing: Arc<dyn FileSystem + Send + Sync> =
                    Arc::new(PassthruFileSystem::new(default_fs_backing()));
                for MappedDirectory { host, guest } in self.mapped_dirs.clone() {
                    let host = if !host.is_absolute() {
                        Path::new("/").join(host)
                    } else {
                        host
                    };
                    root_fs.mount(guest.into(), &fs_backing, host)?;
                }
            }

            // Open the root of the new filesystem
            builder
                .sandbox_fs(root_fs)
                .preopen_dir(Path::new("/"))
                .unwrap()
                .map_dir(".", "/")?
        } else {
            builder
                .fs(default_fs_backing())
                .preopen_dirs(self.pre_opened_directories.clone())?
                .map_dirs(
                    self.mapped_dirs
                        .iter()
                        .map(|d| (d.guest.clone(), d.host.clone())),
                )?
        };

        if self.http_client {
            let caps = wasmer_wasix::http::HttpClientCapabilityV1::new_allow_all();
            builder.capabilities_mut().http_client = caps;
        }

        #[cfg(feature = "experimental-io-devices")]
        {
            if self.enable_experimental_io_devices {
                wasi_state_builder
                    .setup_fs(Box::new(wasmer_wasi_experimental_io_devices::initialize));
            }
        }

        Ok(builder)
    }

    fn prepare_runtime(&self, store: &mut impl AsStoreMut) -> Result<PluggableRuntime> {
        let mut rt = PluggableRuntime::new(Arc::new(TokioTaskManager::shared()));

        if self.networking {
            rt.set_networking_implementation(virtual_net::host::LocalNetworking::default());
        } else {
            rt.set_networking_implementation(virtual_net::UnsupportedVirtualNetworking::default());
        }

        if !self.no_tty {
            let tty = Arc::new(SysTty::default());
            tty.reset();
            rt.set_tty(tty);
        }

        let engine = store.as_store_mut().engine().clone();
        rt.set_engine(Some(engine));

        let resolver = self
            .prepare_resolver()
            .context("Unable to prepare the package resolver")?;
        rt.set_resolver(resolver);

        Ok(rt)
    }

    /// Helper function for instantiating a module with Wasi imports for the `Run` command.
    pub fn instantiate(
        &self,
        store: &mut impl AsStoreMut,
        module: &Module,
        program_name: String,
        args: Vec<String>,
    ) -> Result<(WasiFunctionEnv, Instance)> {
        let builder = self.prepare(store, module, program_name, args)?;
        let (instance, wasi_env) = builder.instantiate(module.clone(), store)?;
        Ok((wasi_env, instance))
    }

    /// Helper function for handling the result of a Wasi _start function.
    pub fn handle_result(&self, result: Result<Box<[Value]>, RuntimeError>) -> Result<i32> {
        match result {
            Ok(_) => Ok(0),
            Err(err) => {
                let err: anyhow::Error = match err.downcast::<WasiError>() {
                    Ok(WasiError::Exit(exit_code)) => {
                        return Ok(exit_code.raw());
                    }
                    Ok(err) => err.into(),
                    Err(err) => err.into(),
                };
                Err(err)
            }
        }
    }

    pub fn for_binfmt_interpreter() -> Result<Self> {
        use std::env;
        let dir = env::var_os("WASMER_BINFMT_MISC_PREOPEN")
            .map(Into::into)
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(Self {
            deny_multiple_wasi_versions: true,
            env_vars: env::vars().collect(),
            pre_opened_directories: vec![dir],
            ..Self::default()
        })
    }

    fn prepare_resolver(&self) -> Result<impl PackageResolver> {
        let mut resolver = wapm_resolver()?;

        for path in &self.include_webcs {
            let pkg = preload_webc(path)
                .with_context(|| format!("Unable to load \"{}\"", path.display()))?;
            resolver.add_preload(pkg);
        }

        Ok(resolver.with_cache())
    }
}

fn wapm_resolver() -> Result<RegistryResolver, anyhow::Error> {
    let wasmer_home = WasmerConfig::get_wasmer_dir().map_err(anyhow::Error::msg)?;
    let cache_dir = wasmer_registry::get_webc_dir(&wasmer_home);
    let config =
        wasmer_registry::WasmerConfig::from_file(&wasmer_home).map_err(anyhow::Error::msg)?;
    let registry = config.registry.get_graphql_url();
    let registry = registry
        .parse()
        .with_context(|| format!("Unable to parse \"{registry}\" as a URL"))?;
    let wapm = RegistryResolver::new(cache_dir, registry);
    Ok(wapm)
}

fn preload_webc(path: &Path) -> Result<BinaryPackage> {
    let bytes = std::fs::read(path)?;
    let webc = wasmer_wasix::wapm::parse_static_webc(bytes)?;
    Ok(webc)
}
