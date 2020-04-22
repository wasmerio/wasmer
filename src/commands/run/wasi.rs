use wasmer_wasi;




#[derive(Debug, StructOpt, Clone)]
/// WASI Options
pub struct WasiOptions {
    /// WASI pre-opened directory
    #[structopt(long = "dir", multiple = true, group = "wasi")]
    pre_opened_directories: Vec<PathBuf>,

    /// Map a host directory to a different location for the wasm module
    #[structopt(long = "mapdir", multiple = true)]
    mapped_dirs: Vec<String>,

    /// Pass custom environment variables
    #[structopt(long = "env", multiple = true)]
    env_vars: Vec<String>,
}


fn get_mapped_dirs(input: &[String]) -> Result<Vec<(String, PathBuf)>, String> {
    let mut md = vec![];
    for entry in input.iter() {
        if let [alias, real_dir] = entry.split(':').collect::<Vec<&str>>()[..] {
            let pb = PathBuf::from(&real_dir);
            if let Ok(pb_metadata) = pb.metadata() {
                if !pb_metadata.is_dir() {
                    return Err(format!(
                        "\"{}\" exists, but it is not a directory",
                        &real_dir
                    ));
                }
            } else {
                return Err(format!("Directory \"{}\" does not exist", &real_dir));
            }
            md.push((alias.to_string(), pb));
            continue;
        }
        return Err(format!(
            "Directory mappings must consist of two paths separate by a colon. Found {}",
            &entry
        ));
    }
    Ok(md)
}

fn get_env_var_args(input: &[String]) -> Result<Vec<(&str, &str)>, String> {
    let mut ev = vec![];
    for entry in input.iter() {
        if let [env_var, value] = entry.split('=').collect::<Vec<&str>>()[..] {
            ev.push((env_var, value));
        } else {
            return Err(format!(
                "Env vars must be of the form <var_name>=<value>. Found {}",
                &entry
            ));
        }
    }
    Ok(ev)
}

/// Helper function for `execute_wasm` (the `Run` command)
fn execute_wasi(
    wasi_version: wasmer_wasi::WasiVersion,
    options: &Run,
    env_vars: Vec<(&str, &str)>,
    module: wasmer_runtime_core::Module,
    mapped_dirs: Vec<(String, PathBuf)>,
    _wasm_binary: &[u8],
) -> Result<()> {
    let name = if let Some(cn) = &options.command_name {
        cn.clone()
    } else {
        options.path.to_str().unwrap().to_owned()
    };

    let args = options.args.iter().cloned().map(|arg| arg.into_bytes());
    let preopened_files = options.pre_opened_directories.clone();
    let mut wasi_state_builder = wasmer_wasi::state::WasiState::new(&name);
    wasi_state_builder
        .args(args)
        .envs(env_vars)
        .preopen_dirs(preopened_files)
        .map_err(|e| format!("Failed to preopen directories: {:?}", e))?
        .map_dirs(mapped_dirs)
        .map_err(|e| format!("Failed to preopen mapped directories: {:?}", e))?;

    #[cfg(feature = "experimental-io-devices")]
    {
        if options.enable_experimental_io_devices {
            wasi_state_builder.setup_fs(Box::new(wasmer_wasi_experimental_io_devices::initialize));
        }
    }
    let wasi_state = wasi_state_builder.build()?;
    let import_object = wasmer_wasi::generate_import_object_from_state(wasi_state, wasi_version);

    let mut instance = module
        .instantiate(&import_object)
        .map_err(|e| format!("Can't instantiate WASI module: {:?}", e))?;

    let start: &Func = instance
        .exports
        .get("_start")?;
    Ok(())
}