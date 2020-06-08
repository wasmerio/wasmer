#[macro_export]
macro_rules! wasmer_compilers {
    { $($code:item)* } => {
        #[cfg(feature = "singlepass")]
        #[cfg(test)]
        mod singlepass {
            static COMPILER_NAME: &str = "singlepass";
            use std::sync::Arc;
            use wasmer::{Features, Store, Tunables};
            use wasmer_engine_jit::JITEngine;
            use test_utils::get_compiler_config_from_str;

            #[allow(dead_code)]
            fn get_store() -> Store {
                let features = Features::default();
                let try_nan_canonicalization = false;
                let compiler_config =
                    get_compiler_config_from_str(COMPILER_NAME, try_nan_canonicalization, features);
                let tunables = Tunables::for_target(compiler_config.target().triple());
                let store = Store::new(Arc::new(JITEngine::new(compiler_config, tunables)));
                store
            }
            $($code)*
        }

        #[cfg(feature = "cranelift")]
        #[cfg(test)]
        mod cranelift {
            static COMPILER_NAME: &str = "cranelift";
            use std::sync::Arc;
            use wasmer::{Features, Store, Tunables};
            use wasmer_engine_jit::JITEngine;
            use test_utils::get_compiler_config_from_str;

            #[allow(dead_code)]
            fn get_store() -> Store {
                let features = Features::default();
                let try_nan_canonicalization = false;
                let compiler_config =
                    get_compiler_config_from_str(COMPILER_NAME, try_nan_canonicalization, features);
                let tunables = Tunables::for_target(compiler_config.target().triple());
                let store = Store::new(Arc::new(JITEngine::new(compiler_config, tunables)));
                store
            }
            $($code)*
        }
        #[cfg(feature = "llvm")]
        #[cfg(test)]
        mod llvm {
            static COMPILER_NAME: &str = "llvm";
            use std::sync::Arc;
            use wasmer::{Features, Store, Tunables};
            use wasmer_engine_jit::JITEngine;
            use test_utils::get_compiler_config_from_str;

            #[allow(dead_code)]
            fn get_store() -> Store {
                let features = Features::default();
                let try_nan_canonicalization = false;
                let compiler_config =
                    get_compiler_config_from_str(COMPILER_NAME, try_nan_canonicalization, features);
                let tunables = Tunables::for_target(compiler_config.target().triple());
                let store = Store::new(Arc::new(JITEngine::new(compiler_config, tunables)));
                store
            }
            $($code)*
        }
    };
}
