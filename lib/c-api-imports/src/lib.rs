use anyhow::{Context, Result, bail};
use std::{
    collections::{HashMap, VecDeque},
    fmt::Display,
    mem::size_of,
    num::NonZeroI32,
    ptr, slice,
    sync::{Arc, Mutex},
};

use wasmer_api::{
    Extern, ExternRef, ExternType, Function, Function as WasmerFunction, FunctionEnv,
    FunctionEnvMut, FunctionType, Global, GlobalType, Imports, Instance, Memory, Memory32,
    MemoryType, Module, Mutability, Pages, RuntimeError, StoreMut, Table, TableType, Type,
    TypedFunction, Value, WasmPtr, namespace,
};

/// Import module name used for host-provided WebAssembly C API bindings.
pub const WASM_C_API_MODULE_NAME: &str = "wasm_c_api_v0";
const WASM_C_API_MODULE_PREFIX: &str = "wasm_c_api_v";

const INVALID_HANDLE: i32 = 0;
const BOOL_FALSE: i32 = 0;
const BOOL_TRUE: i32 = 1;
const INVALID_KIND: i32 = -1;
const INVALID_SIZE: i32 = -1;
const WASM_VEC_SIZE: usize = 8;
const WASM_VEC_DATA_OFFSET: i32 = 4;

/// Version of the host-provided WebAssembly C API import namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmCAPIVersion {
    /// `wasm_c_api_v0`.
    V0,
    /// A namespace with the `wasm_c_api_v` prefix but no supported version.
    Unknown,
}

impl Display for WasmCAPIVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WasmCAPIVersion::V0 => write!(f, "wasm_c_api_v0"),
            WasmCAPIVersion::Unknown => write!(f, "wasm_c_api_unknown"),
        }
    }
}

impl WasmCAPIVersion {
    /// Returns whether `other` can be satisfied by `self`.
    pub const fn is_compatible_with(self, other: Self) -> bool {
        matches!((self, other), (Self::V0, Self::V0))
    }
}

/// Returns the host-provided WebAssembly C API version imported by a module.
pub fn module_wasm_c_api_version_used(module: &Module) -> Option<WasmCAPIVersion> {
    let mut version = None;

    for import in module.imports() {
        let Some(detected_version) = wasm_c_api_version_from_namespace(import.module()) else {
            continue;
        };

        version = Some(match version {
            None => detected_version,
            Some(existing) if existing == detected_version => existing,
            Some(_) => WasmCAPIVersion::Unknown,
        });
    }

    version
}

fn wasm_c_api_version_from_namespace(namespace: &str) -> Option<WasmCAPIVersion> {
    if namespace == WASM_C_API_MODULE_NAME {
        return Some(WasmCAPIVersion::V0);
    }

    let suffix = namespace.strip_prefix(WASM_C_API_MODULE_PREFIX)?;
    Some(match suffix {
        "0" => WasmCAPIVersion::V0,
        _ => WasmCAPIVersion::Unknown,
    })
}

type ResolveModuleSync = Arc<dyn Fn(Vec<u8>) -> Result<Module> + Send + Sync>;

struct WasmCapiSession {
    version: Option<WasmCAPIVersion>,
    imported_memory_type: Option<MemoryType>,
    imported_table_type: Option<wasmer_api::TableType>,
    resolve_module_sync: Option<ResolveModuleSync>,
    func_env: Mutex<Option<FunctionEnv<WasmCapiEnv>>>,
}

impl WasmCapiSession {
    fn new(module: &Module, resolve_module_sync: Option<ResolveModuleSync>) -> Self {
        let imported_memory_type = module.imports().find_map(|import| {
            if import.module() == "env"
                && import.name() == "memory"
                && let ExternType::Memory(ty) = import.ty()
            {
                return Some(*ty);
            }
            None
        });

        let imported_table_type = module.imports().find_map(|import| {
            if import.module() == "env"
                && import.name() == "__indirect_function_table"
                && let ExternType::Table(ty) = import.ty()
            {
                return Some(*ty);
            }
            None
        });

        Self {
            version: module_wasm_c_api_version_used(module),
            imported_memory_type,
            imported_table_type,
            resolve_module_sync,
            func_env: Mutex::new(None),
        }
    }

    fn needs_imports(&self) -> bool {
        self.version.is_some()
    }

    fn validate_supported(&self) -> Result<()> {
        if let Some(version) = self.version
            && !WasmCAPIVersion::V0.is_compatible_with(version)
        {
            bail!("unsupported Wasm C API import version: {version:?}");
        }
        Ok(())
    }

    fn register_imports(&self, store: &mut StoreMut<'_>, io: &mut Imports) -> Result<()> {
        self.validate_supported()?;
        if self.version.is_none() {
            return Ok(());
        }

        let func_env = FunctionEnv::new(
            &mut *store,
            WasmCapiEnv {
                resolve_module_sync: self.resolve_module_sync.clone(),
                ..WasmCapiEnv::default()
            },
        );
        *self
            .func_env
            .lock()
            .expect("poisoned WasmCapiSession mutex") = Some(func_env.clone());
        register_wasm_c_api_imports(store, &func_env, io);

        if let Some(memory_type) = self.imported_memory_type {
            if let Some(existing) = io.get_export("env", "memory") {
                let Extern::Memory(memory) = existing else {
                    bail!("env.memory import for Wasm C API module is not a memory");
                };
                self.set_memory(store, &memory)?;
            } else {
                let memory = Memory::new(&mut *store, memory_type)?;
                io.define("env", "memory", memory.clone());
                self.set_memory(store, &memory)?;
            }
        }

        if let Some(table_type) = self.imported_table_type {
            if let Some(existing) = io.get_export("env", "__indirect_function_table") {
                let Extern::Table(table) = existing else {
                    bail!(
                        "env.__indirect_function_table import for Wasm C API module is not a table"
                    );
                };
                self.set_table(store, &table)?;
            } else {
                let table = Table::new(&mut *store, table_type, Value::FuncRef(None))?;
                io.define("env", "__indirect_function_table", table.clone());
                self.set_table(store, &table)?;
            }
        }

        Ok(())
    }

    fn set_memory(&self, store: &mut StoreMut<'_>, memory: &Memory) -> Result<()> {
        let Some(func_env) = self.func_env()? else {
            return Ok(());
        };
        func_env.as_mut(&mut *store).memory = Some(memory.clone());
        Ok(())
    }

    fn set_table(&self, store: &mut StoreMut<'_>, table: &Table) -> Result<()> {
        let Some(func_env) = self.func_env()? else {
            return Ok(());
        };
        func_env.as_mut(&mut *store).table = Some(table.clone());
        Ok(())
    }

    fn configure_instance(
        &self,
        store: &mut StoreMut<'_>,
        instance: &Instance,
        imported_memory: Option<&Memory>,
    ) -> Result<()> {
        let Some(func_env) = self.func_env()? else {
            return Ok(());
        };

        if let Some(memory) = imported_memory {
            func_env.as_mut(&mut *store).memory = Some(memory.clone());
        } else if let Ok(memory) = instance.exports.get_memory("memory") {
            func_env.as_mut(&mut *store).memory = Some(memory.clone());
        }

        for export_name in ["unofficial_napi_guest_malloc", "malloc"] {
            if let Ok(malloc) = instance
                .exports
                .get_typed_function::<i32, i32>(&store, export_name)
            {
                func_env.as_mut(&mut *store).malloc_fn = Some(malloc);
                break;
            }
        }

        for export_name in ["unofficial_napi_guest_free", "free"] {
            if let Ok(free) = instance
                .exports
                .get_typed_function::<i32, ()>(&store, export_name)
            {
                func_env.as_mut(&mut *store).free_fn = Some(free);
                break;
            }
        }

        if let Ok(table) = instance.exports.get_table("__indirect_function_table") {
            func_env.as_mut(&mut *store).table = Some(table.clone());
        }

        Ok(())
    }

    fn func_env(&self) -> Result<Option<FunctionEnv<WasmCapiEnv>>> {
        if self.version.is_none() {
            return Ok(None);
        }

        self.func_env
            .lock()
            .expect("poisoned WasmCapiSession mutex")
            .clone()
            .context("missing Wasm C API function env during instance setup")
            .map(Some)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ModuleKey(String);

impl ModuleKey {
    fn new(module: &Module) -> Self {
        Self(module.info().id.id())
    }
}

/// Runtime hooks that provide `wasm_c_api_v0` imports for WASIX guests.
#[derive(Clone, Default)]
pub struct WasmCapiRuntimeHooks {
    /// Pending per-instantiation sessions, grouped by compiled module.
    ///
    /// `add_imports` creates host functions before instantiation, while
    /// `configure_instance` can only discover exports like guest malloc/free
    /// after instantiation. The queue preserves that pairing when the same
    /// module is instantiated multiple times.
    sessions: Arc<Mutex<HashMap<ModuleKey, VecDeque<WasmCapiSession>>>>,
    resolve_module_sync: Option<ResolveModuleSync>,
}

impl WasmCapiRuntimeHooks {
    /// Creates an empty hook set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Uses an embedder-provided resolver for `wasm_module_new`.
    pub fn with_resolve_module_sync(
        mut self,
        resolve: impl Fn(Vec<u8>) -> Result<Module> + Send + Sync + 'static,
    ) -> Self {
        self.resolve_module_sync = Some(Arc::new(resolve));
        self
    }

    fn module_key(module: &Module) -> ModuleKey {
        ModuleKey::new(module)
    }

    /// Creates `wasm_c_api_v0` imports when `module` requests them.
    pub fn additional_imports(&self, module: &Module, store: &mut StoreMut<'_>) -> Result<Imports> {
        let mut imports = Imports::new();
        self.add_imports(module, store, &mut imports)?;
        Ok(imports)
    }

    /// Merges `wasm_c_api_v0` imports into an existing import object when needed.
    pub fn add_imports(
        &self,
        module: &Module,
        store: &mut StoreMut<'_>,
        imports: &mut Imports,
    ) -> Result<()> {
        let session = WasmCapiSession::new(module, self.resolve_module_sync.clone());
        if !session.needs_imports() {
            return Ok(());
        }

        session.register_imports(store, imports)?;
        let mut sessions = self
            .sessions
            .lock()
            .expect("poisoned WasmCapiRuntimeHooks session queue");
        sessions
            .entry(Self::module_key(module))
            .or_default()
            .push_back(session);
        Ok(())
    }

    /// Completes memory, table, and guest allocation wiring after instantiation.
    pub fn configure_instance(
        &self,
        module: &Module,
        store: &mut StoreMut<'_>,
        instance: &Instance,
        imported_memory: Option<&Memory>,
    ) -> Result<()> {
        if module_wasm_c_api_version_used(module).is_none() {
            return Ok(());
        }

        let session = {
            let mut sessions = self
                .sessions
                .lock()
                .expect("poisoned WasmCapiRuntimeHooks session queue");
            let key = Self::module_key(module);
            let Some(queue) = sessions.get_mut(&key) else {
                bail!("missing pending Wasm C API session for module instance setup");
            };
            let session = queue
                .pop_front()
                .context("missing queued Wasm C API session for module instance setup")?;
            if queue.is_empty() {
                sessions.remove(&key);
            }
            session
        };

        session.configure_instance(store, instance, imported_memory)
    }
}

/// Per-instance state captured by every imported `wasm_c_api_v0` function.
///
/// Handles in `state` refer to host-side Wasmer objects. Pointers passed over
/// the ABI always refer to the guest module memory in `memory`.
#[derive(Default)]
struct WasmCapiEnv {
    /// Optional embedder-provided module loader used by `wasm_module_new`.
    resolve_module_sync: Option<ResolveModuleSync>,
    /// Guest linear memory used for C ABI structs and buffers.
    memory: Option<Memory>,
    /// Guest allocator used when the host must return C API-owned buffers.
    malloc_fn: Option<TypedFunction<i32, i32>>,
    /// Optional guest deallocator for replacing host-created guest buffers.
    free_fn: Option<TypedFunction<i32, ()>>,
    /// Guest indirect function table used for wasm_func_new_with_env callbacks.
    table: Option<Table>,
    /// Host-side object and shadow-memory handles visible to the guest as i32s.
    state: WasmCapiState,
    /// Self-reference needed when creating host functions that call guest callbacks.
    func_env: Option<FunctionEnv<WasmCapiEnv>>,
}

// ABI discriminants and layout constants copied from the WebAssembly C API
// headers (`wasm.h`).
const WASM_I32: u8 = 0;
const WASM_I64: u8 = 1;
const WASM_F32: u8 = 2;
const WASM_F64: u8 = 3;
const WASM_EXTERNREF: u8 = 128;
const WASM_FUNCREF: u8 = 129;

const WASM_EXTERN_FUNC: i32 = 0;
const WASM_EXTERN_GLOBAL: i32 = 1;
const WASM_EXTERN_TABLE: i32 = 2;
const WASM_EXTERN_MEMORY: i32 = 3;

const WASM_CONST: i32 = 0;
const WASM_VAR: i32 = 1;

// `wasm_val_t` stores the one-byte kind first, then C ABI padding, then the
// eight-byte payload union at byte offset 8.
const WASM_VAL_SIZE: usize = 16;
const WASM_VAL_PAYLOAD_OFFSET: i32 = 8;

#[derive(Clone)]
enum WasmExtern {
    Func(Function),
    Global(Global),
    Table(Table),
    Memory(Memory),
}

#[derive(Clone)]
enum WasmObject {
    Engine,
    Store,
    Module(Module),
    Instance(Instance),
    Func(Function),
    FuncType(FunctionType),
    ValType(Type),
    ExternType(ExternType),
    ImportType {
        module: String,
        name: String,
        ty: ExternType,
    },
    ExportType {
        name: String,
        ty: ExternType,
    },
    Extern(WasmExtern),
    Memory(Memory),
    MemoryType(MemoryType),
    Global(Global),
    GlobalType(GlobalType),
    Table(Table),
    TableType(TableType),
    /// A boxed reference value (`externref`/`funcref`) exposed to the guest as a
    /// `wasm_ref_t*` handle.
    Ref(Value),
    Trap(String),
}

/// A guest-visible shadow of a Wasmer memory data pointer.
#[derive(Clone, Copy)]
struct MemoryShadow {
    /// Guest allocation returned from `wasm_memory_data`.
    guest_ptr: i32,
    /// Number of memory bytes mirrored into `guest_ptr`.
    len: usize,
}

/// Host-side storage for opaque WebAssembly C API handles.
///
/// The guest sees pointers from `wasm.h` as positive i32 handles. Handles are
/// never reused during one instance lifetime, and invalid/unknown handles
/// become lookup failures that callers map to the C API's null/false result.
struct WasmCapiState {
    /// Next positive i32 handle to expose to the guest.
    next_handle: i32,
    /// Opaque objects addressed by guest-visible handles.
    objects: HashMap<i32, WasmObject>,
    /// Guest shadow allocations for `wasm_memory_data`, keyed by memory handle.
    memory_shadows: HashMap<i32, MemoryShadow>,
}

impl Default for WasmCapiState {
    fn default() -> Self {
        Self {
            next_handle: 1,
            objects: HashMap::new(),
            memory_shadows: HashMap::new(),
        }
    }
}

impl WasmCapiState {
    fn insert(&mut self, object: WasmObject) -> i32 {
        let handle = self.next_handle;
        if handle <= INVALID_HANDLE {
            return INVALID_HANDLE;
        }

        self.next_handle = handle.checked_add(1).unwrap_or(INVALID_HANDLE);
        self.objects.insert(handle, object);
        handle
    }

    fn get(&self, handle: i32) -> Option<&WasmObject> {
        if handle <= INVALID_HANDLE {
            return None;
        }
        self.objects.get(&handle)
    }

    fn remove(&mut self, handle: i32) -> Option<MemoryShadow> {
        if handle > INVALID_HANDLE {
            self.objects.remove(&handle);
            return self.memory_shadows.remove(&handle);
        }
        None
    }
}

fn guest_memory_offset(guest_ptr: i32) -> Option<u64> {
    u64::try_from(guest_ptr).ok()
}

fn guest_byte_ptr(guest_ptr: i32) -> Option<WasmPtr<u8, Memory32>> {
    Some(WasmPtr::new(u32::try_from(guest_ptr).ok()?))
}

fn checked_memory_offset(guest_ptr: i32, len: usize, data_size: u64) -> Option<usize> {
    let offset = usize::try_from(guest_ptr).ok()?;
    let end = offset.checked_add(len)?;
    (u64::try_from(end).ok()? <= data_size).then_some(offset)
}

fn non_null_guest_ptr(guest_ptr: i32) -> Option<NonZeroI32> {
    let ptr = NonZeroI32::new(guest_ptr)?;
    (ptr.get() > INVALID_HANDLE).then_some(ptr)
}

fn write_guest_bytes(env: &mut FunctionEnvMut<WasmCapiEnv>, guest_ptr: i32, data: &[u8]) -> bool {
    let Some(ptr) = guest_byte_ptr(guest_ptr) else {
        return false;
    };
    let Ok(len) = u32::try_from(data.len()) else {
        return false;
    };
    let (state, store) = env.data_and_store_mut();
    let Some(memory) = state.memory.clone() else {
        return false;
    };
    let view = memory.view(&store);
    ptr.slice(&view, len)
        .and_then(|slice| slice.write_slice(data))
        .is_ok()
}

fn write_guest_u32(env: &mut FunctionEnvMut<WasmCapiEnv>, guest_ptr: i32, val: u32) -> bool {
    write_guest_bytes(env, guest_ptr, &val.to_le_bytes())
}

fn write_guest_i32(env: &mut FunctionEnvMut<WasmCapiEnv>, guest_ptr: i32, val: i32) -> bool {
    write_guest_bytes(env, guest_ptr, &val.to_le_bytes())
}

fn write_guest_i32_offset(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    guest_ptr: i32,
    offset: i32,
    val: i32,
) -> bool {
    let Some(ptr) = guest_ptr.checked_add(offset) else {
        return false;
    };
    write_guest_i32(env, ptr, val)
}

fn write_guest_bytes_offset(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    guest_ptr: i32,
    offset: i32,
    data: &[u8],
) -> bool {
    let Some(ptr) = guest_ptr.checked_add(offset) else {
        return false;
    };
    write_guest_bytes(env, ptr, data)
}

fn guest_ptr_with_offset(guest_ptr: i32, offset: i32) -> Option<i32> {
    guest_ptr
        .checked_add(offset)
        .filter(|ptr| *ptr >= INVALID_HANDLE)
}

fn read_guest_bytes(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    guest_ptr: i32,
    len: usize,
) -> Option<Vec<u8>> {
    let ptr = guest_byte_ptr(guest_ptr)?;
    let len = u32::try_from(len).ok()?;
    if len == 0 {
        return Some(Vec::new());
    }

    let (state, store) = env.data_and_store_mut();
    let memory = state.memory.clone()?;
    let view = memory.view(&store);

    ptr.slice(&view, len).ok()?.read_to_vec().ok()
}

fn read_guest_array<const N: usize>(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    guest_ptr: i32,
) -> Option<[u8; N]> {
    let ptr = guest_byte_ptr(guest_ptr)?;
    let len = u32::try_from(N).ok()?;
    let (state, store) = env.data_and_store_mut();
    let memory = state.memory.clone()?;
    let mut bytes = [0u8; N];
    let view = memory.view(&store);
    ptr.slice(&view, len).ok()?.read_slice(&mut bytes).ok()?;
    Some(bytes)
}

fn read_guest_i32_vec(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    guest_ptr: i32,
    len: usize,
) -> Option<Vec<i32>> {
    let offset = guest_memory_offset(guest_ptr)?;
    if len == 0 {
        return Some(Vec::new());
    }
    let byte_len = len.checked_mul(size_of::<i32>())?;

    let (state, store) = env.data_and_store_mut();
    let memory = state.memory.clone()?;
    let view = memory.view(&store);

    let mut out = Vec::<i32>::with_capacity(len);
    unsafe {
        let spare = out.spare_capacity_mut();
        let bytes = slice::from_raw_parts_mut(spare.as_mut_ptr().cast::<u8>(), byte_len);
        view.read(offset, bytes).ok()?;
        out.set_len(len);
    }

    // Guest ABI vectors are little-endian, matching the write path's
    // `to_le_bytes` encoding. This is a no-op on little-endian hosts.
    for value in &mut out {
        *value = i32::from_le(*value);
    }

    Some(out)
}

fn write_guest_i32_slice(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    guest_ptr: i32,
    values: &[i32],
) -> bool {
    let Some(byte_len) = values.len().checked_mul(size_of::<i32>()) else {
        return false;
    };
    if byte_len == 0 {
        return true;
    }

    for (index, value) in values.iter().enumerate() {
        let Ok(index) = i32::try_from(index) else {
            return false;
        };
        let Some(offset) = index.checked_mul(size_of::<i32>() as i32) else {
            return false;
        };
        if !write_guest_bytes_offset(env, guest_ptr, offset, &value.to_le_bytes()) {
            return false;
        }
    }
    true
}

fn allocate_guest_memory(env: &mut FunctionEnvMut<WasmCapiEnv>, len: usize) -> Option<i32> {
    if len == 0 {
        return Some(0);
    }

    let malloc_fn = env.data().malloc_fn.clone()?;
    let len = i32::try_from(len).ok()?;
    let guest_ptr: i32 = {
        let (_, mut store_ref) = env.data_and_store_mut();
        malloc_fn.call(&mut store_ref, len).ok()?
    };
    if guest_ptr <= INVALID_HANDLE {
        return None;
    }
    Some(guest_ptr)
}

fn free_guest_memory(env: &mut FunctionEnvMut<WasmCapiEnv>, guest_ptr: i32) {
    if guest_ptr <= INVALID_HANDLE {
        return;
    }

    let Some(free_fn) = env.data().free_fn.clone() else {
        return;
    };
    let (_, mut store_ref) = env.data_and_store_mut();
    let _ = free_fn.call(&mut store_ref, guest_ptr);
}

struct GuestAllocation<'env, 'store> {
    env: &'env mut FunctionEnvMut<'store, WasmCapiEnv>,
    ptr: i32,
}

impl<'env, 'store> GuestAllocation<'env, 'store> {
    fn new(env: &'env mut FunctionEnvMut<'store, WasmCapiEnv>, len: usize) -> Option<Self> {
        let ptr = allocate_guest_memory(env, len)?;
        non_null_guest_ptr(ptr)?;
        Some(Self { env, ptr })
    }

    fn write_bytes(&mut self, data: &[u8]) -> bool {
        write_guest_bytes(self.env, self.ptr, data)
    }

    fn write_i32_slice(&mut self, values: &[i32]) -> bool {
        write_guest_i32_slice(self.env, self.ptr, values)
    }

    fn write_wasm_val_at(&mut self, offset: usize, value: &Value) -> bool {
        let Ok(offset) = i32::try_from(offset) else {
            return false;
        };
        let Some(ptr) = self.ptr.checked_add(offset) else {
            return false;
        };
        write_wasm_val(self.env, ptr, value)
    }

    fn write_vec_header(&mut self, vec_ptr: NonZeroI32, len: u32) -> bool {
        write_guest_vec_header(self.env, vec_ptr.get(), len, self.ptr)
    }

    fn write_self_vec_header(&mut self, len: u32, data_ptr: i32) -> bool {
        write_guest_vec_header(self.env, self.ptr, len, data_ptr)
    }

    fn write_handles(&mut self, handles: &[i32]) -> bool {
        write_handle_vec(self.env, self.ptr, handles)
    }

    fn allocate_vec_header(&mut self, len: u32) -> Option<i32> {
        allocate_guest_vec_header(self.env, len, self.ptr)
    }

    fn copy_from_wasmer_memory(&mut self, memory: &Memory, len: usize) -> bool {
        copy_wasmer_memory_to_guest(self.env, memory, self.ptr, len)
    }

    fn into_raw(mut self) -> i32 {
        let ptr = self.ptr;
        self.ptr = INVALID_HANDLE;
        ptr
    }
}

impl Drop for GuestAllocation<'_, '_> {
    fn drop(&mut self) {
        if self.ptr != INVALID_HANDLE {
            free_guest_memory(self.env, self.ptr);
        }
    }
}

fn read_u32(env: &mut FunctionEnvMut<WasmCapiEnv>, ptr: i32) -> Option<u32> {
    Some(u32::from_le_bytes(read_guest_array(env, ptr)?))
}

fn read_i32(env: &mut FunctionEnvMut<WasmCapiEnv>, ptr: i32) -> Option<i32> {
    Some(read_u32(env, ptr)? as i32)
}

fn read_u64(env: &mut FunctionEnvMut<WasmCapiEnv>, ptr: i32) -> Option<u64> {
    Some(u64::from_le_bytes(read_guest_array(env, ptr)?))
}

fn write_guest_vec_header(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    vec_ptr: i32,
    len: u32,
    data_ptr: i32,
) -> bool {
    write_guest_u32(env, vec_ptr, len)
        && write_guest_i32_offset(env, vec_ptr, WASM_VEC_DATA_OFFSET, data_ptr)
}

fn allocate_guest_vec_header(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    len: u32,
    data_ptr: i32,
) -> Option<i32> {
    let mut allocation = GuestAllocation::new(env, WASM_VEC_SIZE)?;
    if !allocation.write_self_vec_header(len, data_ptr) {
        return None;
    }
    Some(allocation.into_raw())
}

fn read_byte_vec(env: &mut FunctionEnvMut<WasmCapiEnv>, vec_ptr: i32) -> Option<Vec<u8>> {
    if vec_ptr <= INVALID_HANDLE {
        return None;
    }
    let size = read_u32(env, vec_ptr)? as usize;
    let data_ptr = read_i32(env, guest_ptr_with_offset(vec_ptr, WASM_VEC_DATA_OFFSET)?)?;
    if size == 0 {
        return Some(Vec::new());
    }
    read_guest_bytes(env, data_ptr, size)
}

fn write_byte_vec(env: &mut FunctionEnvMut<WasmCapiEnv>, vec_ptr: i32, bytes: &[u8]) -> bool {
    let Some(vec_ptr) = non_null_guest_ptr(vec_ptr) else {
        return false;
    };
    let Ok(len) = u32::try_from(bytes.len()) else {
        return false;
    };

    if bytes.is_empty() {
        return write_guest_vec_header(env, vec_ptr.get(), len, 0);
    }

    let Some(mut data) = GuestAllocation::new(env, bytes.len()) else {
        return false;
    };
    if !data.write_bytes(bytes) || !data.write_vec_header(vec_ptr, len) {
        return false;
    }
    let _ = data.into_raw();
    true
}

fn allocate_name(env: &mut FunctionEnvMut<WasmCapiEnv>, name: &str) -> i32 {
    let Ok(len) = u32::try_from(name.len()) else {
        return INVALID_HANDLE;
    };

    if name.is_empty() {
        return allocate_guest_vec_header(env, len, 0).unwrap_or(INVALID_HANDLE);
    }

    let Some(mut data) = GuestAllocation::new(env, name.len()) else {
        return INVALID_HANDLE;
    };
    if !data.write_bytes(name.as_bytes()) {
        return INVALID_HANDLE;
    }
    let Some(vec_ptr) = data.allocate_vec_header(len) else {
        return INVALID_HANDLE;
    };
    let _ = data.into_raw();
    vec_ptr
}

fn write_handle_vec(env: &mut FunctionEnvMut<WasmCapiEnv>, out_ptr: i32, handles: &[i32]) -> bool {
    let Some(out_ptr) = non_null_guest_ptr(out_ptr) else {
        return false;
    };
    let Ok(len) = u32::try_from(handles.len()) else {
        return false;
    };
    let Some(byte_len) = handles.len().checked_mul(size_of::<i32>()) else {
        return false;
    };

    if handles.is_empty() {
        return write_guest_vec_header(env, out_ptr.get(), len, 0);
    }

    let Some(mut data) = GuestAllocation::new(env, byte_len) else {
        return false;
    };
    if !data.write_i32_slice(handles) || !data.write_vec_header(out_ptr, len) {
        return false;
    }
    let _ = data.into_raw();
    true
}

fn read_handle_vec(env: &mut FunctionEnvMut<WasmCapiEnv>, vec_ptr: i32) -> Option<Vec<i32>> {
    if vec_ptr <= INVALID_HANDLE {
        return None;
    }
    let size = read_u32(env, vec_ptr)? as usize;
    let data_ptr = read_i32(env, guest_ptr_with_offset(vec_ptr, WASM_VEC_DATA_OFFSET)?)?;
    if size == 0 {
        return Some(Vec::new());
    }
    read_guest_i32_vec(env, data_ptr, size)
}

fn type_to_wasm_kind(ty: Type) -> Option<u8> {
    Some(match ty {
        Type::I32 => WASM_I32,
        Type::I64 => WASM_I64,
        Type::F32 => WASM_F32,
        Type::F64 => WASM_F64,
        Type::FuncRef => WASM_FUNCREF,
        Type::ExternRef => WASM_EXTERNREF,
        Type::V128 | Type::ExceptionRef => return None,
    })
}

fn wasm_kind_to_type(kind: i32) -> Option<Type> {
    match kind as u8 {
        WASM_I32 => Some(Type::I32),
        WASM_I64 => Some(Type::I64),
        WASM_F32 => Some(Type::F32),
        WASM_F64 => Some(Type::F64),
        WASM_FUNCREF => Some(Type::FuncRef),
        WASM_EXTERNREF => Some(Type::ExternRef),
        _ => None,
    }
}

fn extern_kind(ty: &ExternType) -> i32 {
    match ty {
        ExternType::Function(_) => WASM_EXTERN_FUNC,
        ExternType::Global(_) => WASM_EXTERN_GLOBAL,
        ExternType::Table(_) => WASM_EXTERN_TABLE,
        ExternType::Memory(_) => WASM_EXTERN_MEMORY,
        ExternType::Tag(_) => INVALID_KIND,
    }
}

fn read_wasm_val(env: &mut FunctionEnvMut<WasmCapiEnv>, val_ptr: i32, ty: Type) -> Option<Value> {
    match ty {
        Type::I32 => Some(Value::I32(read_i32(
            env,
            val_ptr + WASM_VAL_PAYLOAD_OFFSET,
        )?)),
        Type::I64 => Some(Value::I64(
            read_u64(env, val_ptr + WASM_VAL_PAYLOAD_OFFSET)? as i64,
        )),
        Type::F32 => {
            let raw = read_u32(env, val_ptr + WASM_VAL_PAYLOAD_OFFSET)?;
            Some(Value::F32(f32::from_bits(raw)))
        }
        Type::F64 => {
            let raw = read_u64(env, val_ptr + WASM_VAL_PAYLOAD_OFFSET)?;
            Some(Value::F64(f64::from_bits(raw)))
        }
        // The guest stores a `wasm_ref_t*` as an i32 handle in the payload.
        Type::FuncRef | Type::ExternRef => {
            let handle = read_i32(env, val_ptr + WASM_VAL_PAYLOAD_OFFSET)?;
            Some(ref_value_from_handle(env, handle, ty))
        }
        Type::V128 | Type::ExceptionRef => None,
    }
}

/// Resolve a guest `wasm_ref_t*` handle into a reference [`Value`] of the
/// expected reference kind. A null handle (`0`) is the null reference.
fn ref_value_from_handle(env: &FunctionEnvMut<WasmCapiEnv>, handle: i32, ty: Type) -> Value {
    let null = match ty {
        Type::FuncRef => Value::FuncRef(None),
        _ => Value::ExternRef(None),
    };
    if handle <= INVALID_HANDLE {
        return null;
    }
    match env.data().state.get(handle) {
        Some(WasmObject::Ref(value)) => value.clone(),
        // A funcref may also be handed over as a `wasm_func_t*` handle.
        Some(WasmObject::Func(f)) | Some(WasmObject::Extern(WasmExtern::Func(f))) => {
            Value::FuncRef(Some(f.clone()))
        }
        _ => null,
    }
}

/// Materialize a reference [`Value`] into a guest `wasm_ref_t*` handle, or `0`
/// for the null reference.
fn ref_value_to_handle(env: &mut FunctionEnvMut<WasmCapiEnv>, value: &Value) -> i32 {
    match value {
        Value::ExternRef(None) | Value::FuncRef(None) => INVALID_HANDLE,
        Value::ExternRef(Some(_)) | Value::FuncRef(Some(_)) => {
            insert(env, WasmObject::Ref(value.clone()))
        }
        _ => INVALID_HANDLE,
    }
}

fn write_wasm_val(env: &mut FunctionEnvMut<WasmCapiEnv>, val_ptr: i32, value: &Value) -> bool {
    let Some(val_ptr) = non_null_guest_ptr(val_ptr) else {
        return false;
    };
    let val_ptr = val_ptr.get();
    let Some(kind) = type_to_wasm_kind(value.ty()) else {
        return false;
    };
    // For references, mint the guest handle before writing (mutates state).
    let ref_handle = matches!(value, Value::FuncRef(_) | Value::ExternRef(_))
        .then(|| ref_value_to_handle(env, value));
    if !write_guest_bytes(env, val_ptr, &[kind]) {
        return false;
    }
    match value {
        Value::I32(v) => {
            write_guest_bytes_offset(env, val_ptr, WASM_VAL_PAYLOAD_OFFSET, &v.to_le_bytes())
        }
        Value::I64(v) => {
            write_guest_bytes_offset(env, val_ptr, WASM_VAL_PAYLOAD_OFFSET, &v.to_le_bytes())
        }
        Value::F32(v) => write_guest_bytes_offset(
            env,
            val_ptr,
            WASM_VAL_PAYLOAD_OFFSET,
            &v.to_bits().to_le_bytes(),
        ),
        Value::F64(v) => write_guest_bytes_offset(
            env,
            val_ptr,
            WASM_VAL_PAYLOAD_OFFSET,
            &v.to_bits().to_le_bytes(),
        ),
        Value::FuncRef(_) | Value::ExternRef(_) => write_guest_bytes_offset(
            env,
            val_ptr,
            WASM_VAL_PAYLOAD_OFFSET,
            &ref_handle.unwrap_or(INVALID_HANDLE).to_le_bytes(),
        ),
        Value::V128(_) | Value::ExceptionRef(_) => false,
    }
}

fn read_limits(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    limits_ptr: i32,
) -> Option<(u32, Option<u32>)> {
    if limits_ptr <= INVALID_HANDLE {
        return None;
    }
    let min = read_u32(env, limits_ptr)?;
    let max = read_u32(env, guest_ptr_with_offset(limits_ptr, 4)?)?;
    let max = if max == u32::MAX { None } else { Some(max) };
    Some((min, max))
}

fn clone_extern_from_handle(env: &FunctionEnvMut<WasmCapiEnv>, handle: i32) -> Option<Extern> {
    match env.data().state.get(handle)? {
        WasmObject::Extern(WasmExtern::Func(f)) => Some(Extern::Function(f.clone())),
        WasmObject::Extern(WasmExtern::Global(g)) => Some(Extern::Global(g.clone())),
        WasmObject::Extern(WasmExtern::Table(t)) => Some(Extern::Table(t.clone())),
        WasmObject::Extern(WasmExtern::Memory(m)) => Some(Extern::Memory(m.clone())),
        WasmObject::Func(f) => Some(Extern::Function(f.clone())),
        WasmObject::Global(g) => Some(Extern::Global(g.clone())),
        WasmObject::Table(t) => Some(Extern::Table(t.clone())),
        WasmObject::Memory(m) => Some(Extern::Memory(m.clone())),
        _ => None,
    }
}

fn insert(env: &mut FunctionEnvMut<WasmCapiEnv>, object: WasmObject) -> i32 {
    env.data_mut().state.insert(object)
}

fn delete_handle(mut env: FunctionEnvMut<WasmCapiEnv>, handle: i32) {
    if let Some(shadow) = env.data_mut().state.remove(handle) {
        free_guest_memory(&mut env, shadow.guest_ptr);
    }
}

fn wasm_engine_new(mut env: FunctionEnvMut<WasmCapiEnv>) -> i32 {
    insert(&mut env, WasmObject::Engine)
}

fn wasm_store_new(mut env: FunctionEnvMut<WasmCapiEnv>, _engine: i32) -> i32 {
    insert(&mut env, WasmObject::Store)
}

fn wasm_module_new(mut env: FunctionEnvMut<WasmCapiEnv>, _store: i32, bytes_ptr: i32) -> i32 {
    let Some(bytes) = read_byte_vec(&mut env, bytes_ptr) else {
        return INVALID_HANDLE;
    };

    let module = if let Some(resolve_module_sync) = env.data().resolve_module_sync.clone() {
        resolve_module_sync(bytes)
    } else {
        let (_, store) = env.data_and_store_mut();
        Module::new(&store, bytes).map_err(Into::into)
    };

    match module {
        Ok(module) => env.data_mut().state.insert(WasmObject::Module(module)),
        Err(_) => INVALID_HANDLE,
    }
}

fn wasm_module_validate(mut env: FunctionEnvMut<WasmCapiEnv>, _store: i32, bytes_ptr: i32) -> i32 {
    let Some(bytes) = read_byte_vec(&mut env, bytes_ptr) else {
        return BOOL_FALSE;
    };
    let (_, store) = env.data_and_store_mut();
    Module::validate(&store, &bytes).is_ok() as i32
}

fn wasm_module_imports(mut env: FunctionEnvMut<WasmCapiEnv>, module_handle: i32, out_ptr: i32) {
    let module = match env.data().state.get(module_handle) {
        Some(WasmObject::Module(module)) => module.clone(),
        _ => {
            write_handle_vec(&mut env, out_ptr, &[]);
            return;
        }
    };
    let handles: Vec<i32> = module
        .imports()
        .map(|import| {
            insert(
                &mut env,
                WasmObject::ImportType {
                    module: import.module().to_string(),
                    name: import.name().to_string(),
                    ty: import.ty().clone(),
                },
            )
        })
        .collect();
    write_handle_vec(&mut env, out_ptr, &handles);
}

fn wasm_module_exports(mut env: FunctionEnvMut<WasmCapiEnv>, module_handle: i32, out_ptr: i32) {
    let module = match env.data().state.get(module_handle) {
        Some(WasmObject::Module(module)) => module.clone(),
        _ => {
            write_handle_vec(&mut env, out_ptr, &[]);
            return;
        }
    };
    let handles: Vec<i32> = module
        .exports()
        .map(|export| {
            insert(
                &mut env,
                WasmObject::ExportType {
                    name: export.name().to_string(),
                    ty: export.ty().clone(),
                },
            )
        })
        .collect();
    write_handle_vec(&mut env, out_ptr, &handles);
}

fn wasm_importtype_module(mut env: FunctionEnvMut<WasmCapiEnv>, import_handle: i32) -> i32 {
    let name = match env.data().state.get(import_handle) {
        Some(WasmObject::ImportType { module, .. }) => module.clone(),
        _ => return INVALID_HANDLE,
    };
    allocate_name(&mut env, &name)
}

fn wasm_importtype_name(mut env: FunctionEnvMut<WasmCapiEnv>, import_handle: i32) -> i32 {
    let name = match env.data().state.get(import_handle) {
        Some(WasmObject::ImportType { name, .. }) => name.clone(),
        _ => return INVALID_HANDLE,
    };
    allocate_name(&mut env, &name)
}

fn wasm_importtype_type(mut env: FunctionEnvMut<WasmCapiEnv>, import_handle: i32) -> i32 {
    let ty = match env.data().state.get(import_handle) {
        Some(WasmObject::ImportType { ty, .. }) => ty.clone(),
        _ => return INVALID_HANDLE,
    };
    insert(&mut env, WasmObject::ExternType(ty))
}

fn wasm_exporttype_name(mut env: FunctionEnvMut<WasmCapiEnv>, export_handle: i32) -> i32 {
    let name = match env.data().state.get(export_handle) {
        Some(WasmObject::ExportType { name, .. }) => name.clone(),
        _ => return INVALID_HANDLE,
    };
    allocate_name(&mut env, &name)
}

fn wasm_exporttype_type(mut env: FunctionEnvMut<WasmCapiEnv>, export_handle: i32) -> i32 {
    let ty = match env.data().state.get(export_handle) {
        Some(WasmObject::ExportType { ty, .. }) => ty.clone(),
        _ => return INVALID_HANDLE,
    };
    insert(&mut env, WasmObject::ExternType(ty))
}

fn wasm_externtype_kind(env: FunctionEnvMut<WasmCapiEnv>, type_handle: i32) -> i32 {
    match env.data().state.get(type_handle) {
        Some(WasmObject::ExternType(ty)) => extern_kind(ty),
        _ => INVALID_KIND,
    }
}

fn wasm_externtype_as_functype_const(
    mut env: FunctionEnvMut<WasmCapiEnv>,
    type_handle: i32,
) -> i32 {
    let ty = match env.data().state.get(type_handle) {
        Some(WasmObject::ExternType(ExternType::Function(ty))) => ty.clone(),
        _ => return INVALID_HANDLE,
    };
    insert(&mut env, WasmObject::FuncType(ty))
}

fn wasm_functype_copy(mut env: FunctionEnvMut<WasmCapiEnv>, type_handle: i32) -> i32 {
    let ty = match env.data().state.get(type_handle) {
        Some(WasmObject::FuncType(ty)) => ty.clone(),
        _ => return INVALID_HANDLE,
    };
    insert(&mut env, WasmObject::FuncType(ty))
}

fn write_valtype_vec_for_types(env: &mut FunctionEnvMut<WasmCapiEnv>, types: &[Type]) -> i32 {
    let handles: Vec<i32> = types
        .iter()
        .map(|ty| insert(env, WasmObject::ValType(*ty)))
        .collect();
    let Some(mut allocation) = GuestAllocation::new(env, WASM_VEC_SIZE) else {
        return INVALID_HANDLE;
    };
    if !allocation.write_handles(&handles) {
        return INVALID_HANDLE;
    }
    allocation.into_raw()
}

fn wasm_functype_params(mut env: FunctionEnvMut<WasmCapiEnv>, type_handle: i32) -> i32 {
    let params = match env.data().state.get(type_handle) {
        Some(WasmObject::FuncType(ty)) => ty.params().to_vec(),
        _ => return INVALID_HANDLE,
    };
    write_valtype_vec_for_types(&mut env, &params)
}

fn wasm_functype_results(mut env: FunctionEnvMut<WasmCapiEnv>, type_handle: i32) -> i32 {
    let results = match env.data().state.get(type_handle) {
        Some(WasmObject::FuncType(ty)) => ty.results().to_vec(),
        _ => return INVALID_HANDLE,
    };
    write_valtype_vec_for_types(&mut env, &results)
}

fn wasm_valtype_new(mut env: FunctionEnvMut<WasmCapiEnv>, kind: i32) -> i32 {
    match wasm_kind_to_type(kind) {
        Some(ty) => insert(&mut env, WasmObject::ValType(ty)),
        None => INVALID_HANDLE,
    }
}

fn wasm_valtype_kind(env: FunctionEnvMut<WasmCapiEnv>, valtype_handle: i32) -> i32 {
    match env.data().state.get(valtype_handle) {
        Some(WasmObject::ValType(ty)) => type_to_wasm_kind(*ty)
            .map(i32::from)
            .unwrap_or(INVALID_KIND),
        _ => INVALID_KIND,
    }
}

fn wasm_memorytype_new(mut env: FunctionEnvMut<WasmCapiEnv>, limits_ptr: i32) -> i32 {
    let Some((min, max)) = read_limits(&mut env, limits_ptr) else {
        return INVALID_HANDLE;
    };
    insert(
        &mut env,
        WasmObject::MemoryType(MemoryType::new(Pages(min), max.map(Pages), false)),
    )
}

fn wasm_memory_new(mut env: FunctionEnvMut<WasmCapiEnv>, _store: i32, type_handle: i32) -> i32 {
    let ty = match env.data().state.get(type_handle) {
        Some(WasmObject::MemoryType(ty)) => *ty,
        _ => return INVALID_HANDLE,
    };
    match Memory::new(&mut env, ty) {
        Ok(memory) => insert(&mut env, WasmObject::Memory(memory)),
        Err(_) => INVALID_HANDLE,
    }
}

fn wasm_memory_size(env: FunctionEnvMut<WasmCapiEnv>, memory_handle: i32) -> i32 {
    let Some(memory) = memory_from_handle(&env, memory_handle) else {
        return INVALID_SIZE;
    };
    i32::try_from(memory.size(&env).0).unwrap_or(INVALID_SIZE)
}

fn wasm_memory_grow(mut env: FunctionEnvMut<WasmCapiEnv>, memory_handle: i32, delta: i32) -> i32 {
    let Ok(delta) = u32::try_from(delta) else {
        return BOOL_FALSE;
    };
    let memory = match env.data().state.get(memory_handle) {
        Some(WasmObject::Memory(memory)) => memory.clone(),
        Some(WasmObject::Extern(WasmExtern::Memory(memory))) => memory.clone(),
        _ => return BOOL_FALSE,
    };
    if memory.grow(&mut env, Pages(delta)).is_ok() {
        BOOL_TRUE
    } else {
        BOOL_FALSE
    }
}

fn memory_from_handle(env: &FunctionEnvMut<WasmCapiEnv>, memory_handle: i32) -> Option<Memory> {
    match env.data().state.get(memory_handle)? {
        WasmObject::Memory(memory) => Some(memory.clone()),
        WasmObject::Extern(WasmExtern::Memory(memory)) => Some(memory.clone()),
        _ => None,
    }
}

fn memory_supports_shadow(env: &FunctionEnvMut<WasmCapiEnv>, memory: &Memory) -> bool {
    // C-API-created memories are currently non-shared, but instance exports can
    // still expose shared memories. Guest shadow buffers cannot represent
    // concurrent writes coherently, so shadow APIs fail closed for those.
    !memory.ty(env).shared
}

fn copy_wasmer_memory_to_guest(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    memory: &Memory,
    guest_ptr: i32,
    len: usize,
) -> bool {
    if len == 0 {
        return true;
    }
    let Some(guest_memory) = env.data().memory.clone() else {
        return false;
    };
    let source_view = memory.view(&*env);
    let guest_view = guest_memory.view(&*env);
    let Some(source_offset) = checked_memory_offset(0, len, source_view.data_size()) else {
        return false;
    };
    let Some(guest_offset) = checked_memory_offset(guest_ptr, len, guest_view.data_size()) else {
        return false;
    };
    let source_base = source_view.data_ptr();
    let guest_base = guest_view.data_ptr();
    if ptr::eq(source_base, guest_base) {
        return false;
    }
    unsafe {
        // Both ranges are bounds-checked above and same-memory copies are rejected.
        ptr::copy_nonoverlapping(
            source_base.add(source_offset),
            guest_base.add(guest_offset),
            len,
        );
    }
    true
}

fn copy_guest_memory_to_wasmer(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    guest_ptr: i32,
    memory: &Memory,
    len: usize,
) -> bool {
    if len == 0 {
        return true;
    }
    let Some(guest_memory) = env.data().memory.clone() else {
        return false;
    };
    let guest_view = guest_memory.view(&*env);
    let target_view = memory.view(&*env);
    let Some(guest_offset) = checked_memory_offset(guest_ptr, len, guest_view.data_size()) else {
        return false;
    };
    let Some(target_offset) = checked_memory_offset(0, len, target_view.data_size()) else {
        return false;
    };
    let guest_base = guest_view.data_ptr();
    let target_base = target_view.data_ptr();
    if ptr::eq(guest_base, target_base) {
        return false;
    }
    unsafe {
        // Both ranges are bounds-checked above and same-memory copies are rejected.
        ptr::copy_nonoverlapping(
            guest_base.add(guest_offset),
            target_base.add(target_offset),
            len,
        );
    }
    true
}

fn wasm_memory_data_size(env: FunctionEnvMut<WasmCapiEnv>, memory_handle: i32) -> i32 {
    let Some(memory) = memory_from_handle(&env, memory_handle) else {
        return INVALID_SIZE;
    };
    if !memory_supports_shadow(&env, &memory) {
        return INVALID_SIZE;
    }
    i32::try_from(memory.view(&env).data_size()).unwrap_or(INVALID_SIZE)
}

fn wasm_memory_data(mut env: FunctionEnvMut<WasmCapiEnv>, memory_handle: i32) -> i32 {
    let Some(memory) = memory_from_handle(&env, memory_handle) else {
        return INVALID_HANDLE;
    };

    if !memory_supports_shadow(&env, &memory) {
        return INVALID_HANDLE;
    };
    let size = {
        let view = memory.view(&env);
        let Ok(size) = usize::try_from(view.data_size()) else {
            return INVALID_HANDLE;
        };
        size
    };
    if size == 0 {
        return INVALID_HANDLE;
    }

    let existing = env.data().state.memory_shadows.get(&memory_handle).copied();
    let shadow = match existing {
        // Size unchanged: the shadow is kept coherent by the sync/refresh
        // brackets around every wasm call and guest callback, and re-copying
        // here would DESTROY guest writes that have not been synced yet (the
        // guest may query the data pointer between writing to the shadow and
        // the call that flushes it).
        Some(existing) if existing.len >= size => MemoryShadow {
            guest_ptr: existing.guest_ptr,
            len: size,
        },
        _ => {
            let Some(mut allocation) = GuestAllocation::new(&mut env, size) else {
                return INVALID_HANDLE;
            };
            if !allocation.copy_from_wasmer_memory(&memory, size) {
                return INVALID_HANDLE;
            }
            let guest_ptr = allocation.into_raw();
            if let Some(existing) = existing {
                free_guest_memory(&mut env, existing.guest_ptr);
            }
            MemoryShadow {
                guest_ptr,
                len: size,
            }
        }
    };

    env.data_mut()
        .state
        .memory_shadows
        .insert(memory_handle, shadow);
    shadow.guest_ptr
}

fn sync_memory_shadows_to_wasmer(env: &mut FunctionEnvMut<WasmCapiEnv>) {
    let shadows: Vec<_> = env
        .data()
        .state
        .memory_shadows
        .iter()
        .map(|(&handle, &shadow)| (handle, shadow))
        .collect();

    for (handle, shadow) in shadows {
        let Some(memory) = memory_from_handle(env, handle) else {
            continue;
        };
        if !memory_supports_shadow(env, &memory) {
            continue;
        };
        let _ = copy_guest_memory_to_wasmer(env, shadow.guest_ptr, &memory, shadow.len);
    }
}

fn refresh_memory_shadows_from_wasmer(env: &mut FunctionEnvMut<WasmCapiEnv>) {
    let shadows: Vec<_> = env
        .data()
        .state
        .memory_shadows
        .iter()
        .map(|(&handle, &shadow)| (handle, shadow))
        .collect();

    for (handle, shadow) in shadows {
        let Some(memory) = memory_from_handle(env, handle) else {
            continue;
        };
        if !memory_supports_shadow(env, &memory) {
            continue;
        }
        let _ = copy_wasmer_memory_to_guest(env, &memory, shadow.guest_ptr, shadow.len);
    }
}

fn wasm_globaltype_new(
    mut env: FunctionEnvMut<WasmCapiEnv>,
    valtype_handle: i32,
    mutability: i32,
) -> i32 {
    let ty = match env.data().state.get(valtype_handle) {
        Some(WasmObject::ValType(ty)) => *ty,
        _ => return INVALID_HANDLE,
    };
    let mutability = if mutability == WASM_VAR {
        Mutability::Var
    } else {
        Mutability::Const
    };
    insert(
        &mut env,
        WasmObject::GlobalType(GlobalType::new(ty, mutability)),
    )
}

fn wasm_globaltype_content(mut env: FunctionEnvMut<WasmCapiEnv>, globaltype_handle: i32) -> i32 {
    let ty = match env.data().state.get(globaltype_handle) {
        Some(WasmObject::GlobalType(ty)) => ty.ty,
        _ => return INVALID_HANDLE,
    };
    insert(&mut env, WasmObject::ValType(ty))
}

fn wasm_globaltype_mutability(env: FunctionEnvMut<WasmCapiEnv>, globaltype_handle: i32) -> i32 {
    match env.data().state.get(globaltype_handle) {
        Some(WasmObject::GlobalType(ty)) if ty.mutability == Mutability::Var => WASM_VAR,
        Some(WasmObject::GlobalType(_)) => WASM_CONST,
        _ => WASM_CONST,
    }
}

fn wasm_global_new(
    mut env: FunctionEnvMut<WasmCapiEnv>,
    _store: i32,
    globaltype_handle: i32,
    val_ptr: i32,
) -> i32 {
    let ty = match env.data().state.get(globaltype_handle) {
        Some(WasmObject::GlobalType(ty)) => *ty,
        _ => return INVALID_HANDLE,
    };
    let Some(value) = read_wasm_val(&mut env, val_ptr, ty.ty) else {
        return INVALID_HANDLE;
    };
    let global = if ty.mutability == Mutability::Var {
        Global::new_mut(&mut env, value)
    } else {
        Global::new(&mut env, value)
    };
    insert(&mut env, WasmObject::Global(global))
}

fn global_from_handle(env: &FunctionEnvMut<WasmCapiEnv>, global_handle: i32) -> Option<Global> {
    match env.data().state.get(global_handle)? {
        WasmObject::Global(global) => Some(global.clone()),
        WasmObject::Extern(WasmExtern::Global(global)) => Some(global.clone()),
        _ => None,
    }
}

fn wasm_global_type(mut env: FunctionEnvMut<WasmCapiEnv>, global_handle: i32) -> i32 {
    let Some(global) = global_from_handle(&env, global_handle) else {
        return INVALID_HANDLE;
    };
    let ty = global.ty(&env);
    insert(&mut env, WasmObject::GlobalType(ty))
}

fn wasm_global_get(mut env: FunctionEnvMut<WasmCapiEnv>, global_handle: i32, out_ptr: i32) {
    let Some(global) = global_from_handle(&env, global_handle) else {
        return;
    };
    let value = global.get(&mut env);
    write_wasm_val(&mut env, out_ptr, &value);
}

fn wasm_global_set(mut env: FunctionEnvMut<WasmCapiEnv>, global_handle: i32, val_ptr: i32) {
    let Some(global) = global_from_handle(&env, global_handle) else {
        return;
    };
    let ty = global.ty(&env).ty;
    if let Some(value) = read_wasm_val(&mut env, val_ptr, ty) {
        let _ = global.set(&mut env, value);
    }
}

fn wasm_instance_new(
    mut env: FunctionEnvMut<WasmCapiEnv>,
    _store: i32,
    module_handle: i32,
    imports_vec_ptr: i32,
    trap_out_ptr: i32,
) -> i32 {
    if let Some(trap_out_ptr) = non_null_guest_ptr(trap_out_ptr) {
        write_guest_u32(&mut env, trap_out_ptr.get(), INVALID_HANDLE as u32);
    }
    let module = match env.data().state.get(module_handle) {
        Some(WasmObject::Module(module)) => module.clone(),
        _ => return INVALID_HANDLE,
    };

    let import_handles = read_handle_vec(&mut env, imports_vec_ptr).unwrap_or_default();
    let mut imports = Imports::new();
    for (import, handle) in module.imports().zip(import_handles.into_iter()) {
        let Some(ext) = clone_extern_from_handle(&env, handle) else {
            return INVALID_HANDLE;
        };
        imports.define(import.module(), import.name(), ext);
    }

    // Instantiation runs data-segment initialization and the start function,
    // which mutate memories a guest shadow may already mirror.
    let result = Instance::new(&mut env, &module, &imports);
    refresh_memory_shadows_from_wasmer(&mut env);
    match result {
        Ok(instance) => insert(&mut env, WasmObject::Instance(instance)),
        Err(err) => {
            if let Some(trap_out_ptr) = non_null_guest_ptr(trap_out_ptr) {
                let trap = insert(&mut env, WasmObject::Trap(err.to_string()));
                write_guest_u32(
                    &mut env,
                    trap_out_ptr.get(),
                    u32::try_from(trap).unwrap_or(INVALID_HANDLE as u32),
                );
            }
            INVALID_HANDLE
        }
    }
}

fn wasm_instance_exports(mut env: FunctionEnvMut<WasmCapiEnv>, instance_handle: i32, out_ptr: i32) {
    let instance = match env.data().state.get(instance_handle) {
        Some(WasmObject::Instance(instance)) => instance.clone(),
        _ => {
            write_handle_vec(&mut env, out_ptr, &[]);
            return;
        }
    };
    let handles: Vec<i32> = instance
        .exports
        .iter()
        .map(|(_, ext)| {
            let object = match ext {
                Extern::Function(func) => WasmObject::Extern(WasmExtern::Func(func.clone())),
                Extern::Global(global) => WasmObject::Extern(WasmExtern::Global(global.clone())),
                Extern::Table(table) => WasmObject::Extern(WasmExtern::Table(table.clone())),
                Extern::Memory(memory) => WasmObject::Extern(WasmExtern::Memory(memory.clone())),
                Extern::Tag(_) => WasmObject::Trap("unsupported tag export".to_string()),
            };
            insert(&mut env, object)
        })
        .collect();
    write_handle_vec(&mut env, out_ptr, &handles);
}

fn wasm_extern_kind(env: FunctionEnvMut<WasmCapiEnv>, extern_handle: i32) -> i32 {
    match env.data().state.get(extern_handle) {
        Some(WasmObject::Extern(WasmExtern::Func(_))) | Some(WasmObject::Func(_)) => {
            WASM_EXTERN_FUNC
        }
        Some(WasmObject::Extern(WasmExtern::Global(_))) | Some(WasmObject::Global(_)) => {
            WASM_EXTERN_GLOBAL
        }
        Some(WasmObject::Extern(WasmExtern::Table(_))) | Some(WasmObject::Table(_)) => {
            WASM_EXTERN_TABLE
        }
        Some(WasmObject::Extern(WasmExtern::Memory(_))) | Some(WasmObject::Memory(_)) => {
            WASM_EXTERN_MEMORY
        }
        _ => INVALID_KIND,
    }
}

fn wasm_extern_as_func(mut env: FunctionEnvMut<WasmCapiEnv>, extern_handle: i32) -> i32 {
    let object = match env.data().state.get(extern_handle) {
        Some(WasmObject::Extern(WasmExtern::Func(func))) => WasmObject::Func(func.clone()),
        _ => return INVALID_HANDLE,
    };
    insert(&mut env, object)
}

fn wasm_extern_as_global(mut env: FunctionEnvMut<WasmCapiEnv>, extern_handle: i32) -> i32 {
    let object = match env.data().state.get(extern_handle) {
        Some(WasmObject::Extern(WasmExtern::Global(global))) => WasmObject::Global(global.clone()),
        _ => return INVALID_HANDLE,
    };
    insert(&mut env, object)
}

fn wasm_extern_as_table(mut env: FunctionEnvMut<WasmCapiEnv>, extern_handle: i32) -> i32 {
    let object = match env.data().state.get(extern_handle) {
        Some(WasmObject::Extern(WasmExtern::Table(table))) => WasmObject::Table(table.clone()),
        _ => return INVALID_HANDLE,
    };
    insert(&mut env, object)
}

fn wasm_extern_as_memory(mut env: FunctionEnvMut<WasmCapiEnv>, extern_handle: i32) -> i32 {
    let object = match env.data().state.get(extern_handle) {
        Some(WasmObject::Extern(WasmExtern::Memory(memory))) => WasmObject::Memory(memory.clone()),
        _ => return INVALID_HANDLE,
    };
    insert(&mut env, object)
}

fn wasm_func_copy(mut env: FunctionEnvMut<WasmCapiEnv>, func_handle: i32) -> i32 {
    let object = match env.data().state.get(func_handle) {
        Some(WasmObject::Func(func)) => WasmObject::Func(func.clone()),
        Some(WasmObject::Extern(WasmExtern::Func(func))) => WasmObject::Func(func.clone()),
        _ => return INVALID_HANDLE,
    };
    insert(&mut env, object)
}

fn wasm_global_copy(mut env: FunctionEnvMut<WasmCapiEnv>, global_handle: i32) -> i32 {
    let Some(global) = global_from_handle(&env, global_handle) else {
        return INVALID_HANDLE;
    };
    insert(&mut env, WasmObject::Global(global))
}

fn wasm_memory_copy(mut env: FunctionEnvMut<WasmCapiEnv>, memory_handle: i32) -> i32 {
    let Some(memory) = memory_from_handle(&env, memory_handle) else {
        return INVALID_HANDLE;
    };
    insert(&mut env, WasmObject::Memory(memory))
}

fn wasm_table_copy(mut env: FunctionEnvMut<WasmCapiEnv>, table_handle: i32) -> i32 {
    let object = match env.data().state.get(table_handle) {
        Some(WasmObject::Table(table)) => WasmObject::Table(table.clone()),
        Some(WasmObject::Extern(WasmExtern::Table(table))) => WasmObject::Table(table.clone()),
        _ => return INVALID_HANDLE,
    };
    insert(&mut env, object)
}

fn wasm_func_as_extern(mut env: FunctionEnvMut<WasmCapiEnv>, func_handle: i32) -> i32 {
    let object = match env.data().state.get(func_handle) {
        Some(WasmObject::Func(func)) => WasmObject::Extern(WasmExtern::Func(func.clone())),
        _ => return INVALID_HANDLE,
    };
    insert(&mut env, object)
}

fn wasm_global_as_extern(mut env: FunctionEnvMut<WasmCapiEnv>, global_handle: i32) -> i32 {
    let Some(global) = global_from_handle(&env, global_handle) else {
        return INVALID_HANDLE;
    };
    insert(&mut env, WasmObject::Extern(WasmExtern::Global(global)))
}

fn wasm_memory_as_extern(mut env: FunctionEnvMut<WasmCapiEnv>, memory_handle: i32) -> i32 {
    let Some(memory) = memory_from_handle(&env, memory_handle) else {
        return INVALID_HANDLE;
    };
    insert(&mut env, WasmObject::Extern(WasmExtern::Memory(memory)))
}

fn wasm_table_as_extern(mut env: FunctionEnvMut<WasmCapiEnv>, table_handle: i32) -> i32 {
    let object = match env.data().state.get(table_handle) {
        Some(WasmObject::Table(table)) => WasmObject::Extern(WasmExtern::Table(table.clone())),
        _ => return INVALID_HANDLE,
    };
    insert(&mut env, object)
}

fn wasm_func_type(mut env: FunctionEnvMut<WasmCapiEnv>, func_handle: i32) -> i32 {
    let func = match env.data().state.get(func_handle) {
        Some(WasmObject::Func(func)) => func.clone(),
        Some(WasmObject::Extern(WasmExtern::Func(func))) => func.clone(),
        _ => return INVALID_HANDLE,
    };
    let ty = func.ty(&env);
    insert(&mut env, WasmObject::FuncType(ty))
}

fn wasm_func_call(
    mut env: FunctionEnvMut<WasmCapiEnv>,
    func_handle: i32,
    args_vec_ptr: i32,
    results_vec_ptr: i32,
) -> i32 {
    let func = match env.data().state.get(func_handle) {
        Some(WasmObject::Func(func)) => func.clone(),
        Some(WasmObject::Extern(WasmExtern::Func(func))) => func.clone(),
        _ => return insert(&mut env, WasmObject::Trap("invalid function".to_string())),
    };
    let ty = func.ty(&env);
    let arg_data_ptr = guest_ptr_with_offset(args_vec_ptr, WASM_VEC_DATA_OFFSET)
        .and_then(|ptr| read_i32(&mut env, ptr))
        .unwrap_or(INVALID_HANDLE);
    let mut args = Vec::with_capacity(ty.params().len());
    for (index, ty) in ty.params().iter().enumerate() {
        let val_ptr = arg_data_ptr + (index * WASM_VAL_SIZE) as i32;
        let Some(value) = read_wasm_val(&mut env, val_ptr, *ty) else {
            return insert(
                &mut env,
                WasmObject::Trap("invalid function argument".to_string()),
            );
        };
        args.push(value);
    }
    sync_memory_shadows_to_wasmer(&mut env);
    match func.call(&mut env, &args) {
        Ok(results) => {
            refresh_memory_shadows_from_wasmer(&mut env);
            let result_data_ptr = guest_ptr_with_offset(results_vec_ptr, WASM_VEC_DATA_OFFSET)
                .and_then(|ptr| read_i32(&mut env, ptr))
                .unwrap_or(INVALID_HANDLE);
            for (index, value) in results.iter().enumerate() {
                let val_ptr = result_data_ptr + (index * WASM_VAL_SIZE) as i32;
                write_wasm_val(&mut env, val_ptr, value);
            }
            INVALID_HANDLE
        }
        Err(err) => {
            refresh_memory_shadows_from_wasmer(&mut env);
            insert(&mut env, WasmObject::Trap(err.to_string()))
        }
    }
}

fn allocate_wasm_val_vec_for_values(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    values: &[Value],
) -> Option<(i32, i32)> {
    let len = u32::try_from(values.len()).ok()?;
    let byte_len = values.len().checked_mul(WASM_VAL_SIZE)?;

    if byte_len == 0 {
        let vec_ptr = allocate_guest_vec_header(env, len, 0)?;
        return Some((vec_ptr, 0));
    }

    let mut data = GuestAllocation::new(env, byte_len)?;
    for (index, value) in values.iter().enumerate() {
        let offset = index.checked_mul(WASM_VAL_SIZE)?;
        if !data.write_wasm_val_at(offset, value) {
            return None;
        }
    }

    let vec_ptr = data.allocate_vec_header(len)?;
    let data_ptr = data.into_raw();
    Some((vec_ptr, data_ptr))
}

fn allocate_uninitialized_wasm_val_vec(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    len: usize,
) -> Option<(i32, i32)> {
    let len_u32 = u32::try_from(len).ok()?;
    let byte_len = len.checked_mul(WASM_VAL_SIZE)?;

    if byte_len == 0 {
        let vec_ptr = allocate_guest_vec_header(env, len_u32, 0)?;
        return Some((vec_ptr, 0));
    }

    let mut data = GuestAllocation::new(env, byte_len)?;
    let vec_ptr = data.allocate_vec_header(len_u32)?;
    let data_ptr = data.into_raw();
    Some((vec_ptr, data_ptr))
}

fn trap_message_from_handle(env: &FunctionEnvMut<WasmCapiEnv>, trap_handle: i32) -> String {
    match env.data().state.get(trap_handle) {
        Some(WasmObject::Trap(message)) => message.clone(),
        _ => "WebAssembly import callback trapped".to_string(),
    }
}

fn call_guest_wasm_callback(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    callback: u32,
    callback_env: i32,
    args_vec_ptr: i32,
    results_vec_ptr: i32,
) -> i32 {
    let Some(table) = env.data().table.clone() else {
        return insert(
            env,
            WasmObject::Trap("missing guest indirect function table".to_string()),
        );
    };
    let Some(elem) = table.get(&mut *env, callback) else {
        return insert(
            env,
            WasmObject::Trap("invalid guest WebAssembly callback pointer".to_string()),
        );
    };
    let func = match elem {
        Value::FuncRef(Some(func)) => func,
        _ => {
            return insert(
                env,
                WasmObject::Trap("invalid guest WebAssembly callback reference".to_string()),
            );
        }
    };
    // Mirror of the wasm_func_call bracketing, in the opposite direction: the
    // guest callback reads the target module's memory through its shadow (so
    // it must see mutations made since the last refresh), and its writes into
    // the shadow (e.g. wasm-bindgen retptr stores) must land back in the real
    // memory before the target module resumes.
    refresh_memory_shadows_from_wasmer(env);
    let call_result = func.call(
        env,
        &[
            Value::I32(callback_env),
            Value::I32(args_vec_ptr),
            Value::I32(results_vec_ptr),
        ],
    );
    sync_memory_shadows_to_wasmer(env);
    match call_result {
        Ok(values) => match values.first() {
            Some(Value::I32(value)) => *value,
            Some(Value::I64(value)) => *value as i32,
            _ => INVALID_HANDLE,
        },
        Err(err) => insert(env, WasmObject::Trap(err.to_string())),
    }
}

fn wasm_func_new_with_env(
    mut env: FunctionEnvMut<WasmCapiEnv>,
    _store: i32,
    type_handle: i32,
    callback: i32,
    callback_env: i32,
    _finalizer: i32,
) -> i32 {
    if callback <= INVALID_HANDLE {
        return INVALID_HANDLE;
    }
    let ty = match env.data().state.get(type_handle) {
        Some(WasmObject::FuncType(ty)) => ty.clone(),
        _ => return INVALID_HANDLE,
    };
    let Some(func_env) = env.data().func_env.clone() else {
        return INVALID_HANDLE;
    };
    let Ok(callback) = u32::try_from(callback) else {
        return INVALID_HANDLE;
    };
    let result_types = ty.results().to_vec();
    let func = WasmerFunction::new_with_env(&mut env, &func_env, ty, move |mut env, args| {
        let Some((args_vec_ptr, _args_data_ptr)) = allocate_wasm_val_vec_for_values(&mut env, args)
        else {
            return Err(RuntimeError::new(
                "failed to allocate WebAssembly callback arguments",
            ));
        };
        let Some((results_vec_ptr, results_data_ptr)) =
            allocate_uninitialized_wasm_val_vec(&mut env, result_types.len())
        else {
            return Err(RuntimeError::new(
                "failed to allocate WebAssembly callback results",
            ));
        };

        let trap = call_guest_wasm_callback(
            &mut env,
            callback,
            callback_env,
            args_vec_ptr,
            results_vec_ptr,
        );
        if trap != INVALID_HANDLE {
            return Err(RuntimeError::new(trap_message_from_handle(&env, trap)));
        }

        let mut results = Vec::with_capacity(result_types.len());
        for (index, ty) in result_types.iter().enumerate() {
            let val_ptr = results_data_ptr + (index * WASM_VAL_SIZE) as i32;
            let Some(value) = read_wasm_val(&mut env, val_ptr, *ty) else {
                return Err(RuntimeError::new(
                    "guest WebAssembly callback returned an invalid value",
                ));
            };
            results.push(value);
        }
        Ok(results)
    });
    insert(&mut env, WasmObject::Func(func))
}

fn wasm_table_size(env: FunctionEnvMut<WasmCapiEnv>, table_handle: i32) -> i32 {
    match env.data().state.get(table_handle) {
        Some(WasmObject::Table(table)) => i32::try_from(table.size(&env)).unwrap_or(INVALID_SIZE),
        Some(WasmObject::Extern(WasmExtern::Table(table))) => {
            i32::try_from(table.size(&env)).unwrap_or(INVALID_SIZE)
        }
        _ => INVALID_SIZE,
    }
}

/// Look up a table handle (either a bare table or an extern-wrapped one).
fn table_from_handle(env: &FunctionEnvMut<WasmCapiEnv>, handle: i32) -> Option<Table> {
    match env.data().state.get(handle) {
        Some(WasmObject::Table(table)) => Some(table.clone()),
        Some(WasmObject::Extern(WasmExtern::Table(table))) => Some(table.clone()),
        _ => None,
    }
}

/// The null reference of a table's element kind.
fn null_ref_for(element_ty: Type) -> Value {
    match element_ty {
        Type::FuncRef => Value::FuncRef(None),
        _ => Value::ExternRef(None),
    }
}

fn wasm_table_grow(
    mut env: FunctionEnvMut<WasmCapiEnv>,
    table_handle: i32,
    delta: i32,
    init: i32,
) -> i32 {
    let Ok(delta) = u32::try_from(delta) else {
        return BOOL_FALSE;
    };
    let Some(table) = table_from_handle(&env, table_handle) else {
        return BOOL_FALSE;
    };
    let element_ty = table.ty(&env).ty;
    let init_val = if init <= INVALID_HANDLE {
        null_ref_for(element_ty)
    } else {
        ref_value_from_handle(&env, init, element_ty)
    };
    if table.grow(&mut env, delta, init_val).is_ok() {
        BOOL_TRUE
    } else {
        BOOL_FALSE
    }
}

fn wasm_table_get(mut env: FunctionEnvMut<WasmCapiEnv>, table_handle: i32, index: i32) -> i32 {
    let Ok(index) = u32::try_from(index) else {
        return INVALID_HANDLE;
    };
    let Some(table) = table_from_handle(&env, table_handle) else {
        return INVALID_HANDLE;
    };
    match table.get(&mut env, index) {
        Some(value) => ref_value_to_handle(&mut env, &value),
        None => INVALID_HANDLE,
    }
}

fn wasm_table_set(
    mut env: FunctionEnvMut<WasmCapiEnv>,
    table_handle: i32,
    index: i32,
    ref_handle: i32,
) -> i32 {
    let Ok(index) = u32::try_from(index) else {
        return BOOL_FALSE;
    };
    let Some(table) = table_from_handle(&env, table_handle) else {
        return BOOL_FALSE;
    };
    let element_ty = table.ty(&env).ty;
    let value = ref_value_from_handle(&env, ref_handle, element_ty);
    if table.set(&mut env, index, value).is_ok() {
        BOOL_TRUE
    } else {
        BOOL_FALSE
    }
}

fn wasm_tabletype_new(
    mut env: FunctionEnvMut<WasmCapiEnv>,
    valtype_handle: i32,
    limits_ptr: i32,
) -> i32 {
    let ty = match env.data().state.get(valtype_handle) {
        Some(WasmObject::ValType(ty)) => *ty,
        _ => return INVALID_HANDLE,
    };
    let Some((min, max)) = read_limits(&mut env, limits_ptr) else {
        return INVALID_HANDLE;
    };
    insert(
        &mut env,
        WasmObject::TableType(TableType::new(ty, min, max)),
    )
}

fn wasm_tabletype_element(mut env: FunctionEnvMut<WasmCapiEnv>, tabletype_handle: i32) -> i32 {
    let ty = match env.data().state.get(tabletype_handle) {
        Some(WasmObject::TableType(ty)) => ty.ty,
        _ => return INVALID_HANDLE,
    };
    insert(&mut env, WasmObject::ValType(ty))
}

fn wasm_table_type(mut env: FunctionEnvMut<WasmCapiEnv>, table_handle: i32) -> i32 {
    let Some(table) = table_from_handle(&env, table_handle) else {
        return INVALID_HANDLE;
    };
    let ty = table.ty(&env);
    insert(&mut env, WasmObject::TableType(ty))
}

fn wasm_table_new(
    mut env: FunctionEnvMut<WasmCapiEnv>,
    _store: i32,
    tabletype_handle: i32,
    init_handle: i32,
) -> i32 {
    let ty = match env.data().state.get(tabletype_handle) {
        Some(WasmObject::TableType(ty)) => *ty,
        _ => return INVALID_HANDLE,
    };
    let init_val = if init_handle <= INVALID_HANDLE {
        null_ref_for(ty.ty)
    } else {
        ref_value_from_handle(&env, init_handle, ty.ty)
    };
    match Table::new(&mut env, ty, init_val) {
        Ok(table) => insert(&mut env, WasmObject::Table(table)),
        Err(_) => INVALID_HANDLE,
    }
}

fn wasm_func_as_ref(mut env: FunctionEnvMut<WasmCapiEnv>, func_handle: i32) -> i32 {
    let func = match env.data().state.get(func_handle) {
        Some(WasmObject::Func(func)) => func.clone(),
        Some(WasmObject::Extern(WasmExtern::Func(func))) => func.clone(),
        _ => return INVALID_HANDLE,
    };
    insert(&mut env, WasmObject::Ref(Value::FuncRef(Some(func))))
}

fn wasm_ref_as_func(mut env: FunctionEnvMut<WasmCapiEnv>, ref_handle: i32) -> i32 {
    let func = match env.data().state.get(ref_handle) {
        Some(WasmObject::Ref(Value::FuncRef(Some(func)))) => func.clone(),
        _ => return INVALID_HANDLE,
    };
    insert(&mut env, WasmObject::Func(func))
}

/// Host-info payload carried by a `wasm_foreign_new` externref. Values are
/// guest `void*` / function pointers, i.e. i32s in the wasm32 guest.
struct GuestForeign {
    host_info: std::cell::Cell<i32>,
    finalizer: std::cell::Cell<i32>,
}

// SAFETY: guest stores are single-threaded; these cells are never shared across
// threads.
unsafe impl Send for GuestForeign {}
unsafe impl Sync for GuestForeign {}

fn wasm_foreign_new(mut env: FunctionEnvMut<WasmCapiEnv>, _store: i32) -> i32 {
    let extern_ref = {
        let (_, mut store) = env.data_and_store_mut();
        ExternRef::new(
            &mut store,
            GuestForeign {
                host_info: std::cell::Cell::new(0),
                finalizer: std::cell::Cell::new(0),
            },
        )
    };
    insert(
        &mut env,
        WasmObject::Ref(Value::ExternRef(Some(extern_ref))),
    )
}

fn wasm_ref_copy(mut env: FunctionEnvMut<WasmCapiEnv>, ref_handle: i32) -> i32 {
    match env.data().state.get(ref_handle) {
        Some(WasmObject::Ref(value)) => {
            let value = value.clone();
            insert(&mut env, WasmObject::Ref(value))
        }
        _ => INVALID_HANDLE,
    }
}

/// Whether two reference values point at the same underlying object.
fn ref_values_same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::ExternRef(Some(x)), Value::ExternRef(Some(y))) => x.ptr_eq(y),
        (Value::ExternRef(None), Value::ExternRef(None)) => true,
        (Value::FuncRef(Some(x)), Value::FuncRef(Some(y))) => x == y,
        (Value::FuncRef(None), Value::FuncRef(None)) => true,
        _ => false,
    }
}

fn wasm_ref_same(env: FunctionEnvMut<WasmCapiEnv>, ref1: i32, ref2: i32) -> i32 {
    if ref1 == ref2 {
        return BOOL_TRUE;
    }
    let state = &env.data().state;
    let (Some(WasmObject::Ref(a)), Some(WasmObject::Ref(b))) = (state.get(ref1), state.get(ref2))
    else {
        return BOOL_FALSE;
    };
    if ref_values_same(a, b) {
        BOOL_TRUE
    } else {
        BOOL_FALSE
    }
}

/// Access the [`GuestForeign`] payload of a foreign externref handle.
fn with_guest_foreign<R>(
    env: &FunctionEnvMut<WasmCapiEnv>,
    ref_handle: i32,
    f: impl FnOnce(&GuestForeign) -> R,
) -> Option<R> {
    match env.data().state.get(ref_handle) {
        Some(WasmObject::Ref(Value::ExternRef(Some(e)))) => e.downcast::<GuestForeign>(env).map(f),
        _ => None,
    }
}

fn wasm_ref_get_host_info(env: FunctionEnvMut<WasmCapiEnv>, ref_handle: i32) -> i32 {
    with_guest_foreign(&env, ref_handle, |g| g.host_info.get()).unwrap_or(0)
}

fn wasm_ref_set_host_info(env: FunctionEnvMut<WasmCapiEnv>, ref_handle: i32, info: i32) {
    with_guest_foreign(&env, ref_handle, |g| {
        g.finalizer.set(0);
        g.host_info.set(info);
    });
}

fn wasm_ref_set_host_info_with_finalizer(
    env: FunctionEnvMut<WasmCapiEnv>,
    ref_handle: i32,
    info: i32,
    finalizer: i32,
) {
    with_guest_foreign(&env, ref_handle, |g| {
        g.host_info.set(info);
        g.finalizer.set(finalizer);
    });
}

fn wasm_trap_new(mut env: FunctionEnvMut<WasmCapiEnv>, _store: i32, message_ptr: i32) -> i32 {
    let bytes = read_byte_vec(&mut env, message_ptr).unwrap_or_default();
    let mut message = String::from_utf8_lossy(&bytes).to_string();
    if message.ends_with('\0') {
        message.pop();
    }
    insert(&mut env, WasmObject::Trap(message))
}

fn wasm_trap_message(mut env: FunctionEnvMut<WasmCapiEnv>, trap_handle: i32, out_ptr: i32) {
    let message = match env.data().state.get(trap_handle) {
        Some(WasmObject::Trap(message)) => message.clone(),
        _ => "WebAssembly trap".to_string(),
    };
    let mut bytes = message.into_bytes();
    bytes.push(0);
    write_byte_vec(&mut env, out_ptr, &bytes);
}

fn wasm_byte_vec_new(mut env: FunctionEnvMut<WasmCapiEnv>, out_ptr: i32, size: i32, data_ptr: i32) {
    if out_ptr <= INVALID_HANDLE || size < 0 || data_ptr < 0 {
        return;
    }
    let Ok(size) = usize::try_from(size) else {
        return;
    };
    let bytes = if size == 0 {
        Vec::new()
    } else {
        let Some(bytes) = read_guest_bytes(&mut env, data_ptr, size) else {
            return;
        };
        bytes
    };
    write_byte_vec(&mut env, out_ptr, &bytes);
}

fn wasm_val_vec_new_uninitialized(mut env: FunctionEnvMut<WasmCapiEnv>, out_ptr: i32, size: i32) {
    if out_ptr <= INVALID_HANDLE || size < 0 {
        return;
    }
    let Ok(len) = u32::try_from(size) else {
        return;
    };
    let Ok(size) = usize::try_from(size) else {
        return;
    };
    let Some(byte_len) = size.checked_mul(WASM_VAL_SIZE) else {
        return;
    };

    if byte_len == 0 {
        let Some(out_ptr) = non_null_guest_ptr(out_ptr) else {
            return;
        };
        write_guest_vec_header(&mut env, out_ptr.get(), len, 0);
        return;
    }

    let Some(mut data) = GuestAllocation::new(&mut env, byte_len) else {
        return;
    };
    let Some(out_ptr) = non_null_guest_ptr(out_ptr) else {
        return;
    };
    if data.write_vec_header(out_ptr, len) {
        let _ = data.into_raw();
    }
}

fn noop_delete(_env: FunctionEnvMut<WasmCapiEnv>, _handle: i32) {}

/// `wasm_foreign_t` and `wasm_ref_t` share a handle representation, so the cast
/// helpers (`wasm_foreign_as_ref`, `wasm_ref_as_foreign`) are the identity.
fn handle_identity(_env: FunctionEnvMut<WasmCapiEnv>, handle: i32) -> i32 {
    handle
}

fn vec_delete(mut env: FunctionEnvMut<WasmCapiEnv>, vec_ptr: i32) {
    if let Some(vec_ptr) = non_null_guest_ptr(vec_ptr) {
        write_guest_vec_header(&mut env, vec_ptr.get(), 0, 0);
    }
}

fn register_wasm_c_api_imports(
    store: &mut impl wasmer_api::AsStoreMut,
    fe: &FunctionEnv<WasmCapiEnv>,
    io: &mut Imports,
) {
    fe.as_mut(&mut *store).func_env = Some(fe.clone());

    let ns = namespace! {
        "wasm_engine_new" => WasmerFunction::new_typed_with_env(store, fe, wasm_engine_new),
        "wasm_engine_delete" => WasmerFunction::new_typed_with_env(store, fe, delete_handle),
        "wasm_store_new" => WasmerFunction::new_typed_with_env(store, fe, wasm_store_new),
        "wasm_store_delete" => WasmerFunction::new_typed_with_env(store, fe, delete_handle),
        "wasm_module_new" => WasmerFunction::new_typed_with_env(store, fe, wasm_module_new),
        "wasm_module_validate" => WasmerFunction::new_typed_with_env(store, fe, wasm_module_validate),
        "wasm_module_delete" => WasmerFunction::new_typed_with_env(store, fe, delete_handle),
        "wasm_module_imports" => WasmerFunction::new_typed_with_env(store, fe, wasm_module_imports),
        "wasm_module_exports" => WasmerFunction::new_typed_with_env(store, fe, wasm_module_exports),
        "wasm_importtype_module" => WasmerFunction::new_typed_with_env(store, fe, wasm_importtype_module),
        "wasm_importtype_name" => WasmerFunction::new_typed_with_env(store, fe, wasm_importtype_name),
        "wasm_importtype_type" => WasmerFunction::new_typed_with_env(store, fe, wasm_importtype_type),
        "wasm_importtype_vec_delete" => WasmerFunction::new_typed_with_env(store, fe, vec_delete),
        "wasm_exporttype_name" => WasmerFunction::new_typed_with_env(store, fe, wasm_exporttype_name),
        "wasm_exporttype_type" => WasmerFunction::new_typed_with_env(store, fe, wasm_exporttype_type),
        "wasm_exporttype_vec_delete" => WasmerFunction::new_typed_with_env(store, fe, vec_delete),
        "wasm_externtype_kind" => WasmerFunction::new_typed_with_env(store, fe, wasm_externtype_kind),
        "wasm_externtype_as_functype_const" => WasmerFunction::new_typed_with_env(store, fe, wasm_externtype_as_functype_const),
        "wasm_functype_copy" => WasmerFunction::new_typed_with_env(store, fe, wasm_functype_copy),
        "wasm_functype_delete" => WasmerFunction::new_typed_with_env(store, fe, delete_handle),
        "wasm_functype_params" => WasmerFunction::new_typed_with_env(store, fe, wasm_functype_params),
        "wasm_functype_results" => WasmerFunction::new_typed_with_env(store, fe, wasm_functype_results),
        "wasm_valtype_new" => WasmerFunction::new_typed_with_env(store, fe, wasm_valtype_new),
        "wasm_valtype_kind" => WasmerFunction::new_typed_with_env(store, fe, wasm_valtype_kind),
        "wasm_val_delete" => WasmerFunction::new_typed_with_env(store, fe, noop_delete),
        "wasm_val_vec_new_uninitialized" => WasmerFunction::new_typed_with_env(store, fe, wasm_val_vec_new_uninitialized),
        "wasm_val_vec_delete" => WasmerFunction::new_typed_with_env(store, fe, vec_delete),
        "wasm_memorytype_new" => WasmerFunction::new_typed_with_env(store, fe, wasm_memorytype_new),
        "wasm_memorytype_delete" => WasmerFunction::new_typed_with_env(store, fe, delete_handle),
        "wasm_memory_new" => WasmerFunction::new_typed_with_env(store, fe, wasm_memory_new),
        "wasm_memory_delete" => WasmerFunction::new_typed_with_env(store, fe, delete_handle),
        "wasm_memory_copy" => WasmerFunction::new_typed_with_env(store, fe, wasm_memory_copy),
        "wasm_memory_size" => WasmerFunction::new_typed_with_env(store, fe, wasm_memory_size),
        "wasm_memory_grow" => WasmerFunction::new_typed_with_env(store, fe, wasm_memory_grow),
        "wasm_memory_data" => WasmerFunction::new_typed_with_env(store, fe, wasm_memory_data),
        "wasm_memory_data_size" => WasmerFunction::new_typed_with_env(store, fe, wasm_memory_data_size),
        "wasm_memory_as_extern" => WasmerFunction::new_typed_with_env(store, fe, wasm_memory_as_extern),
        "wasm_globaltype_new" => WasmerFunction::new_typed_with_env(store, fe, wasm_globaltype_new),
        "wasm_globaltype_delete" => WasmerFunction::new_typed_with_env(store, fe, delete_handle),
        "wasm_globaltype_content" => WasmerFunction::new_typed_with_env(store, fe, wasm_globaltype_content),
        "wasm_globaltype_mutability" => WasmerFunction::new_typed_with_env(store, fe, wasm_globaltype_mutability),
        "wasm_global_new" => WasmerFunction::new_typed_with_env(store, fe, wasm_global_new),
        "wasm_global_delete" => WasmerFunction::new_typed_with_env(store, fe, delete_handle),
        "wasm_global_copy" => WasmerFunction::new_typed_with_env(store, fe, wasm_global_copy),
        "wasm_global_type" => WasmerFunction::new_typed_with_env(store, fe, wasm_global_type),
        "wasm_global_get" => WasmerFunction::new_typed_with_env(store, fe, wasm_global_get),
        "wasm_global_set" => WasmerFunction::new_typed_with_env(store, fe, wasm_global_set),
        "wasm_global_as_extern" => WasmerFunction::new_typed_with_env(store, fe, wasm_global_as_extern),
        "wasm_table_new" => WasmerFunction::new_typed_with_env(store, fe, wasm_table_new),
        "wasm_table_delete" => WasmerFunction::new_typed_with_env(store, fe, delete_handle),
        "wasm_table_copy" => WasmerFunction::new_typed_with_env(store, fe, wasm_table_copy),
        "wasm_table_type" => WasmerFunction::new_typed_with_env(store, fe, wasm_table_type),
        "wasm_table_size" => WasmerFunction::new_typed_with_env(store, fe, wasm_table_size),
        "wasm_table_grow" => WasmerFunction::new_typed_with_env(store, fe, wasm_table_grow),
        "wasm_table_get" => WasmerFunction::new_typed_with_env(store, fe, wasm_table_get),
        "wasm_table_set" => WasmerFunction::new_typed_with_env(store, fe, wasm_table_set),
        "wasm_table_as_extern" => WasmerFunction::new_typed_with_env(store, fe, wasm_table_as_extern),
        "wasm_tabletype_new" => WasmerFunction::new_typed_with_env(store, fe, wasm_tabletype_new),
        "wasm_tabletype_delete" => WasmerFunction::new_typed_with_env(store, fe, delete_handle),
        "wasm_tabletype_element" => WasmerFunction::new_typed_with_env(store, fe, wasm_tabletype_element),
        "wasm_func_as_ref" => WasmerFunction::new_typed_with_env(store, fe, wasm_func_as_ref),
        "wasm_ref_as_func" => WasmerFunction::new_typed_with_env(store, fe, wasm_ref_as_func),
        "wasm_foreign_new" => WasmerFunction::new_typed_with_env(store, fe, wasm_foreign_new),
        "wasm_foreign_delete" => WasmerFunction::new_typed_with_env(store, fe, delete_handle),
        "wasm_foreign_as_ref" => WasmerFunction::new_typed_with_env(store, fe, handle_identity),
        "wasm_ref_as_foreign" => WasmerFunction::new_typed_with_env(store, fe, handle_identity),
        "wasm_ref_delete" => WasmerFunction::new_typed_with_env(store, fe, delete_handle),
        "wasm_ref_copy" => WasmerFunction::new_typed_with_env(store, fe, wasm_ref_copy),
        "wasm_ref_same" => WasmerFunction::new_typed_with_env(store, fe, wasm_ref_same),
        "wasm_ref_get_host_info" => WasmerFunction::new_typed_with_env(store, fe, wasm_ref_get_host_info),
        "wasm_ref_set_host_info" => WasmerFunction::new_typed_with_env(store, fe, wasm_ref_set_host_info),
        "wasm_ref_set_host_info_with_finalizer" => WasmerFunction::new_typed_with_env(store, fe, wasm_ref_set_host_info_with_finalizer),
        "wasm_instance_new" => WasmerFunction::new_typed_with_env(store, fe, wasm_instance_new),
        "wasm_instance_delete" => WasmerFunction::new_typed_with_env(store, fe, delete_handle),
        "wasm_instance_exports" => WasmerFunction::new_typed_with_env(store, fe, wasm_instance_exports),
        "wasm_extern_kind" => WasmerFunction::new_typed_with_env(store, fe, wasm_extern_kind),
        "wasm_extern_as_func" => WasmerFunction::new_typed_with_env(store, fe, wasm_extern_as_func),
        "wasm_extern_as_global" => WasmerFunction::new_typed_with_env(store, fe, wasm_extern_as_global),
        "wasm_extern_as_table" => WasmerFunction::new_typed_with_env(store, fe, wasm_extern_as_table),
        "wasm_extern_as_memory" => WasmerFunction::new_typed_with_env(store, fe, wasm_extern_as_memory),
        "wasm_extern_vec_delete" => WasmerFunction::new_typed_with_env(store, fe, vec_delete),
        "wasm_func_copy" => WasmerFunction::new_typed_with_env(store, fe, wasm_func_copy),
        "wasm_func_delete" => WasmerFunction::new_typed_with_env(store, fe, delete_handle),
        "wasm_func_as_extern" => WasmerFunction::new_typed_with_env(store, fe, wasm_func_as_extern),
        "wasm_func_type" => WasmerFunction::new_typed_with_env(store, fe, wasm_func_type),
        "wasm_func_call" => WasmerFunction::new_typed_with_env(store, fe, wasm_func_call),
        "wasm_func_new_with_env" => WasmerFunction::new_typed_with_env(store, fe, wasm_func_new_with_env),
        "wasm_trap_new" => WasmerFunction::new_typed_with_env(store, fe, wasm_trap_new),
        "wasm_trap_delete" => WasmerFunction::new_typed_with_env(store, fe, delete_handle),
        "wasm_trap_message" => WasmerFunction::new_typed_with_env(store, fe, wasm_trap_message),
        "wasm_byte_vec_new" => WasmerFunction::new_typed_with_env(store, fe, wasm_byte_vec_new),
        "wasm_byte_vec_delete" => WasmerFunction::new_typed_with_env(store, fe, vec_delete),
    };

    io.register_namespace(WASM_C_API_MODULE_NAME, ns);
}

#[cfg(test)]
mod tests {
    use super::{
        BOOL_FALSE, BOOL_TRUE, INVALID_HANDLE, INVALID_SIZE, MemoryShadow, Type, WASM_EXTERNREF,
        WASM_F64, WASM_FUNCREF, WASM_I32, WASM_VAL_PAYLOAD_OFFSET, WasmCAPIVersion, WasmCapiEnv,
        WasmCapiState, WasmObject, copy_guest_memory_to_wasmer, copy_wasmer_memory_to_guest,
        guest_byte_ptr, guest_memory_offset, guest_ptr_with_offset, module_wasm_c_api_version_used,
        non_null_guest_ptr, read_wasm_val, ref_values_same, refresh_memory_shadows_from_wasmer,
        sync_memory_shadows_to_wasmer, type_to_wasm_kind, wasm_foreign_new, wasm_func_as_ref,
        wasm_kind_to_type, wasm_memory_data, wasm_memory_data_size, wasm_memory_size,
        wasm_ref_as_func, wasm_ref_copy, wasm_ref_get_host_info, wasm_ref_same,
        wasm_ref_set_host_info, wasm_table_get, wasm_table_grow, wasm_table_set, wasm_table_size,
        write_wasm_val,
    };
    use wasmer_api::{
        Function, FunctionEnv, Memory, MemoryType, Module, Pages, Store, Table, TableType, Value,
    };
    use wat::parse_str;

    #[cfg(feature = "wasi")]
    use super::WasmCapiRuntimeHooks;
    #[cfg(feature = "wasi")]
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    #[cfg(feature = "wasi")]
    use wasmer_types::ModuleHash;
    #[cfg(feature = "wasi")]
    use wasmer_wasix::{
        PluggableRuntime, WasiError,
        runners::wasi::{RuntimeOrEngine, WasiRunner},
        runtime::task_manager::tokio::TokioTaskManager,
    };

    const EMPTY_WASM_MODULE: &[u8] = b"\0asm\x01\0\0\0";

    fn compile_wat(store: &Store, wat: &str) -> Module {
        let wasm = parse_str(wat).expect("wat module parses");
        Module::new(store, wasm).expect("wat module compiles")
    }

    #[test]
    fn module_wasm_c_api_version_used_detects_none() {
        let store = Store::default();
        let module = Module::new(&store, EMPTY_WASM_MODULE).expect("empty wasm module compiles");

        assert_eq!(module_wasm_c_api_version_used(&module), None);
    }

    #[test]
    fn module_wasm_c_api_version_used_detects_v0() {
        let store = Store::default();
        let module = compile_wat(
            &store,
            r#"(module
                (import "wasm_c_api_v0" "wasm_engine_new" (func))
            )"#,
        );

        assert_eq!(
            module_wasm_c_api_version_used(&module),
            Some(WasmCAPIVersion::V0)
        );
    }

    #[test]
    fn module_wasm_c_api_version_used_detects_unknown_version() {
        let store = Store::default();
        let module = compile_wat(
            &store,
            r#"(module
                (import "wasm_c_api_v1" "wasm_engine_new" (func))
            )"#,
        );

        assert_eq!(
            module_wasm_c_api_version_used(&module),
            Some(WasmCAPIVersion::Unknown)
        );
    }

    #[test]
    fn module_wasm_c_api_version_used_detects_mixed_versions() {
        let store = Store::default();
        let module = compile_wat(
            &store,
            r#"(module
                (import "wasm_c_api_v0" "wasm_engine_new" (func))
                (import "wasm_c_api_v1" "wasm_store_new" (func))
            )"#,
        );

        assert_eq!(
            module_wasm_c_api_version_used(&module),
            Some(WasmCAPIVersion::Unknown)
        );
    }

    #[test]
    fn wasm_c_api_version_compatibility_is_strict() {
        assert!(WasmCAPIVersion::V0.is_compatible_with(WasmCAPIVersion::V0));
        assert!(!WasmCAPIVersion::V0.is_compatible_with(WasmCAPIVersion::Unknown));
        assert!(!WasmCAPIVersion::Unknown.is_compatible_with(WasmCAPIVersion::V0));
        assert!(!WasmCAPIVersion::Unknown.is_compatible_with(WasmCAPIVersion::Unknown));
    }

    #[cfg(feature = "wasi")]
    #[test]
    fn wasm_c_api_imports_run_from_wasix_guest() {
        let wasm = parse_str(
            r#"(module
                (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
                (import "wasm_c_api_v0" "wasm_engine_new" (func $wasm_engine_new (result i32)))
                (import "wasm_c_api_v0" "wasm_store_new" (func $wasm_store_new (param i32) (result i32)))
                (import "wasm_c_api_v0" "wasm_module_validate" (func $wasm_module_validate (param i32 i32) (result i32)))
                (import "wasm_c_api_v0" "wasm_module_new" (func $wasm_module_new (param i32 i32) (result i32)))

                (memory (export "memory") 1)
                ;; wasm_byte_vec_t { size: 8, data: 32 }
                (data (i32.const 16) "\08\00\00\00\20\00\00\00")
                ;; Empty wasm module: \0asm + version 1.
                (data (i32.const 32) "\00asm\01\00\00\00")

                (func (export "_start")
                    (local $engine i32)
                    (local $store i32)
                    (local $module i32)

                    (local.set $engine (call $wasm_engine_new))
                    (if (i32.eqz (local.get $engine))
                        (then (call $proc_exit (i32.const 10))))

                    (local.set $store (call $wasm_store_new (local.get $engine)))
                    (if (i32.eqz (local.get $store))
                        (then (call $proc_exit (i32.const 11))))

                    (if (i32.eqz (call $wasm_module_validate (local.get $store) (i32.const 16)))
                        (then (call $proc_exit (i32.const 12))))

                    (local.set $module (call $wasm_module_new (local.get $store) (i32.const 16)))
                    (if (i32.eqz (local.get $module))
                        (then (call $proc_exit (i32.const 13))))
                )
            )"#,
        )
        .expect("guest wat parses");
        let store = Store::default();
        let module = Module::new(&store, &wasm).expect("guest module compiles");

        let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime starts");
        let _guard = tokio_runtime.enter();
        let mut runtime = PluggableRuntime::new(Arc::new(TokioTaskManager::new(
            tokio_runtime.handle().clone(),
        )));
        runtime.set_engine(store.engine().clone());

        let resolve_calls = Arc::new(AtomicUsize::new(0));
        let resolve_engine = store.engine().clone();
        let hooks = WasmCapiRuntimeHooks::new().with_resolve_module_sync({
            let resolve_calls = resolve_calls.clone();
            move |bytes| {
                resolve_calls.fetch_add(1, Ordering::SeqCst);
                Ok(Module::new(&resolve_engine, bytes)?)
            }
        });
        runtime
            .with_additional_imports({
                let hooks = hooks.clone();
                move |module, store| hooks.additional_imports(module, store)
            })
            .with_instance_setup(move |module, store, instance, imported_memory| {
                hooks.configure_instance(module, store, instance, imported_memory)
            });

        let result = WasiRunner::new().run_wasm(
            RuntimeOrEngine::Runtime(Arc::new(runtime)),
            "wasm-c-api-smoke",
            module,
            ModuleHash::new(&wasm),
        );

        match result {
            Ok(()) => {}
            Err(err) => {
                if let Some(WasiError::Exit(code)) = err.downcast_ref::<WasiError>() {
                    panic!("guest exited with status {code}");
                }
                panic!("guest failed: {err:?}");
            }
        }
        assert_eq!(resolve_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn wasm_capi_state_allocates_positive_non_reused_handles() {
        let mut state = WasmCapiState::default();

        let first = state.insert(WasmObject::Engine);
        assert_eq!(first, 1);
        assert!(state.get(first).is_some());

        let _ = state.remove(first);
        assert!(state.get(first).is_none());

        let second = state.insert(WasmObject::Store);
        assert_eq!(second, 2);
        assert!(state.get(INVALID_HANDLE).is_none());
        assert!(state.get(-1).is_none());
    }

    #[test]
    fn wasm_capi_state_fails_closed_on_handle_exhaustion() {
        let mut state = WasmCapiState {
            next_handle: i32::MAX,
            ..WasmCapiState::default()
        };

        assert_eq!(state.insert(WasmObject::Engine), i32::MAX);
        assert_eq!(state.insert(WasmObject::Store), INVALID_HANDLE);
    }

    #[test]
    fn guest_pointer_helpers_reject_invalid_values() {
        assert_eq!(guest_memory_offset(-1), None);
        assert_eq!(guest_memory_offset(0), Some(0));
        assert_eq!(guest_byte_ptr(-1), None);
        assert_eq!(guest_byte_ptr(1).map(|ptr| ptr.offset()), Some(1));
        assert_eq!(non_null_guest_ptr(0), None);
        assert_eq!(non_null_guest_ptr(1).map(|ptr| ptr.get()), Some(1));
        assert_eq!(guest_ptr_with_offset(i32::MAX, 4), None);
    }

    #[test]
    fn size_apis_distinguish_invalid_from_zero() {
        let mut store = Store::default();
        let memory = Memory::new(&mut store, MemoryType::new(Pages(0), Some(Pages(1)), false))
            .expect("zero-page memory can be created");
        let table = Table::new(
            &mut store,
            TableType::new(Type::FuncRef, 0, Some(1)),
            Value::FuncRef(None),
        )
        .expect("zero-sized table can be created");
        let func_env = FunctionEnv::new(&mut store, WasmCapiEnv::default());
        let (memory_handle, table_handle) = {
            let env = func_env.as_mut(&mut store);
            let memory_handle = env.state.insert(WasmObject::Memory(memory));
            let table_handle = env.state.insert(WasmObject::Table(table));
            (memory_handle, table_handle)
        };

        assert_eq!(
            wasm_memory_size(func_env.clone().into_mut(&mut store), INVALID_HANDLE),
            INVALID_SIZE
        );
        assert_eq!(
            wasm_memory_data_size(func_env.clone().into_mut(&mut store), INVALID_HANDLE),
            INVALID_SIZE
        );
        assert_eq!(
            wasm_table_size(func_env.clone().into_mut(&mut store), INVALID_HANDLE),
            INVALID_SIZE
        );
        assert_eq!(
            wasm_memory_size(func_env.clone().into_mut(&mut store), memory_handle),
            0
        );
        assert_eq!(
            wasm_memory_data_size(func_env.clone().into_mut(&mut store), memory_handle),
            0
        );
        assert_eq!(
            wasm_table_size(func_env.into_mut(&mut store), table_handle),
            0
        );
    }

    #[test]
    fn shared_memory_data_shadow_fails_closed() {
        let mut store = Store::default();
        let memory = Memory::new(&mut store, MemoryType::new(Pages(1), Some(Pages(2)), true))
            .expect("shared memory can be created");
        let func_env = FunctionEnv::new(&mut store, WasmCapiEnv::default());
        let memory_handle = func_env
            .as_mut(&mut store)
            .state
            .insert(WasmObject::Memory(memory));

        assert_eq!(
            wasm_memory_data_size(func_env.clone().into_mut(&mut store), memory_handle),
            INVALID_SIZE
        );
        assert_eq!(
            wasm_memory_data(func_env.into_mut(&mut store), memory_handle),
            INVALID_HANDLE
        );
    }

    #[test]
    fn memory_shadow_copy_rejects_same_memory() {
        let mut store = Store::default();
        let memory = Memory::new(&mut store, MemoryType::new(Pages(1), Some(Pages(1)), false))
            .expect("memory can be created");
        let func_env = FunctionEnv::new(
            &mut store,
            WasmCapiEnv {
                memory: Some(memory.clone()),
                ..WasmCapiEnv::default()
            },
        );
        let mut env = func_env.into_mut(&mut store);

        assert!(!copy_wasmer_memory_to_guest(&mut env, &memory, 16, 4));
        assert!(!copy_guest_memory_to_wasmer(&mut env, 16, &memory, 4));
    }

    #[test]
    fn memory_shadow_sync_preserves_registered_shadows() {
        let mut store = Store::default();
        let guest_memory =
            Memory::new(&mut store, MemoryType::new(Pages(1), Some(Pages(1)), false))
                .expect("guest memory can be created");
        let memory = Memory::new(&mut store, MemoryType::new(Pages(1), Some(Pages(1)), false))
            .expect("memory can be created");
        guest_memory
            .view(&store)
            .write(16, &[1, 2, 3, 4])
            .expect("guest shadow write succeeds");
        let func_env = FunctionEnv::new(
            &mut store,
            WasmCapiEnv {
                memory: Some(guest_memory.clone()),
                ..WasmCapiEnv::default()
            },
        );
        let memory_handle = {
            let env = func_env.as_mut(&mut store);
            let memory_handle = env.state.insert(WasmObject::Memory(memory.clone()));
            env.state.memory_shadows.insert(
                memory_handle,
                MemoryShadow {
                    guest_ptr: 16,
                    len: 4,
                },
            );
            memory_handle
        };

        {
            let mut env = func_env.clone().into_mut(&mut store);
            sync_memory_shadows_to_wasmer(&mut env);
            assert!(env.data().state.memory_shadows.contains_key(&memory_handle));
        }
        let mut bytes = [0; 4];
        memory
            .view(&store)
            .read(0, &mut bytes)
            .expect("memory read succeeds");
        assert_eq!(bytes, [1, 2, 3, 4]);

        memory
            .view(&store)
            .write(0, &[5, 6, 7, 8])
            .expect("memory write succeeds");
        {
            let mut env = func_env.into_mut(&mut store);
            refresh_memory_shadows_from_wasmer(&mut env);
            assert!(env.data().state.memory_shadows.contains_key(&memory_handle));
        }
        guest_memory
            .view(&store)
            .read(16, &mut bytes)
            .expect("guest shadow read succeeds");
        assert_eq!(bytes, [5, 6, 7, 8]);
    }

    #[test]
    fn wasm_kind_conversion_is_explicit_for_supported_types() {
        assert_eq!(type_to_wasm_kind(Type::I32), Some(WASM_I32));
        assert_eq!(type_to_wasm_kind(Type::F64), Some(WASM_F64));
        assert_eq!(type_to_wasm_kind(Type::ExternRef), Some(WASM_EXTERNREF));
        assert_eq!(type_to_wasm_kind(Type::FuncRef), Some(WASM_FUNCREF));
        assert_eq!(type_to_wasm_kind(Type::V128), None);
        assert_eq!(type_to_wasm_kind(Type::ExceptionRef), None);

        assert_eq!(wasm_kind_to_type(i32::from(WASM_I32)), Some(Type::I32));
        assert_eq!(
            wasm_kind_to_type(i32::from(WASM_EXTERNREF)),
            Some(Type::ExternRef)
        );
        assert_eq!(wasm_kind_to_type(-1), None);
    }

    #[test]
    fn null_ref_values_marshal_to_a_null_handle() {
        let mut store = Store::default();
        let memory = Memory::new(&mut store, MemoryType::new(Pages(1), Some(Pages(1)), false))
            .expect("memory can be created");
        let func_env = FunctionEnv::new(
            &mut store,
            WasmCapiEnv {
                memory: Some(memory.clone()),
                ..WasmCapiEnv::default()
            },
        );

        // A null reference marshals as the value kind plus a null (0) handle.
        for (value, kind) in [
            (Value::FuncRef(None), WASM_FUNCREF),
            (Value::ExternRef(None), WASM_EXTERNREF),
        ] {
            let val_ptr = 16;
            assert!(write_wasm_val(
                &mut func_env.clone().into_mut(&mut store),
                val_ptr,
                &value
            ));
            let mut kind_byte = [0u8; 1];
            memory
                .view(&store)
                .read(val_ptr as u64, &mut kind_byte)
                .expect("kind read succeeds");
            assert_eq!(kind_byte[0], kind);
            let mut handle = [0u8; 4];
            memory
                .view(&store)
                .read((val_ptr + WASM_VAL_PAYLOAD_OFFSET) as u64, &mut handle)
                .expect("handle read succeeds");
            assert_eq!(i32::from_le_bytes(handle), INVALID_HANDLE);
        }
    }

    #[test]
    fn ref_values_same_matches_identity_and_kind() {
        assert!(ref_values_same(
            &Value::ExternRef(None),
            &Value::ExternRef(None)
        ));
        assert!(ref_values_same(
            &Value::FuncRef(None),
            &Value::FuncRef(None)
        ));
        // A null externref and a null funcref are not the same reference.
        assert!(!ref_values_same(
            &Value::ExternRef(None),
            &Value::FuncRef(None)
        ));
    }

    // Externref/table reference ops are only implemented on the `sys` backend.
    #[cfg(feature = "sys")]
    #[test]
    fn foreign_ref_host_info_and_copy_share_the_payload() {
        let mut store = Store::default();
        let func_env = FunctionEnv::new(&mut store, WasmCapiEnv::default());

        let ref_handle = wasm_foreign_new(func_env.clone().into_mut(&mut store), 0);
        assert!(ref_handle > INVALID_HANDLE);
        // Host info defaults to 0.
        assert_eq!(
            wasm_ref_get_host_info(func_env.clone().into_mut(&mut store), ref_handle),
            0
        );
        // set/get roundtrips.
        wasm_ref_set_host_info(func_env.clone().into_mut(&mut store), ref_handle, 42);
        assert_eq!(
            wasm_ref_get_host_info(func_env.clone().into_mut(&mut store), ref_handle),
            42
        );

        // A copy is a distinct handle that refers to the same extern object, so
        // it is `same` and observes the same (shared) host info.
        let copy = wasm_ref_copy(func_env.clone().into_mut(&mut store), ref_handle);
        assert!(copy > INVALID_HANDLE && copy != ref_handle);
        assert_eq!(
            wasm_ref_same(func_env.clone().into_mut(&mut store), ref_handle, copy),
            BOOL_TRUE
        );
        assert_eq!(
            wasm_ref_get_host_info(func_env.clone().into_mut(&mut store), copy),
            42
        );

        // Two independently minted foreign refs are not the same.
        let other = wasm_foreign_new(func_env.clone().into_mut(&mut store), 0);
        assert_eq!(
            wasm_ref_same(func_env.clone().into_mut(&mut store), ref_handle, other),
            BOOL_FALSE
        );
    }

    #[cfg(feature = "sys")]
    #[test]
    fn externref_table_get_set_grow_via_handles() {
        let mut store = Store::default();
        let table = Table::new(
            &mut store,
            TableType::new(Type::ExternRef, 2, Some(10)),
            Value::ExternRef(None),
        )
        .expect("externref table can be created");
        let func_env = FunctionEnv::new(&mut store, WasmCapiEnv::default());
        let table_handle = {
            let mut env = func_env.clone().into_mut(&mut store);
            env.data_mut().state.insert(WasmObject::Table(table))
        };

        // Empty slot reads back as the null handle.
        assert_eq!(
            wasm_table_get(func_env.clone().into_mut(&mut store), table_handle, 0),
            INVALID_HANDLE
        );

        let ref_handle = wasm_foreign_new(func_env.clone().into_mut(&mut store), 0);
        assert_eq!(
            wasm_table_set(
                func_env.clone().into_mut(&mut store),
                table_handle,
                0,
                ref_handle
            ),
            BOOL_TRUE
        );

        // Reading the slot yields a fresh handle to the same extern object.
        let got = wasm_table_get(func_env.clone().into_mut(&mut store), table_handle, 0);
        assert!(got > INVALID_HANDLE);
        assert_eq!(
            wasm_ref_same(func_env.clone().into_mut(&mut store), got, ref_handle),
            BOOL_TRUE
        );

        // Out-of-bounds set fails without aborting.
        assert_eq!(
            wasm_table_set(
                func_env.clone().into_mut(&mut store),
                table_handle,
                5,
                ref_handle
            ),
            BOOL_FALSE
        );

        // Grow with a null init, then confirm the new size.
        assert_eq!(
            wasm_table_grow(
                func_env.clone().into_mut(&mut store),
                table_handle,
                3,
                INVALID_HANDLE
            ),
            BOOL_TRUE
        );
        assert_eq!(
            wasm_table_size(func_env.clone().into_mut(&mut store), table_handle),
            5
        );
    }

    #[cfg(feature = "sys")]
    #[test]
    fn func_as_ref_and_back_roundtrips() {
        let mut store = Store::default();
        let func = Function::new_typed(&mut store, |x: i32| x);
        let func_env = FunctionEnv::new(&mut store, WasmCapiEnv::default());
        let func_handle = {
            let mut env = func_env.clone().into_mut(&mut store);
            env.data_mut().state.insert(WasmObject::Func(func))
        };

        let ref_handle = wasm_func_as_ref(func_env.clone().into_mut(&mut store), func_handle);
        assert!(ref_handle > INVALID_HANDLE);

        let back = wasm_ref_as_func(func_env.clone().into_mut(&mut store), ref_handle);
        assert!(back > INVALID_HANDLE);
        // The recovered handle is a function.
        let env = func_env.into_mut(&mut store);
        assert!(matches!(
            env.data().state.get(back),
            Some(WasmObject::Func(_))
        ));
    }

    #[cfg(feature = "sys")]
    #[test]
    fn externref_marshals_through_guest_memory() {
        let mut store = Store::default();
        let memory = Memory::new(&mut store, MemoryType::new(Pages(1), Some(Pages(1)), false))
            .expect("memory can be created");
        let func_env = FunctionEnv::new(
            &mut store,
            WasmCapiEnv {
                memory: Some(memory.clone()),
                ..WasmCapiEnv::default()
            },
        );

        // Grab the actual externref value minted by wasm_foreign_new.
        let ref_handle = wasm_foreign_new(func_env.clone().into_mut(&mut store), 0);
        let value = {
            let env = func_env.clone().into_mut(&mut store);
            match env.data().state.get(ref_handle) {
                Some(WasmObject::Ref(value)) => value.clone(),
                _ => panic!("expected a Ref object"),
            }
        };

        // Write it to guest memory, then read it back: the roundtripped value
        // must point at the same extern object.
        let val_ptr = 32;
        assert!(write_wasm_val(
            &mut func_env.clone().into_mut(&mut store),
            val_ptr,
            &value
        ));
        let roundtripped = read_wasm_val(
            &mut func_env.clone().into_mut(&mut store),
            val_ptr,
            Type::ExternRef,
        )
        .expect("externref reads back");
        assert!(ref_values_same(&value, &roundtripped));
    }
}
