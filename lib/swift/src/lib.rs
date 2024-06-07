use std::sync::Arc;
use tokio::runtime::Runtime;
use virtual_fs::{AsyncReadExt, AsyncSeekExt};
use wasmer_wasix::{
    bin_factory::BinaryPackage,
    runners::{wasi::WasiRunner, Runner},
    runtime::{package_loader::BuiltinPackageLoader, task_manager::tokio::TokioTaskManager},
    PluggableRuntime,
};

uniffi::setup_scaffolding!();

macro_rules! err {
    ($e:expr) => {
        match $e {
            Ok(r) => r,
            Err(e) => {
                println!("{e}");
                return Err(WasmerError::Err { e: e.to_string() });
            }
        }
    };
}

#[derive(Debug, uniffi::Error, thiserror::Error)]
pub enum WasmerError {
    #[error("An error occurred during execution: {e:?}")]
    Err { e: String },
}

#[uniffi::export]
pub fn run_package(webc_bytes: Vec<u8>, args: Vec<String>) -> Result<String, WasmerError> {
    let tokio_rt = Runtime::new().unwrap();
    let _enter = tokio_rt.enter();
    let container = err!(webc::Container::from_bytes(webc_bytes));
    let tasks = TokioTaskManager::new(tokio_rt.handle().clone());
    let tasks = Arc::new(tasks);
    let mut rt = PluggableRuntime::new(Arc::clone(&tasks) as Arc<_>);
    rt.set_engine(Some(wasmer::Engine::default()))
        .set_package_loader(BuiltinPackageLoader::new());

    let pkg = tokio_rt
        .handle()
        .block_on(async { BinaryPackage::from_webc(&container, &rt).await.unwrap() });

    if pkg.entrypoint_cmd.is_none() {
        return Ok(format!("This WEBC ({}) has no entrypoint!", pkg.id));
    }

    let entrypoint = pkg.entrypoint_cmd.clone().unwrap();

    let mut stdout = virtual_fs::ArcFile::new(Box::<virtual_fs::BufferFile>::default());

    let stdout_2 = stdout.clone();

    let handle = std::thread::spawn(move || {
        let _guard = tasks.runtime_handle().enter();
        WasiRunner::new()
            .with_args(args)
            .with_stdin(Box::<virtual_fs::NullFile>::default())
            .with_stdout(Box::new(stdout_2) as Box<_>)
            .with_stderr(Box::<virtual_fs::NullFile>::default())
            .run_command(&entrypoint, &pkg, Arc::new(rt))
    });

    let _ = handle.join();

    let mut output = Vec::new();

    tokio_rt.handle().block_on(async {
        stdout.rewind().await.unwrap();
        stdout.read_to_end(&mut output).await.unwrap();
    });

    Ok(err!(String::from_utf8(output)))
}
