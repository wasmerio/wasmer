use wasmer_compiler::CompilerConfig;
use wasmer_compiler_cranelift::CraneliftConfig;

pub fn get_compiler_config_from_str(
    compiler_name: &str,
    try_nan_canonicalization: bool,
) -> impl CompilerConfig {
    match compiler_name {
        "cranelift" => {
            let mut cranelift_config = CraneliftConfig::default();
            cranelift_config.enable_nan_canonicalization = try_nan_canonicalization;
            cranelift_config
        }
        _ => panic!("Compiler {} not supported", compiler_name),
    }
}
