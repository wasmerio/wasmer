use crate::imports::Imports;
use crate::module::ModuleInfo;
use crate::vmcontext::VMGlobalDefinition;
use std::cell::UnsafeCell;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::Mutex;
use thiserror::Error;
use wasm_common::{GlobalInit, GlobalType, Mutability, Type, Value};

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

    /// Create a new, zero bit-pattern initialized global from a [`GlobalType`].
    pub fn new_with_init(
        module: &ModuleInfo,
        imports: &Imports,
        mutability: Mutability,
        value: GlobalInit,
    ) -> Result<Arc<Self>, GlobalError> {
        let mut vm_global_definition = VMGlobalDefinition::new();
        let ty = unsafe {
            match value {
                GlobalInit::I32Const(v) => {
                    *vm_global_definition.as_i32_mut() = v;
                    Type::I32
                }
                GlobalInit::I64Const(v) => {
                    *vm_global_definition.as_i64_mut() = v;
                    Type::I64
                }
                GlobalInit::F32Const(v) => {
                    *vm_global_definition.as_f32_mut() = v;
                    Type::F32
                }
                GlobalInit::F64Const(v) => {
                    *vm_global_definition.as_f64_mut() = v;
                    Type::F64
                }
                GlobalInit::GetGlobal(global_index) => {
                    /*let other_type = module.globals.get(global_index).map_err(|e| "global does not exist")?;
                     */
                    if let Some(global_value) = imports.globals.get(global_index) {
                        return Ok(global_value.from.clone());
                    } else {
                        let idx = module.local_global_index(global_index).unwrap();
                        if let Some(&init) = module.global_initializers.get(idx) {
                            // TODO: test for cycles/infinite loops here
                            return Self::new_with_init(module, imports, mutability, init);
                        }
                        // global may not exist here.
                        // TODO: tests should test if initializing with local global with no initializer and
                        // initializing with an invalid global index
                        todo!("check for initializer?, otherwise 0")
                    }
                }
                _ => unimplemented!(
                    "Global `new_with_init` is not implemented for initializer {:?}",
                    value
                ),
            }
        };
        Ok(Arc::new(Self {
            ty: GlobalType { ty, mutability },
            vm_global_definition: Box::new(UnsafeCell::new(vm_global_definition)),
            lock: Mutex::new(()),
        }))
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

    /// Get a value from the global.
    pub fn get<T>(&self) -> Value<T> {
        let _global_guard = self.lock.lock().unwrap();
        unsafe {
            let definition = &*self.vm_global_definition.get();
            match self.ty().ty {
                Type::I32 => Value::from(*definition.as_i32()),
                Type::I64 => Value::from(*definition.as_i64()),
                Type::F32 => Value::F32(*definition.as_f32()),
                Type::F64 => Value::F64(*definition.as_f64()),
                _ => unimplemented!("Global::get for {:?}", self.ty),
            }
        }
    }

    /// Set a value for the global.
    ///
    /// # Safety
    /// The caller should check that the `val` comes from the same store as this global.
    pub unsafe fn set<T>(&self, val: Value<T>) -> Result<(), GlobalError> {
        let _global_guard = self.lock.lock().unwrap();
        if self.ty().mutability != Mutability::Var {
            return Err(GlobalError::ImmutableGlobalCannotBeSet);
        }
        if val.ty() != self.ty().ty {
            return Err(GlobalError::IncorrectType {
                expected: self.ty.ty,
                found: val.ty(),
            });
        }
        // TODO: checking which store values are in is not done in this function
        self.set_unchecked(val)
    }

    /// Set a value from the global (unchecked)
    ///
    /// # Safety
    /// The caller should check that the `val` comes from the same store as this global.
    pub unsafe fn set_unchecked<T>(&self, val: Value<T>) -> Result<(), GlobalError> {
        // ideally we'd use atomics here
        let definition = &mut *self.vm_global_definition.get();
        match val {
            Value::I32(i) => *definition.as_i32_mut() = i,
            Value::I64(i) => *definition.as_i64_mut() = i,
            Value::F32(f) => *definition.as_f32_mut() = f,
            Value::F64(f) => *definition.as_f64_mut() = f,
            _ => unimplemented!("Global::set for {:?}", val.ty()),
        }
        Ok(())
    }
}
