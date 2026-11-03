use crate::{store::MaybeInstanceOwned, vmcontext::VMGlobalDefinition};
use std::{cell::UnsafeCell, ptr::NonNull};
use wasmer_types::GlobalType;

/// A Global instance
#[derive(Debug)]
pub struct VMGlobal {
    ty: GlobalType,
    vm_global_definition: MaybeInstanceOwned<VMGlobalDefinition>,
    _definition_storage: Option<std::sync::Arc<crate::VMInstanceStorage>>,
}

impl VMGlobal {
    /// Create a new, zero bit-pattern initialized global from a [`GlobalType`].
    pub fn new(global_type: GlobalType) -> Self {
        Self {
            ty: global_type,
            _definition_storage: None,
            vm_global_definition: MaybeInstanceOwned::Host(Box::new(UnsafeCell::new(
                VMGlobalDefinition::new(),
            ))),
        }
    }

    /// Create a new global backed by a `VMGlobalDefinition` stored inline in a `VMContext`.
    ///
    /// # Safety
    /// The location must provide aligned, exclusively writable storage for
    /// initialization. A managed token retains its allocation; for an unmanaged
    /// raw pointer, the caller must retain the bytes until the last physical
    /// owner/clone is gone. Later access still requires the original Store or
    /// memory synchronization; allocation custody alone grants no data access.
    pub unsafe fn new_instance(
        global_type: GlobalType,
        vm_definition_location: impl Into<crate::VMDefinitionLocation<VMGlobalDefinition>>,
    ) -> Self {
        let (vm_definition_location, definition_storage) =
            vm_definition_location.into().into_parts();
        unsafe {
            vm_definition_location
                .as_ptr()
                .write(VMGlobalDefinition::new());
        }
        Self {
            ty: global_type,
            _definition_storage: definition_storage,
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
                _definition_storage: None,
                vm_global_definition: MaybeInstanceOwned::Host(Box::new(UnsafeCell::new(
                    self.vm_global_definition.as_ptr().as_ref().clone(),
                ))),
            }
        }
    }
}
