use crate::exports::{ExportError, Exportable};
use crate::externals::Extern;
use crate::memory_view::MemoryView;
use crate::store::Store;
use crate::MemoryType;
use std::ptr::NonNull;
use std::slice;
use wasm_common::{Pages, ValueType};
use wasmer_runtime::{
    Export, ExportMemory, Memory as MemoryTrait, MemoryError, VMMemoryDefinition,
};

#[derive(Clone)]
pub struct Memory {
    store: Store,
    // If the Memory is owned by the Store, not the instance
    owned_by_store: bool,
    exported: ExportMemory,
}

impl Memory {
    pub fn new(store: &Store, ty: MemoryType) -> Result<Memory, MemoryError> {
        let tunables = store.tunables();
        let style = tunables.memory_style(&ty);
        let memory = tunables.create_memory(&ty, &style)?;
        let definition = memory.vmmemory();

        Ok(Memory {
            store: store.clone(),
            owned_by_store: true,
            exported: ExportMemory {
                from: memory,
                definition,
            },
        })
    }

    fn definition(&self) -> NonNull<VMMemoryDefinition> {
        self.memory().vmmemory()
    }

    pub fn ty(&self) -> &MemoryType {
        self.exported.ty()
    }

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
        let definition = self.definition();
        let def = definition.as_ref();
        slice::from_raw_parts_mut(def.base, def.current_length)
    }

    pub fn data_ptr(&self) -> *mut u8 {
        let definition = self.definition();
        let def = unsafe { definition.as_ref() };
        def.base
    }

    pub fn data_size(&self) -> usize {
        let definition = self.definition();
        let def = unsafe { definition.as_ref() };
        def.current_length
    }

    pub fn size(&self) -> Pages {
        self.memory().size()
    }

    fn memory(&self) -> &dyn MemoryTrait {
        &*self.exported.from
    }

    pub fn grow<IntoPages>(&self, delta: IntoPages) -> Result<Pages, MemoryError>
    where
        IntoPages: Into<Pages>,
    {
        self.memory().grow(delta.into())
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
            owned_by_store: false,
            exported: wasmer_export,
        }
    }

    /// Returns whether or not these two globals refer to the same data.
    pub fn same(&self, other: &Memory) -> bool {
        self.exported.same(&other.exported)
    }
}

impl<'a> Exportable<'a> for Memory {
    fn to_export(&self) -> Export {
        self.exported.clone().into()
    }
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Memory(memory) => Ok(memory),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        if self.owned_by_store {
            // let r = unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.len) };
            // assert_eq!(r, 0, "munmap failed: {}", std::io::Error::last_os_error());
        }
    }
}
