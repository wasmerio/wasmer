//! Data types, functions and traits for `sys` runtime's `Memory` implementation.
use std::{
    convert::TryInto,
    marker::PhantomData,
    mem::{self, MaybeUninit},
    slice,
    sync::Arc,
};

use tracing::warn;
use wasmer_types::{MemoryType, Pages};
use wasmer_vm::{
    LinearMemory, MemoryError, StoreHandle, ThreadConditionsHandle, VMMemory as SysVMMemory,
};

use crate::{
    BackendMemory, MemoryAccessError, SharedMemory,
    backend::sys::entities::{engine::NativeEngineExt, memory::MemoryView},
    entities::store::{AsStoreMut, AsStoreRef},
    location::{MemoryLocation, SharedMemoryOps},
    vm::{VMExtern, VMExternMemory},
};

pub(crate) mod view;
pub use view::*;

use super::store::Store;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
/// A WebAssembly `memory` in the `sys` runtime.
pub struct Memory {
    pub(crate) handle: StoreHandle<SysVMMemory>,
}

impl Memory {
    pub(crate) fn new(store: &mut impl AsStoreMut, ty: MemoryType) -> Result<Self, MemoryError> {
        let mut store = store.as_store_mut();
        let tunables = store.engine().tunables();
        let style = tunables.memory_style(&ty);
        let memory = tunables.create_host_memory(&ty, &style)?;

        Ok(Self {
            handle: StoreHandle::new(store.as_store_mut().objects_mut().as_sys_mut(), memory),
        })
    }

    pub(crate) fn new_from_existing(new_store: &mut impl AsStoreMut, memory: SysVMMemory) -> Self {
        let handle = StoreHandle::new(new_store.objects_mut().as_sys_mut(), memory);
        Self::from_vm_extern(new_store, VMExternMemory::Sys(handle.internal_handle()))
    }

    pub(crate) fn ty(&self, store: &impl AsStoreRef) -> MemoryType {
        self.handle
            .get(store.as_store_ref().objects().as_sys())
            .ty()
    }

    pub(crate) fn size(&self, store: &impl AsStoreRef) -> Pages {
        self.handle
            .get(store.as_store_ref().objects().as_sys())
            .size()
    }

    pub(crate) fn grow<IntoPages>(
        &self,
        store: &mut impl AsStoreMut,
        delta: IntoPages,
    ) -> Result<Pages, MemoryError>
    where
        IntoPages: Into<Pages>,
    {
        self.handle
            .get_mut(store.objects_mut().as_sys_mut())
            .grow(delta.into())
    }

    pub(crate) fn grow_at_least(
        &self,
        store: &mut impl AsStoreMut,
        min_size: u64,
    ) -> Result<(), MemoryError> {
        self.handle
            .get_mut(store.objects_mut().as_sys_mut())
            .grow_at_least(min_size)
    }

    pub(crate) fn reset(&self, store: &mut impl AsStoreMut) -> Result<(), MemoryError> {
        self.handle
            .get_mut(store.as_store_mut().objects_mut().as_sys_mut())
            .reset()?;
        Ok(())
    }

    pub(crate) fn from_vm_extern(store: &impl AsStoreRef, vm_extern: VMExternMemory) -> Self {
        Self {
            handle: unsafe {
                StoreHandle::from_internal(
                    store.as_store_ref().objects().id(),
                    vm_extern.unwrap_sys(),
                )
            },
        }
    }

    /// Checks whether this `Memory` can be used with the given context.
    pub(crate) fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.handle.store_id() == store.as_store_ref().objects().id()
    }

    pub(crate) fn copy(&self, store: &impl AsStoreRef) -> Result<SharedMemory, MemoryError> {
        if self.ty(store).shared {
            let mem = self.handle.get(store.as_store_ref().objects().as_sys());
            let copied = mem.copy()?;
            let ops: Option<Arc<dyn SharedMemoryOps + Send + Sync>> = copied
                .thread_conditions()
                .map(|conditions| Arc::new(conditions.downgrade()) as Arc<_>);
            let memory = crate::vm::VMMemory::Sys(SysVMMemory::from(copied)).as_shared()?;

            Ok(match ops {
                Some(ops) => SharedMemory::new_with_ops(memory, ops),
                None => SharedMemory::new(memory),
            })
        } else {
            Err(MemoryError::MemoryNotShared)
        }
    }

    pub(crate) fn as_shared(&self, store: &impl AsStoreRef) -> Result<SharedMemory, MemoryError> {
        let mem = self.handle.get(store.as_store_ref().objects().as_sys());
        let conds = mem
            .thread_conditions()
            .ok_or(MemoryError::MemoryNotShared)?
            .downgrade();
        let cloned = mem.try_clone()?;

        Ok(SharedMemory::new_with_ops(
            crate::vm::VMMemory::Sys(cloned).as_shared()?,
            Arc::new(conds),
        ))
    }

    /// To `VMExtern`.
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Sys(wasmer_vm::VMExtern::Memory(self.handle.internal_handle()))
    }
}

impl SharedMemoryOps for ThreadConditionsHandle {
    fn notify(&self, dst: MemoryLocation, count: u32) -> Result<u32, crate::AtomicsError> {
        let count = self
            .upgrade()
            .ok_or(crate::AtomicsError::MemoryDropped)?
            .do_notify(dst.address, count);
        Ok(count)
    }

    fn wait(
        &self,
        dst: MemoryLocation,
        timeout: Option<std::time::Duration>,
    ) -> Result<u32, crate::AtomicsError> {
        // Safety: `ExpectedValue::None` has no safety requirements.
        unsafe {
            self.upgrade()
                .ok_or(crate::AtomicsError::MemoryDropped)?
                .do_wait(
                    wasmer_vm::NotifyLocation {
                        memory_base: std::ptr::null_mut(),
                        address: dst.address,
                    },
                    wasmer_vm::ExpectedValue::None,
                    timeout,
                )
                .map_err(|e| match e {
                    wasmer_vm::WaiterError::Unimplemented => crate::AtomicsError::Unimplemented,
                    wasmer_vm::WaiterError::TooManyWaiters => crate::AtomicsError::TooManyWaiters,
                    wasmer_vm::WaiterError::AtomicsDisabled => crate::AtomicsError::AtomicsDisabled,
                    _ => crate::AtomicsError::Unimplemented,
                })
        }
    }

    fn disable_atomics(&self) -> Result<(), crate::AtomicsError> {
        self.upgrade()
            .ok_or(crate::AtomicsError::MemoryDropped)?
            .disable_atomics();
        Ok(())
    }

    fn wake_all_atomic_waiters(&self) -> Result<(), crate::AtomicsError> {
        self.upgrade()
            .ok_or(crate::AtomicsError::MemoryDropped)?
            .wake_all_atomic_waiters();
        Ok(())
    }
}

impl std::cmp::PartialEq for Memory {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}

impl std::cmp::Eq for Memory {}

/// Underlying buffer for a memory.
#[derive(Debug, Copy, Clone)]
pub(crate) struct MemoryBuffer<'a> {
    pub(crate) base: *mut u8,
    pub(crate) len: usize,
    pub(crate) marker: PhantomData<&'a MemoryView<'a>>,
}

impl MemoryBuffer<'_> {
    pub(crate) fn read(&self, offset: u64, buf: &mut [u8]) -> Result<(), MemoryAccessError> {
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or(MemoryAccessError::Overflow)?;
        if end > self.len.try_into().unwrap() {
            warn!(
                "attempted to read ({} bytes) beyond the bounds of the memory view ({} > {})",
                buf.len(),
                end,
                self.len
            );
            return Err(MemoryAccessError::HeapOutOfBounds);
        }
        unsafe {
            volatile_memcpy_read(self.base.add(offset as usize), buf.as_mut_ptr(), buf.len());
        }
        Ok(())
    }

    pub(crate) fn read_uninit<'b>(
        &self,
        offset: u64,
        buf: &'b mut [MaybeUninit<u8>],
    ) -> Result<&'b mut [u8], MemoryAccessError> {
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or(MemoryAccessError::Overflow)?;
        if end > self.len.try_into().unwrap() {
            warn!(
                "attempted to read ({} bytes) beyond the bounds of the memory view ({} > {})",
                buf.len(),
                end,
                self.len
            );
            return Err(MemoryAccessError::HeapOutOfBounds);
        }
        let buf_ptr = buf.as_mut_ptr() as *mut u8;
        unsafe {
            volatile_memcpy_read(self.base.add(offset as usize), buf_ptr, buf.len());
        }

        Ok(unsafe { slice::from_raw_parts_mut(buf_ptr, buf.len()) })
    }

    pub(crate) fn write(&self, offset: u64, data: &[u8]) -> Result<(), MemoryAccessError> {
        let end = offset
            .checked_add(data.len() as u64)
            .ok_or(MemoryAccessError::Overflow)?;
        if end > self.len.try_into().unwrap() {
            warn!(
                "attempted to write ({} bytes) beyond the bounds of the memory view ({} > {})",
                data.len(),
                end,
                self.len
            );
            return Err(MemoryAccessError::HeapOutOfBounds);
        }
        unsafe {
            volatile_memcpy_write(data.as_ptr(), self.base.add(offset as usize), data.len());
        }
        Ok(())
    }
}

// We can't use a normal memcpy here because it has undefined behavior if the
// memory is being concurrently modified. So we need to write our own memcpy
// implementation which uses volatile operations.
//
// The implementation of these functions can optimize very well when inlined
// with a fixed length: they should compile down to a single load/store
// instruction for small (8/16/32/64-bit) copies.
#[inline]
unsafe fn volatile_memcpy_read(mut src: *const u8, mut dst: *mut u8, mut len: usize) {
    #[inline]
    unsafe fn copy_one<T>(src: &mut *const u8, dst: &mut *mut u8, len: &mut usize) {
        #[repr(C, packed)]
        struct Unaligned<T>(T);

        unsafe {
            let val = (*src as *const Unaligned<T>).read_volatile();
            (*dst as *mut Unaligned<T>).write(val);

            *src = src.add(mem::size_of::<T>());
            *dst = dst.add(mem::size_of::<T>());
            *len -= mem::size_of::<T>();
        }
    }

    unsafe {
        while len >= 8 {
            copy_one::<u64>(&mut src, &mut dst, &mut len);
        }
        if len >= 4 {
            copy_one::<u32>(&mut src, &mut dst, &mut len);
        }
        if len >= 2 {
            copy_one::<u16>(&mut src, &mut dst, &mut len);
        }
        if len >= 1 {
            copy_one::<u8>(&mut src, &mut dst, &mut len);
        }
    }
}
#[inline]
unsafe fn volatile_memcpy_write(mut src: *const u8, mut dst: *mut u8, mut len: usize) {
    #[inline]
    unsafe fn copy_one<T>(src: &mut *const u8, dst: &mut *mut u8, len: &mut usize) {
        #[repr(C, packed)]
        struct Unaligned<T>(T);

        unsafe {
            let val = (*src as *const Unaligned<T>).read();
            (*dst as *mut Unaligned<T>).write_volatile(val);
            *src = src.add(mem::size_of::<T>());
            *dst = dst.add(mem::size_of::<T>());
            *len -= mem::size_of::<T>();
        }
    }

    unsafe {
        while len >= 8 {
            copy_one::<u64>(&mut src, &mut dst, &mut len);
        }
        if len >= 4 {
            copy_one::<u32>(&mut src, &mut dst, &mut len);
        }
        if len >= 2 {
            copy_one::<u16>(&mut src, &mut dst, &mut len);
        }
        if len >= 1 {
            copy_one::<u8>(&mut src, &mut dst, &mut len);
        }
    }
}

impl crate::Memory {
    /// Admits a finite physical replacement before any OS operation or callback.
    /// Keep this memory and its Store alive until the change finishes.
    #[doc(hidden)]
    pub fn begin_host_mapping_change(
        &self,
        store: &impl AsStoreRef,
        range: std::ops::Range<u64>,
    ) -> Result<wasmer_vm::HostMemoryMappingChange, MemoryError> {
        match &self.0 {
            BackendMemory::Sys(memory) => memory
                .handle
                .get(store.as_store_ref().objects().as_sys())
                .begin_host_mapping_change(range),
            #[allow(unreachable_patterns)]
            _ => Err(MemoryError::UnsupportedOperation {
                message: "native mapping changes are unsupported by this backend".into(),
            }),
        }
    }

    /// Consume [`self`] into a [`crate::backend::sys::memory::Memory`].
    pub fn into_sys(self) -> crate::backend::sys::memory::Memory {
        match self.0 {
            BackendMemory::Sys(s) => s,
            _ => panic!("Not a `sys` memory!"),
        }
    }

    /// Convert a reference to [`self`] into a reference to [`crate::backend::sys::memory::Memory`].
    pub fn as_sys(&self) -> &crate::backend::sys::memory::Memory {
        match self.0 {
            BackendMemory::Sys(ref s) => s,
            _ => panic!("Not a `sys` memory!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference to [`crate::backend::sys::memory::Memory`].
    pub fn as_sys_mut(&mut self) -> &mut crate::backend::sys::memory::Memory {
        match self.0 {
            BackendMemory::Sys(ref mut s) => s,
            _ => panic!("Not a `sys` memory!"),
        }
    }
}

/// Run custom growth work without lending its currently borrowed Store.
///
/// Only compiled memory.grow activates this boundary. Direct host operations
/// execute `f` unchanged. The caller must finish all callback-bearing growth
/// work inside `f` and return its owned outcome immediately afterward.
/// Ordinary panics retain their payload; a managed root retired by a callback
/// traps at the VM boundary after the mutable memory borrows have ended.
/// Successful Rust cleanup during an existing unwind does not start another
/// managed abort; the original unwind already prevents guest continuation.
#[doc(hidden)]
pub fn with_memory_grow_callback<R>(f: impl FnOnce() -> R) -> R {
    use crate::entities::store::StoreContext;
    use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
    if !wasmer_vm::libcalls::memory_grow_active() {
        return f();
    }
    let already_unwinding = std::thread::panicking();
    #[cfg(feature = "experimental-async")]
    let native = crate::backend::sys::async_runtime::MemoryGrowCallbackGuard::enter();
    let stores = StoreContext::detach_current();
    // Verify metadata before any user code can try to derive a Store pointer.
    assert!(
        StoreContext::is_empty(),
        "memory grow callback Store is not hidden"
    );
    let mut failures = Vec::new();
    let mut value = None;
    let mut callback = Some(f);
    // An existing unwind already prevents guest continuation. Let successful
    // Rust cleanup finish without starting a second managed-abort unwind.
    let mut aborted: Option<crate::RuntimeError> = if already_unwinding {
        None
    } else {
        #[cfg(feature = "experimental-async")]
        {
            native.retirement_error()
        }
        #[cfg(not(feature = "experimental-async"))]
        {
            None
        }
    };
    if aborted.is_none() {
        match catch_unwind(AssertUnwindSafe(|| callback.take().unwrap()())) {
            Ok(result) => value = Some(result),
            Err(panic) => failures.push(panic),
        }
        if failures.is_empty() && !already_unwinding {
            aborted = {
                #[cfg(feature = "experimental-async")]
                {
                    native.retirement_error()
                }
                #[cfg(not(feature = "experimental-async"))]
                {
                    None
                }
            };
        }
    } else if let Err(panic) = catch_unwind(AssertUnwindSafe(|| drop(callback.take()))) {
        failures.push(panic);
    }
    // A generic result can own a callback-bearing destructor. Dispose it
    // before restoring Store visibility or allowing a yield.
    if aborted.is_some()
        && let Err(panic) = catch_unwind(AssertUnwindSafe(|| drop(value.take())))
    {
        failures.push(panic);
    }
    if let Err(panic) = catch_unwind(AssertUnwindSafe(|| drop(stores))) {
        failures.push(panic);
    }
    #[cfg(feature = "experimental-async")]
    if let Err(panic) = catch_unwind(AssertUnwindSafe(|| drop(native))) {
        failures.push(panic);
    }
    if !failures.is_empty() {
        // As in notify_scope_after, a failed restoration must dispose any
        // retained user result under a fresh detached context before replay.
        #[cfg(feature = "experimental-async")]
        let recovery = crate::backend::sys::async_runtime::MemoryGrowCallbackGuard::enter();
        let stores = StoreContext::detach_current();
        if let Err(panic) = catch_unwind(AssertUnwindSafe(|| drop(value.take()))) {
            failures.push(panic);
        }
        if let Err(panic) = catch_unwind(AssertUnwindSafe(|| drop(aborted.take()))) {
            failures.push(panic);
        }
        let first = failures.remove(0);
        for secondary in failures {
            let _ = catch_unwind(AssertUnwindSafe(|| drop(secondary)));
        }
        let restored = catch_unwind(AssertUnwindSafe(|| drop(stores)));
        if let Err(secondary) = restored {
            let _ = catch_unwind(AssertUnwindSafe(|| drop(secondary)));
        }
        #[cfg(feature = "experimental-async")]
        if let Err(secondary) = catch_unwind(AssertUnwindSafe(|| drop(recovery))) {
            let _ = catch_unwind(AssertUnwindSafe(|| drop(secondary)));
        }
        resume_unwind(first);
    }
    if let Some(error) = aborted {
        wasmer_vm::libcalls::raise_memory_grow_trap(Box::new(error));
    }
    value.unwrap()
}

#[cfg(all(test, feature = "compiler", feature = "wat"))]
mod callback_tests {
    use super::with_memory_grow_callback;
    #[cfg(feature = "experimental-async")]
    use crate::AsStoreAsync;
    use crate::entities::store::StoreContext;
    use crate::sys::{BaseTunables, NativeEngineExt, Tunables};
    use crate::{Function, Imports, Instance, Memory, MemoryType, Module, Pages, Store};
    use std::{
        cell::RefCell,
        panic::{AssertUnwindSafe, catch_unwind, panic_any},
        process::{Child, Command},
        ptr::NonNull,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };
    use wasmer_vm::{
        LinearMemory, MemoryError, MemoryStyle, TableStyle, VMDefinitionLocation, VMMemory,
        VMMemoryDefinition, VMTable, VMTableDefinition,
    };
    thread_local! {
        static HOOK: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    }
    #[derive(Debug)]
    struct State {
        armed: AtomicBool,
        calls: AtomicUsize,
        hidden: AtomicBool,
        continued: AtomicUsize,
        discard_result: AtomicBool,
        discard_drops: AtomicUsize,
        discard_bad_context: AtomicUsize,
        discard_panics: AtomicBool,
        discard_payload: Arc<()>,
    }
    struct Discarded(Arc<State>);
    impl Drop for Discarded {
        fn drop(&mut self) {
            self.0.discard_drops.fetch_add(1, Ordering::SeqCst);
            if !StoreContext::is_empty() {
                self.0.discard_bad_context.fetch_add(1, Ordering::SeqCst);
            }
            if self.0.discard_panics.load(Ordering::SeqCst) {
                panic_any(self.0.discard_payload.clone());
            }
        }
    }
    #[derive(Debug)]
    struct CallbackMemory {
        inner: VMMemory,
        state: Arc<State>,
    }
    impl CallbackMemory {
        fn run_hook(&self) {
            if self.state.armed.swap(false, Ordering::SeqCst) {
                self.state.calls.fetch_add(1, Ordering::SeqCst);
                let hidden = StoreContext::is_empty();
                self.state.hidden.store(hidden, Ordering::SeqCst);
                // Never derive a conflicting Store on a defective test run.
                if hidden {
                    assert!(Store::with_current(|_| ()).is_none());
                }
                let hook = HOOK.with(|slot| slot.borrow_mut().take());
                if let Some(hook) = hook {
                    hook();
                }
            }
        }
    }
    impl LinearMemory for CallbackMemory {
        fn ty(&self) -> MemoryType {
            self.inner.ty()
        }
        fn size(&self) -> Pages {
            self.inner.size()
        }
        fn style(&self) -> MemoryStyle {
            self.inner.style()
        }
        fn vmmemory(&self) -> NonNull<VMMemoryDefinition> {
            self.inner.vmmemory()
        }
        fn grow(&mut self, delta: Pages) -> Result<Pages, MemoryError> {
            if self.state.discard_result.load(Ordering::SeqCst) {
                // This is the OUTERMOST scope: a nested scope would hide an
                // incorrectly delayed result destructor behind its caller.
                let _value = with_memory_grow_callback(|| {
                    self.run_hook();
                    Discarded(self.state.clone())
                });
                self.inner.grow(delta)
            } else {
                with_memory_grow_callback(|| {
                    self.run_hook();
                    self.inner.grow(delta)
                })
            }
        }
        fn try_clone(&self) -> Result<Box<dyn LinearMemory + Send + Sync>, MemoryError> {
            Ok(Box::new(Self {
                inner: self.inner.try_clone()?,
                state: self.state.clone(),
            }))
        }
        fn copy(&self) -> Result<Box<dyn LinearMemory + Send + Sync>, MemoryError> {
            Ok(Box::new(Self {
                inner: VMMemory::from_custom(self.inner.copy()?),
                state: self.state.clone(),
            }))
        }
    }
    struct CallbackTunables {
        base: BaseTunables,
        state: Arc<State>,
    }
    impl CallbackTunables {
        fn wrap(&self, inner: VMMemory) -> VMMemory {
            VMMemory::from_custom(Box::new(CallbackMemory {
                inner,
                state: self.state.clone(),
            }) as Box<dyn LinearMemory + Send + Sync>)
        }
    }
    impl Tunables for CallbackTunables {
        fn memory_style(&self, ty: &MemoryType) -> MemoryStyle {
            self.base.memory_style(ty)
        }
        fn table_style(&self, ty: &crate::TableType) -> TableStyle {
            self.base.table_style(ty)
        }
        fn create_host_memory(
            &self,
            ty: &MemoryType,
            style: &MemoryStyle,
        ) -> Result<VMMemory, MemoryError> {
            self.base
                .create_host_memory(ty, style)
                .map(|inner| self.wrap(inner))
        }
        unsafe fn create_vm_memory(
            &self,
            ty: &MemoryType,
            style: &MemoryStyle,
            location: VMDefinitionLocation<VMMemoryDefinition>,
        ) -> Result<VMMemory, MemoryError> {
            // SAFETY: forward the exact pointer and definition storage together.
            unsafe {
                self.base
                    .create_vm_memory(ty, style, location)
                    .map(|inner| self.wrap(inner))
            }
        }
        fn create_host_table(
            &self,
            ty: &crate::TableType,
            style: &TableStyle,
        ) -> Result<VMTable, String> {
            self.base.create_host_table(ty, style)
        }
        unsafe fn create_vm_table(
            &self,
            ty: &crate::TableType,
            style: &TableStyle,
            location: VMDefinitionLocation<VMTableDefinition>,
        ) -> Result<VMTable, String> {
            // SAFETY: forward the exact pointer and definition storage together.
            unsafe { self.base.create_vm_table(ty, style, location) }
        }
    }
    fn setup(imported: bool) -> (Store, Instance, Arc<State>) {
        let state = Arc::new(State {
            armed: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
            hidden: AtomicBool::new(false),
            continued: AtomicUsize::new(0),
            discard_result: AtomicBool::new(false),
            discard_drops: AtomicUsize::new(0),
            discard_bad_context: AtomicUsize::new(0),
            discard_panics: AtomicBool::new(false),
            discard_payload: Arc::new(()),
        });
        let mut store = Store::default();
        store.engine_mut().set_tunables(CallbackTunables {
            base: BaseTunables::new(),
            state: state.clone(),
        });
        let after = state.clone();
        let continued = Function::new_typed(&mut store, move || {
            after.continued.fetch_add(1, Ordering::SeqCst);
        });
        let declaration = if imported {
            "(import \"env\" \"memory\" (memory 1 3))"
        } else {
            "(memory 1 3)"
        };
        let module = Module::new(
            &store,
            format!(
                r#"(module
            (import "env" "continued" (func $continued)) {declaration}
            (export "memory" (memory 0))
            (func (export "run") (result i32)
                i32.const 1 memory.grow drop
                i32.const 0 i32.const 1 i32.store
                call $continued i32.const 42)
            (func (export "fresh") (result i32) i32.const 42))"#
            ),
        )
        .unwrap();
        let mut imports = Imports::new();
        imports.define("env", "continued", continued);
        if imported {
            let memory = Memory::new(&mut store, MemoryType::new(1, Some(3), false)).unwrap();
            imports.define("env", "memory", memory);
        }
        let instance = Instance::new(&mut store, &module, &imports).unwrap();
        state.armed.store(true, Ordering::SeqCst);
        (store, instance, state)
    }
    fn continuation(instance: &Instance, store: &impl crate::AsStoreRef) -> u32 {
        let mut bytes = [0; 4];
        instance
            .exports
            .get_memory("memory")
            .unwrap()
            .view(store)
            .read(0, &mut bytes)
            .unwrap();
        u32::from_le_bytes(bytes)
    }
    fn ready_work() {
        let mut other = Store::default();
        let function = Function::new_typed(&mut other, || -> i32 { 42 });
        assert_eq!(
            function
                .typed::<(), i32>(&other)
                .unwrap()
                .call(&mut other)
                .unwrap(),
            42
        );
    }
    fn subprocess(name: &str) -> bool {
        const CHILD: &str = "WASMER_MEMORY_CALLBACK_SCOPE_CHILD";
        if std::env::var_os(CHILD).is_some() {
            return true;
        }
        struct OwnedChild(Child);
        impl Drop for OwnedChild {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", name, "--nocapture"])
                .env(CHILD, "1")
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match child.0.try_wait() {
                Ok(Some(status)) => {
                    assert!(status.success(), "memory callback proof failed: {status}");
                    break;
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                result => panic!("memory callback proof did not finish: {result:?}"),
            }
        }
        false
    }
    #[derive(Debug)]
    struct UserPanic(Arc<()>);
    impl std::fmt::Display for UserPanic {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("ordinary trap-valued panic")
        }
    }
    impl std::error::Error for UserPanic {}

    #[test]
    fn builtin_callbacks_allow_ready_work_and_return_growth() {
        if !subprocess(
            "backend::sys::entities::memory::callback_tests::builtin_callbacks_allow_ready_work_and_return_growth",
        ) {
            return;
        }
        for imported in [false, true] {
            let (mut store, instance, state) = setup(imported);
            HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(ready_work)));
            let call = instance
                .exports
                .get_typed_function::<(), i32>(&store, "run")
                .unwrap();
            assert_eq!(call.call(&mut store).unwrap(), 42);
            assert_eq!(state.calls.load(Ordering::SeqCst), 1);
            assert!(state.hidden.load(Ordering::SeqCst));
            assert_eq!(state.continued.load(Ordering::SeqCst), 1);
            assert_eq!(continuation(&instance, &store), 1);
            assert!(!wasmer_vm::libcalls::memory_grow_active());
            assert!(StoreContext::is_empty());
            let fresh = instance
                .exports
                .get_typed_function::<(), i32>(&store, "fresh")
                .unwrap();
            assert_eq!(fresh.call(&mut store).unwrap(), 42);
        }
    }

    #[test]
    fn builtin_callbacks_hide_store_and_preserve_ordinary_panics() {
        if !subprocess(
            "backend::sys::entities::memory::callback_tests::builtin_callbacks_hide_store_and_preserve_ordinary_panics",
        ) {
            return;
        }
        for imported in [false, true] {
            for trap_value in [false, true] {
                let (mut store, instance, state) = setup(imported);
                let identity = Arc::new(());
                let payload = identity.clone();
                HOOK.with(|slot| {
                    *slot.borrow_mut() = Some(Box::new(move || {
                        ready_work();
                        if trap_value {
                            panic_any(wasmer_vm::Trap::user(Box::new(UserPanic(payload))));
                        } else {
                            panic_any(payload);
                        }
                    }))
                });
                let call = instance
                    .exports
                    .get_typed_function::<(), i32>(&store, "run")
                    .unwrap();
                let panic = catch_unwind(AssertUnwindSafe(|| call.call(&mut store))).unwrap_err();
                if trap_value {
                    let trap = *panic.downcast::<wasmer_vm::Trap>().unwrap();
                    let original = trap.downcast::<UserPanic>().unwrap();
                    assert!(Arc::ptr_eq(&original.0, &identity));
                } else {
                    assert!(Arc::ptr_eq(
                        &panic.downcast::<Arc<()>>().unwrap(),
                        &identity
                    ));
                }
                assert_eq!(state.calls.load(Ordering::SeqCst), 1);
                assert!(state.hidden.load(Ordering::SeqCst));
                assert_eq!(state.continued.load(Ordering::SeqCst), 0);
                assert_eq!(continuation(&instance, &store), 0);
                assert!(!wasmer_vm::libcalls::memory_grow_active());
                assert!(StoreContext::is_empty());
                let fresh = instance
                    .exports
                    .get_typed_function::<(), i32>(&store, "fresh")
                    .unwrap();
                assert_eq!(fresh.call(&mut store).unwrap(), 42);
            }
        }
    }

    #[cfg(feature = "experimental-async")]
    #[test]
    fn retired_builtin_drops_results_hidden_before_abort_or_panic_transport() {
        if !subprocess(
            "backend::sys::entities::memory::callback_tests::retired_builtin_drops_results_hidden_before_abort_or_panic_transport",
        ) {
            return;
        }
        use crate::{
            AsStoreMut,
            sys::{NativeInvocationScope, WeakNativeInvocation},
        };
        use std::{
            future::Future,
            rc::Rc,
            task::{Context, Poll},
        };
        for imported in [false, true] {
            for drop_panics in [false, true] {
                let (mut store, instance, state) = setup(imported);
                let scope = Rc::new(RefCell::new(
                    NativeInvocationScope::bind(&mut store).unwrap(),
                ));
                let root_scope = scope.clone();
                let expected = Rc::new(RefCell::new(None));
                let expected_abort = expected.clone();
                state.discard_result.store(true, Ordering::SeqCst);
                state.discard_panics.store(drop_panics, Ordering::SeqCst);
                let payload = state.discard_payload.clone();
                HOOK.with(|slot| {
                    *slot.borrow_mut() = Some(Box::new(move || {
                        ready_work();
                        let root = WeakNativeInvocation::current().unwrap();
                        root_scope.borrow().close();
                        *expected_abort.borrow_mut() = root.retirement_error();
                    }))
                });
                let function = instance.exports.get_function("run").unwrap().clone();
                let fresh = instance.exports.get_function("fresh").unwrap().clone();
                let store = store.into_async();
                let mut call = Box::pin(function.call_async(&store, vec![]));
                let waker = futures::task::noop_waker();
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    call.as_mut().poll(&mut Context::from_waker(&waker))
                }));
                drop(call);
                let expected = expected.borrow_mut().take().unwrap();
                if drop_panics {
                    let panic = outcome.unwrap_err().downcast::<Arc<()>>().unwrap();
                    assert!(Arc::ptr_eq(&panic, &payload));
                } else {
                    let Poll::Ready(Err(error)) = outcome.unwrap() else {
                        panic!("retired builtin did not abort");
                    };
                    assert!(error.is_host_abort());
                    assert!(std::ptr::eq(
                        error.downcast_ref::<std::io::Error>().unwrap(),
                        expected.downcast_ref::<std::io::Error>().unwrap()
                    ));
                }
                assert_eq!(state.discard_drops.load(Ordering::SeqCst), 1);
                assert_eq!(
                    state.discard_bad_context.load(Ordering::SeqCst),
                    0,
                    "discarded callback result regained the borrowed Store"
                );
                assert_eq!(state.calls.load(Ordering::SeqCst), 1);
                assert!(state.hidden.load(Ordering::SeqCst));
                assert_eq!(state.continued.load(Ordering::SeqCst), 0);
                {
                    let guard = store.store().write();
                    assert_eq!(continuation(&instance, &guard), 0);
                }
                assert!(!wasmer_vm::libcalls::memory_grow_active());
                assert!(StoreContext::is_empty());
                futures::executor::block_on(scope.borrow_mut().close_and_wait()).unwrap();
                let (_next_scope, next_store) = {
                    let mut guard = store.store().write();
                    scope
                        .borrow()
                        .successor(&store, &mut guard.as_store_mut())
                        .unwrap()
                };
                assert_eq!(
                    futures::executor::block_on(fresh.call_async(&next_store, vec![])).unwrap()[0]
                        .i32(),
                    Some(42)
                );
            }
        }
    }

    #[cfg(feature = "experimental-async")]
    #[test]
    fn cleanup_growth_during_unwind_preserves_the_original_panic() {
        if !subprocess(
            "backend::sys::entities::memory::callback_tests::cleanup_growth_during_unwind_preserves_the_original_panic",
        ) {
            return;
        }
        use crate::{AsStoreMut, sys::NativeInvocationScope};
        use std::{future::Future, rc::Rc, task::Context};

        struct CleanupGrowth {
            memory: CallbackMemory,
            succeeded: Arc<AtomicBool>,
        }
        impl Drop for CleanupGrowth {
            fn drop(&mut self) {
                // Catch the defective implementation's second abort locally:
                // the probe never deliberately starts a double-panic abort.
                match catch_unwind(AssertUnwindSafe(|| self.memory.grow(Pages(1)))) {
                    Ok(Ok(_)) => self.succeeded.store(true, Ordering::SeqCst),
                    Ok(Err(_)) => {}
                    Err(secondary) => {
                        let _ = catch_unwind(AssertUnwindSafe(|| drop(secondary)));
                    }
                }
            }
        }
        for imported in [false, true] {
            let (mut store, instance, state) = setup(imported);
            let scope = Rc::new(RefCell::new(
                NativeInvocationScope::bind(&mut store).unwrap(),
            ));
            let root_scope = scope.clone();
            let payload = Arc::new(());
            let original = payload.clone();
            let succeeded = Arc::new(AtomicBool::new(false));
            let cleanup_state = Arc::new(State {
                armed: AtomicBool::new(true),
                calls: AtomicUsize::new(0),
                hidden: AtomicBool::new(false),
                continued: AtomicUsize::new(0),
                discard_result: AtomicBool::new(false),
                discard_drops: AtomicUsize::new(0),
                discard_bad_context: AtomicUsize::new(0),
                discard_panics: AtomicBool::new(false),
                discard_payload: Arc::new(()),
            });
            let cleanup = CleanupGrowth {
                memory: CallbackMemory {
                    inner: VMMemory::new(
                        &MemoryType::new(1, Some(3), false),
                        &MemoryStyle::Dynamic {
                            offset_guard_size: 0,
                        },
                    )
                    .unwrap(),
                    state: cleanup_state.clone(),
                },
                succeeded: succeeded.clone(),
            };
            HOOK.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move || {
                    let _cleanup = cleanup;
                    root_scope.borrow().close();
                    panic_any(original);
                }));
            });
            let function = instance.exports.get_function("run").unwrap().clone();
            let fresh = instance.exports.get_function("fresh").unwrap().clone();
            let store = store.into_async();
            let mut call = Box::pin(function.call_async(&store, vec![]));
            let waker = futures::task::noop_waker();
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                call.as_mut().poll(&mut Context::from_waker(&waker))
            }));
            drop(call);
            let panic = outcome.unwrap_err().downcast::<Arc<()>>().unwrap();
            assert!(Arc::ptr_eq(&panic, &payload));
            assert!(
                succeeded.load(Ordering::SeqCst),
                "cleanup growth started a second managed abort"
            );
            assert_eq!(cleanup_state.calls.load(Ordering::SeqCst), 1);
            assert!(cleanup_state.hidden.load(Ordering::SeqCst));
            assert_eq!(state.continued.load(Ordering::SeqCst), 0);
            {
                let guard = store.store().write();
                assert_eq!(continuation(&instance, &guard), 0);
            }
            assert!(!wasmer_vm::libcalls::memory_grow_active());
            assert!(StoreContext::is_empty());
            futures::executor::block_on(scope.borrow_mut().close_and_wait()).unwrap();
            let (_next_scope, next_store) = {
                let mut guard = store.store().write();
                scope
                    .borrow()
                    .successor(&store, &mut guard.as_store_mut())
                    .unwrap()
            };
            assert_eq!(
                futures::executor::block_on(fresh.call_async(&next_store, vec![])).unwrap()[0]
                    .i32(),
                Some(42)
            );
        }
    }

    #[cfg(feature = "experimental-async")]
    #[test]
    fn host_future_body_sync_growth_traps_before_guest_continuation() {
        if !subprocess(
            "backend::sys::entities::memory::callback_tests::host_future_body_sync_growth_traps_before_guest_continuation",
        ) {
            return;
        }
        use crate::{
            AsStoreMut, FunctionType, RuntimeError, StoreAsync, Type, TypedFunction, Value,
            sys::{NativeInvocationScope, WeakNativeInvocation},
        };
        use std::{
            future::Future,
            rc::Rc,
            task::{Context, Poll},
        };
        thread_local! {
            static BODY_STORE: RefCell<Option<StoreAsync>> = const { RefCell::new(None) };
            static BODY_GROW: RefCell<Option<TypedFunction<(), i32>>> = const { RefCell::new(None) };
            static BODY_ROOT: RefCell<Option<WeakNativeInvocation>> = const { RefCell::new(None) };
        }
        for imported in [false, true] {
            let (mut store, instance, state) = setup(imported);
            let scope = Rc::new(RefCell::new(
                NativeInvocationScope::bind(&mut store).unwrap(),
            ));
            let ready = Arc::new(AtomicBool::new(false));
            let body_ready = ready.clone();
            let body =
                Function::new_async(&mut store, FunctionType::new([], [Type::I32]), move |_| {
                    BODY_ROOT.with(|slot| *slot.borrow_mut() = WeakNativeInvocation::current());
                    let store = BODY_STORE.with(|slot| slot.borrow().as_ref().unwrap().store());
                    let grow = BODY_GROW.with(|slot| slot.borrow().as_ref().unwrap().clone());
                    let ready = body_ready.clone();
                    async move {
                        std::future::poll_fn(|_| {
                            if ready.load(Ordering::SeqCst) {
                                Poll::Ready(())
                            } else {
                                Poll::Pending
                            }
                        })
                        .await;
                        // This is the external body under RetirementBoundary(A),
                        // with no physical native coroutine. A's Store is released.
                        assert!(WeakNativeInvocation::current().is_none());
                        let mut guard = store.store().write();
                        let _caught = grow.call(&mut guard);
                        // Deliberately catch any sync-call error and return success.
                        // The root's abort must remain exact and the direct Wasm
                        // store following memory.grow must never have executed.
                        Ok::<_, RuntimeError>(vec![Value::I32(42)])
                    }
                });
            let grow = instance
                .exports
                .get_typed_function::<(), i32>(&store, "run")
                .unwrap();
            let fresh = instance.exports.get_function("fresh").unwrap().clone();
            let store = store.into_async();
            BODY_STORE.with(|slot| *slot.borrow_mut() = Some(store.store()));
            BODY_GROW.with(|slot| *slot.borrow_mut() = Some(grow));
            let root_scope = scope.clone();
            let expected = Rc::new(RefCell::new(None));
            let expected_abort = expected.clone();
            HOOK.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move || {
                    assert!(WeakNativeInvocation::current().is_none());
                    ready_work();
                    root_scope.borrow().close();
                    *expected_abort.borrow_mut() =
                        BODY_ROOT.with(|slot| slot.borrow().as_ref().unwrap().retirement_error());
                }))
            });
            let mut call = Box::pin(body.call_async(&store, vec![]));
            let waker = futures::task::noop_waker();
            assert!(
                call.as_mut()
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending()
            );
            ready.store(true, Ordering::SeqCst);
            let outcome = call.as_mut().poll(&mut Context::from_waker(&waker));
            drop(call);
            drop(BODY_STORE.with(|slot| slot.borrow_mut().take()));
            BODY_GROW.with(|slot| {
                slot.borrow_mut().take();
            });
            BODY_ROOT.with(|slot| {
                slot.borrow_mut().take();
            });
            let Poll::Ready(Err(error)) = outcome else {
                panic!("logical-body root did not preserve abort");
            };
            let expected = expected.borrow_mut().take().unwrap();
            assert!(error.is_host_abort());
            assert!(std::ptr::eq(
                error.downcast_ref::<std::io::Error>().unwrap(),
                expected.downcast_ref::<std::io::Error>().unwrap()
            ));
            assert!(state.hidden.load(Ordering::SeqCst));
            assert_eq!(state.calls.load(Ordering::SeqCst), 1);
            assert_eq!(state.continued.load(Ordering::SeqCst), 0);
            {
                let guard = store.store().write();
                assert_eq!(
                    continuation(&instance, &guard),
                    0,
                    "logical-body abort allowed guest code after memory.grow"
                );
            }
            assert!(StoreContext::is_empty());
            assert!(!wasmer_vm::libcalls::memory_grow_active());
            futures::executor::block_on(scope.borrow_mut().close_and_wait()).unwrap();
            let (_next_scope, next_store) = {
                let mut guard = store.store().write();
                scope
                    .borrow()
                    .successor(&store, &mut guard.as_store_mut())
                    .unwrap()
            };
            assert_eq!(
                futures::executor::block_on(fresh.call_async(&next_store, vec![])).unwrap()[0]
                    .i32(),
                Some(42)
            );
        }
    }
}
