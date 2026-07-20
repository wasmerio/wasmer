use super::*;
use pretty_assertions::assert_eq;

macro_rules! gen_tests {(
    $(
        $test_name:ident:
        stringify! {
            #[$function:ident $(($($attrs:tt)*))?]
            $($input:tt)*
        } == $output:expr;
    )*
) => (
    $(
        #[test]
        fn $test_name()
        {
            let input: TokenStream =
                stringify!($($input)*)
                    .parse()
                    .expect("Syntax error in test");
            let output: TokenStream =
                $output
                    .parse()
                    .expect("Syntax error in test");
            let attrs: TokenStream =
                stringify!($($($attrs)*)?)
                    .parse()
                    .expect("Syntax error in test");
            let ret = $function(attrs, input).to_string();
            eprintln!("{}", ret);
            assert_eq!(ret, output.to_string());
        }
    )*
)}

gen_tests! {
    identity_for_no_unsafe:
    stringify! {
        #[compiler_test_impl(derive_test)]
        #[cold]
        fn foo(config: crate::Config) {
            // Do tests
        }
    } == stringify! {
        #[cfg(test)]
        mod foo {
            use super:: * ;

            #[allow(unused)]
            fn foo(config: crate::Config) {
                // Do tests
            }

            #[cfg(feature = "singlepass")]
            mod singlepass {
                use super:: * ;
                #[test_log::test]
                #[cold]
                #[cfg(feature = "singlepass")]
                fn singlepass() {
                    foo(crate::Config::new(
                        crate::Compiler::Singlepass
                    ))
                }
            }

            #[cfg(feature = "cranelift")]
            mod cranelift {
                use super:: * ;
                #[test_log::test]
                #[cold]
                #[cfg(feature = "cranelift")]
                fn cranelift() {
                    foo(crate::Config::new(
                        crate::Compiler::Cranelift
                    ))
                }
            }

            #[cfg(feature = "llvm")]
            mod llvm {
                use super:: * ;
                #[test_log::test]
                #[cold]
                #[cfg(feature = "llvm")]
                fn llvm() {
                    foo(crate::Config::new(
                        crate::Compiler::LLVM
                    ))
                }
            }

            #[cfg(feature = "v8")]
            mod v8 {
                use super:: * ;
                #[test_log::test]
                #[cold]
                #[cfg(feature = "v8")]
                fn v8() {
                    foo(crate::Config::new(
                        crate::Compiler::V8
                    ))
                }
            }

            #[cfg(all(
                feature = "llvm",
                feature = "experimental-artifact",
                target_os = "linux",
                target_arch = "x86_64"
            ))]
            mod llvm_exp_artifact {
                use super:: * ;
                #[test_log::test]
                #[cold]
                #[cfg(all(
                    feature = "llvm",
                    feature = "experimental-artifact",
                    target_os = "linux",
                    target_arch = "x86_64"
                ))]
                fn llvm_exp_artifact() {
                    let mut config = crate::Config::new(crate::Compiler::LLVM);
                    config.set_elf_artifact(true);
                    foo(config)
                }
            }

            #[cfg(all(
                feature = "singlepass",
                feature = "experimental-artifact",
                target_os = "linux",
                target_arch = "x86_64"
            ))]
            mod singlepass_exp_artifact {
                use super:: * ;
                #[test_log::test]
                #[cold]
                #[cfg(all(
                    feature = "singlepass",
                    feature = "experimental-artifact",
                    target_os = "linux",
                    target_arch = "x86_64"
                ))]
                fn singlepass_exp_artifact() {
                    let mut config = crate::Config::new(crate::Compiler::Singlepass);
                    config.set_elf_artifact(true);
                    foo(config)
                }
            }

            #[cfg(all(
                feature = "cranelift",
                feature = "experimental-artifact",
                target_os = "linux",
                target_arch = "x86_64"
            ))]
            mod cranelift_exp_artifact {
                use super:: * ;
                #[test_log::test]
                #[cold]
                #[cfg(all(
                    feature = "cranelift",
                    feature = "experimental-artifact",
                    target_os = "linux",
                    target_arch = "x86_64"
                ))]
                fn cranelift_exp_artifact() {
                    let mut config = crate::Config::new(crate::Compiler::Cranelift);
                    config.set_elf_artifact(true);
                    foo(config)
                }
            }
        }
    };
}
