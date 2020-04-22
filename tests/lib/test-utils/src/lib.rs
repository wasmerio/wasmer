use wasmer_compiler::CompilerConfig;
use wasmer_compiler_cranelift::CraneliftConfig;
use wasmer_compiler_singlepass::SinglepassConfig;

pub fn get_compiler_config_from_str(
    compiler_name: &str,
    try_nan_canonicalization: bool,
) -> Box<dyn CompilerConfig> {
    match compiler_name {
        "singlepass" => {
            let mut singlepass_config = SinglepassConfig::default();
            singlepass_config.enable_nan_canonicalization = try_nan_canonicalization;
            Box::new(singlepass_config)
        }
        "cranelift" => {
            let mut cranelift_config = CraneliftConfig::default();
            cranelift_config.enable_nan_canonicalization = try_nan_canonicalization;
            Box::new(cranelift_config)
        }
        _ => panic!("Compiler {} not supported", compiler_name),
    }
}
