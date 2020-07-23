use crate::vmcontext::VMGlobalDefinition;
use std::cell::UnsafeCell;
use std::ptr::NonNull;
use std::sync::Mutex;
use thiserror::Error;
use wasm_common::{GlobalType, Type};

#[derive(Debug)]
/// TODO: figure out a decent name for this thing
pub struct Global {
    ty: GlobalType,
    // this box may be unnecessary
    vm_global_definition: Box<UnsafeCell<VMGlobalDefinition>>,
    // used to synchronize gets/sets
    lock: Mutex<()>,
}

/// # Safety
/// This is safe to send between threads because there is no-thread specific logic.
/// TODO: look into other reasons that make something not `Send`
unsafe impl Send for Global {}
/// # Safety
/// This is safe to share between threads because it uses a `Mutex` internally.
unsafe impl Sync for Global {}

/// Error type describing things that can go wrong when operating on Wasm Globals.
#[derive(Error, Debug, Clone, PartialEq, Hash)]
pub enum GlobalError {
    /// The error returned when attempting to set an immutable global.
    #[error("Attempted to set an immutable global")]
    ImmutableGlobalCannotBeSet,

    /// The error returned when attempting to operate on a global as a specific type
    /// that differs from the global's own type.
    #[error("Attempted to operate on a global of type {expected} as a global of type {found}")]
    IncorrectType {
        /// The type that the global is.
        expected: Type,
        /// The type that we were asked to use it as.
        found: Type,
    },
}

impl Global {
    /// Create a new, zero bit-pattern initialized global from a [`GlobalType`].
    pub fn new(global_type: GlobalType) -> Self {
        Self {
            ty: global_type,
            vm_global_definition: Box::new(UnsafeCell::new(VMGlobalDefinition::new())),
            lock: Mutex::new(()),
        }
    }

    /// Get the type of the global.
    pub fn ty(&self) -> &GlobalType {
        &self.ty
    }

    /// Get a pointer to the underlying definition used by the generated code.
    pub fn vmglobal(&self) -> NonNull<VMGlobalDefinition> {
        let ptr = self.vm_global_definition.as_ref() as *const UnsafeCell<VMGlobalDefinition>
            as *const VMGlobalDefinition as *mut VMGlobalDefinition;
        unsafe { NonNull::new_unchecked(ptr) }
    }

    /// Get a reference to the definition
    pub fn get(&self) -> &VMGlobalDefinition {
        unsafe { &*self.vm_global_definition.get() }
    }

    /// Get a mutable reference to the definition
    pub fn get_mut(&self) -> &mut VMGlobalDefinition {
        unsafe { &mut *self.vm_global_definition.get() }
    }
}
