//! This module defines traits to handle abstractions created by the runtimes.

mod impls;

use crate::VMExternToExtern;
use wasmer_types::RawValue;

macro_rules! define_vm_like {
    ($name: ident $(, $derive:ident)*) => {
        paste::paste! {
        /// The enum for all those VM values of this kind.
        $(#[derive($derive)])*
        #[derive(derive_more::Unwrap)]
        #[unwrap(owned, ref, ref_mut)]
        #[repr(C)]
        pub enum [<VM $name>] {
            #[cfg(feature = "sys")]
            Sys(crate::backend::sys::vm::[<VM $name>]),
            #[cfg(feature = "wamr")]
            Wamr(crate::backend::wamr::vm::[<VM $name>]),
            #[cfg(feature = "wasmi")]
            Wasmi(crate::backend::wasmi::vm::[<VM $name>]),
            #[cfg(feature = "v8")]
            V8(crate::backend::v8::vm::[<VM $name>]),
            #[cfg(feature = "js")]
            Js(crate::backend::js::vm::[<VM $name>]),
            #[cfg(feature = "jsc")]
            Jsc(crate::backend::jsc::vm::[<VM $name>]),
        }
        }
    };
}

define_vm_like!(Extern);
define_vm_like!(ExternFunction, Debug);
define_vm_like!(ExternGlobal);
define_vm_like!(ExternTag);
define_vm_like!(ExternMemory);
define_vm_like!(ExternTable);
//define_vm_like!(ExternObj, Debug);
define_vm_like!(FunctionCallback, Debug);
define_vm_like!(FunctionBody);
define_vm_like!(FunctionEnvironment, Debug);
define_vm_like!(Instance, Debug);
define_vm_like!(Trampoline, Debug);

//define_vm_like!(Config);
define_vm_like!(Function, Debug);
define_vm_like!(Global, Debug);
define_vm_like!(Tag, Debug);
define_vm_like!(Memory, Debug);
define_vm_like!(SharedMemory);
define_vm_like!(Table, Debug);

define_vm_like!(ExceptionRef);
define_vm_like!(ExternRef);
define_vm_like!(FuncRef);
