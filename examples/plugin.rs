use serde::{Deserialize, Serialize};
use wasmer_runtime::{compile, func, imports};
use wasmer_runtime_core::vm::Ctx;
use wasmer_wasi::{
    generate_import_object_for_version,
    state::{self, WasiFile, WasiFsError},
    types,
};

static PLUGIN_LOCATION: &'static str = "examples/plugin-for-example.wasm";

fn it_works(_ctx: &mut Ctx) -> i32 {
    println!("Hello from outside WASI");
    5
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LoggingWrapper {
    pub wasm_module_name: String,
}

// std io trait boiler plate so we can implement WasiFile
// LoggingWrapper is a write-only type so we just want to immediately
// fail when reading or Seeking
impl std::io::Read for LoggingWrapper {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not read from logging wrapper",
        ))
    }
    fn read_to_end(&mut self, _buf: &mut Vec<u8>) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not read from logging wrapper",
        ))
    }
    fn read_to_string(&mut self, _buf: &mut String) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not read from logging wrapper",
        ))
    }
    fn read_exact(&mut self, _buf: &mut [u8]) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not read from logging wrapper",
        ))
    }
}
impl std::io::Seek for LoggingWrapper {
    fn seek(&mut self, _pos: std::io::SeekFrom) -> std::io::Result<u64> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not seek logging wrapper",
        ))
    }
}
impl std::io::Write for LoggingWrapper {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        out.write(b"[")?;
        out.write(self.wasm_module_name.as_bytes())?;
        out.write(b"]: ")?;
        out.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stdout().flush()
    }
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        out.write(b"[")?;
        out.write(self.wasm_module_name.as_bytes())?;
        out.write(b"]: ")?;
        out.write_all(buf)
    }
    fn write_fmt(&mut self, fmt: std::fmt::Arguments) -> std::io::Result<()> {
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        out.write(b"[")?;
        out.write(self.wasm_module_name.as_bytes())?;
        out.write(b"]: ")?;
        out.write_fmt(fmt)
    }
}

// the WasiFile methods aren't relevant for a write-only Stdout-like implementation
// we must use typetag and serde so that our trait objects can be safely Serialized and Deserialized
#[typetag::serde]
impl WasiFile for LoggingWrapper {
    fn last_accessed(&self) -> u64 {
        0
    }
    fn last_modified(&self) -> u64 {
        0
    }
    fn created_time(&self) -> u64 {
        0
    }
    fn size(&self) -> u64 {
        0
    }
    fn set_len(&mut self, _len: u64) -> Result<(), WasiFsError> {
        Ok(())
    }
    fn unlink(&mut self) -> Result<(), WasiFsError> {
        Ok(())
    }
    fn bytes_available(&self) -> Result<usize, WasiFsError> {
        // return an arbitrary amount
        Ok(1024)
    }
}

/// Called by the program when it wants to set itself up
fn initialize(ctx: &mut Ctx) {
    let state = unsafe { state::get_wasi_state(ctx) };
    let wasi_file_inner = LoggingWrapper {
        wasm_module_name: "example module name".to_string(),
    };
    // swap stdout with our new wasifile
    let _old_stdout = state
        .fs
        .swap_file(types::__WASI_STDOUT_FILENO, Box::new(wasi_file_inner))
        .unwrap();
}

fn main() {
    // Load the plugin data
    let wasm_bytes = std::fs::read(PLUGIN_LOCATION).expect(&format!(
        "Could not read in WASM plugin at {}",
        PLUGIN_LOCATION
    ));
    let module = compile(&wasm_bytes).expect("wasm compilation");

    // get the version of the WASI module in a non-strict way, meaning we're
    // allowed to have extra imports
    let wasi_version = wasmer_wasi::get_wasi_version(&module, false)
        .expect("WASI version detected from Wasm module");

    // WASI imports
    let mut base_imports =
        generate_import_object_for_version(wasi_version, vec![], vec![], vec![], vec![]);
    // env is the default namespace for extern functions
    let custom_imports = imports! {
        "env" => {
            "it_works" => func!(it_works),
        },
    };
    // The WASI imports object contains all required import functions for a WASI module to run.
    // Extend this imports with our custom imports containing "it_works" function so that our custom wasm code may run.
    base_imports.extend(custom_imports);
    let mut instance = module
        .instantiate(&base_imports)
        .expect("failed to instantiate wasm module");
    // set up logging by replacing stdout
    initialize(instance.context_mut());

    // get a reference to the function "plugin_entrypoint" which takes an i32 and returns an i32
    let entry_point = instance.func::<(i32), i32>("plugin_entrypoint").unwrap();
    // call the "entry_point" function in WebAssembly with the number "2" as the i32 argument
    let result = entry_point.call(2).expect("failed to execute plugin");
    println!("result: {}", result);
}
