use crate::{store::MaybeInstanceOwned, vmcontext::VMGlobalDefinition};
use std::{cell::UnsafeCell, ptr::NonNull};
use wasmer_types::{GlobalType, StoreSnapshot};
use derivative::Derivative;

/// A Global instance
#[derive(Derivative)]
#[derivative(Debug)]
pub struct VMGlobal {
    ty: GlobalType,
    #[derivative(Debug = "ignore")]
    vm_global_definition: MaybeInstanceOwned<VMGlobalDefinition>,
}

impl VMGlobal {
    /// Create a new, zero bit-pattern initialized global from a [`GlobalType`].
    pub fn new(global_type: GlobalType) -> Self {
        Self {
            ty: global_type,
            // TODO: Currently all globals are host-owned, we should inline the
            // VMGlobalDefinition in VMContext for instance-defined globals.
            vm_global_definition: MaybeInstanceOwned::Host(Box::new(UnsafeCell::new(
                VMGlobalDefinition::new(),
            ))),
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
                    self.vm_global_definition.as_ptr().as_ref().clone()
                ))),
            }
        }
    }

    /// Saves the global value into the snapshot
    pub fn save_snapshot(&self, index: usize, snapshot: &mut StoreSnapshot) {
        let entry = snapshot.globals
            .entry(index as u32)
            .or_default();
        
        let val = unsafe {
            self.vm_global_definition.as_ptr().as_ref().val.u128
        };
        *entry = val;
    }

    /// Restores the global value from the snapshot
    pub fn restore_snapshot(&mut self, index: usize, snapshot: &StoreSnapshot) {
        let index = index as u32;
        if let Some(entry) = snapshot.globals.get(&index) {
            let existing = unsafe {
                self.vm_global_definition.as_ptr().as_ref().val.u128
            };
            if existing != *entry {
                unsafe {
                    self.vm_global_definition.as_ptr().as_mut().val.u128 = *entry;
                }
            }
        }
    }
}
