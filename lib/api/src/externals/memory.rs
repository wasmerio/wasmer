use crate::exports::{ExportError, Exportable};
use crate::externals::Extern;
use crate::store::Store;
use crate::{MemoryType, MemoryView};
use std::slice;
use std::sync::Arc;
use wasm_common::{Pages, ValueType};
use wasmer_vm::{Export, ExportMemory, Memory as RuntimeMemory, MemoryError};

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
/// Spec: https://webassembly.github.io/spec/core/exec/runtime.html#memory-instances
#[derive(Clone)]
pub struct Memory {
    store: Store,
    memory: Arc<dyn RuntimeMemory>,
}

impl Memory {
    /// Creates a new host `Memory` from the provided [`MemoryType`].
    ///
    /// This function will construct the `Memory` using the store [`Tunables`].
    ///
    /// [`Tunables`]: crate::tunables::Tunables
    pub fn new(store: &Store, ty: MemoryType) -> Result<Memory, MemoryError> {
        let tunables = store.tunables();
        let style = tunables.memory_style(&ty);
        let memory = tunables.create_memory(&ty, &style)?;

        Ok(Memory {
            store: store.clone(),
            memory,
        })
    }

    /// Returns the [`MemoryType`] of the `Memory`.
    pub fn ty(&self) -> &MemoryType {
        self.memory.ty()
    }

    /// Returns the [`Store`] where the `Memory` belongs.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// TODO: document this function.
    ///
    /// # Safety
    ///
    /// To be defined (TODO).
    pub unsafe fn data_unchecked(&self) -> &[u8] {
        self.data_unchecked_mut()
    }

    /// TODO: document this function, it's trivial to cause UB/break soundness with this
    /// method.
    ///
    /// # Safety
    ///
    /// To be defined (TODO).
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn data_unchecked_mut(&self) -> &mut [u8] {
        let definition = self.memory.vmmemory();
        let def = definition.as_ref();
        slice::from_raw_parts_mut(def.base, def.current_length)
    }

    /// Returns the pointer to the raw bytes of the `Memory`.
    pub fn data_ptr(&self) -> *mut u8 {
        let definition = self.memory.vmmemory();
        let def = unsafe { definition.as_ref() };
        def.base
    }

    /// Returns the size (in bytes) of the `Memory`.
    pub fn data_size(&self) -> usize {
        let definition = self.memory.vmmemory();
        let def = unsafe { definition.as_ref() };
        def.current_length
    }

    /// Returns the size (in [`Pages`]) of the `Memory`.
    pub fn size(&self) -> Pages {
        self.memory.size()
    }

    /// Grow memory by the specified amount of WebAssembly [`Pages`].
    ///
    /// # Errors
    ///
    /// Returns an error if memory can't be grown by the specified amount
    /// of pages.
    pub fn grow<IntoPages>(&self, delta: IntoPages) -> Result<Pages, MemoryError>
    where
        IntoPages: Into<Pages>,
    {
        self.memory.grow(delta.into())
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
        let base = self.data_ptr();

        let length = self.size().bytes().0 / std::mem::size_of::<T>();

        unsafe { MemoryView::new(base as _, length as u32) }
    }

    pub(crate) fn from_export(store: &Store, wasmer_export: ExportMemory) -> Memory {
        Memory {
            store: store.clone(),
            memory: wasmer_export.from,
        }
    }

    /// Returns whether or not these two globals refer to the same data.
    pub fn same(&self, other: &Memory) -> bool {
        Arc::ptr_eq(&self.memory, &other.memory)
    }
}

impl<'a> Exportable<'a> for Memory {
    fn to_export(&self) -> Export {
        ExportMemory {
            from: self.memory.clone(),
        }
        .into()
    }

    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Memory(memory) => Ok(memory),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
