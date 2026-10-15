//! Runs modules that import `wasixv2` on the experimental WASIX v2 stack.

use std::path::Path;

use anyhow::{Error, bail};
use wasixv2::{Machine, ProcessConfig, Stdio};
use wasmer_wasix::WasiError;

use super::{Run, exit_with_wasi_exit_code, wasi::utf8_env_part};
use crate::backend::RuntimeOptions;

impl Run {
    pub(super) fn execute_wasixv2(
        &self,
        path: &Path,
        wasm: &[u8],
        runtime: &tokio::runtime::Runtime,
    ) -> Result<(), Error> {
        self.reject_options_unsupported_by_wasixv2()?;

        let config = ProcessConfig {
            args: std::iter::once(path.display().to_string())
                .chain(self.args.iter().cloned())
                .collect(),
            env: self.wasixv2_env()?,
            stdio: Stdio::inherit(),
        };
        // The machine's host calls are not `Send`, so drive it on this thread.
        let result = match runtime.block_on(Machine::new().run_process(wasm, config)) {
            Ok(0) => Ok(()),
            Ok(status) => Err(WasiError::Exit(status.into()).into()),
            Err(err) => Err(err.into()),
        };
        // Exit before the tokio runtime is dropped: a guest blocked reading stdin leaves an
        // uncancellable read on the blocking pool, and dropping the runtime would wait for it.
        exit_with_wasi_exit_code(result)
    }

    /// Same sources and precedence as the v1 runner: forwarded host variables, then the env
    /// file, then `--env`.
    fn wasixv2_env(&self) -> Result<Vec<String>, Error> {
        let mut env = Vec::new();
        if self.wasi.forward_host_env {
            for (key, value) in std::env::vars_os() {
                env.push((utf8_env_part(key)?, utf8_env_part(value)?));
            }
        }
        env.extend(self.wasi.resolved_env_vars()?);
        Ok(env
            .into_iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect())
    }

    fn reject_options_unsupported_by_wasixv2(&self) -> Result<(), Error> {
        let wasi = &self.wasi;
        #[allow(unused_mut)]
        let mut given = vec![
            ("--volume", !wasi.volumes.is_empty()),
            ("--dir", !wasi.pre_opened_directories.is_empty()),
            ("--mapdir", !wasi.mapped_dirs.is_empty()),
            ("--cwd", wasi.cwd.is_some()),
            ("--use", !wasi.uses.is_empty()),
            ("--include-webc", !wasi.include_webcs.is_empty()),
            ("--map-command", !wasi.map_commands.is_empty()),
            ("--net", wasi.networking.is_some()),
            ("--http-client", wasi.http_client),
            (
                "--enable-async-threads",
                wasi.enable_async_threads.is_some(),
            ),
            ("--enable-cpu-backoff", wasi.enable_cpu_backoff.is_some()),
            ("--stack-size", self.stack_size.is_some()),
            ("--entrypoint", self.entrypoint.is_some()),
            ("--invoke", self.invoke.is_some()),
            ("--coredump-on-trap", self.coredump_on_trap.is_some()),
            ("--experimental-napi", self.experimental_napi),
            (
                "compiler and backend options",
                self.rt != RuntimeOptions::default(),
            ),
        ];
        #[cfg(feature = "journal")]
        given.extend([
            ("--journal", !wasi.read_only_journals.is_empty()),
            ("--journal-writable", !wasi.writable_journals.is_empty()),
            ("--snapshot-on", !wasi.snapshot_on.is_empty()),
            ("--snapshot-period", wasi.snapshot_interval.is_some()),
            ("--stop-after-snapshot", wasi.stop_after_snapshot),
        ]);

        let unsupported: Vec<_> = given
            .into_iter()
            .filter_map(|(name, is_given)| is_given.then_some(name))
            .collect();
        if !unsupported.is_empty() {
            bail!(
                "this module targets WASIX v2, which does not support {} yet",
                unsupported.join(", ")
            );
        }
        Ok(())
    }
}
