//! This submodule has the concrete definitions for all the available implenters of the WebAssembly
//! types needed to create a runtime.

#[cfg(feature = "sys")]
pub(crate) mod sys;

#[derive(Debug, Clone, Copy)]
pub enum Embedder {
    #[cfg(feature = "sys")]
    Sys,
}

/// A macro useful to automatically generate suitable names for the VM impors for
/// a specific embedder.
#[macro_use]
macro_rules! vm_impex {
    ($crate_name: ident, $prefix: ident, $($import: ident),*) => {
        paste::paste!{
        pub(crate) use $crate_name::{
            $(
                $import as [<$prefix $import>]
            ),*
        };
        }
    };
}

/// A macro useful to automatically generate `-Like` trait impls for VM types.
#[macro_use]
macro_rules! vm_gen_impl{
    ($crate_name: ident, $prefix: ident, $($import: ident),*) => {
    paste::paste!{
        $(
        impl [<$import Like>] for [<$prefix $import>] {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }

            fn [<as_ $crate_name>](&self) -> Option<&crate::embedders::$crate_name::vm::[<$prefix $import>]> {
                Some(self)
            }

            fn [<as_ $crate_name _mut>](&mut self) -> Option<&mut crate::embedders::$crate_name::vm::[<$prefix $import>]> {
                Some(self)
            }
        }
        )*
    }
    };
}

pub(self) use vm_gen_impl;
pub(self) use vm_impex;
