use anyhow::{anyhow, bail, Context, Result};
use napi_wasmer::{
    configure_runner_mounts, load_wasix_module, run_wasm_main_i32, GuestMount, NapiCtx,
};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use virtual_fs::AsyncReadExt;
use wasmer_wasix::{
    runners::wasi::{RuntimeOrEngine, WasiRunner},
    runtime::task_manager::tokio::TokioTaskManager,
    Pipe, PluggableRuntime, WasiError,
};

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .try_init();
}

const BUILTIN_JS_GUEST_PATH: &str = "/edgejs-builtins";
const BUILTIN_JS_ENV_VAR: &str = "WASMER_NAPI_BUILTIN_JS_DIR";

fn maybe_add_builtin_mounts(
    extra_mounts: &mut Vec<GuestMount>,
    explicit_builtin_dir: Option<String>,
) -> Result<()> {
    let explicit_builtin_dir =
        explicit_builtin_dir.or_else(|| std::env::var(BUILTIN_JS_ENV_VAR).ok());
    let builtin_dir = if let Some(dir) = explicit_builtin_dir {
        let path = std::fs::canonicalize(&dir)
            .with_context(|| format!("failed to resolve builtin js dir {}", dir))?;
        if !path.is_dir() {
            bail!("builtin js dir must be a directory: {}", path.display());
        }
        path
    } else {
        let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR").ok_or_else(|| {
            anyhow!(
                "builtin js dir is not configured: set --builtin-js-dir, {BUILTIN_JS_ENV_VAR}, or CARGO_MANIFEST_DIR"
            )
        })?;
        let repo_root = PathBuf::from(manifest_dir).join("../..");
        let lib = repo_root.join("lib");
        if lib.is_dir() {
            std::fs::canonicalize(&lib).ok().unwrap_or(lib)
        } else {
            let node_lib = repo_root.join("node-lib");
            if node_lib.is_dir() {
                std::fs::canonicalize(&node_lib).ok().unwrap_or(node_lib)
            } else {
                bail!(
                    "builtin js dir is not configured: neither {} nor {} exists under {}",
                    repo_root.join("lib").display(),
                    repo_root.join("node-lib").display(),
                    repo_root.display()
                );
            }
        }
    };

    if !extra_mounts
        .iter()
        .any(|m| m.guest_path == Path::new(BUILTIN_JS_GUEST_PATH))
    {
        extra_mounts.push(GuestMount {
            host_path: builtin_dir.clone(),
            guest_path: PathBuf::from(BUILTIN_JS_GUEST_PATH),
        });
    }

    if !extra_mounts
        .iter()
        .any(|m| m.guest_path == Path::new("/lib"))
    {
        extra_mounts.push(GuestMount {
            host_path: builtin_dir.clone(),
            guest_path: PathBuf::from("/lib"),
        });
    }

    if !extra_mounts
        .iter()
        .any(|m| m.guest_path == Path::new("/node-lib"))
    {
        extra_mounts.push(GuestMount {
            host_path: builtin_dir.clone(),
            guest_path: PathBuf::from("/node-lib"),
        });
    }

    if let Some(parent) = builtin_dir.parent() {
        let node_deps_dir = parent.join("node/deps");
        if node_deps_dir.is_dir()
            && !extra_mounts
                .iter()
                .any(|m| m.guest_path == Path::new("/node/deps"))
        {
            extra_mounts.push(GuestMount {
                host_path: node_deps_dir,
                guest_path: PathBuf::from("/node/deps"),
            });
        }
    }

    Ok(())
}

fn parse_mount(spec: &str) -> Result<GuestMount> {
    let (host, guest) = spec
        .split_once(':')
        .ok_or_else(|| anyhow!("invalid mount {spec:?}, expected <host-dir>:<guest-dir>"))?;
    let host_path = std::fs::canonicalize(host)
        .with_context(|| format!("failed to resolve host mount path {}", host))?;
    if !host_path.is_dir() {
        bail!("mount source must be a directory: {}", host_path.display());
    }
    let guest_path = PathBuf::from(guest);
    if !guest_path.is_absolute() {
        bail!(
            "mount target must be an absolute guest path: {}",
            guest_path.display()
        );
    }
    Ok(GuestMount {
        host_path,
        guest_path,
    })
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

fn run_wasix_example(
    napi: &NapiCtx,
    wasm_path: &Path,
    args: &[String],
    extra_mounts: &[GuestMount],
) -> Result<i32> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to create tokio runtime for WASIX")?;
    let _guard = runtime.enter();

    let loaded = load_wasix_module(wasm_path)?;
    let engine = loaded.store.engine().clone();
    let module = loaded.module;
    let module_hash = loaded.module_hash;

    let (stdout_tx, stdout_rx) = Pipe::channel();
    let (stderr_tx, stderr_rx) = Pipe::channel();
    let stdout_thread = spawn_pipe_drain_thread(stdout_rx, Box::new(std::io::stdout()));
    let stderr_thread = spawn_pipe_drain_thread(stderr_rx, Box::new(std::io::stderr()));

    let exit_code = {
        let mut runner = WasiRunner::new();
        runner
            .with_stdout(Box::new(stdout_tx))
            .with_stderr(Box::new(stderr_tx))
            .with_args(args.iter().cloned());
        configure_runner_mounts(&mut runner, wasm_path, extra_mounts)?;

        let task_manager = Arc::new(TokioTaskManager::new(tokio::runtime::Handle::current()));
        let mut runtime = PluggableRuntime::new(task_manager);
        runtime.set_engine(engine);
        napi.extend_wasi_runner(&mut runner, &mut runtime, &module);

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

    stdout_thread
        .join()
        .map_err(|_| anyhow!("stdout drain thread panicked"))??;
    stderr_thread
        .join()
        .map_err(|_| anyhow!("stderr drain thread panicked"))??;

    Ok(exit_code)
}

fn main() -> Result<()> {
    init_tracing();
    let mut args = std::env::args().skip(1);
    let wasm_path = match args.next() {
        Some(p) => p,
        None => {
            bail!(
                "usage: cargo run -p napi_wasmer -- <wasm-file> [<script.js>] [--builtin-js-dir <host-dir>] [--app-dir <host-dir>] [--mount <host-dir>:<guest-dir>] [wasix|main]"
            );
        }
    };
    let wasm_path = Path::new(&wasm_path);
    let mut entry = "wasix".to_string();
    let mut script_arg: Option<String> = None;
    let mut builtin_js_dir: Option<String> = None;
    let mut extra_mounts = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "wasix" | "main" => entry = arg,
            "--app-dir" => {
                let host_dir = args
                    .next()
                    .ok_or_else(|| anyhow!("--app-dir requires a host directory"))?;
                let host_path = std::fs::canonicalize(&host_dir)
                    .with_context(|| format!("failed to resolve app dir {}", host_dir))?;
                if !host_path.is_dir() {
                    bail!("app dir must be a directory: {}", host_path.display());
                }
                extra_mounts.push(GuestMount {
                    host_path,
                    guest_path: PathBuf::from("/app"),
                });
            }
            "--mount" => {
                let spec = args
                    .next()
                    .ok_or_else(|| anyhow!("--mount requires <host-dir>:<guest-dir>"))?;
                extra_mounts.push(parse_mount(&spec)?);
            }
            "--builtin-js-dir" => {
                let dir = args
                    .next()
                    .ok_or_else(|| anyhow!("--builtin-js-dir requires a host directory"))?;
                builtin_js_dir = Some(dir);
            }
            _ if arg.starts_with("--mount=") => {
                extra_mounts.push(parse_mount(arg.trim_start_matches("--mount="))?);
            }
            _ if arg.starts_with("--builtin-js-dir=") => {
                builtin_js_dir = Some(arg.trim_start_matches("--builtin-js-dir=").to_string());
            }
            _ if arg.starts_with("--app-dir=") => {
                let host_dir = arg.trim_start_matches("--app-dir=");
                let host_path = std::fs::canonicalize(host_dir)
                    .with_context(|| format!("failed to resolve app dir {}", host_dir))?;
                if !host_path.is_dir() {
                    bail!("app dir must be a directory: {}", host_path.display());
                }
                extra_mounts.push(GuestMount {
                    host_path,
                    guest_path: PathBuf::from("/app"),
                });
            }
            _ if script_arg.is_none() => script_arg = Some(arg),
            _ => bail!("unexpected argument: {arg}"),
        }
    }

    if entry == "wasix" {
        let napi = NapiCtx::builder().build();
        let mut guest_args = Vec::new();
        if let Some(script) = script_arg.clone() {
            let host_script = PathBuf::from(&script);
            let host_script = if host_script.is_absolute() {
                host_script
            } else {
                std::env::current_dir()
                    .context("failed to resolve current dir")?
                    .join(host_script)
            };
            let host_script = std::fs::canonicalize(&host_script)
                .with_context(|| format!("failed to resolve script {}", script))?;
            let script_parent = host_script
                .parent()
                .ok_or_else(|| anyhow!("script has no parent dir: {}", host_script.display()))?;
            if !extra_mounts
                .iter()
                .any(|m| m.guest_path == Path::new("/app"))
            {
                extra_mounts.push(GuestMount {
                    host_path: script_parent.to_path_buf(),
                    guest_path: PathBuf::from("/app"),
                });
            }
            let script_name = host_script
                .file_name()
                .ok_or_else(|| anyhow!("script has no file name: {}", host_script.display()))?;
            guest_args.push(format!("/app/{}", script_name.to_string_lossy()));
        }
        maybe_add_builtin_mounts(&mut extra_mounts, builtin_js_dir)?;
        let exit = run_wasix_example(&napi, wasm_path, &guest_args, &extra_mounts)?;
        if exit != 0 {
            bail!("non-zero exit code: {exit}");
        }
        return Ok(());
    }

    let result = run_wasm_main_i32(wasm_path)?;
    println!("main() => {result}");
    Ok(())
}
