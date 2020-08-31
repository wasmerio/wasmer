use wasmer_compiler::CompiledFunctionUnwindInfo;

pub trait UnwindRegistryExt {
    /// Registers a function given the start offset, length, and
    /// unwind information.
    fn register(
        &mut self,
        base_address: usize,
        func_start: u32,
        func_len: u32,
        info: &CompiledFunctionUnwindInfo,
    ) -> Result<(), String>;

    /// Publishes all registered functions.
    fn publish(&mut self, eh_frame: Option<Vec<u8>>) -> Result<(), String>;
}

cfg_if::cfg_if! {
    if #[cfg(all(windows, target_arch = "x86_64"))] {
        mod windows_x64;
        pub use self::windows_x64::*;
    } else if #[cfg(unix)] {
        mod systemv;
        pub use self::systemv::*;
    } else {
        // Otherwise, we provide a dummy fallback without unwinding
        mod dummy;
        pub use self::dummy::DummyUnwindRegistry as UnwindRegistry;
    }
}
