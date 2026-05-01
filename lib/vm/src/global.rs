use crate::{store::MaybeInstanceOwned, vmcontext::VMGlobalDefinition};
use std::{cell::UnsafeCell, ptr::NonNull};
use wasmer_types::GlobalType;

/// A Global instance
#[derive(Debug)]
pub struct VMGlobal {
    ty: GlobalType,
    vm_global_definition: MaybeInstanceOwned<VMGlobalDefinition>,
}

impl VMGlobal {
    /// Create a new, zero bit-pattern initialized global from a [`GlobalType`].
    pub fn new(global_type: GlobalType) -> Self {
        Self {
            ty: global_type,
            vm_global_definition: MaybeInstanceOwned::Host(Box::new(UnsafeCell::new(
                VMGlobalDefinition::new(),
            ))),
        }
    }

    /// Create a new global backed by a `VMGlobalDefinition` stored inline in a `VMContext`.
    ///
    /// # Safety
    /// - `vm_definition_location` must be a valid, properly aligned location for
    ///   a `VMGlobalDefinition`.
    pub unsafe fn new_instance(
        global_type: GlobalType,
        vm_definition_location: NonNull<VMGlobalDefinition>,
    ) -> Self {
        unsafe {
            vm_definition_location
                .as_ptr()
                .write(VMGlobalDefinition::new());
        }
        Self {
            ty: global_type,
            vm_global_definition: MaybeInstanceOwned::Instance(vm_definition_location),
        }
    }

    /// Get the type of the global.
    pub fn ty(&self) -> &GlobalType {
        &self.ty
    }

    /// Get a pointer to the underlying definition used by the generated code.
    pub fn vmglobal(&self) -> NonNull<VMGlobalDefinition> {
        self.vm_global_definition.as_ptr()
    }

    /// Copies this global
    pub fn copy_on_write(&self) -> Self {
        unsafe {
            Self {
                ty: self.ty,
                vm_global_definition: MaybeInstanceOwned::Host(Box::new(UnsafeCell::new(
                    self.vm_global_definition.as_ptr().as_ref().clone(),
                ))),
            }
        }
    }
}
