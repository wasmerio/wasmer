use anyhow::{Context, Result, bail};
use std::{
    collections::{HashMap, VecDeque},
    fmt::Display,
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
        {
            let mut guard = self
                .func_env
                .lock()
                .expect("poisoned WasmCapiSession mutex");
            *guard = Some(func_env.clone());
        }
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

/// Runtime hooks that provide `wasm_c_api_v0` imports for WASIX guests.
#[derive(Clone, Default)]
pub struct WasmCapiRuntimeHooks {
    sessions: Arc<Mutex<HashMap<usize, VecDeque<WasmCapiSession>>>>,
}

impl WasmCapiRuntimeHooks {
    /// Creates an empty hook set.
    pub fn new() -> Self {
        Self::default()
    }

    fn module_key(module: &Module) -> usize {
        module as *const Module as usize
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

#[derive(Default)]
struct WasmCapiEnv {
    memory: Option<Memory>,
    malloc_fn: Option<TypedFunction<i32, i32>>,
    table: Option<Table>,
    state: WasmCapiState,
    func_env: Option<FunctionEnv<WasmCapiEnv>>,
}

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

#[derive(Default)]
struct WasmCapiState {
    next_handle: u32,
    objects: HashMap<u32, WasmObject>,
    memory_shadows: HashMap<u32, (u32, usize)>,
}

impl WasmCapiState {
    fn insert(&mut self, object: WasmObject) -> i32 {
        let mut handle = self.next_handle.max(1);
        while self.objects.contains_key(&handle) {
            handle = handle.saturating_add(1).max(1);
        }
        self.next_handle = handle.saturating_add(1).max(1);
        self.objects.insert(handle, object);
        handle as i32
    }

    fn get(&self, handle: i32) -> Option<&WasmObject> {
        if handle <= 0 {
            return None;
        }
        self.objects.get(&(handle as u32))
    }

    fn remove(&mut self, handle: i32) {
        if handle > 0 {
            self.objects.remove(&(handle as u32));
            self.memory_shadows.remove(&(handle as u32));
        }
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

fn read_guest_bytes(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    guest_ptr: i32,
    len: usize,
) -> Option<Vec<u8>> {
    if guest_ptr < 0 {
        return None;
    }
    let (state, store) = env.data_and_store_mut();
    let memory = state.memory.clone()?;
    let view = memory.view(&store);
    let mut out = vec![0u8; len];
    view.read(guest_ptr as u64, &mut out).ok()?;
    Some(out)
}

fn allocate_guest_bytes(env: &mut FunctionEnvMut<WasmCapiEnv>, data: &[u8]) -> Option<u32> {
    let malloc_fn = env.data().malloc_fn.clone()?;
    let len = i32::try_from(data.len()).ok()?;
    let guest_ptr: i32 = {
        let (_, mut store_ref) = env.data_and_store_mut();
        malloc_fn.call(&mut store_ref, len).ok()?
    };
    if guest_ptr <= 0 {
        return None;
    }
    if !write_guest_bytes(env, guest_ptr as u32, data) {
        return None;
    }
    Some(guest_ptr as u32)
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

fn read_byte_vec(env: &mut FunctionEnvMut<WasmCapiEnv>, vec_ptr: i32) -> Option<Vec<u8>> {
    if vec_ptr <= 0 {
        return None;
    }
    let size = read_u32(env, vec_ptr)? as usize;
    let data_ptr = read_i32(env, vec_ptr + 4)?;
    if size == 0 {
        return Some(Vec::new());
    }
    read_guest_bytes(env, data_ptr, size)
}

fn write_byte_vec(env: &mut FunctionEnvMut<WasmCapiEnv>, vec_ptr: i32, bytes: &[u8]) -> bool {
    if vec_ptr <= 0 {
        return false;
    }
    let data_ptr = if bytes.is_empty() {
        0
    } else {
        let Some(ptr) = allocate_guest_bytes(env, bytes) else {
            return false;
        };
        ptr
    };
    write_guest_u32(env, vec_ptr as u32, bytes.len() as u32)
        && write_guest_u32(env, vec_ptr as u32 + 4, data_ptr)
}

fn allocate_name(env: &mut FunctionEnvMut<WasmCapiEnv>, name: &str) -> i32 {
    let mut bytes = Vec::with_capacity(8);
    let data_ptr = if name.is_empty() {
        0
    } else {
        match allocate_guest_bytes(env, name.as_bytes()) {
            Some(ptr) => ptr,
            None => return 0,
        }
    };
    bytes.extend_from_slice(&(name.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&data_ptr.to_le_bytes());
    allocate_guest_bytes(env, &bytes).unwrap_or(0) as i32
}

fn write_handle_vec(env: &mut FunctionEnvMut<WasmCapiEnv>, out_ptr: i32, handles: &[i32]) -> bool {
    if out_ptr <= 0 {
        return false;
    }
    let mut bytes = Vec::with_capacity(handles.len() * 4);
    for handle in handles {
        bytes.extend_from_slice(&(*handle as u32).to_le_bytes());
    }
    let data_ptr = if bytes.is_empty() {
        0
    } else {
        let Some(ptr) = allocate_guest_bytes(env, &bytes) else {
            return false;
        };
        ptr
    };
    write_guest_u32(env, out_ptr as u32, handles.len() as u32)
        && write_guest_u32(env, out_ptr as u32 + 4, data_ptr)
}

fn read_handle_vec(env: &mut FunctionEnvMut<WasmCapiEnv>, vec_ptr: i32) -> Option<Vec<i32>> {
    let size = read_u32(env, vec_ptr)? as usize;
    let data_ptr = read_i32(env, vec_ptr + 4)?;
    let bytes = read_guest_bytes(env, data_ptr, size * 4)?;
    let mut out = Vec::with_capacity(size);
    for chunk in bytes.chunks_exact(4) {
        out.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as i32);
    }
    Some(out)
}

fn type_to_wasm_kind(ty: Type) -> u8 {
    match ty {
        Type::I32 => WASM_I32,
        Type::I64 => WASM_I64,
        Type::F32 => WASM_F32,
        Type::F64 => WASM_F64,
        Type::FuncRef => WASM_FUNCREF,
        Type::ExternRef => WASM_EXTERNREF,
        _ => WASM_EXTERNREF,
    }
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
        _ => -1,
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
        Type::FuncRef => Some(Value::FuncRef(None)),
        Type::ExternRef => Some(Value::ExternRef(None)),
        _ => None,
    }
}

fn write_wasm_val(env: &mut FunctionEnvMut<WasmCapiEnv>, val_ptr: i32, value: &Value) -> bool {
    if val_ptr <= 0 {
        return false;
    }
    let kind = type_to_wasm_kind(value.ty());
    if !write_guest_bytes(env, val_ptr as u32, &[kind]) {
        return false;
    }
    match value {
        Value::I32(v) => write_guest_bytes(
            env,
            val_ptr as u32 + WASM_VAL_PAYLOAD_OFFSET,
            &v.to_le_bytes(),
        ),
        Value::I64(v) => write_guest_bytes(
            env,
            val_ptr as u32 + WASM_VAL_PAYLOAD_OFFSET,
            &v.to_le_bytes(),
        ),
        Value::F32(v) => write_guest_bytes(
            env,
            val_ptr as u32 + WASM_VAL_PAYLOAD_OFFSET,
            &v.to_bits().to_le_bytes(),
        ),
        Value::F64(v) => write_guest_bytes(
            env,
            val_ptr as u32 + WASM_VAL_PAYLOAD_OFFSET,
            &v.to_bits().to_le_bytes(),
        ),
        _ => write_guest_u32(env, val_ptr as u32 + WASM_VAL_PAYLOAD_OFFSET, 0),
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
    let max = read_u32(env, limits_ptr + 4)?;
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
    env.data_mut().state.remove(handle);
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
        _ => -1,
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
    let vec_bytes = vec![0u8; 8];
    let Some(vec_ptr) = allocate_guest_bytes(env, &vec_bytes) else {
        return 0;
    };
    if !write_handle_vec(env, vec_ptr as i32, &handles) {
        return 0;
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
        Some(WasmObject::ValType(ty)) => type_to_wasm_kind(*ty) as i32,
        _ => -1,
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
    if delta < 0 {
        return 0;
    }
    let memory = match env.data().state.get(memory_handle) {
        Some(WasmObject::Memory(memory)) => memory.clone(),
        Some(WasmObject::Extern(WasmExtern::Memory(memory))) => memory.clone(),
        _ => return 0,
    };
    memory.grow(&mut env, Pages(delta as u32)).is_ok() as i32
}

fn memory_from_handle(env: &FunctionEnvMut<WasmCapiEnv>, memory_handle: i32) -> Option<Memory> {
    match env.data().state.get(memory_handle)? {
        WasmObject::Memory(memory) => Some(memory.clone()),
        WasmObject::Extern(WasmExtern::Memory(memory)) => Some(memory.clone()),
        _ => None,
    }
}

fn wasm_memory_data_size(env: FunctionEnvMut<WasmCapiEnv>, memory_handle: i32) -> i32 {
    let Some(memory) = memory_from_handle(&env, memory_handle) else {
        return 0;
    };
    i32::try_from(memory.view(&env).data_size()).unwrap_or(0)
}

fn wasm_memory_data(mut env: FunctionEnvMut<WasmCapiEnv>, memory_handle: i32) -> i32 {
    let Some(memory) = memory_from_handle(&env, memory_handle) else {
        return 0;
    };
    let view = memory.view(&env);
    let Ok(size) = usize::try_from(view.data_size()) else {
        return 0;
    };
    if size == 0 {
        return 0;
    }
    let mut bytes = vec![0u8; size];
    if view.read(0, &mut bytes).is_err() {
        return 0;
    }
    let key = memory_handle as u32;
    let existing = env.data().state.memory_shadows.get(&key).copied();
    let ptr = if let Some((ptr, len)) = existing {
        if len >= size {
            ptr
        } else {
            match allocate_guest_bytes(&mut env, &bytes) {
                Some(ptr) => {
                    env.data_mut().state.memory_shadows.insert(key, (ptr, size));
                    ptr
                }
                None => return 0,
            }
        }
    } else {
        match allocate_guest_bytes(&mut env, &bytes) {
            Some(ptr) => {
                env.data_mut().state.memory_shadows.insert(key, (ptr, size));
                ptr
            }
            None => return 0,
        }
    };
    if !write_guest_bytes(&mut env, ptr, &bytes) {
        return 0;
    }
    ptr as i32
}

fn sync_memory_shadows_to_wasmer(env: &mut FunctionEnvMut<WasmCapiEnv>) {
    let shadows: Vec<(u32, u32, usize)> = env
        .data()
        .state
        .memory_shadows
        .iter()
        .map(|(handle, (ptr, size))| (*handle, *ptr, *size))
        .collect();

    for (handle, ptr, size) in shadows {
        let Some(memory) = memory_from_handle(env, handle as i32) else {
            continue;
        };
        let Some(bytes) = read_guest_bytes(env, ptr as i32, size) else {
            continue;
        };
        let view = memory.view(&*env);
        let _ = view.write(0, &bytes);
    }
}

fn refresh_memory_shadows_from_wasmer(env: &mut FunctionEnvMut<WasmCapiEnv>) {
    let shadows: Vec<(u32, u32, usize)> = env
        .data()
        .state
        .memory_shadows
        .iter()
        .map(|(handle, (ptr, size))| (*handle, *ptr, *size))
        .collect();

    for (handle, ptr, size) in shadows {
        let Some(memory) = memory_from_handle(env, handle as i32) else {
            continue;
        };
        let view = memory.view(&*env);
        let mut bytes = vec![0u8; size];
        if view.read(0, &mut bytes).is_ok() {
            write_guest_bytes(env, ptr, &bytes);
        }
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
    if trap_out_ptr > 0 {
        write_guest_u32(&mut env, trap_out_ptr as u32, 0);
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
            if trap_out_ptr > 0 {
                let trap = insert(&mut env, WasmObject::Trap(err.to_string()));
                write_guest_u32(&mut env, trap_out_ptr as u32, trap as u32);
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
        _ => -1,
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
    let arg_data_ptr = read_i32(&mut env, args_vec_ptr + 4).unwrap_or(0);
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
            let result_data_ptr = read_i32(&mut env, results_vec_ptr + 4).unwrap_or(0);
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
    let data = vec![0u8; values.len() * WASM_VAL_SIZE];
    let data_ptr = if data.is_empty() {
        0
    } else {
        allocate_guest_bytes(env, &data)? as i32
    };
    for (index, value) in values.iter().enumerate() {
        let val_ptr = data_ptr + (index * WASM_VAL_SIZE) as i32;
        if !write_wasm_val(env, val_ptr, value) {
            return None;
        }
    }

    let mut vec_bytes = Vec::with_capacity(8);
    vec_bytes.extend_from_slice(&(values.len() as u32).to_le_bytes());
    vec_bytes.extend_from_slice(&(data_ptr as u32).to_le_bytes());
    let vec_ptr = allocate_guest_bytes(env, &vec_bytes)? as i32;
    Some((vec_ptr, data_ptr))
}

fn allocate_uninitialized_wasm_val_vec(
    env: &mut FunctionEnvMut<WasmCapiEnv>,
    len: usize,
) -> Option<(i32, i32)> {
    let data = vec![0u8; len * WASM_VAL_SIZE];
    let data_ptr = if data.is_empty() {
        0
    } else {
        allocate_guest_bytes(env, &data)? as i32
    };
    let mut vec_bytes = Vec::with_capacity(8);
    vec_bytes.extend_from_slice(&(len as u32).to_le_bytes());
    vec_bytes.extend_from_slice(&(data_ptr as u32).to_le_bytes());
    let vec_ptr = allocate_guest_bytes(env, &vec_bytes)? as i32;
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
    let callback = callback as u32;
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
    if delta < 0 {
        return 0;
    }
    let table = match env.data().state.get(table_handle) {
        Some(WasmObject::Table(table)) => table.clone(),
        Some(WasmObject::Extern(WasmExtern::Table(table))) => table.clone(),
        _ => return 0,
    };
    table
        .grow(&mut env, delta as u32, Value::FuncRef(None))
        .is_ok() as i32
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
    let bytes = if size <= 0 {
        Vec::new()
    } else {
        read_guest_bytes(&mut env, data_ptr, size as usize).unwrap_or_default()
    };
    write_byte_vec(&mut env, out_ptr, &bytes);
}

fn wasm_val_vec_new_uninitialized(mut env: FunctionEnvMut<WasmCapiEnv>, out_ptr: i32, size: i32) {
    if out_ptr <= 0 || size < 0 {
        return;
    }
    let bytes = vec![0u8; size as usize * WASM_VAL_SIZE];
    let data_ptr = if bytes.is_empty() {
        0
    } else {
        allocate_guest_bytes(&mut env, &bytes).unwrap_or(0)
    };
    write_guest_u32(&mut env, out_ptr as u32, size as u32);
    write_guest_u32(&mut env, out_ptr as u32 + 4, data_ptr);
}

fn noop_delete(_env: FunctionEnvMut<WasmCapiEnv>, _handle: i32) {}

fn vec_delete(mut env: FunctionEnvMut<WasmCapiEnv>, vec_ptr: i32) {
    if vec_ptr > 0 {
        write_guest_u32(&mut env, vec_ptr as u32, 0);
        write_guest_u32(&mut env, vec_ptr as u32 + 4, 0);
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
    use super::{WasmCAPIVersion, module_needs_wasm_c_api};
    use wasmer_api::{Module, Store};
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
}
