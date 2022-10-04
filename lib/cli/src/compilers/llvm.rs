#[derive(Debug, Parser, Clone)]
/// LLVM backend flags.
pub struct LLVMCLIOptions {
    /// Emit LLVM IR before optimization pipeline.
    #[clap(long = "llvm-pre-opt-ir", value_parser = clap::value_parser!(std::ffi::OsString))]
    pre_opt_ir: Option<PathBuf>,

    /// Emit LLVM IR after optimization pipeline.
    #[clap(long = "llvm-post-opt-ir", value_parser = clap::value_parser!(std::ffi::OsString))]
    post_opt_ir: Option<PathBuf>,

    /// Emit LLVM generated native code object file.
    #[clap(long = "llvm-object-file", value_parser = clap::value_parser!(std::ffi::OsString))]
    obj_file: Option<PathBuf>,
}

impl LLVMCallbacks for LLVMCLIOptions {
    fn preopt_ir_callback(&mut self, module: &InkwellModule) {
        if let Some(filename) = &self.pre_opt_ir {
            module.print_to_file(filename).unwrap();
        }
    }

    fn postopt_ir_callback(&mut self, module: &InkwellModule) {
        if let Some(filename) = &self.post_opt_ir {
            module.print_to_file(filename).unwrap();
        }
    }

    fn obj_memory_buffer_callback(&mut self, memory_buffer: &InkwellMemoryBuffer) {
        if let Some(filename) = &self.obj_file {
            let mem_buf_slice = memory_buffer.as_slice();
            let mut file = fs::File::create(filename).unwrap();
            let mut pos = 0;
            while pos < mem_buf_slice.len() {
                pos += file.write(&mem_buf_slice[pos..]).unwrap();
            }
        }
    }
}
