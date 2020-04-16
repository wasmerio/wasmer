#[macro_export]
macro_rules! wasmer_backends {
    { $($code:item)* } => {
        #[cfg(feature = "backend-singlepass")]
        #[cfg(test)]
        mod singlepass {
            use wasmer::compiler::{Backend, Compiler, compiler_for_backend};
            #[allow(dead_code)]
            fn get_backend() -> Backend {
                Backend::Singlepass
            }
            #[allow(dead_code)]
            fn get_compiler() -> Box<dyn Compiler> {
                compiler_for_backend(get_backend()).expect("Backend must have a compiler")
            }
            $($code)*
        }
        #[cfg(feature = "backend-cranelift")]
        #[cfg(test)]
        mod cranelift {
            use wasmer::compiler::{Backend, Compiler, compiler_for_backend};
            #[allow(dead_code)]
            fn get_backend() -> Backend {
                Backend::Cranelift
            }
            #[allow(dead_code)]
            fn get_compiler() -> Box<dyn Compiler> {
                compiler_for_backend(get_backend()).expect("Backend must have a compiler")
            }
            $($code)*
        }
        #[cfg(feature = "backend-llvm")]
        #[cfg(test)]
        mod llvm {
            use wasmer::compiler::{Backend, Compiler, compiler_for_backend};
            #[allow(dead_code)]
            fn get_backend() -> Backend {
                Backend::LLVM
            }
            #[allow(dead_code)]
            fn get_compiler() -> Box<dyn Compiler> {
                compiler_for_backend(get_backend()).expect("Backend must have a compiler")
            }
            $($code)*
        }
    };
}
