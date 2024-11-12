//! This module defines traits to handle abstractions created by the embedders.

use crate::VMExternToExtern;
use wasmer_types::RawValue;

macro_rules! define_vm_like {
    ($name: ident) => {
        paste::paste! {
        /// The trait that defines the shared behaviour of those types that can be considered [<VM
        /// $name>]-like.
        pub trait [<VM $name Like>] {
            #[cfg(feature = "sys")]
            fn as_sys(&self) -> Option<&crate::embedders::sys::vm::[<SysVM $name>]> {
                None
            }
            #[cfg(feature = "sys")]
            fn as_sys_mut(&mut self) -> Option<&mut crate::embedders::sys::vm::[<SysVM $name>]> {
                None
            }

            #[cfg(feature = "sys")]
            fn into_sys(self) -> Option<crate::embedders::sys::vm::[<SysVM $name>]> where Self: Sized {
                None
            }

            fn as_any(&self) -> &dyn std::any::Any;

            fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
        }
        /// A newtype for references to those that implement [<VM $name Like>].
        pub type [<VM $name>]  = Box<dyn [<VM $name Like>]>;
        }
    };
}

define_vm_like!(Extern);
define_vm_like!(ExternFunction);
define_vm_like!(ExternGlobal);
define_vm_like!(ExternMemory);
define_vm_like!(ExternTable);
define_vm_like!(FunctionCallback);
define_vm_like!(FunctionBody);
define_vm_like!(FunctionEnvironment);
define_vm_like!(Instance);
define_vm_like!(Trampoline);

define_vm_like!(Config);
define_vm_like!(Function);
define_vm_like!(Global);
define_vm_like!(Memory);
define_vm_like!(SharedMemory);
define_vm_like!(Table);

define_vm_like!(ExternRef);
define_vm_like!(FuncRef);

/// The trait implemented by all those that can create new VM external references.
pub trait VMExternRefCreator {
    /// Extracts a [`VMExternRef`] from a [`RawValue`].
    ///
    /// # Safety
    /// `raw` must be a valid [`VMExternRef`] instance.
    unsafe fn extern_ref_from_raw(&self, raw: RawValue) -> Option<VMExternRef>;
}

/// The trait implemented by all those that can inspect and resolve VM external references.
pub trait VMExternRefResolver {
    /// Converts the [`VMExternRef`] into a [`RawValue`].
    fn extern_ref_into_raw(&self, value: VMExternRef) -> RawValue;
}

/// The trait implemented by all those that can create new VM function references.
pub trait VMFuncRefCreator {
    /// Extracts a [`VMFuncRef`] from a [`RawValue`].
    ///
    /// # Safety
    /// `raw` must be a valid [`VMFuncRef`] instance.
    unsafe fn func_ref_from_raw(&self, raw: RawValue) -> Option<VMFuncRef>;
}

/// The trait implemented by all those that can inspect and resolve VM function references.
pub trait VMFuncRefResolver {
    /// Converts the [`VMFuncRef`] into a [`RawValue`].
    fn func_ref_into_raw(&self, value: VMFuncRef) -> RawValue;
}

impl VMExternToExtern for VMExtern {
    fn to_extern(self, store: &mut impl crate::AsStoreMut) -> crate::Extern {
        todo!()
    }
}
