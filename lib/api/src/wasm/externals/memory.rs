use crate::wasm::export::{Export, VMMemory};
use crate::wasm::exports::{ExportError, Exportable};
use crate::wasm::externals::Extern;
use crate::wasm::store::Store;
use crate::wasm::{MemoryType, MemoryView};
use std::convert::TryInto;
use thiserror::Error;

use wasmer_types::{Bytes, Pages, ValueType};

/// Error type describing things that can go wrong when operating on Wasm Memories.
#[derive(Error, Debug, Clone, PartialEq, Hash)]
pub enum MemoryError {
    /// The operation would cause the size of the memory to exceed the maximum or would cause
    /// an overflow leading to unindexable memory.
    #[error("The memory could not grow: current size {} pages, requested increase: {} pages", current.0, attempted_delta.0)]
    CouldNotGrow {
        /// The current size in pages.
        current: Pages,
        /// The attempted amount to grow by in pages.
        attempted_delta: Pages,
    },
    /// A user defined error value, used for error cases not listed above.
    #[error("A user-defined error occurred: {0}")]
    Generic(String),
}

/// A WebAssembly `memory` instance.
///
/// A memory instance is the runtime representation of a linear memory.
/// It consists of a vector of bytes and an optional maximum size.
///
/// The length of the vector always is a multiple of the WebAssembly
/// page size, which is defined to be the constant 65536 – abbreviated 64Ki.
/// Like in a memory type, the maximum size in a memory instance is
/// given in units of this page size.
///
/// A memory created by the host or in WebAssembly code will be accessible and
/// mutable from both host and WebAssembly.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#memory-instances>
#[derive(Debug, Clone)]
pub struct Memory {
    store: Store,
    vm_memory: VMMemory,
}

impl Memory {
    /// Creates a new host `Memory` from the provided [`MemoryType`].
    ///
    /// This function will construct the `Memory` using the store
    /// [`BaseTunables`][crate::js::tunables::BaseTunables].
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Memory, MemoryType, Pages, Store, Type, Value};
    /// # let store = Store::default();
    /// #
    /// let m = Memory::new(&store, MemoryType::new(1, None, false)).unwrap();
    /// ```
    pub fn new(store: &Store, ty: MemoryType) -> Result<Self, MemoryError> {
        panic!("Not implemented!")
    }

    /// Returns the [`MemoryType`] of the `Memory`.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Memory, MemoryType, Pages, Store, Type, Value};
    /// # let store = Store::default();
    /// #
    /// let mt = MemoryType::new(1, None, false);
    /// let m = Memory::new(&store, mt).unwrap();
    ///
    /// assert_eq!(m.ty(), mt);
    /// ```
    pub fn ty(&self) -> MemoryType {
        let mut ty = self.vm_memory.ty().clone();
        ty.minimum = self.size();
        ty
    }

    /// Returns the [`Store`] where the `Memory` belongs.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Memory, MemoryType, Pages, Store, Type, Value};
    /// # let store = Store::default();
    /// #
    /// let m = Memory::new(&store, MemoryType::new(1, None, false)).unwrap();
    ///
    /// assert_eq!(m.store(), &store);
    /// ```
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Retrieve a slice of the memory contents.
    ///
    /// # Safety
    ///
    /// Until the returned slice is dropped, it is undefined behaviour to
    /// modify the memory contents in any way including by calling a wasm
    /// function that writes to the memory or by resizing the memory.
    pub unsafe fn data_unchecked(&self) -> &[u8] {
        unimplemented!("direct data pointer access is not possible in JavaScript");
    }

    /// Retrieve a mutable slice of the memory contents.
    ///
    /// # Safety
    ///
    /// This method provides interior mutability without an UnsafeCell. Until
    /// the returned value is dropped, it is undefined behaviour to read or
    /// write to the pointed-to memory in any way except through this slice,
    /// including by calling a wasm function that reads the memory contents or
    /// by resizing this Memory.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn data_unchecked_mut(&self) -> &mut [u8] {
        unimplemented!("direct data pointer access is not possible in JavaScript");
    }

    /// Returns the pointer to the raw bytes of the `Memory`.
    pub fn data_ptr(&self) -> *mut u8 {
        unimplemented!("direct data pointer access is not possible in JavaScript");
    }

    /// Returns the size (in bytes) of the `Memory`.
    pub fn data_size(&self) -> u64 {
        panic!("Not implemented!")
    }

    /// Returns the size (in [`Pages`]) of the `Memory`.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Memory, MemoryType, Pages, Store, Type, Value};
    /// # let store = Store::default();
    /// #
    /// let m = Memory::new(&store, MemoryType::new(1, None, false)).unwrap();
    ///
    /// assert_eq!(m.size(), Pages(1));
    /// ```
    pub fn size(&self) -> Pages {
        panic!("Not implemented!")
    }

    /// Grow memory by the specified amount of WebAssembly [`Pages`] and return
    /// the previous memory size.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Memory, MemoryType, Pages, Store, Type, Value, WASM_MAX_PAGES};
    /// # let store = Store::default();
    /// #
    /// let m = Memory::new(&store, MemoryType::new(1, Some(3), false)).unwrap();
    /// let p = m.grow(2).unwrap();
    ///
    /// assert_eq!(p, Pages(1));
    /// assert_eq!(m.size(), Pages(3));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if memory can't be grown by the specified amount
    /// of pages.
    ///
    /// ```should_panic
    /// # use wasmer::{Memory, MemoryType, Pages, Store, Type, Value, WASM_MAX_PAGES};
    /// # let store = Store::default();
    /// #
    /// let m = Memory::new(&store, MemoryType::new(1, Some(1), false)).unwrap();
    ///
    /// // This results in an error: `MemoryError::CouldNotGrow`.
    /// let s = m.grow(1).unwrap();
    /// ```
    pub fn grow<IntoPages>(&self, delta: IntoPages) -> Result<Pages, MemoryError>
    where
        IntoPages: Into<Pages>,
    {
        panic!("Not implemented!")
    }

    /// Return a "view" of the currently accessible memory. By
    /// default, the view is unsynchronized, using regular memory
    /// accesses. You can force a memory view to use atomic accesses
    /// by calling the [`MemoryView::atomically`] method.
    ///
    /// # Notes:
    ///
    /// This method is safe (as in, it won't cause the host to crash or have UB),
    /// but it doesn't obey rust's rules involving data races, especially concurrent ones.
    /// Therefore, if this memory is shared between multiple threads, a single memory
    /// location can be mutated concurrently without synchronization.
    ///
    /// # Usage:
    ///
    /// ```
    /// # use wasmer::{Memory, MemoryView};
    /// # use std::{cell::Cell, sync::atomic::Ordering};
    /// # fn view_memory(memory: Memory) {
    /// // Without synchronization.
    /// let view: MemoryView<u8> = memory.view();
    /// for byte in view[0x1000 .. 0x1010].iter().map(Cell::get) {
    ///     println!("byte: {}", byte);
    /// }
    ///
    /// // With synchronization.
    /// let atomic_view = view.atomically();
    /// for byte in atomic_view[0x1000 .. 0x1010].iter().map(|atom| atom.load(Ordering::SeqCst)) {
    ///     println!("byte: {}", byte);
    /// }
    /// # }
    /// ```
    pub fn view<T: ValueType>(&self) -> MemoryView<T> {
        unimplemented!("The view function is not yet implemented in Wasmer Javascript");
    }

    pub(crate) fn from_vm_export(store: &Store, vm_memory: VMMemory) -> Self {
        Self {
            store: store.clone(),
            vm_memory,
        }
    }

    /// Returns whether or not these two memories refer to the same data.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Memory, MemoryType, Store, Value};
    /// # let store = Store::default();
    /// #
    /// let m = Memory::new(&store, MemoryType::new(1, None, false)).unwrap();
    ///
    /// assert!(m.same(&m));
    /// ```
    pub fn same(&self, other: &Self) -> bool {
        panic!("Not implemented!")
    }
}

impl<'a> Exportable<'a> for Memory {
    fn to_export(&self) -> Export {
        Export::Memory(self.vm_memory.clone())
    }

    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Memory(memory) => Ok(memory),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
