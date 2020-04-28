use wasmer_compiler::CompilerConfig;

pub fn get_compiler_config_from_str(
    compiler_name: &str,
    try_nan_canonicalization: bool,
) -> Box<dyn CompilerConfig> {
    match compiler_name {
        #[cfg(feature = "compiler-singlepass")]
        "singlepass" => {
            let mut singlepass_config = wasmer_compiler_singlepass::SinglepassConfig::default();
            singlepass_config.enable_nan_canonicalization = try_nan_canonicalization;
            Box::new(singlepass_config)
        }
        #[cfg(feature = "compiler-cranelift")]
        "cranelift" => {
            let mut cranelift_config = wasmer_compiler_cranelift::CraneliftConfig::default();
            cranelift_config.enable_nan_canonicalization = try_nan_canonicalization;
            Box::new(cranelift_config)
        }
        #[cfg(feature = "compiler-llvm")]
        "llvm" => {
            let mut llvm_config = wasmer_compiler_llvm::LLVMConfig::default();
            llvm_config.enable_nan_canonicalization = try_nan_canonicalization;
            Box::new(llvm_config)
        }
        _ => panic!("Compiler {} not supported", compiler_name),
    }
}
