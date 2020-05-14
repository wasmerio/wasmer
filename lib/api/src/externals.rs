use crate::exports::{ExportError, Exportable};
use crate::memory_view::MemoryView;
use crate::store::{Store, StoreObject};
use crate::types::{Val, ValAnyFunc};
use crate::Mutability;
use crate::RuntimeError;
use crate::{ExternType, FunctionType, GlobalType, MemoryType, TableType, ValType};
use std::cmp::max;
use std::slice;
use wasm_common::{HostFunction, Pages, ValueType, WasmTypeList, WithEnv, WithoutEnv};
use wasmer_runtime::{
    wasmer_call_trampoline, Export, ExportFunction, ExportGlobal, ExportMemory, ExportTable,
    LinearMemory, MemoryError, Table as RuntimeTable, VMCallerCheckedAnyfunc, VMContext,
    VMFunctionBody, VMGlobalDefinition, VMMemoryDefinition, VMTrampoline,
};

#[derive(Clone)]
pub enum Extern {
    Function(Function),
    Global(Global),
    Table(Table),
    Memory(Memory),
}

impl Extern {
    pub fn ty(&self) -> ExternType {
        match self {
            Extern::Function(ft) => ExternType::Function(ft.ty().clone()),
            Extern::Memory(ft) => ExternType::Memory(ft.ty().clone()),
            Extern::Table(tt) => ExternType::Table(tt.ty().clone()),
            Extern::Global(gt) => ExternType::Global(gt.ty().clone()),
        }
    }

    pub(crate) fn from_export(store: &Store, export: Export) -> Extern {
        match export {
            Export::Function(f) => Extern::Function(Function::from_export(store, f)),
            Export::Memory(m) => Extern::Memory(Memory::from_export(store, m)),
            Export::Global(g) => Extern::Global(Global::from_export(store, g)),
            Export::Table(t) => Extern::Table(Table::from_export(store, t)),
        }
    }
}

impl<'a> Exportable<'a> for Extern {
    fn to_export(&self) -> Export {
        match self {
            Extern::Function(f) => f.to_export(),
            Extern::Global(g) => g.to_export(),
            Extern::Memory(m) => m.to_export(),
            Extern::Table(t) => t.to_export(),
        }
    }

    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        // Since this is already an extern, we can just return it.
        Ok(_extern)
    }
}

impl StoreObject for Extern {
    fn comes_from_same_store(&self, store: &Store) -> bool {
        let my_store = match self {
            Extern::Function(f) => f.store(),
            Extern::Global(g) => g.store(),
            Extern::Memory(m) => m.store(),
            Extern::Table(t) => t.store(),
        };
        Store::same(my_store, store)
    }
}

impl From<Function> for Extern {
    fn from(r: Function) -> Self {
        Extern::Function(r)
    }
}

impl From<Global> for Extern {
    fn from(r: Global) -> Self {
        Extern::Global(r)
    }
}

impl From<Memory> for Extern {
    fn from(r: Memory) -> Self {
        Extern::Memory(r)
    }
}

impl From<Table> for Extern {
    fn from(r: Table) -> Self {
        Extern::Table(r)
    }
}

#[derive(Clone)]
pub struct Global {
    store: Store,
    exported: ExportGlobal,
}

impl Global {
    pub fn new(store: &Store, val: Val) -> Global {
        // Note: we unwrap because the provided type should always match
        // the value type, so it's safe to unwrap.
        Self::from_type(store, GlobalType::new(val.ty(), Mutability::Const), val).unwrap()
    }

    pub fn new_mut(store: &Store, val: Val) -> Global {
        // Note: we unwrap because the provided type should always match
        // the value type, so it's safe to unwrap.
        Self::from_type(store, GlobalType::new(val.ty(), Mutability::Var), val).unwrap()
    }

    fn from_type(store: &Store, ty: GlobalType, val: Val) -> Result<Global, RuntimeError> {
        if !val.comes_from_same_store(store) {
            return Err(RuntimeError::new("cross-`Store` globals are not supported"));
        }
        let mut definition = VMGlobalDefinition::new();
        unsafe {
            match val {
                Val::I32(x) => *definition.as_i32_mut() = x,
                Val::I64(x) => *definition.as_i64_mut() = x,
                Val::F32(x) => *definition.as_f32_mut() = x,
                Val::F64(x) => *definition.as_f64_mut() = x,
                _ => return Err(RuntimeError::new(format!("create_global for {:?}", val))),
                // Val::V128(x) => *definition.as_u128_bits_mut() = x,
            }
        };
        let exported = ExportGlobal {
            definition: Box::leak(Box::new(definition)),
            global: ty,
        };
        Ok(Global {
            store: store.clone(),
            exported,
        })
    }

    pub fn ty(&self) -> &GlobalType {
        &self.exported.global
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn get(&self) -> Val {
        unsafe {
            let definition = &mut *self.exported.definition;
            match self.ty().ty {
                ValType::I32 => Val::from(*definition.as_i32()),
                ValType::I64 => Val::from(*definition.as_i64()),
                ValType::F32 => Val::F32(*definition.as_f32()),
                ValType::F64 => Val::F64(*definition.as_f64()),
                _ => unimplemented!("Global::get for {:?}", self.ty().ty),
            }
        }
    }

    pub fn set(&self, val: Val) -> Result<(), RuntimeError> {
        if self.ty().mutability != Mutability::Var {
            return Err(RuntimeError::new(format!("immutable global cannot be set")));
        }
        if val.ty() != self.ty().ty {
            return Err(RuntimeError::new(format!(
                "global of type {:?} cannot be set to {:?}",
                self.ty().ty,
                val.ty()
            )));
        }
        if !val.comes_from_same_store(&self.store) {
            return Err(RuntimeError::new("cross-`Store` values are not supported"));
        }
        unsafe {
            let definition = &mut *self.exported.definition;
            match val {
                Val::I32(i) => *definition.as_i32_mut() = i,
                Val::I64(i) => *definition.as_i64_mut() = i,
                Val::F32(f) => *definition.as_f32_mut() = f,
                Val::F64(f) => *definition.as_f64_mut() = f,
                _ => unimplemented!("Global::set for {:?}", val.ty()),
            }
        }
        Ok(())
    }

    pub(crate) fn from_export(store: &Store, wasmer_export: ExportGlobal) -> Global {
        Global {
            store: store.clone(),
            exported: wasmer_export,
        }
    }
}

impl<'a> Exportable<'a> for Global {
    fn to_export(&self) -> Export {
        self.exported.clone().into()
    }

    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Global(global) => Ok(global),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}

#[derive(Clone)]
pub struct Table {
    store: Store,
    // If the Table is owned by the Store, not the instance
    owned_by_store: bool,
    exported: ExportTable,
}

fn set_table_item(
    table: &RuntimeTable,
    item_index: u32,
    item: VMCallerCheckedAnyfunc,
) -> Result<(), RuntimeError> {
    table.set(item_index, item).map_err(|e| e.into())
}

impl Table {
    pub fn new(store: &Store, ty: TableType, init: Val) -> Result<Table, RuntimeError> {
        let item = init.into_checked_anyfunc(store)?;
        let tunables = store.engine().tunables();
        let table_plan = tunables.table_plan(ty);
        let table = tunables
            .create_table(table_plan)
            .map_err(RuntimeError::new)?;

        let definition = table.vmtable();
        for i in 0..definition.current_elements {
            set_table_item(&table, i, item.clone())?;
        }

        Ok(Table {
            store: store.clone(),
            owned_by_store: true,
            exported: ExportTable {
                from: Box::leak(Box::new(table)),
                definition: Box::leak(Box::new(definition)),
            },
        })
    }

    fn table(&self) -> &RuntimeTable {
        unsafe { &*self.exported.from }
    }

    pub fn ty(&self) -> &TableType {
        &self.exported.plan().table
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn get(&self, index: u32) -> Option<Val> {
        let item = self.table().get(index)?;
        Some(ValAnyFunc::from_checked_anyfunc(item, &self.store))
    }

    pub fn set(&self, index: u32, val: Val) -> Result<(), RuntimeError> {
        let item = val.into_checked_anyfunc(&self.store)?;
        set_table_item(self.table(), index, item)
    }

    pub fn size(&self) -> u32 {
        self.table().size()
    }

    pub fn grow(&self, delta: u32, init: Val) -> Result<u32, RuntimeError> {
        let item = init.into_checked_anyfunc(&self.store)?;
        let table = self.table();
        if let Some(len) = table.grow(delta) {
            for i in 0..delta {
                let i = len - (delta - i);
                set_table_item(table, i, item.clone())?;
            }
            Ok(len)
        } else {
            Err(RuntimeError::new(format!(
                "failed to grow table by `{}`",
                delta
            )))
        }
    }

    pub fn copy(
        dst_table: &Table,
        dst_index: u32,
        src_table: &Table,
        src_index: u32,
        len: u32,
    ) -> Result<(), RuntimeError> {
        if !Store::same(&dst_table.store, &src_table.store) {
            return Err(RuntimeError::new(
                "cross-`Store` table copies are not supported",
            ));
        }
        RuntimeTable::copy(
            dst_table.table(),
            src_table.table(),
            dst_index,
            src_index,
            len,
        )
        .map_err(|e| RuntimeError::from_trap(e))?;
        Ok(())
    }

    pub(crate) fn from_export(store: &Store, wasmer_export: ExportTable) -> Table {
        Table {
            store: store.clone(),
            owned_by_store: false,
            exported: wasmer_export,
        }
    }
}

impl<'a> Exportable<'a> for Table {
    fn to_export(&self) -> Export {
        self.exported.clone().into()
    }
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Table(table) => Ok(table),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}

#[derive(Clone)]
pub struct Memory {
    store: Store,
    // If the Memory is owned by the Store, not the instance
    owned_by_store: bool,
    exported: ExportMemory,
}

impl Memory {
    pub fn new(store: &Store, ty: MemoryType) -> Result<Memory, MemoryError> {
        let tunables = store.engine().tunables();
        let memory_plan = tunables.memory_plan(ty);
        let memory = tunables.create_memory(memory_plan)?;

        let definition = memory.vmmemory();

        Ok(Memory {
            store: store.clone(),
            owned_by_store: true,
            exported: ExportMemory {
                from: Box::leak(Box::new(memory)),
                definition: Box::leak(Box::new(definition)),
            },
        })
    }

    fn definition(&self) -> VMMemoryDefinition {
        self.memory().vmmemory()
    }

    pub fn ty(&self) -> &MemoryType {
        &self.exported.plan().memory
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub unsafe fn data_unchecked(&self) -> &[u8] {
        self.data_unchecked_mut()
    }

    /// TODO: document this function, it's trivial to cause UB/break soundness with this
    /// method.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn data_unchecked_mut(&self) -> &mut [u8] {
        let definition = self.definition();
        slice::from_raw_parts_mut(definition.base, definition.current_length)
    }

    pub fn data_ptr(&self) -> *mut u8 {
        self.definition().base
    }

    pub fn data_size(&self) -> usize {
        self.definition().current_length
    }

    pub fn size(&self) -> Pages {
        self.memory().size()
    }

    fn memory(&self) -> &LinearMemory {
        unsafe { &*self.exported.from }
    }

    pub fn grow(&self, delta: Pages) -> Result<Pages, MemoryError> {
        self.memory().grow(delta)
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
            exported: wasmer_export.clone(),
        }
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

/// A function defined in the Wasm module
#[derive(Clone, PartialEq)]
pub struct WasmFunc {
    // The trampoline to do the call
    trampoline: VMTrampoline,
}

/// A function defined in the Host
#[derive(Clone, PartialEq)]
pub struct HostFunc {
    // func: wasm_common::Func<Args, Rets>,
}

/// The inner helper
#[derive(Clone, PartialEq)]
pub enum InnerFunc {
    /// A function defined in the Wasm side
    Wasm(WasmFunc),
    /// A function defined in the Host side
    Host(HostFunc),
}

/// A WebAssembly `function`.
#[derive(Clone, PartialEq)]
pub struct Function {
    store: Store,
    // If the Function is owned by the Store, not the instance
    inner: InnerFunc,
    owned_by_store: bool,
    exported: ExportFunction,
}

impl Function {
    /// Creates a new `Func` with the given parameters.
    ///
    /// * `store` - a global cache to store information in
    /// * `func` - the function.
    pub fn new<F, Args, Rets, Env>(store: &Store, func: F) -> Self
    where
        F: HostFunction<Args, Rets, WithoutEnv, Env>,
        Args: WasmTypeList,
        Rets: WasmTypeList,
        Env: Sized,
    {
        let func: wasm_common::Func<Args, Rets, Env> = wasm_common::Func::new(func);
        let address = func.address() as *const VMFunctionBody;
        let vmctx = (func.env().unwrap_or(std::ptr::null_mut()) as *mut _) as *mut VMContext;
        let func_type = func.ty();
        let signature = store.engine().register_signature(&func_type);
        Self {
            store: store.clone(),
            owned_by_store: true,
            inner: InnerFunc::Host(HostFunc {
                // func
            }),
            exported: ExportFunction {
                address,
                vmctx,
                signature,
            },
        }
    }

    pub fn new_dynamic<F>(store: &Store, ty: &FunctionType, func: F) -> Self
    where
        F: Fn(&[Val], &mut [Val]) -> Result<(), RuntimeError> + 'static,
    {
        use std::panic::{self, AssertUnwindSafe};
        // The `DynamicCtx` holds the function type as well
        // as the function that we will be calling in the wrapper.
        struct DynamicCtx {
            ty: FunctionType,
            func: Box<dyn Fn(&[Val], &mut [Val]) -> Result<(), RuntimeError> + 'static>,
        }
        // This function wraps our func, to make it compatible with the
        // reverse trampoline signature
        unsafe fn func_wrapper(
            dynamic_ctx: &DynamicCtx,
            _caller_vmctx: *mut VMContext,
            values_vec: *mut i128,
        ) {
            let result = panic::catch_unwind(AssertUnwindSafe(|| {
                let mut args = Vec::with_capacity(dynamic_ctx.ty.params().len());
                for (i, ty) in dynamic_ctx.ty.params().iter().enumerate() {
                    args.push(Val::read_value_from(values_vec.add(i), *ty));
                }
                let mut returns = vec![Val::null(); dynamic_ctx.ty.results().len()];
                (*dynamic_ctx.func)(&args, &mut returns)
                // println!("CALLED {:?}", args);
            }));

            match result {
                Ok(Ok(())) => {}
                Ok(Err(trap)) => {
                    wasmer_runtime::raise_user_trap(Box::new(trap))
                }
                Err(panic) => wasmer_runtime::resume_panic(panic),
            }
        };
        let dynamic_ctx = DynamicCtx {
            ty: ty.clone(),
            func: Box::new(func),
        };
        let address = store
            .engine()
            .reverse_trampoline(&ty, func_wrapper as *const () as usize);
        // let vmctx = std::ptr::null_mut() as *mut _ as *mut VMContext;
        let vmctx = Box::leak(Box::new(dynamic_ctx)) as *mut _ as *mut VMContext;
        let signature = store.engine().register_signature(&ty);
        Self {
            store: store.clone(),
            owned_by_store: true,
            inner: InnerFunc::Host(HostFunc {}),
            exported: ExportFunction {
                address,
                vmctx,
                signature,
            },
        }
    }

    /// Creates a new `Func` with the given parameters.
    ///
    /// * `store` - a global cache to store information in.
    /// * `env` - the function environment.
    /// * `func` - the function.
    pub fn new_env<F, Args, Rets, Env>(store: &Store, env: &mut Env, func: F) -> Self
    where
        F: HostFunction<Args, Rets, WithEnv, Env>,
        Args: WasmTypeList,
        Rets: WasmTypeList,
        Env: Sized,
    {
        let func: wasm_common::Func<Args, Rets, Env> = wasm_common::Func::new_env(env, func);
        let address = func.address() as *const VMFunctionBody;
        let vmctx = (func.env().unwrap_or(std::ptr::null_mut()) as *mut _) as *mut VMContext;
        let func_type = func.ty();
        let signature = store.engine().register_signature(&func_type);
        Self {
            store: store.clone(),
            owned_by_store: true,
            inner: InnerFunc::Host(HostFunc {
                // func
            }),
            exported: ExportFunction {
                address,
                vmctx,
                signature,
            },
        }
    }

    /// Returns the underlying type of this function.
    pub fn ty(&self) -> FunctionType {
        self.store
            .engine()
            .lookup_signature(self.exported.signature)
            .expect("missing signature")
        // self.inner.unwrap().ty()
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    fn call_wasm(
        &self,
        func: &WasmFunc,
        params: &[Val],
        results: &mut [Val],
    ) -> Result<(), RuntimeError> {
        let format_types_for_error_message = |items: &[Val]| {
            items
                .iter()
                .map(|param| param.ty().to_string())
                .collect::<Vec<String>>()
                .join(", ")
        };
        let signature = self.ty();
        if signature.params().len() != params.len() {
            return Err(RuntimeError::new(format!(
                "expected {} arguments, got {}: Parameters of type [{}] did not match signature {}",
                signature.params().len(),
                params.len(),
                format_types_for_error_message(params),
                &signature
            )));
        }
        if signature.results().len() != results.len() {
            return Err(RuntimeError::new(format!(
                "expected {} results, got {}: Results of type [{}] did not match signature {}",
                signature.results().len(),
                results.len(),
                format_types_for_error_message(results),
                &signature,
            )));
        }

        let mut values_vec = vec![0; max(params.len(), results.len())];

        // Store the argument values into `values_vec`.
        let param_tys = signature.params().iter();
        for ((arg, slot), ty) in params.iter().zip(&mut values_vec).zip(param_tys) {
            if arg.ty() != ty.clone() {
                let param_types = format_types_for_error_message(params);
                return Err(RuntimeError::new(format!(
                    "Parameters of type [{}] did not match signature {}",
                    param_types, &signature,
                )));
            }
            unsafe {
                arg.write_value_to(slot);
            }
        }

        // Call the trampoline.
        if let Err(error) = unsafe {
            wasmer_call_trampoline(
                self.exported.vmctx,
                std::ptr::null_mut(),
                func.trampoline,
                self.exported.address,
                values_vec.as_mut_ptr() as *mut u8,
            )
        } {
            return Err(RuntimeError::from_trap(error));
        }

        // Load the return values out of `values_vec`.
        for (index, value_type) in signature.results().iter().enumerate() {
            unsafe {
                let ptr = values_vec.as_ptr().add(index);
                results[index] = Val::read_value_from(ptr, value_type.clone());
            }
        }

        Ok(())
    }

    /// Returns the number of parameters that this function takes.
    pub fn param_arity(&self) -> usize {
        self.ty().params().len()
    }

    /// Returns the number of results this function produces.
    pub fn result_arity(&self) -> usize {
        self.ty().results().len()
    }

    /// Call the [`Function`] function.
    ///
    /// Depending on where the Function is defined, it will call it.
    /// 1. If the function is defined inside a WebAssembly, it will call the trampoline
    ///    for the function signature.
    /// 2. If the function is defined in the host (in a native way), it will
    ///    call the trampoline.
    pub fn call(&self, params: &[Val]) -> Result<Box<[Val]>, RuntimeError> {
        let mut results = vec![Val::null(); self.result_arity()];
        match &self.inner {
            InnerFunc::Wasm(wasm) => {
                self.call_wasm(&wasm, params, &mut results)?;
            }
            _ => {} // _ => unimplemented!("The host is unimplemented"),
        }
        Ok(results.into_boxed_slice())
    }

    pub(crate) fn from_export(store: &Store, wasmer_export: ExportFunction) -> Self {
        let trampoline = store.engine().trampoline(wasmer_export.signature).unwrap();
        Self {
            store: store.clone(),
            owned_by_store: false,
            inner: InnerFunc::Wasm(WasmFunc { trampoline }),
            exported: wasmer_export,
        }
    }

    pub(crate) fn checked_anyfunc(&self) -> VMCallerCheckedAnyfunc {
        VMCallerCheckedAnyfunc {
            func_ptr: self.exported.address,
            type_index: self.exported.signature,
            vmctx: self.exported.vmctx,
        }
    }
}

impl<'a> Exportable<'a> for Function {
    fn to_export(&self) -> Export {
        self.exported.clone().into()
    }
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Function(func) => Ok(func),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}

impl std::fmt::Debug for Function {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}
