use crate::utils::{parse_envvar, parse_mapdir};
use anyhow::Result;
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{mpsc::Sender, Arc},
};
use virtual_fs::{DeviceFile, FileSystem, PassthruFileSystem, RootFileSystemBuilder};
use wasmer::{
    AsStoreMut, Function, Instance, Memory32, Memory64, Module, RuntimeError, Store, Value,
};
use wasmer_wasix::{
    default_fs_backing, get_wasi_versions,
    os::{tty_sys::SysTty, TtyBridge},
    rewind,
    runners::MappedDirectory,
    runtime::task_manager::tokio::TokioTaskManager,
    types::__WASI_STDIN_FILENO,
    wasmer_wasix_types::wasi::Errno,
    PluggableRuntime, RewindState, WasiEnv, WasiEnvBuilder, WasiError, WasiFunctionEnv,
    WasiVersion,
};

use clap::Parser;

use super::RunWithPathBuf;

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

    /// List of webc packages that are explicitely included for execution
    /// Note: these packages will be used instead of those in the registry
    #[clap(long = "include-webc", name = "WEBC")]
    include_webcs: Vec<String>,

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

    /// Enables asynchronous threading
    #[clap(long = "enable-async-threads")]
    pub enable_async_threads: bool,

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

pub struct RunProperties {
    pub ctx: WasiFunctionEnv,
    pub instance: Instance,
    pub path: PathBuf,
    pub invoke: Option<String>,
    pub args: Vec<String>,
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

        let mut rt = PluggableRuntime::new(Arc::new(TokioTaskManager::shared()));

        if self.networking {
            rt.set_networking_implementation(virtual_net::host::LocalNetworking::default());
        } else {
            rt.set_networking_implementation(virtual_net::UnsupportedVirtualNetworking::default());
        }

        if !self.no_tty {
            let tty = Arc::new(SysTty::default());
            tty.reset();
            rt.set_tty(tty)
        }

        let engine = store.as_store_mut().engine().clone();
        rt.set_engine(Some(engine));

        let builder = WasiEnv::builder(program_name)
            .runtime(Arc::new(rt))
            .args(args)
            .envs(self.env_vars.clone())
            .uses(self.uses.clone())
            .include_webcs(self.include_webcs.clone())
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

        let is_tiered_module = module.is_upgradable();
        if self.enable_async_threads || is_tiered_module {
            builder
                .capabilities_mut()
                .threading
                .enable_asynchronous_threading = true;
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

    /// Helper function for instantiating a module with Wasi imports for the `Run` command.
    pub fn instantiate(
        &self,
        store: &mut Store,
        module: &Module,
        program_name: String,
        args: Vec<String>,
    ) -> Result<(WasiFunctionEnv, Instance)> {
        let builder = self.prepare(store, module, program_name, args)?;
        let (instance, wasi_env) = builder.instantiate(module.clone(), store)?;
        Ok((wasi_env, instance))
    }

    // Runs the Wasi process
    pub fn run(run: RunProperties, store: Store) -> Result<i32> {
        let tasks = run.ctx.data(&store).tasks().clone();

        // The return value is passed synchronously and will block until the result is returned
        // this is because the main thread can go into a deep sleep and exit the dedicated thread
        let (tx, rx) = std::sync::mpsc::channel();

        // We run it in a blocking thread as the WASM function may otherwise hold
        // up the IO operations
        tasks.task_dedicated(Box::new(move || {
            Self::run_with_deep_sleep(run, store, tx, None);
        }))?;
        rx.recv()
            .expect("main thread terminated without a result, this normally means a panic occurred within the main thread")
    }

    // Runs the Wasi process (asynchronously)
    pub fn run_with_deep_sleep(
        run: RunProperties,
        mut store: Store,
        tx: Sender<Result<i32>>,
        rewind_state: Option<(RewindState, Result<(), Errno>)>,
    ) {
        // If we need to rewind then do so
        let ctx = run.ctx;
        if let Some((mut rewind_state, trigger_res)) = rewind_state {
            if rewind_state.is_64bit {
                if let Err(exit_code) = rewind_state
                    .rewinding_finish::<Memory64>(ctx.env.clone().into_mut(&mut store), trigger_res)
                {
                    tx.send(Ok(exit_code.raw())).ok();
                    return;
                }
                let res = rewind::<Memory64>(
                    ctx.env.clone().into_mut(&mut store),
                    rewind_state.memory_stack,
                    rewind_state.rewind_stack,
                    rewind_state.store_data,
                );
                if res != Errno::Success {
                    tx.send(Ok(res as i32)).ok();
                    return;
                }
            } else {
                if let Err(exit_code) = rewind_state
                    .rewinding_finish::<Memory32>(ctx.env.clone().into_mut(&mut store), trigger_res)
                {
                    tx.send(Ok(exit_code.raw())).ok();
                    return;
                }
                let res = rewind::<Memory32>(
                    ctx.env.clone().into_mut(&mut store),
                    rewind_state.memory_stack,
                    rewind_state.rewind_stack,
                    rewind_state.store_data,
                );
                if res != Errno::Success {
                    tx.send(Ok(res as i32)).ok();
                    return;
                }
            }
        }

        // Do we want to invoke a function?
        if let Some(ref invoke) = run.invoke {
            let res = RunWithPathBuf::inner_module_invoke_function(
                &mut store,
                run.instance,
                run.path.as_path(),
                invoke,
                &run.args,
            )
            .map(|()| 0);

            ctx.cleanup(&mut store, None);

            tx.send(res).unwrap();
        } else {
            let start: Function =
                RunWithPathBuf::try_find_function(&run.instance, run.path.as_path(), "_start", &[])
                    .unwrap();

            let result = start.call(&mut store, &[]);
            Self::handle_result(
                RunProperties {
                    ctx,
                    instance: run.instance,
                    path: run.path,
                    invoke: run.invoke,
                    args: run.args,
                },
                store,
                result,
                tx,
            )
        }
    }

    /// Helper function for handling the result of a Wasi _start function.
    pub fn handle_result(
        run: RunProperties,
        mut store: Store,
        result: Result<Box<[Value]>, RuntimeError>,
        tx: Sender<Result<i32>>,
    ) {
        let ctx = run.ctx;
        let ret: Result<i32> = match result {
            Ok(_) => Ok(0),
            Err(err) => {
                match err.downcast::<WasiError>() {
                    Ok(WasiError::Exit(exit_code)) => Ok(exit_code.raw()),
                    Ok(WasiError::DeepSleep(deep)) => {
                        let pid = ctx.data(&store).pid();
                        let tid = ctx.data(&store).tid();
                        tracing::trace!(%pid, %tid, "entered a deep sleep");

                        // Create the respawn function
                        let module = run.instance.module().clone();
                        let tasks = ctx.data(&store).tasks().clone();
                        let rewind = deep.rewind;
                        let respawn = {
                            let mut run = RunProperties {
                                ctx: ctx.clone(),
                                instance: run.instance,
                                path: run.path,
                                invoke: run.invoke,
                                args: run.args,
                            };
                            move |mut store, module, res| {
                                // Reinitialize and then call the thread
                                if let Err(err) = run.ctx.reinitialize(&mut store, &module) {
                                    tracing::warn!("failed to reinitialize module - {}", err);
                                }
                                Self::run_with_deep_sleep(run, store, tx, Some((rewind, res)));
                            }
                        };

                        // Spawns the WASM process after a trigger
                        tasks
                            .resume_wasm_after_poller(
                                Box::new(respawn),
                                store,
                                module,
                                ctx,
                                deep.work,
                            )
                            .unwrap();
                        return;
                    }
                    Ok(err) => Err(err.into()),
                    Err(err) => Err(err.into()),
                }
            }
        };

        ctx.cleanup(&mut store, None);

        tx.send(ret).unwrap();
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
}
