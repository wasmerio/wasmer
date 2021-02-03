//! TODO: document this
//!
//! We store metadata for functions here. This is what a funcref actually points to.

use crate::vmcontext::VMCallerCheckedAnyfunc;
use std::collections::HashMap;
use std::sync::Mutex;

/// WebAssembly requires that the caller and callee signatures in an indirect
/// call must match. To implement this efficiently, keep a registry of all
/// signatures, shared by all instances, so that call sites can just do an
/// index comparison.
#[derive(Debug)]
pub struct FuncDataRegistry {
    // This structure is stored in an `Engine` and is intended to be shared
    // across many instances. Ideally instances can themselves be sent across
    // threads, and ideally we can compile across many threads. As a result we
    // use interior mutability here with a lock to avoid having callers to
    // externally synchronize calls to compilation.
    inner: Mutex<Inner>,
}

// We use raw pointers but the data never moves, so it's not a problem
unsafe impl Send for FuncDataRegistry {}
unsafe impl Sync for FuncDataRegistry {}

/// VM Func Ref, TODO document this
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VMFuncRef(pub(crate) *const VMCallerCheckedAnyfunc);

impl VMFuncRef {
    /// Check if the FuncRef is null
    // TODO: make this const when `std::ptr::is_null` is const
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }

    /// Create a new null FuncRef
    pub const fn null() -> Self {
        Self(std::ptr::null())
    }
}

impl std::ops::Deref for VMFuncRef {
    type Target = *const VMCallerCheckedAnyfunc;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for VMFuncRef {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// We use raw pointers but the data never moves, so it's not a problem
// TODO: update docs
unsafe impl Send for VMFuncRef {}
unsafe impl Sync for VMFuncRef {}

#[derive(Debug, Default)]
struct Inner {
    func_data: Vec<Box<VMCallerCheckedAnyfunc>>,
    anyfunc_to_index: HashMap<VMCallerCheckedAnyfunc, usize>,
}

impl FuncDataRegistry {
    /// Create a new `FuncDataRegistry`.
    pub fn new() -> Self {
        Self {
            inner: Default::default(),
        }
    }

    /// Register a signature and return its unique index.
    pub fn register(&self, anyfunc: VMCallerCheckedAnyfunc) -> VMFuncRef {
        let mut inner = self.inner.lock().unwrap();
        if let Some(&idx) = inner.anyfunc_to_index.get(&anyfunc) {
            let data: &Box<_> = &inner.func_data[idx];
            let inner_ptr: &VMCallerCheckedAnyfunc = &*data;
            VMFuncRef(inner_ptr)
        } else {
            let idx = inner.func_data.len();
            inner.func_data.push(Box::new(anyfunc.clone()));
            inner.anyfunc_to_index.insert(anyfunc, idx);

            let data: &Box<_> = &inner.func_data[idx];
            let inner_ptr: &VMCallerCheckedAnyfunc = &*data;
            VMFuncRef(inner_ptr)
        }
    }
}
