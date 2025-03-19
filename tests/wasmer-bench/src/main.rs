use clap::Parser;
use std::process::Command;
use std::{path::PathBuf, sync::Arc};
use wasmer::{Engine, Module};
use wasmer_wasix::{
    runtime::task_manager::tokio::TokioTaskManager, PluggableRuntime, WasiEnvBuilder,
};

#[derive(clap::ValueEnum, Debug, Clone, serde::Serialize, serde::Deserialize)]
enum BenchEngine {
    /// Use LLVM.
    LLVM,
    /// Use Singlepass.
    Singlepass,
    /// Use Cranelift.
    Cranelift,
}

#[derive(clap::ValueEnum, Debug, Clone, Default, derive_more::Display)]
enum Fmt {
    /// Output "normal" human-readable format.
    #[default]
    Normal,

    /// Output in Json.
    Json,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BenchResult {
    engine: BenchEngine,
    times: Vec<u64>,
    mean: u64,
    module: String,
    args: Vec<String>,
}

/// Simple benchmarking tool for Wasm modules.
#[derive(clap::Parser, Debug)]
struct App {
    /// Select the engine to run on.
    #[clap(long, short)]
    pub engine: BenchEngine,

    /// Specify a number of warm-up runs.
    #[clap(long, short, default_value_t = 20)]
    pub warmup: usize,

    /// Specify a number of times to execute the module.
    #[clap(long, short, default_value_t = 20)]
    pub runs: usize,

    /// Select the format of the output.
    #[clap(long, default_value = "normal")]
    pub fmt: Fmt,

    /// Specify an output path to serialize the execution times to.
    #[clap(long)]
    pub out_path: Option<PathBuf>,

    /// The path to the Wasm module.
    pub module: PathBuf,

    /// Arguments to pass to the Wasm module.
    #[clap(long)]
    pub args: Vec<String>,

    /// The path to the native executable to confront the Wasm module to.
    pub native: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = App::parse();

    let engine: Engine = match app.engine {
        BenchEngine::LLVM => wasmer::sys::LLVM::default().into(),
        BenchEngine::Singlepass => wasmer::sys::Singlepass::default().into(),
        BenchEngine::Cranelift => wasmer::sys::Cranelift::default().into(),
    };

    let module = Module::from_file(&engine, &app.module)?;

    let program_name = app.module.as_path().display().to_string();

    let handle = tokio::runtime::Handle::current();
    let tasks = TokioTaskManager::new(handle.clone());
    let tasks = Arc::new(tasks);
    let module_cache = wasmer_wasix::runtime::module_cache::in_memory();
    let mut rt = PluggableRuntime::new(Arc::clone(&tasks) as Arc<_>);
    rt.set_module_cache(module_cache);
    let rt = Arc::new(rt);

    // The first run is bonus -- we just want to get the module compiled.
    let runner = WasiEnvBuilder::new(&program_name)
        .runtime(rt.clone())
        .stdin(Box::<virtual_fs::NullFile>::default())
        .stdout(Box::<virtual_fs::NullFile>::default())
        .stderr(Box::<virtual_fs::NullFile>::default())
        .args(&app.args);

    runner.run(module.clone()).unwrap();

    for _ in 0..app.warmup {
        let runner = WasiEnvBuilder::new(&program_name)
            .runtime(rt.clone())
            .args(&app.args)
            .stdin(Box::<virtual_fs::NullFile>::default())
            .stdout(Box::<virtual_fs::NullFile>::default())
            .stderr(Box::<virtual_fs::NullFile>::default());

        runner.run(module.clone()).unwrap();
    }

    let mut times = vec![];

    for _ in 0..app.runs {
        let runner = WasiEnvBuilder::new(&program_name)
            .runtime(rt.clone())
            .args(&app.args)
            .stdin(Box::<virtual_fs::NullFile>::default())
            .stdout(Box::<virtual_fs::NullFile>::default())
            .stderr(Box::<virtual_fs::NullFile>::default());

        let start = std::time::Instant::now();
        runner.run(module.clone()).unwrap();
        times.push((std::time::Instant::now() - start).as_nanos() as u64);
    }

    let wasm_mean = {
        let total: u64 = times.iter().sum();
        total / (times.len() as u64)
    };

    times.clear();

    if let Some(ref native) = app.native {
        for _ in 0..app.warmup {
            let start = std::time::Instant::now();
            Command::new(native).args(&app.args).output()?;
            times.push((std::time::Instant::now() - start).as_nanos() as u64);
        }

        for _ in 0..app.runs {
            let start = std::time::Instant::now();
            Command::new(native).args(&app.args).output()?;
            times.push((std::time::Instant::now() - start).as_nanos() as u64);
        }
    }

    let native_mean = if app.native.is_some() {
        let total: u64 = times.iter().sum();
        Some(total / (times.len() as u64))
    } else {
        None
    };

    let result = BenchResult {
        engine: app.engine,
        times,
        mean: wasm_mean,
        module: program_name,
        args: app.args,
    };

    match app.fmt {
        Fmt::Normal => {
            println!(
                "wasm: {}",
                std::time::Duration::from_nanos(wasm_mean).as_secs_f64()
            );
            if let Some(native_mean) = native_mean {
                println!(
                    "native: {}",
                    std::time::Duration::from_nanos(native_mean).as_secs_f64()
                )
            }
        }
        Fmt::Json => println!("{}", serde_json::to_value(result)?),
    }

    Ok(())
}
