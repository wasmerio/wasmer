use anyhow::{Context, Result, bail};
use std::{
    collections::{HashMap, VecDeque},
    fmt::Display,
    mem::MaybeUninit,
    sync::{Arc, Mutex},
};

use wasmer_api::{
    Extern, ExternType, Function, Function as WasmerFunction, FunctionEnv, FunctionEnvMut,
    FunctionType, Global, GlobalType, Imports, Instance, Memory, MemoryType, Module, Mutability,
    Pages, RuntimeError, StoreMut, Table, Type, TypedFunction, Value, namespace,
};

/// Import module name used for host-provided WebAssembly C API bindings.
pub const WASM_C_API_MODULE_NAME: &str = "wasm_c_api_v0";
const WASM_C_API_MODULE_PREFIX: &str = "wasm_c_api_v";

const INVALID_HANDLE: i32 = 0;
const BOOL_FALSE: i32 = 0;
const BOOL_TRUE: i32 = 1;
const INVALID_KIND: i32 = -1;
const WASM_VEC_SIZE: usize = 8;
const WASM_VEC_DATA_OFFSET: i32 = 4;
const WASM_C_API_SHADOW_COPY_CHUNK_SIZE: usize = 64 * 1024;

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

/// Detects whether a module imports the host-provided WebAssembly C API.
pub fn module_needs_wasm_c_api(module: &Module) -> Option<WasmCAPIVersion> {
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

struct WasmCapiSession {
    version: Option<WasmCAPIVersion>,
    imported_memory_type: Option<MemoryType>,
    imported_table_type: Option<wasmer_api::TableType>,
    func_env: Mutex<Option<FunctionEnv<WasmCapiEnv>>>,
}

impl WasmCapiSession {
    fn new(module: &Module) -> Self {
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
            version: module_needs_wasm_c_api(module),
            imported_memory_type,
            imported_table_type,
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

        let func_env = FunctionEnv::new(&mut *store, WasmCapiEnv::default());
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
}

impl WasmCapiRuntimeHooks {
    /// Creates an empty hook set.
    pub fn new() -> Self {
        Self::default()
    }

    fn module_key(module: &Module) -> ModuleKey {
        ModuleKey::new(module)
    }

    /// Adds `wasm_c_api_v0` imports when `module` requests them.
    pub fn add_imports(
        &self,
        module: &Module,
        store: &mut StoreMut<'_>,
        imports: &mut Imports,
    ) -> Result<()> {
        let session = WasmCapiSession::new(module);
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
        if module_needs_wasm_c_api(module).is_none() {
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
const WASM_VAL_PAYLOAD_OFFSET: u32 = 8;

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
    Trap(String),
}

/// A guest-visible shadow of a Wasmer memory data pointer.
#[derive(Clone, Copy)]
struct MemoryShadow {
    /// Guest allocation returned from `wasm_memory_data`.
    guest_ptr: u32,
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

fn write_guest_bytes(env: &mut FunctionEnvMut<WasmCapiEnv>, guest_ptr: u32, data: &[u8]) -> bool {
    let (state, store) = env.data_and_store_mut();
    let Some(memory) = state.memory.clone() else {
        return false;
    };
    memory.view(&store).write(guest_ptr as u64, data).is_ok()
}

fn write_guest_u32(env: &mut FunctionEnvMut<WasmCapiEnv>, guest_ptr: u32, val: u32) -> bool {
    write_guest_bytes(env, guest_ptr, &val.to_le_bytes())
}

fn write_guest_u32_offset(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    guest_ptr: u32,
    offset: u32,
    val: u32,
) -> bool {
    let Some(ptr) = guest_ptr.checked_add(offset) else {
        return false;
    };
    write_guest_u32(env, ptr, val)
}

fn write_guest_bytes_offset(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    guest_ptr: u32,
    offset: u32,
    data: &[u8],
) -> bool {
    let Some(ptr) = guest_ptr.checked_add(offset) else {
        return false;
    };
    write_guest_bytes(env, ptr, data)
}

fn guest_ptr_u32(guest_ptr: i32) -> Option<u32> {
    u32::try_from(guest_ptr).ok()
}

fn non_null_guest_ptr(guest_ptr: i32) -> Option<u32> {
    let guest_ptr = guest_ptr_u32(guest_ptr)?;
    (guest_ptr != 0).then_some(guest_ptr)
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
    let guest_ptr = guest_ptr_u32(guest_ptr)?;
    if len == 0 {
        return Some(Vec::new());
    }

    let (state, store) = env.data_and_store_mut();
    let memory = state.memory.clone()?;
    let view = memory.view(&store);

    let mut out = Vec::new();
    out.try_reserve_exact(len).ok()?;
    out.resize_with(len, MaybeUninit::uninit);
    let initialized = view.read_uninit(guest_ptr as u64, &mut out).ok()?;
    Some(initialized.to_vec())
}

fn allocate_guest_memory(env: &mut FunctionEnvMut<WasmCapiEnv>, len: usize) -> Option<u32> {
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
    u32::try_from(guest_ptr).ok()
}

fn free_guest_memory(env: &mut FunctionEnvMut<WasmCapiEnv>, guest_ptr: u32) {
    if guest_ptr == 0 {
        return;
    }

    let Some(free_fn) = env.data().free_fn.clone() else {
        return;
    };
    let Ok(guest_ptr) = i32::try_from(guest_ptr) else {
        return;
    };
    let (_, mut store_ref) = env.data_and_store_mut();
    let _ = free_fn.call(&mut store_ref, guest_ptr);
}

fn allocate_guest_bytes(env: &mut FunctionEnvMut<WasmCapiEnv>, data: &[u8]) -> Option<u32> {
    let guest_ptr = allocate_guest_memory(env, data.len())?;
    if !data.is_empty() && !write_guest_bytes(env, guest_ptr, data) {
        free_guest_memory(env, guest_ptr);
        return None;
    }
    Some(guest_ptr)
}

fn read_u32(env: &mut FunctionEnvMut<WasmCapiEnv>, ptr: i32) -> Option<u32> {
    let bytes = read_guest_bytes(env, ptr, 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_i32(env: &mut FunctionEnvMut<WasmCapiEnv>, ptr: i32) -> Option<i32> {
    Some(read_u32(env, ptr)? as i32)
}

fn read_u64(env: &mut FunctionEnvMut<WasmCapiEnv>, ptr: i32) -> Option<u64> {
    let bytes = read_guest_bytes(env, ptr, 8)?;
    Some(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn write_guest_vec_header(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    vec_ptr: u32,
    len: u32,
    data_ptr: u32,
) -> bool {
    write_guest_u32(env, vec_ptr, len)
        && write_guest_u32_offset(env, vec_ptr, WASM_VEC_DATA_OFFSET as u32, data_ptr)
}

fn allocate_guest_vec_header(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    len: u32,
    data_ptr: u32,
) -> Option<u32> {
    let vec_ptr = allocate_guest_memory(env, WASM_VEC_SIZE)?;
    if !write_guest_vec_header(env, vec_ptr, len, data_ptr) {
        free_guest_memory(env, vec_ptr);
        return None;
    }
    Some(vec_ptr)
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

    let data_ptr = if bytes.is_empty() {
        0
    } else {
        let Some(ptr) = allocate_guest_bytes(env, bytes) else {
            return false;
        };
        ptr
    };
    if !write_guest_vec_header(env, vec_ptr, len, data_ptr) {
        free_guest_memory(env, data_ptr);
        return false;
    }
    true
}

fn allocate_name(env: &mut FunctionEnvMut<WasmCapiEnv>, name: &str) -> i32 {
    let Ok(len) = u32::try_from(name.len()) else {
        return INVALID_HANDLE;
    };
    let data_ptr = if name.is_empty() {
        0
    } else {
        match allocate_guest_bytes(env, name.as_bytes()) {
            Some(ptr) => ptr,
            None => return INVALID_HANDLE,
        }
    };
    match allocate_guest_vec_header(env, len, data_ptr) {
        Some(ptr) => ptr as i32,
        None => {
            free_guest_memory(env, data_ptr);
            INVALID_HANDLE
        }
    }
}

fn write_handle_vec(env: &mut FunctionEnvMut<WasmCapiEnv>, out_ptr: i32, handles: &[i32]) -> bool {
    let Some(out_ptr) = non_null_guest_ptr(out_ptr) else {
        return false;
    };
    let Ok(len) = u32::try_from(handles.len()) else {
        return false;
    };
    let Some(byte_len) = handles.len().checked_mul(4) else {
        return false;
    };

    let data_ptr = if handles.is_empty() {
        0
    } else {
        let Some(ptr) = allocate_guest_memory(env, byte_len) else {
            return false;
        };
        for (index, handle) in handles.iter().enumerate() {
            let Ok(handle) = u32::try_from(*handle) else {
                free_guest_memory(env, ptr);
                return false;
            };
            let Some(offset) = u32::try_from(index)
                .ok()
                .and_then(|index| index.checked_mul(4))
            else {
                free_guest_memory(env, ptr);
                return false;
            };
            if !write_guest_u32_offset(env, ptr, offset, handle) {
                free_guest_memory(env, ptr);
                return false;
            }
        }
        ptr
    };

    if !write_guest_vec_header(env, out_ptr, len, data_ptr) {
        free_guest_memory(env, data_ptr);
        return false;
    }
    true
}

fn read_handle_vec(env: &mut FunctionEnvMut<WasmCapiEnv>, vec_ptr: i32) -> Option<Vec<i32>> {
    if vec_ptr <= INVALID_HANDLE {
        return None;
    }
    let size = read_u32(env, vec_ptr)? as usize;
    let data_ptr = read_i32(env, guest_ptr_with_offset(vec_ptr, WASM_VEC_DATA_OFFSET)?)?;
    let byte_len = size.checked_mul(4)?;
    let bytes = read_guest_bytes(env, data_ptr, byte_len)?;
    let mut out = Vec::with_capacity(size);
    for chunk in bytes.chunks_exact(4) {
        out.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as i32);
    }
    Some(out)
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
            val_ptr + WASM_VAL_PAYLOAD_OFFSET as i32,
        )?)),
        Type::I64 => Some(Value::I64(
            read_u64(env, val_ptr + WASM_VAL_PAYLOAD_OFFSET as i32)? as i64,
        )),
        Type::F32 => {
            let raw = read_u32(env, val_ptr + WASM_VAL_PAYLOAD_OFFSET as i32)?;
            Some(Value::F32(f32::from_bits(raw)))
        }
        Type::F64 => {
            let raw = read_u64(env, val_ptr + WASM_VAL_PAYLOAD_OFFSET as i32)?;
            Some(Value::F64(f64::from_bits(raw)))
        }
        // Reference values require object-handle marshalling that this import
        // bridge does not implement yet.
        Type::FuncRef | Type::ExternRef | Type::V128 | Type::ExceptionRef => None,
    }
}

fn write_wasm_val(env: &mut FunctionEnvMut<WasmCapiEnv>, val_ptr: i32, value: &Value) -> bool {
    let Some(val_ptr) = non_null_guest_ptr(val_ptr) else {
        return false;
    };
    let Some(kind) = type_to_wasm_kind(value.ty()) else {
        return false;
    };
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
        // Reference values are intentionally rejected above until the bridge
        // can preserve them as handles instead of silently turning them null.
        Value::FuncRef(_) | Value::ExternRef(_) | Value::V128(_) | Value::ExceptionRef(_) => false,
    }
}

fn read_limits(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    limits_ptr: i32,
) -> Option<(u32, Option<u32>)> {
    if limits_ptr <= 0 {
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
        return 0;
    };
    let (_, store) = env.data_and_store_mut();
    match Module::new(&store, bytes) {
        Ok(module) => env.data_mut().state.insert(WasmObject::Module(module)),
        Err(_) => 0,
    }
}

fn wasm_module_validate(mut env: FunctionEnvMut<WasmCapiEnv>, _store: i32, bytes_ptr: i32) -> i32 {
    let Some(bytes) = read_byte_vec(&mut env, bytes_ptr) else {
        return 0;
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
        _ => return 0,
    };
    allocate_name(&mut env, &name)
}

fn wasm_importtype_name(mut env: FunctionEnvMut<WasmCapiEnv>, import_handle: i32) -> i32 {
    let name = match env.data().state.get(import_handle) {
        Some(WasmObject::ImportType { name, .. }) => name.clone(),
        _ => return 0,
    };
    allocate_name(&mut env, &name)
}

fn wasm_importtype_type(mut env: FunctionEnvMut<WasmCapiEnv>, import_handle: i32) -> i32 {
    let ty = match env.data().state.get(import_handle) {
        Some(WasmObject::ImportType { ty, .. }) => ty.clone(),
        _ => return 0,
    };
    insert(&mut env, WasmObject::ExternType(ty))
}

fn wasm_exporttype_name(mut env: FunctionEnvMut<WasmCapiEnv>, export_handle: i32) -> i32 {
    let name = match env.data().state.get(export_handle) {
        Some(WasmObject::ExportType { name, .. }) => name.clone(),
        _ => return 0,
    };
    allocate_name(&mut env, &name)
}

fn wasm_exporttype_type(mut env: FunctionEnvMut<WasmCapiEnv>, export_handle: i32) -> i32 {
    let ty = match env.data().state.get(export_handle) {
        Some(WasmObject::ExportType { ty, .. }) => ty.clone(),
        _ => return 0,
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
        _ => return 0,
    };
    insert(&mut env, WasmObject::FuncType(ty))
}

fn wasm_functype_copy(mut env: FunctionEnvMut<WasmCapiEnv>, type_handle: i32) -> i32 {
    let ty = match env.data().state.get(type_handle) {
        Some(WasmObject::FuncType(ty)) => ty.clone(),
        _ => return 0,
    };
    insert(&mut env, WasmObject::FuncType(ty))
}

fn write_valtype_vec_for_types(env: &mut FunctionEnvMut<WasmCapiEnv>, types: &[Type]) -> i32 {
    let handles: Vec<i32> = types
        .iter()
        .map(|ty| insert(env, WasmObject::ValType(*ty)))
        .collect();
    let Some(vec_ptr) = allocate_guest_vec_header(env, 0, 0) else {
        return INVALID_HANDLE;
    };
    if !write_handle_vec(env, vec_ptr as i32, &handles) {
        free_guest_memory(env, vec_ptr);
        return INVALID_HANDLE;
    }
    vec_ptr as i32
}

fn wasm_functype_params(mut env: FunctionEnvMut<WasmCapiEnv>, type_handle: i32) -> i32 {
    let params = match env.data().state.get(type_handle) {
        Some(WasmObject::FuncType(ty)) => ty.params().to_vec(),
        _ => return 0,
    };
    write_valtype_vec_for_types(&mut env, &params)
}

fn wasm_functype_results(mut env: FunctionEnvMut<WasmCapiEnv>, type_handle: i32) -> i32 {
    let results = match env.data().state.get(type_handle) {
        Some(WasmObject::FuncType(ty)) => ty.results().to_vec(),
        _ => return 0,
    };
    write_valtype_vec_for_types(&mut env, &results)
}

fn wasm_valtype_new(mut env: FunctionEnvMut<WasmCapiEnv>, kind: i32) -> i32 {
    match wasm_kind_to_type(kind) {
        Some(ty) => insert(&mut env, WasmObject::ValType(ty)),
        None => 0,
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
        return 0;
    };
    insert(
        &mut env,
        WasmObject::MemoryType(MemoryType::new(Pages(min), max.map(Pages), false)),
    )
}

fn wasm_memory_new(mut env: FunctionEnvMut<WasmCapiEnv>, _store: i32, type_handle: i32) -> i32 {
    let ty = match env.data().state.get(type_handle) {
        Some(WasmObject::MemoryType(ty)) => *ty,
        _ => return 0,
    };
    match Memory::new(&mut env, ty) {
        Ok(memory) => insert(&mut env, WasmObject::Memory(memory)),
        Err(_) => 0,
    }
}

fn wasm_memory_size(env: FunctionEnvMut<WasmCapiEnv>, memory_handle: i32) -> i32 {
    match env.data().state.get(memory_handle) {
        Some(WasmObject::Memory(memory)) => memory.size(&env).0 as i32,
        Some(WasmObject::Extern(WasmExtern::Memory(memory))) => memory.size(&env).0 as i32,
        _ => 0,
    }
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

fn copy_wasmer_memory_to_guest(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    memory: &Memory,
    guest_ptr: u32,
    len: usize,
) -> bool {
    let mut offset = 0usize;
    while offset < len {
        let chunk_len = WASM_C_API_SHADOW_COPY_CHUNK_SIZE.min(len - offset);
        let mut bytes = vec![0u8; chunk_len];
        {
            let view = memory.view(&*env);
            if view.read(offset as u64, &mut bytes).is_err() {
                return false;
            }
        }
        let Some(offset_u32) = u32::try_from(offset).ok() else {
            return false;
        };
        let Some(dst) = guest_ptr.checked_add(offset_u32) else {
            return false;
        };
        if !write_guest_bytes(env, dst, &bytes) {
            return false;
        }
        let Some(next_offset) = offset.checked_add(chunk_len) else {
            return false;
        };
        offset = next_offset;
    }
    true
}

fn copy_guest_memory_to_wasmer(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    guest_ptr: u32,
    memory: &Memory,
    len: usize,
) -> bool {
    let mut offset = 0usize;
    while offset < len {
        let chunk_len = WASM_C_API_SHADOW_COPY_CHUNK_SIZE.min(len - offset);
        let Some(offset_u32) = u32::try_from(offset).ok() else {
            return false;
        };
        let Some(src) = guest_ptr.checked_add(offset_u32) else {
            return false;
        };
        let Ok(src) = i32::try_from(src) else {
            return false;
        };
        let Some(bytes) = read_guest_bytes(env, src, chunk_len) else {
            return false;
        };
        let view = memory.view(&*env);
        if view.write(offset as u64, &bytes).is_err() {
            return false;
        }
        let Some(next_offset) = offset.checked_add(chunk_len) else {
            return false;
        };
        offset = next_offset;
    }
    true
}

fn wasm_memory_data_size(env: FunctionEnvMut<WasmCapiEnv>, memory_handle: i32) -> i32 {
    let Some(memory) = memory_from_handle(&env, memory_handle) else {
        return INVALID_HANDLE;
    };
    // Shared memories can change concurrently while guest code is executing.
    // Since this bridge exposes memory data through guest-owned shadow buffers
    // instead of direct host pointers, fail closed instead of publishing a
    // stale size for a shadow pointer that cannot be safely provided.
    if memory.ty(&env).shared {
        return 0;
    }
    i32::try_from(memory.view(&env).data_size()).unwrap_or(0)
}

fn wasm_memory_data(mut env: FunctionEnvMut<WasmCapiEnv>, memory_handle: i32) -> i32 {
    let Some(memory) = memory_from_handle(&env, memory_handle) else {
        return INVALID_HANDLE;
    };

    // `wasm_memory_data` normally exposes a direct host pointer. In this
    // guest-imported C API bridge, the guest cannot safely receive a host
    // pointer, so we return a guest allocation that shadows the Wasmer memory.
    // Shared memories cannot be coherently represented by such a snapshot, so
    // report a null pointer instead of exposing a racy partial copy.
    if memory.ty(&env).shared {
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
    let mut allocated_new = false;
    let shadow = if let Some(shadow) = existing {
        if shadow.len >= size {
            MemoryShadow {
                guest_ptr: shadow.guest_ptr,
                len: size,
            }
        } else {
            let Some(guest_ptr) = allocate_guest_memory(&mut env, size) else {
                return INVALID_HANDLE;
            };
            free_guest_memory(&mut env, shadow.guest_ptr);
            allocated_new = true;
            MemoryShadow {
                guest_ptr,
                len: size,
            }
        }
    } else {
        let Some(guest_ptr) = allocate_guest_memory(&mut env, size) else {
            return INVALID_HANDLE;
        };
        allocated_new = true;
        MemoryShadow {
            guest_ptr,
            len: size,
        }
    };

    if !copy_wasmer_memory_to_guest(&mut env, &memory, shadow.guest_ptr, size) {
        if allocated_new {
            free_guest_memory(&mut env, shadow.guest_ptr);
            env.data_mut().state.memory_shadows.remove(&memory_handle);
        }
        return INVALID_HANDLE;
    }

    env.data_mut()
        .state
        .memory_shadows
        .insert(memory_handle, shadow);
    i32::try_from(shadow.guest_ptr).unwrap_or(INVALID_HANDLE)
}

fn sync_memory_shadows_to_wasmer(env: &mut FunctionEnvMut<WasmCapiEnv>) {
    let shadows = std::mem::take(&mut env.data_mut().state.memory_shadows);

    for (handle, shadow) in shadows {
        let Some(memory) = memory_from_handle(env, handle) else {
            continue;
        };
        if memory.ty(&*env).shared {
            continue;
        };
        let _ = copy_guest_memory_to_wasmer(env, shadow.guest_ptr, &memory, shadow.len);
        env.data_mut().state.memory_shadows.insert(handle, shadow);
    }
}

fn refresh_memory_shadows_from_wasmer(env: &mut FunctionEnvMut<WasmCapiEnv>) {
    let shadows = std::mem::take(&mut env.data_mut().state.memory_shadows);

    for (handle, shadow) in shadows {
        let Some(memory) = memory_from_handle(env, handle) else {
            continue;
        };
        if memory.ty(&*env).shared {
            continue;
        }
        let _ = copy_wasmer_memory_to_guest(env, &memory, shadow.guest_ptr, shadow.len);
        env.data_mut().state.memory_shadows.insert(handle, shadow);
    }
}

fn wasm_globaltype_new(
    mut env: FunctionEnvMut<WasmCapiEnv>,
    valtype_handle: i32,
    mutability: i32,
) -> i32 {
    let ty = match env.data().state.get(valtype_handle) {
        Some(WasmObject::ValType(ty)) => *ty,
        _ => return 0,
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
        _ => return 0,
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
        _ => return 0,
    };
    let Some(value) = read_wasm_val(&mut env, val_ptr, ty.ty) else {
        return 0;
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
        return 0;
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
        write_guest_u32(&mut env, trap_out_ptr, 0);
    }
    let module = match env.data().state.get(module_handle) {
        Some(WasmObject::Module(module)) => module.clone(),
        _ => return 0,
    };

    let import_handles = read_handle_vec(&mut env, imports_vec_ptr).unwrap_or_default();
    let mut imports = Imports::new();
    for (import, handle) in module.imports().zip(import_handles.into_iter()) {
        let Some(ext) = clone_extern_from_handle(&env, handle) else {
            return 0;
        };
        imports.define(import.module(), import.name(), ext);
    }

    match Instance::new(&mut env, &module, &imports) {
        Ok(instance) => insert(&mut env, WasmObject::Instance(instance)),
        Err(err) => {
            if let Some(trap_out_ptr) = non_null_guest_ptr(trap_out_ptr) {
                let trap = insert(&mut env, WasmObject::Trap(err.to_string()));
                write_guest_u32(&mut env, trap_out_ptr, u32::try_from(trap).unwrap_or(0));
            }
            0
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
        _ => return 0,
    };
    insert(&mut env, object)
}

fn wasm_extern_as_global(mut env: FunctionEnvMut<WasmCapiEnv>, extern_handle: i32) -> i32 {
    let object = match env.data().state.get(extern_handle) {
        Some(WasmObject::Extern(WasmExtern::Global(global))) => WasmObject::Global(global.clone()),
        _ => return 0,
    };
    insert(&mut env, object)
}

fn wasm_extern_as_table(mut env: FunctionEnvMut<WasmCapiEnv>, extern_handle: i32) -> i32 {
    let object = match env.data().state.get(extern_handle) {
        Some(WasmObject::Extern(WasmExtern::Table(table))) => WasmObject::Table(table.clone()),
        _ => return 0,
    };
    insert(&mut env, object)
}

fn wasm_extern_as_memory(mut env: FunctionEnvMut<WasmCapiEnv>, extern_handle: i32) -> i32 {
    let object = match env.data().state.get(extern_handle) {
        Some(WasmObject::Extern(WasmExtern::Memory(memory))) => WasmObject::Memory(memory.clone()),
        _ => return 0,
    };
    insert(&mut env, object)
}

fn wasm_func_copy(mut env: FunctionEnvMut<WasmCapiEnv>, func_handle: i32) -> i32 {
    let object = match env.data().state.get(func_handle) {
        Some(WasmObject::Func(func)) => WasmObject::Func(func.clone()),
        Some(WasmObject::Extern(WasmExtern::Func(func))) => WasmObject::Func(func.clone()),
        _ => return 0,
    };
    insert(&mut env, object)
}

fn wasm_global_copy(mut env: FunctionEnvMut<WasmCapiEnv>, global_handle: i32) -> i32 {
    let Some(global) = global_from_handle(&env, global_handle) else {
        return 0;
    };
    insert(&mut env, WasmObject::Global(global))
}

fn wasm_memory_copy(mut env: FunctionEnvMut<WasmCapiEnv>, memory_handle: i32) -> i32 {
    let Some(memory) = memory_from_handle(&env, memory_handle) else {
        return 0;
    };
    insert(&mut env, WasmObject::Memory(memory))
}

fn wasm_table_copy(mut env: FunctionEnvMut<WasmCapiEnv>, table_handle: i32) -> i32 {
    let object = match env.data().state.get(table_handle) {
        Some(WasmObject::Table(table)) => WasmObject::Table(table.clone()),
        Some(WasmObject::Extern(WasmExtern::Table(table))) => WasmObject::Table(table.clone()),
        _ => return 0,
    };
    insert(&mut env, object)
}

fn wasm_func_as_extern(mut env: FunctionEnvMut<WasmCapiEnv>, func_handle: i32) -> i32 {
    let object = match env.data().state.get(func_handle) {
        Some(WasmObject::Func(func)) => WasmObject::Extern(WasmExtern::Func(func.clone())),
        _ => return 0,
    };
    insert(&mut env, object)
}

fn wasm_global_as_extern(mut env: FunctionEnvMut<WasmCapiEnv>, global_handle: i32) -> i32 {
    let Some(global) = global_from_handle(&env, global_handle) else {
        return 0;
    };
    insert(&mut env, WasmObject::Extern(WasmExtern::Global(global)))
}

fn wasm_memory_as_extern(mut env: FunctionEnvMut<WasmCapiEnv>, memory_handle: i32) -> i32 {
    let Some(memory) = memory_from_handle(&env, memory_handle) else {
        return 0;
    };
    insert(&mut env, WasmObject::Extern(WasmExtern::Memory(memory)))
}

fn wasm_table_as_extern(mut env: FunctionEnvMut<WasmCapiEnv>, table_handle: i32) -> i32 {
    let object = match env.data().state.get(table_handle) {
        Some(WasmObject::Table(table)) => WasmObject::Extern(WasmExtern::Table(table.clone())),
        _ => return 0,
    };
    insert(&mut env, object)
}

fn wasm_func_type(mut env: FunctionEnvMut<WasmCapiEnv>, func_handle: i32) -> i32 {
    let func = match env.data().state.get(func_handle) {
        Some(WasmObject::Func(func)) => func.clone(),
        Some(WasmObject::Extern(WasmExtern::Func(func))) => func.clone(),
        _ => return 0,
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
        .unwrap_or(0);
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
                .unwrap_or(0);
            for (index, value) in results.iter().enumerate() {
                let val_ptr = result_data_ptr + (index * WASM_VAL_SIZE) as i32;
                write_wasm_val(&mut env, val_ptr, value);
            }
            0
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
    let data_ptr = if byte_len == 0 {
        0
    } else {
        allocate_guest_memory(env, byte_len)? as i32
    };
    for (index, value) in values.iter().enumerate() {
        let val_ptr = data_ptr + (index * WASM_VAL_SIZE) as i32;
        if !write_wasm_val(env, val_ptr, value) {
            if data_ptr > 0 {
                free_guest_memory(env, data_ptr as u32);
            }
            return None;
        }
    }

    let vec_ptr = match allocate_guest_vec_header(env, len, data_ptr as u32) {
        Some(ptr) => ptr as i32,
        None => {
            if data_ptr > 0 {
                free_guest_memory(env, data_ptr as u32);
            }
            return None;
        }
    };
    Some((vec_ptr, data_ptr))
}

fn allocate_uninitialized_wasm_val_vec(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    len: usize,
) -> Option<(i32, i32)> {
    let len_u32 = u32::try_from(len).ok()?;
    let byte_len = len.checked_mul(WASM_VAL_SIZE)?;
    let data_ptr = if byte_len == 0 {
        0
    } else {
        allocate_guest_memory(env, byte_len)? as i32
    };
    let vec_ptr = match allocate_guest_vec_header(env, len_u32, data_ptr as u32) {
        Some(ptr) => ptr as i32,
        None => {
            if data_ptr > 0 {
                free_guest_memory(env, data_ptr as u32);
            }
            return None;
        }
    };
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
    match func.call(
        env,
        &[
            Value::I32(callback_env),
            Value::I32(args_vec_ptr),
            Value::I32(results_vec_ptr),
        ],
    ) {
        Ok(values) => match values.first() {
            Some(Value::I32(value)) => *value,
            Some(Value::I64(value)) => *value as i32,
            _ => 0,
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
    if callback <= 0 {
        return 0;
    }
    let ty = match env.data().state.get(type_handle) {
        Some(WasmObject::FuncType(ty)) => ty.clone(),
        _ => return 0,
    };
    let Some(func_env) = env.data().func_env.clone() else {
        return 0;
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
        if trap != 0 {
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
        Some(WasmObject::Table(table)) => table.size(&env) as i32,
        Some(WasmObject::Extern(WasmExtern::Table(table))) => table.size(&env) as i32,
        _ => 0,
    }
}

fn wasm_table_grow(
    mut env: FunctionEnvMut<WasmCapiEnv>,
    table_handle: i32,
    delta: i32,
    _init: i32,
) -> i32 {
    let Ok(delta) = u32::try_from(delta) else {
        return BOOL_FALSE;
    };
    let table = match env.data().state.get(table_handle) {
        Some(WasmObject::Table(table)) => table.clone(),
        Some(WasmObject::Extern(WasmExtern::Table(table))) => table.clone(),
        _ => return BOOL_FALSE,
    };
    if table.grow(&mut env, delta, Value::FuncRef(None)).is_ok() {
        BOOL_TRUE
    } else {
        BOOL_FALSE
    }
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
    if out_ptr <= 0 || size < 0 {
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
    let data_ptr = if byte_len == 0 {
        0
    } else {
        let Some(data_ptr) = allocate_guest_memory(&mut env, byte_len) else {
            return;
        };
        data_ptr
    };
    let Some(out_ptr) = non_null_guest_ptr(out_ptr) else {
        free_guest_memory(&mut env, data_ptr);
        return;
    };
    if !write_guest_vec_header(&mut env, out_ptr, len, data_ptr) {
        free_guest_memory(&mut env, data_ptr);
    }
}

fn noop_delete(_env: FunctionEnvMut<WasmCapiEnv>, _handle: i32) {}

fn vec_delete(mut env: FunctionEnvMut<WasmCapiEnv>, vec_ptr: i32) {
    if let Some(vec_ptr) = non_null_guest_ptr(vec_ptr) {
        write_guest_vec_header(&mut env, vec_ptr, 0, 0);
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
        "wasm_table_delete" => WasmerFunction::new_typed_with_env(store, fe, delete_handle),
        "wasm_table_copy" => WasmerFunction::new_typed_with_env(store, fe, wasm_table_copy),
        "wasm_table_size" => WasmerFunction::new_typed_with_env(store, fe, wasm_table_size),
        "wasm_table_grow" => WasmerFunction::new_typed_with_env(store, fe, wasm_table_grow),
        "wasm_table_as_extern" => WasmerFunction::new_typed_with_env(store, fe, wasm_table_as_extern),
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
        INVALID_HANDLE, Type, WASM_EXTERNREF, WASM_F64, WASM_FUNCREF, WASM_I32, WasmCAPIVersion,
        WasmCapiEnv, WasmCapiState, WasmObject, guest_ptr_u32, guest_ptr_with_offset,
        module_needs_wasm_c_api, non_null_guest_ptr, type_to_wasm_kind, wasm_kind_to_type,
        wasm_memory_data, wasm_memory_data_size,
    };
    use wasmer_api::{FunctionEnv, Memory, MemoryType, Module, Pages, Store};
    use wat::parse_str;

    const EMPTY_WASM_MODULE: &[u8] = b"\0asm\x01\0\0\0";

    fn compile_wat(store: &Store, wat: &str) -> Module {
        let wasm = parse_str(wat).expect("wat module parses");
        Module::new(store, wasm).expect("wat module compiles")
    }

    #[test]
    fn module_needs_wasm_c_api_detects_none() {
        let store = Store::default();
        let module = Module::new(&store, EMPTY_WASM_MODULE).expect("empty wasm module compiles");

        assert_eq!(module_needs_wasm_c_api(&module), None);
    }

    #[test]
    fn module_needs_wasm_c_api_detects_v0() {
        let store = Store::default();
        let module = compile_wat(
            &store,
            r#"(module
                (import "wasm_c_api_v0" "wasm_engine_new" (func))
            )"#,
        );

        assert_eq!(module_needs_wasm_c_api(&module), Some(WasmCAPIVersion::V0));
    }

    #[test]
    fn module_needs_wasm_c_api_detects_unknown_version() {
        let store = Store::default();
        let module = compile_wat(
            &store,
            r#"(module
                (import "wasm_c_api_v1" "wasm_engine_new" (func))
            )"#,
        );

        assert_eq!(
            module_needs_wasm_c_api(&module),
            Some(WasmCAPIVersion::Unknown)
        );
    }

    #[test]
    fn module_needs_wasm_c_api_detects_mixed_versions() {
        let store = Store::default();
        let module = compile_wat(
            &store,
            r#"(module
                (import "wasm_c_api_v0" "wasm_engine_new" (func))
                (import "wasm_c_api_v1" "wasm_store_new" (func))
            )"#,
        );

        assert_eq!(
            module_needs_wasm_c_api(&module),
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
        assert_eq!(guest_ptr_u32(-1), None);
        assert_eq!(guest_ptr_u32(0), Some(0));
        assert_eq!(non_null_guest_ptr(0), None);
        assert_eq!(non_null_guest_ptr(1), Some(1));
        assert_eq!(guest_ptr_with_offset(i32::MAX, 4), None);
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
            0
        );
        assert_eq!(
            wasm_memory_data(func_env.into_mut(&mut store), memory_handle),
            INVALID_HANDLE
        );
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
}
