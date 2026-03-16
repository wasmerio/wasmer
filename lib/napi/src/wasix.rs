use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use virtual_fs::{AsyncReadExt, FileSystem};
use wasmer_wasix::{
    Pipe, PluggableRuntime, WasiError,
    runners::wasi::{RuntimeOrEngine, WasiRunner},
    runtime::task_manager::tokio::TokioTaskManager,
};

use crate::{NapiCtx, load_wasix_module};

#[derive(Debug, Clone)]
pub struct GuestMount {
    pub host_path: PathBuf,
    pub guest_path: PathBuf,
}

fn spawn_pipe_drain_thread(
    mut pipe: Pipe,
    mut sink: Box<dyn Write + Send>,
) -> std::thread::JoinHandle<Result<String>> {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to create stdio drain runtime")?;
        let mut captured = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let n = runtime
                .block_on(pipe.read(&mut chunk))
                .context("failed reading WASIX stdio pipe")?;
            if n == 0 {
                break;
            }
            sink.write_all(&chunk[..n])
                .context("failed writing drained WASIX stdio")?;
            sink.flush()
                .context("failed flushing drained WASIX stdio")?;
            captured.extend_from_slice(&chunk[..n]);
        }
        String::from_utf8(captured).context("WASIX stdio was not valid UTF-8")
    })
}

pub fn configure_runner_mounts(
    runner: &mut WasiRunner,
    _wasm_path: &Path,
    extra_mounts: &[GuestMount],
) -> Result<()> {
    if extra_mounts.is_empty() {
        return Ok(());
    }

    let host_handle = tokio::runtime::Handle::current();
    for mount in extra_mounts {
        let host_fs: Arc<dyn FileSystem + Send + Sync> = Arc::new(
            virtual_fs::host_fs::FileSystem::new(host_handle.clone(), mount.host_path.clone())
                .with_context(|| {
                    format!("failed to create host fs for {}", mount.host_path.display())
                })?,
        );
        runner.with_mount(mount.guest_path.display().to_string(), host_fs);
    }

    Ok(())
}

pub fn run_wasix_main_capture_stdio(
    wasm_path: &Path,
    args: &[String],
    extra_mounts: &[GuestMount],
) -> Result<(i32, String, String)> {
    let ctx = NapiCtx::default();
    run_wasix_main_capture_stdio_with_ctx(&ctx, wasm_path, args, extra_mounts)
}

pub fn run_wasix_main_capture_stdio_with_ctx(
    ctx: &NapiCtx,
    wasm_path: &Path,
    args: &[String],
    extra_mounts: &[GuestMount],
) -> Result<(i32, String, String)> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to create tokio runtime for WASIX")?;
    let _guard = runtime.enter();

    let (stdout_tx, stdout_rx) = Pipe::channel();
    let (stderr_tx, stderr_rx) = Pipe::channel();
    let stdout_thread = spawn_pipe_drain_thread(stdout_rx, Box::new(std::io::stdout()));
    let stderr_thread = spawn_pipe_drain_thread(stderr_rx, Box::new(std::io::stderr()));
    let exit_code = {
        let loaded = load_wasix_module(wasm_path)?;
        let engine = loaded.store.engine().clone();
        let module = loaded.module;
        let module_hash = loaded.module_hash;

        let mut runner = WasiRunner::new();
        runner
            .with_stdout(Box::new(stdout_tx))
            .with_stderr(Box::new(stderr_tx))
            .with_args(args.iter().cloned());
        configure_runner_mounts(&mut runner, wasm_path, extra_mounts)?;

        let task_manager = Arc::new(TokioTaskManager::new(tokio::runtime::Handle::current()));
        let mut runtime = PluggableRuntime::new(task_manager);
        runtime.set_engine(engine.clone());
        ctx.extend_wasi_runner(&mut runner, &mut runtime, &module);

        match runner.run_wasm(
            RuntimeOrEngine::Runtime(Arc::new(runtime)),
            "guest-test",
            module,
            module_hash,
        ) {
            Ok(()) => 0,
            Err(err) => {
                if let Some(WasiError::Exit(code)) = err.downcast_ref::<WasiError>() {
                    i32::from(*code)
                } else {
                    return Err(err).context("failed to run WASIX module through WasiRunner");
                }
            }
        }
    };

    let stdout = stdout_thread
        .join()
        .map_err(|_| anyhow::anyhow!("stdout drain thread panicked"))??;
    let stderr = stderr_thread
        .join()
        .map_err(|_| anyhow::anyhow!("stderr drain thread panicked"))??;
    Ok((exit_code, stdout, stderr))
}

pub fn run_wasix_main_capture_stdout(
    wasm_path: &Path,
    args: &[String],
    extra_mounts: &[GuestMount],
) -> Result<(i32, String)> {
    let ctx = NapiCtx::default();
    run_wasix_main_capture_stdout_with_ctx(&ctx, wasm_path, args, extra_mounts)
}

pub fn run_wasix_main_capture_stdout_with_ctx(
    ctx: &NapiCtx,
    wasm_path: &Path,
    args: &[String],
    extra_mounts: &[GuestMount],
) -> Result<(i32, String)> {
    let (exit_code, stdout, _stderr) =
        run_wasix_main_capture_stdio_with_ctx(ctx, wasm_path, args, extra_mounts)?;
    Ok((exit_code, stdout))
}
