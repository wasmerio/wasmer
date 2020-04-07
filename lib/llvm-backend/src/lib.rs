#![deny(
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
#![cfg_attr(
    all(not(target_os = "windows"), not(target_arch = "aarch64")),
    deny(dead_code)
)]
#![cfg_attr(nightly, feature(unwind_attributes))]
#![doc(html_favicon_url = "https://wasmer.io/static/icons/favicon.ico")]
#![doc(html_logo_url = "https://avatars3.githubusercontent.com/u/44205449?s=200&v=4")]

mod backend;
mod code;
mod intrinsics;
mod platform;
mod read_info;
mod stackmap;
mod state;
mod structs;
mod trampolines;

pub use code::LLVMFunctionCodeGenerator as FunctionCodeGenerator;
pub use code::LLVMModuleCodeGenerator as ModuleCodeGenerator;

use wasmer_runtime_core::codegen::SimpleStreamingCompilerGen;

pub type LLVMCompiler = SimpleStreamingCompilerGen<
    code::LLVMModuleCodeGenerator<'static>,
    code::LLVMFunctionCodeGenerator<'static>,
    backend::LLVMBackend,
    code::CodegenError,
>;

pub type InkwellModule<'ctx> = inkwell::module::Module<'ctx>;
pub type InkwellMemoryBuffer = inkwell::memory_buffer::MemoryBuffer;

pub trait LLVMCallbacks: std::any::Any + 'static {
    fn preopt_ir_callback(&mut self, _module: &InkwellModule) {}
    fn postopt_ir_callback(&mut self, _module: &InkwellModule) {}
    fn obj_memory_buffer_callback(&mut self, _memory_buffer: &InkwellMemoryBuffer) {}
}

pub struct LLVMBackendConfig {
    pub callbacks: Option<std::rc::Rc<std::cell::RefCell<dyn LLVMCallbacks>>>,
}
