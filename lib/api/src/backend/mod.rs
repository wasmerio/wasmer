//! This submodule has the concrete definitions for all the available implenters of the WebAssembly
//! types needed to create a runtime.

#[cfg(feature = "sys")]
pub mod sys;

#[cfg(feature = "wamr")]
pub mod wamr;

#[cfg(feature = "wasmi")]
pub mod wasmi;

#[cfg(feature = "v8")]
pub mod v8;

#[cfg(feature = "js")]
pub mod js;

#[cfg(feature = "jsc")]
pub mod jsc;

#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
/// An enumeration over all the supported runtimes.
pub enum BackendKind {
    #[cfg(feature = "cranelift")]
    /// The `cranelift` runtime.
    Cranelift,

    #[cfg(feature = "llvm")]
    /// The `llvm` runtime.
    LLVM,

    #[cfg(feature = "singlepass")]
    /// The `singlepass` runtime.
    Singlepass,

    #[cfg(feature = "sys")]
    /// The sys `headless` runtime.
    Headless,

    #[cfg(feature = "wamr")]
    /// The `wamr` runtime.
    Wamr,

    #[cfg(feature = "wasmi")]
    /// The `wasmi` runtime.
    Wasmi,

    #[cfg(feature = "v8")]
    /// The `v8` runtime.
    V8,

    #[cfg(feature = "js")]
    /// The `js` runtime.
    Js,

    #[cfg(feature = "jsc")]
    /// The `jsc` runtime.
    Jsc,
}

impl Default for BackendKind {
    fn default() -> Self {
        #[cfg(feature = "sys-default")]
        {
            #[cfg(feature = "cranelift")]
            {
                return Self::Cranelift;
            }
            #[cfg(feature = "singlepass")]
            {
                return Self::Singlepass;
            }
            #[cfg(feature = "llvm")]
            {
                return Self::LLVM;
            }
            return Self::Headless;
        }

        #[cfg(feature = "wamr-default")]
        {
            return Self::Wamr;
        }

        #[cfg(feature = "wasmi-default")]
        {
            return Self::Wasmi;
        }

        #[cfg(feature = "v8-default")]
        {
            return Self::V8;
        }

        #[cfg(feature = "js-default")]
        {
            return Self::Js;
        }

        #[cfg(feature = "jsc-default")]
        {
            return Self::Jsc;
        }

        #[cfg(feature = "sys")]
        {
            #[cfg(feature = "cranelift")]
            {
                return Self::Cranelift;
            }
            #[cfg(feature = "singlepass")]
            {
                return Self::Singlepass;
            }
            #[cfg(feature = "llvm")]
            {
                return Self::LLVM;
            }
            return Self::Headless;
        }

        #[cfg(feature = "wamr")]
        {
            return Self::Wamr;
        }

        #[cfg(feature = "wasmi")]
        {
            return Self::Wasmi;
        }

        #[cfg(feature = "v8")]
        {
            return Self::V8;
        }

        #[cfg(feature = "js")]
        {
            return Self::Js;
        }

        #[cfg(feature = "jsc")]
        {
            return Self::Jsc;
        }

        panic!("No runtime enabled!")
    }
}
