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
                #[test_log::test]
                #[cold]
                #[cfg(feature = "singlepass")]
                #[cfg(target_os = "linux")]
                fn singlepass_exp_artifact() {
                    foo(crate::Config::new(
                        crate::Compiler::Singlepass
                    ).with_experimental_artifact())
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
                #[test_log::test]
                #[cold]
                #[cfg(feature = "cranelift")]
                #[cfg(target_os = "linux")]
                fn cranelift_exp_artifact() {
                    foo(crate::Config::new(
                        crate::Compiler::Cranelift
                    ).with_experimental_artifact())
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
                #[test_log::test]
                #[cold]
                #[cfg(feature = "llvm")]
                #[cfg(target_os = "linux")]
                fn llvm_exp_artifact() {
                    foo(crate::Config::new(
                        crate::Compiler::LLVM
                    ).with_experimental_artifact())
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

        }
    };
}
