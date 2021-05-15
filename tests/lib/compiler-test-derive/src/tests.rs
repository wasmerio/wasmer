use super::*;
use ::pretty_assertions::assert_eq;

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
        fn $test_name ()
        {
            let input: TokenStream =
                stringify!($($input)*)
                    .parse()
                    .expect("Syntax error in test")
            ;
            let output: TokenStream =
                $output
                    .parse()
                    .expect("Syntax error in test")
            ;
            let attrs: TokenStream =
                stringify!($($($attrs)*)?)
                    .parse()
                    .expect("Syntax error in test");
            let ret = $function(attrs, input).to_string();
            eprintln!("{}", ret);
            assert_eq!(
                ret,
                output.to_string(),
            )
        }
    )*
)}

gen_tests! {
    identity_for_no_unsafe:
    stringify! {
        #[compiler_test]
        #[cold]
        fn add (config: crate::Config)
        {
            // Do tests
        }
    } == stringify! {
        #[cfg(test)]
        mod add {
            use super::*;

            #[cold]
            fn add(config: crate::Config)
            {
                // Do tests
            }

            mod singlepass {
                use super::*;
                #[test]
                fn jit() {
                    add(crate::Config::new(
                        crate::Engine::JIT,
                        crate::Compiler::Singlepass
                    ))
                }
                #[test]
                fn native() {
                    add(crate::Config::new(
                        crate::Engine::Native,
                        crate::Compiler::Singlepass
                    ))
                }
            }

            mod cranelift {
                use super::*;
                #[test]
                fn jit() {
                    add(crate::Config::new(
                        crate::Engine::JIT,
                        crate::Compiler::Cranelift
                    ))
                }
                #[test]
                fn native() {
                    add(crate::Config::new(
                        crate::Engine::Native,
                        crate::Compiler::Cranelift
                    ))
                }
            }

            mod llvm {
                use super::add;
                #[test]
                fn jit() {
                    add(crate::Config::new(
                        crate::Engine::JIT,
                        crate::Compiler::LLVM
                    ))
                }
                #[test]
                fn native() {
                    add(crate::Config::new(
                        crate::Engine::Native,
                        crate::Compiler::LLVM
                    ))
                }
            }
        }
    };

    // basic_expansion:
    // stringify! {
    //    ...
    // };

}
