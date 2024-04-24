//! WebC container support for running WASI modules

use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Error};
use tracing::Instrument;
use virtual_fs::{ArcBoxFile, FileSystem, TmpFileSystem, VirtualFile};
use wasmer::{Extern, Module};
use webc::metadata::{annotations::Wasi, Command};

use crate::{
    bin_factory::BinaryPackage,
    capabilities::Capabilities,
    journal::{DynJournal, SnapshotTrigger},
    runners::{wasi_common::CommonWasiOptions, MappedDirectory, MountedDirectory},
    runtime::{module_cache::ModuleHash, task_manager::VirtualTaskManagerExt},
    Runtime, WasiEnvBuilder, WasiError, WasiRuntimeError,
};

use super::wasi_common::MappedCommand;

#[derive(Debug, Default, Clone)]
pub struct WasiRunner {
    wasi: CommonWasiOptions,
    stdin: Option<ArcBoxFile>,
    stdout: Option<ArcBoxFile>,
    stderr: Option<ArcBoxFile>,
}

impl WasiRunner {
    /// Constructs a new `WasiRunner`.
    pub fn new() -> Self {
        WasiRunner::default()
    }

    /// Returns the current arguments for this `WasiRunner`
    pub fn get_args(&self) -> Vec<String> {
        self.wasi.args.clone()
    }

    /// Builder method to provide CLI args to the runner
    pub fn with_args<A, S>(&mut self, args: A) -> &mut Self
    where
        A: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.wasi.args = args.into_iter().map(|s| s.into()).collect();
        self
    }

    /// Builder method to provide environment variables to the runner.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.wasi.env.insert(key.into(), value.into());
        self
    }

    pub fn with_envs<I, K, V>(&mut self, envs: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (key, value) in envs {
            self.wasi.env.insert(key.into(), value.into());
        }
        self
    }

    pub fn with_forward_host_env(&mut self, forward: bool) -> &mut Self {
        self.wasi.forward_host_env = forward;
        self
    }

    pub fn with_mapped_directories<I, D>(&mut self, dirs: I) -> &mut Self
    where
        I: IntoIterator<Item = D>,
        D: Into<MappedDirectory>,
    {
        self.with_mounted_directories(dirs.into_iter().map(Into::into).map(MountedDirectory::from))
    }

    pub fn with_mounted_directories<I, D>(&mut self, dirs: I) -> &mut Self
    where
        I: IntoIterator<Item = D>,
        D: Into<MountedDirectory>,
    {
        self.wasi.mounts.extend(dirs.into_iter().map(Into::into));
        self
    }

    /// Mount a [`FileSystem`] instance at a particular location.
    pub fn with_mount(&mut self, dest: String, fs: Arc<dyn FileSystem + Send + Sync>) -> &mut Self {
        self.wasi.mounts.push(MountedDirectory { guest: dest, fs });
        self
    }

    /// Override the directory the WASIX instance will start in.
    pub fn with_current_dir(&mut self, dir: impl Into<PathBuf>) -> &mut Self {
        self.wasi.current_dir = Some(dir.into());
        self
    }

    /// Add a package that should be available to the instance at runtime.
    pub fn with_injected_package(&mut self, pkg: BinaryPackage) -> &mut Self {
        self.wasi.injected_packages.push(pkg);
        self
    }

    /// Add packages that should be available to the instance at runtime.
    pub fn with_injected_packages(
        &mut self,
        packages: impl IntoIterator<Item = BinaryPackage>,
    ) -> &mut Self {
        self.wasi.injected_packages.extend(packages);
        self
    }

    pub fn with_mapped_host_command(
        &mut self,
        alias: impl Into<String>,
        target: impl Into<String>,
    ) -> &mut Self {
        self.wasi.mapped_host_commands.push(MappedCommand {
            alias: alias.into(),
            target: target.into(),
        });
        self
    }

    pub fn with_mapped_host_commands(
        &mut self,
        commands: impl IntoIterator<Item = MappedCommand>,
    ) -> &mut Self {
        self.wasi.mapped_host_commands.extend(commands);
        self
    }

    pub fn capabilities_mut(&mut self) -> &mut Capabilities {
        &mut self.wasi.capabilities
    }

    pub fn with_capabilities(&mut self, capabilities: Capabilities) -> &mut Self {
        self.wasi.capabilities = capabilities;
        self
    }

    pub fn with_snapshot_trigger(&mut self, on: SnapshotTrigger) -> &mut Self {
        self.wasi.snapshot_on.push(on);
        self
    }

    pub fn with_default_snapshot_triggers(&mut self) -> &mut Self {
        for on in crate::journal::DEFAULT_SNAPSHOT_TRIGGERS {
            if !self.has_snapshot_trigger(on) {
                self.with_snapshot_trigger(on);
            }
        }
        self
    }

    pub fn has_snapshot_trigger(&self, on: SnapshotTrigger) -> bool {
        self.wasi.snapshot_on.iter().any(|t| *t == on)
    }

    pub fn with_snapshot_interval(&mut self, period: std::time::Duration) -> &mut Self {
        if !self.has_snapshot_trigger(SnapshotTrigger::PeriodicInterval) {
            self.with_snapshot_trigger(SnapshotTrigger::PeriodicInterval);
        }
        self.wasi.snapshot_interval.replace(period);
        self
    }

    pub fn with_journal(&mut self, journal: Arc<DynJournal>) -> &mut Self {
        self.wasi.journals.push(journal);
        self
    }

    pub fn with_stdin(&mut self, stdin: Box<dyn VirtualFile + Send + Sync>) -> &mut Self {
        self.stdin = Some(ArcBoxFile::new(stdin));
        self
    }

    pub fn with_stdout(&mut self, stdout: Box<dyn VirtualFile + Send + Sync>) -> &mut Self {
        self.stdout = Some(ArcBoxFile::new(stdout));
        self
    }

    pub fn with_stderr(&mut self, stderr: Box<dyn VirtualFile + Send + Sync>) -> &mut Self {
        self.stderr = Some(ArcBoxFile::new(stderr));
        self
    }

    /// Add an item to the list of importable items provided to the instance.
    pub fn with_import(
        &mut self,
        namespace: impl Into<String>,
        name: impl Into<String>,
        value: impl Into<Extern>,
    ) -> &mut Self {
        self.with_imports([((namespace, name), value)])
    }

    /// Add multiple import functions.
    ///
    /// This method will accept a [`&Imports`][wasmer::Imports] object.
    pub fn with_imports<I, S1, S2, E>(&mut self, imports: I) -> &mut Self
    where
        I: IntoIterator<Item = ((S1, S2), E)>,
        S1: Into<String>,
        S2: Into<String>,
        E: Into<Extern>,
    {
        let imports = imports
            .into_iter()
            .map(|((ns, n), e)| ((ns.into(), n.into()), e.into()));
        self.wasi.additional_imports.extend(imports);
        self
    }

    #[tracing::instrument(level = "debug", skip_all)]
    pub fn prepare_webc_env(
        &self,
        program_name: &str,
        wasi: &Wasi,
        pkg: Option<&BinaryPackage>,
        runtime: Arc<dyn Runtime + Send + Sync>,
        root_fs: Option<TmpFileSystem>,
    ) -> Result<WasiEnvBuilder, anyhow::Error> {
        let mut builder = WasiEnvBuilder::new(program_name).runtime(runtime);

        let container_fs = if let Some(pkg) = pkg {
            builder.add_webc(pkg.clone());
            builder.set_module_hash(pkg.hash());
            Some(Arc::clone(&pkg.webc_fs))
        } else {
            None
        };

        self.wasi
            .prepare_webc_env(&mut builder, container_fs, wasi, root_fs)?;

        if let Some(stdin) = &self.stdin {
            builder.set_stdin(Box::new(stdin.clone()));
        }
        if let Some(stdout) = &self.stdout {
            builder.set_stdout(Box::new(stdout.clone()));
        }
        if let Some(stderr) = &self.stderr {
            builder.set_stderr(Box::new(stderr.clone()));
        }

        Ok(builder)
    }

    pub fn run_wasm(
        &self,
        runtime: Arc<dyn Runtime + Send + Sync>,
        program_name: &str,
        module: &Module,
        module_hash: ModuleHash,
        asyncify: bool,
    ) -> Result<(), Error> {
        let wasi = webc::metadata::annotations::Wasi::new(program_name);
        let mut store = runtime.new_store();

        let mut builder = self.prepare_webc_env(program_name, &wasi, None, runtime, None)?;

        #[cfg(feature = "ctrlc")]
        {
            builder = builder.attach_ctrl_c();
        }

        #[cfg(feature = "journal")]
        {
            for trigger in self.wasi.snapshot_on.iter().cloned() {
                builder.add_snapshot_trigger(trigger);
            }
            if self.wasi.snapshot_on.is_empty() && !self.wasi.journals.is_empty() {
                for on in crate::journal::DEFAULT_SNAPSHOT_TRIGGERS {
                    builder.add_snapshot_trigger(on);
                }
            }
            if let Some(period) = self.wasi.snapshot_interval {
                if self.wasi.journals.is_empty() {
                    return Err(anyhow::format_err!(
                            "If you specify a snapshot interval then you must also specify a journal file"
                        ));
                }
                builder.with_snapshot_interval(period);
            }
        }

        if asyncify {
            builder.run_with_store_async(module.clone(), module_hash, store)?;
        } else {
            builder.run_with_store_ext(module.clone(), module_hash, &mut store)?;
        }

        Ok(())
    }
}

impl crate::runners::Runner for WasiRunner {
    fn can_run_command(command: &Command) -> Result<bool, Error> {
        Ok(command
            .runner
            .starts_with(webc::metadata::annotations::WASI_RUNNER_URI))
    }

    #[tracing::instrument(skip_all)]
    fn run_command(
        &mut self,
        command_name: &str,
        pkg: &BinaryPackage,
        runtime: Arc<dyn Runtime + Send + Sync>,
    ) -> Result<(), Error> {
        let cmd = pkg
            .get_command(command_name)
            .with_context(|| format!("The package doesn't contain a \"{command_name}\" command"))?;
        let wasi = cmd
            .metadata()
            .annotation("wasi")?
            .unwrap_or_else(|| Wasi::new(command_name));

        #[allow(unused_mut)]
        let mut env = self
            .prepare_webc_env(command_name, &wasi, Some(pkg), Arc::clone(&runtime), None)
            .context("Unable to prepare the WASI environment")?;

        #[cfg(feature = "journal")]
        {
            for journal in self.wasi.journals.clone() {
                env.add_journal(journal);
            }

            for snapshot_trigger in self.wasi.snapshot_on.iter().cloned() {
                env.add_snapshot_trigger(snapshot_trigger);
            }
        }

        let env = env.build()?;
        let store = runtime.new_store();

        let command_name = command_name.to_string();
        let tasks = runtime.task_manager().clone();
        let pkg = pkg.clone();

        let exit_code = tasks.spawn_and_block_on(
            async move {
                let mut task_handle =
                    crate::bin_factory::spawn_exec(pkg, &command_name, store, env, &runtime)
                        .await
                        .context("Spawn failed")?;

                #[cfg(feature = "ctrlc")]
                task_handle.install_ctrlc_handler();

                task_handle
                    .wait_finished()
                    .await
                    .map_err(|err| {
                        // We do our best to recover the error
                        let msg = err.to_string();
                        let weak = Arc::downgrade(&err);
                        Arc::into_inner(err).unwrap_or_else(|| {
                            weak.upgrade()
                                .map(|err| match err.as_ref() {
                                    WasiRuntimeError::Init(a) => WasiRuntimeError::Init(a.clone()),
                                    WasiRuntimeError::Export(a) => {
                                        WasiRuntimeError::Export(a.clone())
                                    }
                                    WasiRuntimeError::Instantiation(a) => {
                                        WasiRuntimeError::Instantiation(a.clone())
                                    }
                                    WasiRuntimeError::Wasi(WasiError::Exit(a)) => {
                                        WasiRuntimeError::Wasi(WasiError::Exit(*a))
                                    }
                                    WasiRuntimeError::Wasi(WasiError::UnknownWasiVersion) => {
                                        WasiRuntimeError::Wasi(WasiError::UnknownWasiVersion)
                                    }
                                    WasiRuntimeError::Wasi(WasiError::DeepSleep(_)) => {
                                        WasiRuntimeError::Anyhow(Arc::new(anyhow::format_err!(
                                            "deep-sleep"
                                        )))
                                    }
                                    WasiRuntimeError::ControlPlane(a) => {
                                        WasiRuntimeError::ControlPlane(a.clone())
                                    }
                                    WasiRuntimeError::Runtime(a) => {
                                        WasiRuntimeError::Runtime(a.clone())
                                    }
                                    WasiRuntimeError::Thread(a) => {
                                        WasiRuntimeError::Thread(a.clone())
                                    }
                                    WasiRuntimeError::Anyhow(a) => {
                                        WasiRuntimeError::Anyhow(a.clone())
                                    }
                                })
                                .unwrap_or_else(|| {
                                    WasiRuntimeError::Anyhow(Arc::new(anyhow::format_err!(
                                        "{}", msg
                                    )))
                                })
                        })
                    })
                    .context("Unable to wait for the process to exit")
            }
            .in_current_span(),
        )??;

        if exit_code.raw() == 0 {
            Ok(())
        } else {
            Err(WasiRuntimeError::Wasi(crate::WasiError::Exit(exit_code)).into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_and_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<WasiRunner>();
        assert_sync::<WasiRunner>();
    }
}
