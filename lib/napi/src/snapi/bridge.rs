use std::collections::HashMap;
use std::convert::TryInto;
use std::ffi::{CStr, c_void};
use std::mem::MaybeUninit;
use std::ptr;
use std::thread::{self, ThreadId};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicPtr, Ordering};

use libc::free;
use parking_lot::{ReentrantMutex, const_mutex, const_reentrant_mutex};
use rusty_v8 as v8;
use rusty_v8::{ValueDeserializerHelper, ValueSerializerHelper};
use wasmer::{FunctionEnv, FunctionEnvMut, Value as WasmValue};

use super::{
    SnapiEnv, SnapiUnofficialHeapCodeStatistics, SnapiUnofficialHeapSpaceStatistics,
    SnapiUnofficialHeapStatistics,
};
use crate::RuntimeEnv;

const NAPI_OK: i32 = 0;
const NAPI_INVALID_ARG: i32 = 1;
const NAPI_OBJECT_EXPECTED: i32 = 2;
const NAPI_STRING_EXPECTED: i32 = 3;
const NAPI_NAME_EXPECTED: i32 = 4;
const NAPI_FUNCTION_EXPECTED: i32 = 5;
const NAPI_NUMBER_EXPECTED: i32 = 6;
const NAPI_BOOLEAN_EXPECTED: i32 = 7;
const NAPI_ARRAY_EXPECTED: i32 = 8;
const NAPI_GENERIC_FAILURE: i32 = 9;
const NAPI_PENDING_EXCEPTION: i32 = 10;
const NAPI_ESCAPE_CALLED_TWICE: i32 = 12;
const NAPI_BIGINT_EXPECTED: i32 = 16;
const NAPI_DATE_EXPECTED: i32 = 17;
const NAPI_ARRAYBUFFER_EXPECTED: i32 = 18;
const NAPI_TYPEOF_UNDEFINED: i32 = 0;
const NAPI_TYPEOF_NULL: i32 = 1;
const NAPI_TYPEOF_BOOLEAN: i32 = 2;
const NAPI_TYPEOF_NUMBER: i32 = 3;
const NAPI_TYPEOF_STRING: i32 = 4;
const NAPI_TYPEOF_SYMBOL: i32 = 5;
const NAPI_TYPEOF_OBJECT: i32 = 6;
const NAPI_TYPEOF_FUNCTION: i32 = 7;
const NAPI_TYPEOF_EXTERNAL: i32 = 8;
const NAPI_TYPEOF_BIGINT: i32 = 9;
const NAPI_STATIC: i32 = 1 << 10;

const TYPE_TAG_PRIVATE_KEY: &str = "snapi.type_tag";
const WRAP_PRIVATE_KEY: &str = "snapi.wrap";
const FINALIZER_PRIVATE_KEY: &str = "snapi.finalizer";
const BUFFER_PRIVATE_KEY: &str = "snapi.buffer";
const HELPER_OBJECT_NAME: &str = "__snapi";

static V8_INIT: OnceLock<()> = OnceLock::new();
static BRIDGE_LOCK: ReentrantMutex<()> = const_reentrant_mutex(());
static LIVE_ENVS: parking_lot::Mutex<Vec<usize>> = const_mutex(Vec::new());

unsafe extern "C" {
    fn v8__Context__Enter(this: *const v8::Context);
    fn v8__Context__Exit(this: *const v8::Context);
    fn v8__Context__GetIsolate(this: *const v8::Context) -> *mut v8::Isolate;
    fn v8__Context__New(
        isolate: *mut v8::Isolate,
        templ: *const v8::ObjectTemplate,
        global_object: *const v8::Value,
    ) -> *const v8::Context;
    fn v8__HandleScope__CONSTRUCT(buf: *mut RawHandleScope, isolate: *mut v8::Isolate);
    fn v8__HandleScope__DESTRUCT(this: *mut RawHandleScope);
    fn v8__Object__GetIsolate(this: *const v8::Object) -> *mut v8::Isolate;
    fn v8__Private__ForApi(isolate: *mut v8::Isolate, name: *const v8::String) -> *const v8::Private;
    fn v8__Proxy__GetHandler(this: *const v8::Proxy) -> *const v8::Value;
    fn v8__Proxy__GetTarget(this: *const v8::Proxy) -> *const v8::Value;
    fn v8__ArrayBuffer__NewBackingStore__with_data(
        data: *mut c_void,
        byte_length: usize,
        deleter: v8::BackingStoreDeleterCallback,
        deleter_data: *mut c_void,
    ) -> *mut v8::BackingStore;
    fn snapi_v8_set_fatal_error_handler(
        isolate: *mut v8::Isolate,
        callback: Option<extern "C" fn(*const i8, *const i8)>,
    );
    fn snapi_v8_set_oom_error_handler(
        isolate: *mut v8::Isolate,
        callback: Option<extern "C" fn(*const i8, bool)>,
    );
    fn snapi_v8_try_get_current_isolate() -> *mut v8::Isolate;
}

unsafe extern "C" fn noop_backing_store_deleter(
    _data: *mut c_void,
    _byte_length: usize,
    _deleter_data: *mut c_void,
) {
}

macro_rules! bridge_lock {
    () => {
        let _bridge_lock = BRIDGE_LOCK.lock();
    };
}

#[repr(C)]
struct RawHandleScope([usize; 3]);

impl RawHandleScope {
    unsafe fn new(isolate: *mut v8::Isolate) -> Self {
        let mut buf = MaybeUninit::<Self>::uninit();
        unsafe {
            v8__HandleScope__CONSTRUCT(buf.as_mut_ptr(), isolate);
            buf.assume_init()
        }
    }
}

impl Drop for RawHandleScope {
    fn drop(&mut self) {
        unsafe {
            v8__HandleScope__DESTRUCT(self);
        }
    }
}

#[derive(Clone, Copy)]
enum CallbackKind {
    Method,
    Getter,
    Setter,
}

struct SnapiRuntime {
    env_handle_scope: Option<RawHandleScope>,
    isolate: v8::OwnedIsolate,
    context: v8::Global<v8::Context>,
}

struct RefEntry {
    value: v8::Global<v8::Value>,
    refcount: u32,
}

struct DeferredEntry {
    resolver: v8::Global<v8::PromiseResolver>,
}

struct EscapableScopeState {
    escaped: bool,
}

struct TypeTagEntry {
    value: v8::Global<v8::Value>,
    lower: u64,
    upper: u64,
}

struct CallbackInvocation {
    argv_ids: Vec<u32>,
    this_id: u32,
    data_val: u64,
    new_target_id: u32,
}

struct CbRegistration {
    guest_env: u32,
    wasm_fn_ptr: u32,
    wasm_setter_fn_ptr: u32,
    data_val: u64,
}

struct CallbackBinding {
    state: SnapiEnv,
    reg_id: u32,
    kind: CallbackKind,
}

#[derive(Clone)]
struct ModuleImportAttributeRecord {
    key: String,
    value: String,
}

#[derive(Clone)]
struct ModuleRequestRecord {
    specifier: String,
    attributes: Vec<ModuleImportAttributeRecord>,
    phase: i32,
}

struct ModuleWrapHandle {
    wrapper_id: u32,
    synthetic_eval_steps_id: u32,
    source_object_id: u32,
    host_defined_option_id: u32,
    context: v8::Global<v8::Context>,
    module: v8::Global<v8::Module>,
    module_requests: Vec<ModuleRequestRecord>,
    resolve_cache: HashMap<String, u32>,
    linked_requests: Vec<u32>,
    has_top_level_await: bool,
    last_evaluation_promise: Option<v8::Global<v8::Promise>>,
}

struct ActiveScopeGuard {
    state: *mut SnapiEnvState,
    prev_scope: *mut c_void,
}

impl Drop for ActiveScopeGuard {
    fn drop(&mut self) {
        unsafe {
            (*self.state).active_scope = self.prev_scope;
        }
    }
}

pub struct SnapiEnvState {
    runtime: Option<SnapiRuntime>,
    owner_thread: ThreadId,
    isolate_handle: v8::IsolateHandle,
    guest_env_id: u32,
    guest_func_env: Option<FunctionEnv<RuntimeEnv>>,
    guest_store_raw: *mut c_void,
    values: HashMap<u32, v8::Global<v8::Value>>,
    next_value_id: u32,
    refs: HashMap<u32, RefEntry>,
    next_ref_id: u32,
    deferreds: HashMap<u32, DeferredEntry>,
    next_deferred_id: u32,
    esc_scopes: HashMap<u32, EscapableScopeState>,
    next_esc_scope_id: u32,
    module_wrap_handles: HashMap<u32, ModuleWrapHandle>,
    next_module_wrap_handle_id: u32,
    callback_invocations: HashMap<u32, CallbackInvocation>,
    next_callback_invocation_id: u32,
    cb_registry: HashMap<u32, CbRegistration>,
    next_cb_reg_id: u32,
    callback_bindings: Vec<Box<CallbackBinding>>,
    active_callback_ctx: AtomicPtr<c_void>,
    active_scope: *mut c_void,
    pending_exception: Option<v8::Global<v8::Value>>,
    prepare_stack_trace_callback: Option<u32>,
    promise_reject_callback: Option<u32>,
    promise_hook_callbacks: [u32; 4],
    module_wrap_import_module_dynamically_callback: Option<u32>,
    module_wrap_initialize_import_meta_callback: Option<u32>,
    temporary_required_module_facade_original: Option<u32>,
    enqueue_foreground_task_callback: Option<u32>,
    fatal_error_callback: Option<u32>,
    oom_error_callback: Option<u32>,
    near_heap_limit_callback: Option<u32>,
    near_heap_limit_data: u32,
    next_cpu_profile_id: u32,
    active_cpu_profiles: Vec<u32>,
    heap_profile_started: bool,
    continuation_preserved_embedder_data: Option<u32>,
    instance_data: u64,
    adjusted_external_memory: i64,
    hash_seed: u64,
    type_tags: Vec<TypeTagEntry>,
    detached_arraybuffers: Vec<v8::Global<v8::Value>>,
    contexts: HashMap<u32, v8::Global<v8::Context>>,
}

impl SnapiEnvState {
    fn new(runtime: SnapiRuntime, isolate_handle: v8::IsolateHandle) -> Self {
        Self {
            runtime: Some(runtime),
            owner_thread: thread::current().id(),
            isolate_handle,
            guest_env_id: 0,
            guest_func_env: None,
            guest_store_raw: ptr::null_mut(),
            values: HashMap::new(),
            next_value_id: 1,
            refs: HashMap::new(),
            next_ref_id: 1,
            deferreds: HashMap::new(),
            next_deferred_id: 1,
            esc_scopes: HashMap::new(),
            next_esc_scope_id: 1,
            module_wrap_handles: HashMap::new(),
            next_module_wrap_handle_id: 1,
            callback_invocations: HashMap::new(),
            next_callback_invocation_id: 1,
            cb_registry: HashMap::new(),
            next_cb_reg_id: 1,
            callback_bindings: Vec::new(),
            active_callback_ctx: AtomicPtr::new(ptr::null_mut()),
            active_scope: ptr::null_mut(),
            pending_exception: None,
            prepare_stack_trace_callback: None,
            promise_reject_callback: None,
            promise_hook_callbacks: [0; 4],
            module_wrap_import_module_dynamically_callback: None,
            module_wrap_initialize_import_meta_callback: None,
            temporary_required_module_facade_original: None,
            enqueue_foreground_task_callback: None,
            fatal_error_callback: None,
            oom_error_callback: None,
            near_heap_limit_callback: None,
            near_heap_limit_data: 0,
            next_cpu_profile_id: 1,
            active_cpu_profiles: Vec::new(),
            heap_profile_started: false,
            continuation_preserved_embedder_data: None,
            instance_data: 0,
            adjusted_external_memory: 0,
            hash_seed: generate_hash_seed(0),
            type_tags: Vec::new(),
            detached_arraybuffers: Vec::new(),
            contexts: HashMap::new(),
        }
    }
}

fn generate_hash_seed(seed_hint: usize) -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    now.rotate_left(17) ^ (seed_hint as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

fn register_live_env(env: SnapiEnv) {
    let mut live = LIVE_ENVS.lock();
    live.push(env as usize);
}

fn unregister_live_env(env: SnapiEnv) {
    let mut live = LIVE_ENVS.lock();
    live.retain(|entry| *entry != env as usize);
}

fn snapshot_live_envs() -> Vec<SnapiEnv> {
    LIVE_ENVS
        .lock()
        .iter()
        .copied()
        .map(|entry| entry as SnapiEnv)
        .collect()
}

fn state_from_isolate_ptr<'a>(isolate: *mut v8::Isolate) -> Option<&'a mut SnapiEnvState> {
    if isolate.is_null() {
        return None;
    }
    let target = isolate as *const v8::Isolate;
    let env = LIVE_ENVS.lock().iter().copied().find_map(|entry| {
        let env = entry as SnapiEnv;
        let state = unsafe { env.as_ref() }?;
        let runtime = state.runtime.as_ref()?;
        let isolate = std::ptr::from_ref(&*runtime.isolate);
        (isolate == target).then_some(env)
    })?;
    state_mut(env)
}

pub fn snapi_bridge_attach_guest_runtime(
    env: SnapiEnv,
    guest_env_id: u32,
    guest_func_env: FunctionEnv<RuntimeEnv>,
    guest_store_raw: *mut c_void,
) {
    bridge_lock!();
    let Some(state) = state_mut(env) else {
        return;
    };
    state.guest_env_id = guest_env_id;
    state.guest_func_env = Some(guest_func_env);
    state.guest_store_raw = guest_store_raw;
}

fn with_guest_env_mut<R>(
    state: &mut SnapiEnvState,
    f: impl FnOnce(&mut FunctionEnvMut<'_, RuntimeEnv>, u32) -> Option<R>,
) -> Option<R> {
    let guest_env_id = state.guest_env_id;
    let guest_func_env = state.guest_func_env.clone()?;
    if guest_env_id == 0 || state.guest_store_raw.is_null() {
        return None;
    }
    let mut store = unsafe { wasmer::StoreMut::from_raw(state.guest_store_raw) };
    let mut env = guest_func_env.into_mut(&mut store);
    f(&mut env, guest_env_id)
}

fn allocate_guest_c_string(
    env: &mut FunctionEnvMut<'_, RuntimeEnv>,
    value: Option<&CStr>,
) -> Option<i32> {
    let bytes = value?.to_bytes();
    let mut data = Vec::with_capacity(bytes.len() + 1);
    data.extend_from_slice(bytes);
    data.push(0);
    crate::guest::util::allocate_guest_bytes(env, &data).map(|ptr| ptr as i32)
}

fn invoke_guest_fatal_error_callback(
    state: &mut SnapiEnvState,
    callback_id: u32,
    location: Option<&CStr>,
    message: Option<&CStr>,
) {
    let _ = with_guest_env_mut(state, |env, guest_env_id| {
        let location_ptr = allocate_guest_c_string(env, location).unwrap_or(0);
        let message_ptr = allocate_guest_c_string(env, message).unwrap_or(0);
        crate::guest::callback::call_guest_raw_function(
            env,
            callback_id,
            &[
                WasmValue::I32(guest_env_id as i32),
                WasmValue::I32(location_ptr),
                WasmValue::I32(message_ptr),
            ],
        )
        .map(|_| ())
    });
}

fn invoke_guest_oom_error_callback(
    state: &mut SnapiEnvState,
    callback_id: u32,
    location: Option<&CStr>,
    is_heap_oom: bool,
) {
    let _ = with_guest_env_mut(state, |env, guest_env_id| {
        let location_ptr = allocate_guest_c_string(env, location).unwrap_or(0);
        crate::guest::callback::call_guest_raw_function(
            env,
            callback_id,
            &[
                WasmValue::I32(guest_env_id as i32),
                WasmValue::I32(location_ptr),
                WasmValue::I32(if is_heap_oom { 1 } else { 0 }),
                WasmValue::I32(0),
            ],
        )
        .map(|_| ())
    });
}

fn invoke_guest_near_heap_limit_callback(
    state: &mut SnapiEnvState,
    callback_id: u32,
    data: u32,
    current_heap_limit: usize,
    initial_heap_limit: usize,
) -> Option<usize> {
    with_guest_env_mut(state, |env, guest_env_id| {
        let result = crate::guest::callback::call_guest_raw_function(
            env,
            callback_id,
            &[
                WasmValue::I32(guest_env_id as i32),
                WasmValue::I32(data as i32),
                WasmValue::I32(current_heap_limit as i32),
                WasmValue::I32(initial_heap_limit as i32),
            ],
        )?;
        match result.first() {
            Some(WasmValue::I32(value)) if *value > 0 => Some(*value as usize),
            Some(WasmValue::I64(value)) if *value > 0 => Some(*value as usize),
            _ => None,
        }
    })
}

extern "C" fn snapi_fatal_error_callback(location: *const i8, message: *const i8) {
    let isolate = unsafe { snapi_v8_try_get_current_isolate() };
    let Some(state) = state_from_isolate_ptr(isolate) else {
        return;
    };
    let Some(callback_id) = state.fatal_error_callback else {
        return;
    };
    let location = (!location.is_null()).then(|| unsafe { CStr::from_ptr(location) });
    let message = (!message.is_null()).then(|| unsafe { CStr::from_ptr(message) });
    invoke_guest_fatal_error_callback(state, callback_id, location, message);
}

extern "C" fn snapi_oom_error_callback(location: *const i8, is_heap_oom: bool) {
    let isolate = unsafe { snapi_v8_try_get_current_isolate() };
    let Some(state) = state_from_isolate_ptr(isolate) else {
        return;
    };
    let Some(callback_id) = state.oom_error_callback else {
        return;
    };
    let location = (!location.is_null()).then(|| unsafe { CStr::from_ptr(location) });
    invoke_guest_oom_error_callback(state, callback_id, location, is_heap_oom);
}

extern "C" fn snapi_near_heap_limit_callback(
    data: *mut c_void,
    current_heap_limit: usize,
    initial_heap_limit: usize,
) -> usize {
    let Some(state) = state_mut(data as SnapiEnv) else {
        return current_heap_limit;
    };
    let Some(callback_id) = state.near_heap_limit_callback else {
        return current_heap_limit;
    };
    invoke_guest_near_heap_limit_callback(
        state,
        callback_id,
        state.near_heap_limit_data,
        current_heap_limit,
        initial_heap_limit,
    )
    .unwrap_or(current_heap_limit)
}

fn init_v8() {
    V8_INIT.get_or_init(|| {
        let platform = v8::new_default_platform(0, false).make_shared();
        v8::V8::initialize_platform(platform);
        v8::V8::initialize();
    });
}

fn new_runtime(max_heap_size: Option<usize>) -> SnapiRuntime {
    let mut create_params = v8::CreateParams::default();
    if let Some(max_heap_size) = max_heap_size.filter(|size| *size > 0) {
        create_params = create_params.heap_limits(0, max_heap_size);
    }

    let mut isolate = v8::Isolate::new(create_params);
    unsafe {
        isolate.enter();
    }
    let env_handle_scope = Some(unsafe { RawHandleScope::new(&mut *isolate) });
    isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
    let context = {
        let scope = &mut v8::HandleScope::new(&mut isolate);
        let context = v8::Context::new(scope);
        let scope = &mut v8::ContextScope::new(scope, context);
        install_compat_intrinsics(scope).expect("compat intrinsics");
        unsafe {
            v8__Context__Enter(&*context);
        }
        v8::Global::new(scope, context)
    };

    SnapiRuntime {
        env_handle_scope,
        isolate,
        context,
    }
}

fn state_mut<'a>(env: SnapiEnv) -> Option<&'a mut SnapiEnvState> {
    if env.is_null() {
        None
    } else {
        Some(unsafe { &mut *env })
    }
}

fn next_id(next: &mut u32) -> u32 {
    let id = (*next).max(1);
    *next = id.saturating_add(1);
    id
}

fn cstr_bytes(ptr: *const i8, len: u32) -> Vec<u8> {
    if ptr.is_null() {
        return Vec::new();
    }
    if len == u32::MAX {
        return unsafe { CStr::from_ptr(ptr) }.to_bytes().to_vec();
    }
    unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), len as usize) }.to_vec()
}

fn cstr_string(ptr: *const i8, len: u32) -> String {
    String::from_utf8_lossy(&cstr_bytes(ptr, len)).into_owned()
}

fn filtered_v8_flags(flags: &str) -> String {
    const UNSUPPORTED_FLAGS: &[&str] = &[
        "--js-source-phase-imports",
        "--harmony-import-attributes",
    ];

    flags
        .split_ascii_whitespace()
        .filter(|flag| !UNSUPPORTED_FLAGS.contains(flag))
        .collect::<Vec<_>>()
        .join(" ")
}

fn utf16_bytes(ptr: *const u16, len: u32) -> Vec<u16> {
    if ptr.is_null() {
        return Vec::new();
    }
    if len == u32::MAX {
        let mut out = Vec::new();
        let mut offset = 0usize;
        loop {
            let value = unsafe { ptr.add(offset).read() };
            if value == 0 {
                break;
            }
            out.push(value);
            offset += 1;
        }
        return out;
    }
    unsafe { std::slice::from_raw_parts(ptr, len as usize) }.to_vec()
}

fn write_out<T>(out: *mut T, value: T) -> i32 {
    if out.is_null() {
        return NAPI_INVALID_ARG;
    }
    unsafe {
        out.write(value);
    }
    NAPI_OK
}

fn write_buffer_u8(buf: *mut u8, bufsize: usize, data: &[u8]) {
    if buf.is_null() || bufsize == 0 {
        return;
    }
    let copy_len = data.len().min(bufsize.saturating_sub(1));
    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), buf, copy_len);
        buf.add(copy_len).write(0);
    }
}

fn write_buffer_u16(buf: *mut u16, bufsize: usize, data: &[u16]) {
    if buf.is_null() || bufsize == 0 {
        return;
    }
    let copy_len = data.len().min(bufsize.saturating_sub(1));
    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), buf, copy_len);
        buf.add(copy_len).write(0);
    }
}

macro_rules! try_status {
    ($expr:expr) => {
        match $expr {
            Ok(value) => value,
            Err(code) => return code,
        }
    };
}

fn store_global_value<'s>(
    state: &mut SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    value: v8::Local<'s, v8::Value>,
) -> u32 {
    let id = next_id(&mut state.next_value_id);
    state.values.insert(id, v8::Global::new(scope, value));
    id
}

fn create_ref<'s>(
    state: &mut SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    value: v8::Local<'s, v8::Value>,
    refcount: u32,
) -> u32 {
    let id = next_id(&mut state.next_ref_id);
    state.refs.insert(
        id,
        RefEntry {
            value: v8::Global::new(scope, value),
            refcount,
        },
    );
    id
}

fn store_global_context<'s>(
    state: &mut SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    key: v8::Local<'s, v8::Value>,
    context: v8::Local<'s, v8::Context>,
) -> u32 {
    let key_id = store_global_value(state, scope, key);
    state.contexts.insert(key_id, v8::Global::new(scope, context));
    key_id
}

fn local_value<'s>(
    state: &SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    id: u32,
) -> Option<v8::Local<'s, v8::Value>> {
    state.values.get(&id).map(|value| v8::Local::new(scope, value))
}

fn local_function<'s>(
    state: &SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    id: u32,
) -> Option<v8::Local<'s, v8::Function>> {
    local_value(state, scope, id).and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
}

fn local_context<'s>(
    state: &SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    key_id: u32,
) -> Option<v8::Local<'s, v8::Context>> {
    state
        .contexts
        .get(&key_id)
        .map(|value| v8::Local::new(scope, value))
}

fn resolve_context_from_key<'s>(
    state: &SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    key_id: u32,
) -> Option<v8::Local<'s, v8::Context>> {
    if let Some(context) = local_context(state, scope, key_id) {
        return Some(context);
    }
    let key = local_value(state, scope, key_id)?;
    for (candidate_id, context) in &state.contexts {
        let Some(candidate) = local_value(state, scope, *candidate_id) else {
            continue;
        };
        if candidate.strict_equals(key) {
            return Some(v8::Local::new(scope, context));
        }
    }
    None
}

fn module_request_key(specifier: &str, attributes: &[ModuleImportAttributeRecord]) -> String {
    let mut key = String::from(specifier);
    for attr in attributes {
        key.push('\0');
        key.push_str(&attr.key);
        key.push('\0');
        key.push_str(&attr.value);
    }
    key
}

fn fixed_array_value<'s>(
    scope: &mut v8::HandleScope<'s>,
    array: v8::Local<'s, v8::FixedArray>,
    index: usize,
) -> Option<v8::Local<'s, v8::Value>> {
    let data = array.get(scope, index)?;
    v8::Local::<v8::Value>::try_from(data).ok()
}

fn import_attributes_from_fixed_array<'s>(
    scope: &mut v8::HandleScope<'s>,
    import_attributes: v8::Local<'s, v8::FixedArray>,
    step: usize,
) -> Result<Vec<ModuleImportAttributeRecord>, i32> {
    let mut attributes = Vec::new();
    let length = import_attributes.length();
    let mut index = 0usize;
    while index + 1 < length {
        let key = fixed_array_value(scope, import_attributes, index).ok_or(NAPI_GENERIC_FAILURE)?;
        let value =
            fixed_array_value(scope, import_attributes, index + 1).ok_or(NAPI_GENERIC_FAILURE)?;
        let key = key
            .to_string(scope)
            .ok_or(NAPI_GENERIC_FAILURE)?
            .to_rust_string_lossy(scope);
        let value = value
            .to_string(scope)
            .ok_or(NAPI_GENERIC_FAILURE)?
            .to_rust_string_lossy(scope);
        attributes.push(ModuleImportAttributeRecord { key, value });
        index += step;
    }
    Ok(attributes)
}

fn create_dynamic_import_attributes_object<'s>(
    state: &mut SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    import_attributes: v8::Local<'s, v8::FixedArray>,
) -> Result<v8::Local<'s, v8::Object>, i32> {
    let attributes = v8::Object::new(scope);
    let length = import_attributes.length();
    let mut index = 0usize;
    while index + 1 < length {
        let key = fixed_array_value(scope, import_attributes, index).ok_or(NAPI_GENERIC_FAILURE)?;
        let value =
            fixed_array_value(scope, import_attributes, index + 1).ok_or(NAPI_GENERIC_FAILURE)?;
        let key: v8::Local<v8::Name> = key.try_into().map_err(|_| NAPI_GENERIC_FAILURE)?;
        let mut tc = v8::TryCatch::new(scope);
        if !attributes
            .create_data_property(&mut tc, key, value)
            .unwrap_or(false)
        {
            if let Some(exception) = tc.exception() {
                set_pending_exception(state, &mut tc, exception);
                return Err(NAPI_PENDING_EXCEPTION);
            }
            return Err(NAPI_GENERIC_FAILURE);
        }
        index += 2;
    }
    Ok(attributes)
}

fn module_wrap_handle_id_by_module<'s>(
    state: &SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    module: v8::Local<'s, v8::Module>,
) -> Option<u32> {
    for (handle_id, handle) in &state.module_wrap_handles {
        let candidate = v8::Local::new(scope, &handle.module);
        if candidate == module {
            return Some(*handle_id);
        }
    }
    None
}

fn populate_module_requests<'s>(
    scope: &mut v8::HandleScope<'s>,
    module: v8::Local<'s, v8::Module>,
) -> Result<(Vec<ModuleRequestRecord>, HashMap<String, u32>), i32> {
    let raw_requests = module.get_module_requests();
    let mut requests = Vec::with_capacity(raw_requests.length());
    let mut resolve_cache = HashMap::new();
    for index in 0..raw_requests.length() {
        let request_data = raw_requests.get(scope, index).ok_or(NAPI_GENERIC_FAILURE)?;
        let request: v8::Local<v8::ModuleRequest> =
            request_data.try_into().map_err(|_| NAPI_GENERIC_FAILURE)?;
        let specifier = local_for_scope(request.get_specifier()).to_rust_string_lossy(scope);
        let import_assertions = local_for_scope(request.get_import_assertions());
        let attributes = import_attributes_from_fixed_array(scope, import_assertions, 3)?;
        let record = ModuleRequestRecord {
            specifier,
            attributes,
            phase: 2,
        };
        resolve_cache
            .entry(module_request_key(&record.specifier, &record.attributes))
            .or_insert(index as u32);
        requests.push(record);
    }
    Ok((requests, resolve_cache))
}

fn clear_pending_exception(state: &mut SnapiEnvState) {
    state.pending_exception = None;
}

fn set_pending_exception<'s>(
    state: &mut SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    value: v8::Local<'s, v8::Value>,
) {
    state.pending_exception = Some(v8::Global::new(scope, value));
}

fn private_key<'s>(scope: &mut v8::HandleScope<'s>, name: &str) -> v8::Local<'s, v8::Private> {
    let key = v8::String::new(scope, name).expect("private key");
    v8::Private::for_api(scope, Some(key))
}

fn install_compat_intrinsics<'s>(scope: &mut v8::HandleScope<'s>) -> Result<(), i32> {
    let source = v8::String::new(
        scope,
        r#"
        (() => {
          const define = (obj, name, value) => {
            if (typeof value !== 'function' || name in obj) return;
            Object.defineProperty(obj, name, {
              value,
              configurable: true,
              writable: true,
            });
          };
          define(Array.prototype, 'toSorted', function(compareFn) {
            return Array.prototype.slice.call(this).sort(compareFn);
          });
          define(Array.prototype, 'toReversed', function() {
            return Array.prototype.slice.call(this).reverse();
          });
          define(Array.prototype, 'toSpliced', function(start, deleteCount, ...items) {
            const copy = Array.prototype.slice.call(this);
            if (arguments.length === 1) {
              copy.splice(start);
            } else {
              copy.splice(start, deleteCount, ...items);
            }
            return copy;
          });
          define(Array.prototype, 'with', function(index, value) {
            const copy = Array.prototype.slice.call(this);
            let i = Number(index);
            if (i < 0) i += copy.length;
            if (i < 0 || i >= copy.length) throw new RangeError('Invalid index');
            copy[i] = value;
            return copy;
          });

          const typedArrayCompare = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
          const typedArrayCtors = [
            Int8Array,
            Uint8Array,
            Uint8ClampedArray,
            Int16Array,
            Uint16Array,
            Int32Array,
            Uint32Array,
            Float32Array,
            Float64Array,
            BigInt64Array,
            BigUint64Array,
          ].filter((Ctor) => typeof Ctor === 'function');
          for (const Ctor of typedArrayCtors) {
            define(Ctor.prototype, 'toSorted', function(compareFn) {
              const copy = Array.from(this);
              copy.sort(compareFn || typedArrayCompare);
              return new Ctor(copy);
            });
            define(Ctor.prototype, 'toReversed', function() {
              return new Ctor(Array.from(this).reverse());
            });
            define(Ctor.prototype, 'with', function(index, value) {
              const copy = new Ctor(this);
              let i = Number(index);
              if (i < 0) i += copy.length;
              if (i < 0 || i >= copy.length) throw new RangeError('Invalid index');
              copy[i] = value;
              return copy;
            });
          }
        })();
        "#,
    )
    .ok_or(NAPI_GENERIC_FAILURE)?;
    let script = v8::Script::compile(scope, source, None).ok_or(NAPI_GENERIC_FAILURE)?;
    script.run(scope).ok_or(NAPI_GENERIC_FAILURE)?;
    Ok(())
}

fn install_helpers<'s>(scope: &mut v8::HandleScope<'s>) -> Result<(), i32> {
    let global = scope.get_current_context().global(scope);
    let helper_name = v8::String::new(scope, HELPER_OBJECT_NAME).ok_or(NAPI_GENERIC_FAILURE)?;
    if global.has(scope, helper_name.into()).unwrap_or(false) {
        return Ok(());
    }
    let source = v8::String::new(
        scope,
        r#"
        (() => {
          const hasOwn = (obj, key) => Object.prototype.hasOwnProperty.call(obj, key);
          const ownNonIndex = (obj, filterBits = 0) => Reflect.ownKeys(obj).filter((k) => {
            if (typeof k === 'string' && /^(0|[1-9]\d*)$/.test(k)) return false;
            if ((filterBits & 8) !== 0 && typeof k === 'string') return false;
            if ((filterBits & 16) !== 0 && typeof k === 'symbol') return false;
            const desc = Object.getOwnPropertyDescriptor(obj, k);
            if (!desc) return false;
            if ((filterBits & 1) !== 0 && !desc.writable) return false;
            if ((filterBits & 2) !== 0 && !desc.enumerable) return false;
            if ((filterBits & 4) !== 0 && !desc.configurable) return false;
            return true;
          });
          const captureCallSites = (skip, frames) => {
            const old = Error.prepareStackTrace;
            try {
              Error.prepareStackTrace = (_, sites) => sites;
              const holder = {};
              Error.captureStackTrace(holder, captureCallSites);
              return (holder.stack || []).slice(skip, skip + frames);
            } finally {
              Error.prepareStackTrace = old;
            }
          };
          globalThis.__snapi = {
            hasOwn,
            ownNonIndex,
            allProps: (obj, keyMode = 0, filterBits = 0, keyConversion = 0) => {
              const out = [];
              const seen = new Set();
              for (let current = obj; current != null; current = keyMode === 0 ? Object.getPrototypeOf(current) : null) {
                for (const rawKey of Reflect.ownKeys(current)) {
                  let key = rawKey;
                  if (typeof key === 'string' && /^(0|[1-9]\d*)$/.test(key) && keyConversion === 0) {
                    key = Number(key);
                  }
                  const seenKey = typeof key === 'symbol' ? key : `${typeof key}:${String(key)}`;
                  if (seen.has(seenKey)) continue;
                  const desc = Object.getOwnPropertyDescriptor(current, rawKey);
                  if (!desc) continue;
                  if ((filterBits & 1) !== 0 && !desc.writable) continue;
                  if ((filterBits & 2) !== 0 && !desc.enumerable) continue;
                  if ((filterBits & 4) !== 0 && !desc.configurable) continue;
                  if ((filterBits & 8) !== 0 && typeof rawKey === 'string') continue;
                  if ((filterBits & 16) !== 0 && typeof rawKey === 'symbol') continue;
                  seen.add(seenKey);
                  out.push(key);
                }
                if (keyMode !== 0) break;
              }
              return out;
            },
            instanceofFn: (obj, ctor) => obj instanceof ctor,
            freeze: (obj) => Object.freeze(obj),
            seal: (obj) => Object.seal(obj),
            bufferFromArrayBuffer: (buffer, byteOffset, byteLength) => {
              const BufferCtor = globalThis.Buffer;
              return typeof BufferCtor?.from === 'function'
                ? BufferCtor.from(buffer, byteOffset, byteLength)
                : new Uint8Array(buffer, byteOffset, byteLength);
            },
            setProp: (obj, key, value) => Reflect.set(obj, key, value),
            defineProp: (obj, key, value) =>
              Reflect.defineProperty(obj, key, {
                value,
                configurable: true,
                enumerable: true,
                writable: true,
              }),
            defineAccessor: (obj, key, getter, setter, enumerable, configurable) =>
              Object.defineProperty(obj, key, {
                get: getter === undefined ? undefined : getter,
                set: setter === undefined ? undefined : setter,
                enumerable: !!enumerable,
                configurable: !!configurable,
              }),
            getOwnPropDesc: (obj, key) => Object.getOwnPropertyDescriptor(obj, key),
            ctorName: (value) => {
              if (value == null) return '';
              const ctor = value.constructor;
              return ctor && typeof ctor.name === 'string' ? ctor.name : '';
            },
            previewEntries: (value) => {
              if (value instanceof Map) return [Array.from(value.entries()), 1];
              if (value instanceof Set) return [Array.from(value.values()), 0];
              return [[], 0];
            },
            getCallSites: (frames, skip) =>
              captureCallSites(Number(skip) || 0, Number(frames) || 0).map((site) => ({
                functionName: site?.getFunctionName?.() || '',
                scriptId: String(site?.getScriptId?.() ?? ''),
                scriptName: site?.getScriptName?.() || '',
                lineNumber: site?.getLineNumber?.() || 0,
                columnNumber: site?.getColumnNumber?.() || 0,
                column: site?.getColumnNumber?.() || 0,
              })),
            getCallerLocation: () => {
              const site = captureCallSites(1, 2)[1];
              if (!site) return undefined;
              const file = site.getScriptNameOrSourceURL?.();
              if (!file) return undefined;
              return [site.getLineNumber?.() || 0, site.getColumnNumber?.() || 0, file];
            },
          };
        })();
        "#,
    )
    .ok_or(NAPI_GENERIC_FAILURE)?;
    let script = v8::Script::compile(scope, source, None).ok_or(NAPI_GENERIC_FAILURE)?;
    script.run(scope).ok_or(NAPI_GENERIC_FAILURE)?;
    Ok(())
}

fn helper_function<'s>(
    scope: &mut v8::HandleScope<'s>,
    name: &str,
) -> Option<v8::Local<'s, v8::Function>> {
    let global = scope.get_current_context().global(scope);
    let helper_name = v8::String::new(scope, HELPER_OBJECT_NAME)?;
    let helper_obj = global.get(scope, helper_name.into())?.to_object(scope)?;
    let fn_name = v8::String::new(scope, name)?;
    let value = helper_obj.get(scope, fn_name.into())?;
    value.try_into().ok()
}

fn call_helper<'s>(
    state: &mut SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    name: &str,
    args: &[v8::Local<'s, v8::Value>],
) -> Result<v8::Local<'s, v8::Value>, i32> {
    install_helpers(scope)?;
    let func = helper_function(scope, name).ok_or(NAPI_GENERIC_FAILURE)?;
    let recv = scope.get_current_context().global(scope);
    let mut tc = v8::TryCatch::new(scope);
    let result = func.call(&mut tc, recv.into(), args);
    match result {
        Some(value) => Ok(value),
        None => {
            if let Some(exc) = tc.exception() {
                set_pending_exception(state, &mut tc, exc);
                Err(NAPI_PENDING_EXCEPTION)
            } else {
                Err(NAPI_GENERIC_FAILURE)
            }
        }
    }
}

fn with_scope<R>(
    state: &mut SnapiEnvState,
    f: impl for<'s> FnOnce(&mut v8::HandleScope<'s>, &mut SnapiEnvState) -> R,
) -> R {
    bridge_lock!();
    if state.owner_thread != thread::current().id() {
        eprintln!(
            "[snapi] thread mismatch owner={:?} current={:?}",
            state.owner_thread,
            thread::current().id()
        );
    }
    if !state.active_scope.is_null() {
        let scope = unsafe { &mut *(state.active_scope as *mut v8::HandleScope<'static>) };
        return f(scope, state);
    }
    let runtime = state.runtime.as_mut().expect("runtime missing") as *mut SnapiRuntime;
    let mut scope = unsafe {
        v8::HandleScope::with_context(&mut (*runtime).isolate, &(*runtime).context)
    };
    let result = f(&mut scope, state);
    drop(scope);
    result
}

fn value_as_object<'s>(
    value: v8::Local<'s, v8::Value>,
) -> Result<v8::Local<'s, v8::Object>, i32> {
    value.try_into().map_err(|_| NAPI_OBJECT_EXPECTED)
}

fn value_as_function<'s>(
    value: v8::Local<'s, v8::Value>,
) -> Result<v8::Local<'s, v8::Function>, i32> {
    value.try_into().map_err(|_| NAPI_FUNCTION_EXPECTED)
}

fn value_as_array<'s>(value: v8::Local<'s, v8::Value>) -> Result<v8::Local<'s, v8::Array>, i32> {
    value.try_into().map_err(|_| NAPI_ARRAY_EXPECTED)
}

fn string_value<'s>(
    scope: &mut v8::HandleScope<'s>,
    value: v8::Local<'s, v8::Value>,
) -> Result<v8::Local<'s, v8::String>, i32> {
    value.to_string(scope).ok_or(NAPI_STRING_EXPECTED)
}

fn object_value_data<'s>(
    state: &SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    id: u32,
) -> Result<v8::Local<'s, v8::Value>, i32> {
    local_value(state, scope, id).ok_or(NAPI_INVALID_ARG)
}

fn local_for_scope<'s, T>(value: v8::Local<'_, T>) -> v8::Local<'s, T> {
    unsafe { std::mem::transmute(value) }
}

#[repr(C)]
struct LocalHandleRepr<'s, T>(std::ptr::NonNull<T>, std::marker::PhantomData<&'s ()>);

fn local_from_raw<'s, T>(ptr: *const T) -> Option<v8::Local<'s, T>> {
    let ptr = std::ptr::NonNull::new(ptr as *mut T)?;
    // `rusty_v8::Local` is represented as a non-null raw handle plus a lifetime marker.
    Some(unsafe {
        std::mem::transmute::<LocalHandleRepr<'s, T>, v8::Local<'s, T>>(LocalHandleRepr(
            ptr,
            std::marker::PhantomData,
        ))
    })
}

fn local_to_raw<'s, T>(value: v8::Local<'s, T>) -> *const T {
    let repr = unsafe { std::mem::transmute::<v8::Local<'s, T>, LocalHandleRepr<'s, T>>(value) };
    repr.0.as_ptr() as *const T
}

fn unique_ref_from_raw<T>(ptr: *mut T) -> Option<v8::UniqueRef<T>> {
    let ptr = std::ptr::NonNull::new(ptr)?;
    Some(unsafe { std::mem::transmute::<std::ptr::NonNull<T>, v8::UniqueRef<T>>(ptr) })
}

fn private_for_api<'s>(
    scope: &mut v8::HandleScope<'s>,
    name: &str,
) -> Result<v8::Local<'s, v8::Private>, i32> {
    let isolate = unsafe { v8__Context__GetIsolate(&*scope.get_current_context()) };
    let name = v8::String::new(scope, name).ok_or(NAPI_GENERIC_FAILURE)?;
    local_from_raw(unsafe { v8__Private__ForApi(isolate, &*name) }).ok_or(NAPI_GENERIC_FAILURE)
}

fn backing_store_token(data: *mut c_void) -> u64 {
    data as u64
}

fn node_buffer_from_arraybuffer<'s>(
    state: &mut SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    buffer: v8::Local<'s, v8::Value>,
    byte_offset: usize,
    byte_length: usize,
) -> Result<v8::Local<'s, v8::Value>, i32> {
    let offset = v8::Integer::new_from_unsigned(scope, byte_offset as u32);
    let length = v8::Integer::new_from_unsigned(scope, byte_length as u32);
    call_helper(
        state,
        scope,
        "bufferFromArrayBuffer",
        &[buffer, offset.into(), length.into()],
    )
}

fn create_error_common<'s>(
    state: &mut SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    code_id: u32,
    msg_id: u32,
    kind: i32,
) -> Result<u32, i32> {
    let msg_value = object_value_data(state, scope, msg_id)?;
    let message = string_value(scope, msg_value)?;
    let created = match kind {
        0 => v8::Exception::error(scope, message),
        1 => v8::Exception::type_error(scope, message),
        2 => v8::Exception::range_error(scope, message),
        _ => return Err(NAPI_INVALID_ARG),
    };
    let err_obj: v8::Local<v8::Object> = created.try_into().map_err(|_| NAPI_GENERIC_FAILURE)?;
    if code_id != 0 {
        let code = object_value_data(state, scope, code_id)?;
        let key = v8::String::new(scope, "code").ok_or(NAPI_GENERIC_FAILURE)?;
        if !err_obj.set(scope, key.into(), code).unwrap_or(false) {
            return Err(NAPI_GENERIC_FAILURE);
        }
    }
    Ok(store_global_value(state, scope, err_obj.into()))
}

fn throw_code_error_local<'s>(
    state: &mut SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    code: &str,
    message: &str,
) {
    let Some(code_key) = v8::String::new(scope, "code") else {
        return;
    };
    let Some(code_value) = v8::String::new(scope, code) else {
        return;
    };
    let Some(message) = v8::String::new(scope, message) else {
        return;
    };
    let error = v8::Exception::error(scope, message);
    if let Ok(error_obj) = v8::Local::<v8::Object>::try_from(error) {
        let _ = error_obj.set(scope, code_key.into(), code_value.into());
    }
    set_pending_exception(state, scope, error);
    scope.throw_exception(error);
}

fn snapshot_value_bytes_local<'s>(
    scope: &mut v8::HandleScope<'s>,
    value: v8::Local<'s, v8::Value>,
) -> Result<Box<[u8]>, i32> {
    if value.is_array_buffer_view() {
        let view: v8::Local<v8::ArrayBufferView> = value.try_into().map_err(|_| NAPI_INVALID_ARG)?;
        let mut bytes = vec![0u8; view.byte_length()];
        let written = view.copy_contents(&mut bytes);
        bytes.truncate(written);
        return Ok(bytes.into_boxed_slice());
    }

    if value.is_array_buffer() {
        let buffer: v8::Local<v8::ArrayBuffer> = value.try_into().map_err(|_| NAPI_INVALID_ARG)?;
        let backing = buffer.get_backing_store();
        let mut bytes = vec![0u8; buffer.byte_length()];
        if !bytes.is_empty() {
            unsafe {
                ptr::copy_nonoverlapping(backing.data() as *const u8, bytes.as_mut_ptr(), bytes.len());
            }
        }
        return Ok(bytes.into_boxed_slice());
    }

    if value.is_shared_array_buffer() {
        let buffer: v8::Local<v8::SharedArrayBuffer> =
            value.try_into().map_err(|_| NAPI_INVALID_ARG)?;
        let backing = buffer.get_backing_store();
        let mut bytes = vec![0u8; buffer.byte_length()];
        if !bytes.is_empty() {
            unsafe {
                ptr::copy_nonoverlapping(backing.data() as *const u8, bytes.as_mut_ptr(), bytes.len());
            }
        }
        return Ok(bytes.into_boxed_slice());
    }

    Err(NAPI_INVALID_ARG)
}

fn overwrite_value_bytes_local<'s>(
    scope: &mut v8::HandleScope<'s>,
    value: v8::Local<'s, v8::Value>,
    data: &[u8],
) -> Result<(), i32> {
    if value.is_array_buffer_view() {
        let view: v8::Local<v8::ArrayBufferView> = value.try_into().map_err(|_| NAPI_INVALID_ARG)?;
        let len = view.byte_length().min(data.len());
        let buffer = view.buffer(scope).ok_or(NAPI_GENERIC_FAILURE)?;
        let backing = buffer.get_backing_store();
        if len > 0 {
            unsafe {
                ptr::copy_nonoverlapping(
                    data.as_ptr(),
                    (backing.data() as *mut u8).add(view.byte_offset()),
                    len,
                );
            }
        }
        return Ok(());
    }

    if value.is_array_buffer() {
        let buffer: v8::Local<v8::ArrayBuffer> = value.try_into().map_err(|_| NAPI_INVALID_ARG)?;
        let len = buffer.byte_length().min(data.len());
        let backing = buffer.get_backing_store();
        if len > 0 {
            unsafe {
                ptr::copy_nonoverlapping(data.as_ptr(), backing.data() as *mut u8, len);
            }
        }
        return Ok(());
    }

    Err(NAPI_INVALID_ARG)
}

fn set_named_property<'s>(
    scope: &mut v8::HandleScope<'s>,
    target: v8::Local<'s, v8::Object>,
    key: &str,
    value: v8::Local<'s, v8::Value>,
) -> Result<(), i32> {
    let key = v8::String::new(scope, key).ok_or(NAPI_GENERIC_FAILURE)?;
    if target.set(scope, key.into(), value).unwrap_or(false) {
        Ok(())
    } else {
        Err(NAPI_GENERIC_FAILURE)
    }
}

fn copy_utf8_into<const N: usize>(dst: &mut [u8; N], value: &str) {
    dst.fill(0);
    let bytes = value.as_bytes();
    let len = bytes.len().min(N.saturating_sub(1));
    if len > 0 {
        dst[..len].copy_from_slice(&bytes[..len]);
    }
}

struct StructuredCloneSerializer;

impl v8::ValueSerializerImpl for StructuredCloneSerializer {
    fn throw_data_clone_error<'s>(
        &mut self,
        scope: &mut v8::HandleScope<'s>,
        message: v8::Local<'s, v8::String>,
    ) {
        let error = v8::Exception::error(scope, message);
        scope.throw_exception(error);
    }
}

struct StructuredCloneDeserializer;

impl v8::ValueDeserializerImpl for StructuredCloneDeserializer {}

fn structured_clone_local<'s>(
    scope: &mut v8::HandleScope<'s>,
    value: v8::Local<'s, v8::Value>,
    transfer_list: Option<v8::Local<'s, v8::Value>>,
) -> Result<v8::Local<'s, v8::Value>, i32> {
    let context = scope.get_current_context();
    let mut serializer = v8::ValueSerializer::new(scope, Box::new(StructuredCloneSerializer));
    let mut transferred_backing_stores = Vec::new();

    if let Some(transfer_list) = transfer_list {
        let array: v8::Local<v8::Array> = transfer_list.try_into().map_err(|_| NAPI_INVALID_ARG)?;
        for index in 0..array.length() {
            let buffer_value = array.get_index(scope, index).ok_or(NAPI_INVALID_ARG)?;
            let buffer: v8::Local<v8::ArrayBuffer> =
                buffer_value.try_into().map_err(|_| NAPI_INVALID_ARG)?;
            serializer.transfer_array_buffer(index, buffer);
            transferred_backing_stores.push(buffer.get_backing_store());
            buffer.detach();
        }
    }

    serializer
        .write_value(context, value)
        .ok_or(NAPI_PENDING_EXCEPTION)?
        .then_some(())
        .ok_or(NAPI_PENDING_EXCEPTION)?;
    let bytes = serializer.release();

    let mut deserializer =
        v8::ValueDeserializer::new(scope, Box::new(StructuredCloneDeserializer), &bytes);
    for (index, backing_store) in transferred_backing_stores.iter().enumerate() {
        let buffer = v8::ArrayBuffer::with_backing_store(scope, backing_store);
        deserializer.transfer_array_buffer(index as u32, buffer);
    }
    deserializer
        .read_header(context)
        .ok_or(NAPI_PENDING_EXCEPTION)?
        .then_some(())
        .ok_or(NAPI_PENDING_EXCEPTION)?;
    deserializer.read_value(context).ok_or(NAPI_PENDING_EXCEPTION)
}

fn build_call_site_array<'s>(
    state: &mut SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    frames: u32,
    skip_frames: u32,
) -> Result<v8::Local<'s, v8::Array>, i32> {
    if !(1..=200).contains(&frames) {
        return Err(NAPI_INVALID_ARG);
    }
    let frames = v8::Integer::new_from_unsigned(scope, frames);
    let skip = v8::Integer::new_from_unsigned(scope, skip_frames);
    let result = call_helper(state, scope, "getCallSites", &[frames.into(), skip.into()])?;
    result.try_into().map_err(|_| NAPI_GENERIC_FAILURE)
}

fn exception_stack_string<'s>(
    scope: &mut v8::HandleScope<'s>,
    exception: v8::Local<'s, v8::Value>,
) -> v8::Local<'s, v8::Value> {
    exception
        .to_string(scope)
        .map(|value| value.into())
        .unwrap_or_else(|| v8::String::empty(scope).into())
}

fn build_arrow_message<'s>(
    scope: &mut v8::HandleScope<'s>,
    message: v8::Local<'s, v8::Message>,
) -> Option<String> {
    let filename = message
        .get_script_resource_name(scope)
        .and_then(|value| value.to_string(scope))
        .map(|value| value.to_rust_string_lossy(scope))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "<anonymous_script>".to_string());
    let source_line = message
        .get_source_line(scope)?
        .to_rust_string_lossy(scope);
    if source_line.is_empty() {
        return None;
    }

    let line_number = message.get_line_number(scope).unwrap_or(0);
    let mut start = message.get_start_column();
    let mut end = message.get_end_column();
    if end <= start {
        end = start + 1;
    }
    let underline = format!(
        "{}{}",
        " ".repeat(start),
        "^".repeat(std::cmp::max(1, end.saturating_sub(start)))
    );
    Some(format!(
        "{filename}:{line_number}\n{source_line}\n{underline}\n"
    ))
}

fn promise_hook_index(hook_type: v8::PromiseHookType) -> usize {
    match hook_type {
        v8::PromiseHookType::Init => 0,
        v8::PromiseHookType::Before => 1,
        v8::PromiseHookType::After => 2,
        v8::PromiseHookType::Resolve => 3,
    }
}

fn snapi_prepare_stack_trace_callback<'s>(
    scope: &mut v8::HandleScope<'s>,
    exception: v8::Local<'s, v8::Value>,
    trace: v8::Local<'s, v8::Array>,
) -> v8::Local<'s, v8::Value> {
    let context = scope.get_current_context();
    let isolate = unsafe { v8__Context__GetIsolate(&*context) };
    let Some(state) = state_from_isolate_ptr(isolate) else {
        return exception_stack_string(scope, exception);
    };
    let Some(callback_id) = state.prepare_stack_trace_callback else {
        return exception_stack_string(scope, exception);
    };
    let Some(callback) = local_function(state, scope, callback_id) else {
        return exception_stack_string(scope, exception);
    };

    let global = context.global(scope);
    let undefined = v8::undefined(scope).into();
    let trace_value: v8::Local<v8::Value> = trace.into();
    let args = [global.into(), exception, trace_value];

    let mut try_catch = v8::TryCatch::new(scope);
    match callback.call(&mut try_catch, undefined, &args) {
        Some(value) => value,
        None => {
            let _ = try_catch.rethrow();
            exception_stack_string(&mut try_catch, exception)
        }
    }
}

extern "C" fn snapi_promise_reject_callback(message: v8::PromiseRejectMessage<'_>) {
    let scope = &mut unsafe { v8::CallbackScope::new(&message) };
    let promise = message.get_promise();
    let promise_object: v8::Local<v8::Object> = promise.into();
    let isolate = unsafe { v8__Object__GetIsolate(&*promise_object) };
    let Some(state) = state_from_isolate_ptr(isolate) else {
        return;
    };
    let Some(callback_id) = state.promise_reject_callback else {
        return;
    };
    let Some(callback) = local_function(state, scope, callback_id) else {
        return;
    };

    let event = message.get_event();
    let value = match event {
        v8::PromiseRejectEvent::PromiseRejectWithNoHandler
        | v8::PromiseRejectEvent::PromiseResolveAfterResolved
        | v8::PromiseRejectEvent::PromiseRejectAfterResolved => {
            message.get_value().unwrap_or_else(|| v8::undefined(scope).into())
        }
        v8::PromiseRejectEvent::PromiseHandlerAddedAfterReject => v8::undefined(scope).into(),
    };
    let undefined = v8::undefined(scope).into();
    let event_value = v8::Integer::new(scope, event as i32).into();
    let promise_value: v8::Local<v8::Value> = message.get_promise().into();
    let args = [event_value, promise_value, value];

    let mut try_catch = v8::TryCatch::new(scope);
    if callback.call(&mut try_catch, undefined, &args).is_none() {
        if let Some(caught) = try_catch.exception() {
            if let Some(text_value) = caught.to_string(&mut try_catch) {
                let text = text_value.to_rust_string_lossy(&mut try_catch);
                eprintln!("Exception in PromiseRejectCallback:\n{text}");
            } else {
                eprintln!("Exception in PromiseRejectCallback:\n<exception>");
            }
        }
    }
}

extern "C" fn snapi_promise_hook_callback(
    hook_type: v8::PromiseHookType,
    promise: v8::Local<'_, v8::Promise>,
    parent: v8::Local<'_, v8::Value>,
) {
    let scope = &mut unsafe { v8::CallbackScope::new(promise) };
    let promise_object: v8::Local<v8::Object> = promise.into();
    let isolate = unsafe { v8__Object__GetIsolate(&*promise_object) };
    let Some(state) = state_from_isolate_ptr(isolate) else {
        return;
    };
    let callback_id = state.promise_hook_callbacks[promise_hook_index(hook_type)];
    if callback_id == 0 {
        return;
    }
    let Some(callback) = local_function(state, scope, callback_id) else {
        return;
    };

    let undefined = v8::undefined(scope).into();
    let promise_value: v8::Local<v8::Value> = promise.into();
    let args = [promise_value, parent];
    let argc = if matches!(hook_type, v8::PromiseHookType::Init) {
        2
    } else {
        1
    };

    let mut try_catch = v8::TryCatch::new(scope);
    if callback
        .call(&mut try_catch, undefined, &args[..argc])
        .is_none()
    {
        let _ = try_catch.rethrow();
    }
}

fn module_wrap_resolve_callback<'a>(
    context: v8::Local<'a, v8::Context>,
    specifier: v8::Local<'a, v8::String>,
    import_assertions: v8::Local<'a, v8::FixedArray>,
    referrer: v8::Local<'a, v8::Module>,
) -> Option<v8::Local<'a, v8::Module>> {
    let scope = &mut unsafe { v8::CallbackScope::new(context) };
    let isolate = unsafe { v8__Context__GetIsolate(&*context) };
    let state = state_from_isolate_ptr(isolate)?;
    let dependent_id = module_wrap_handle_id_by_module(state, scope, referrer)?;
    let dependent = state.module_wrap_handles.get(&dependent_id)?;
    if dependent.linked_requests.is_empty() {
        throw_code_error_local(state, scope, "ERR_VM_MODULE_LINK_FAILURE", "Module is not linked");
        return None;
    }
    let attributes = match import_attributes_from_fixed_array(scope, import_assertions, 3) {
        Ok(attributes) => attributes,
        Err(_) => return None,
    };
    let specifier = specifier.to_rust_string_lossy(scope);
    let key = module_request_key(&specifier, &attributes);
    let Some(index) = dependent.resolve_cache.get(&key).copied() else {
        throw_code_error_local(
            state,
            scope,
            "ERR_VM_MODULE_LINK_FAILURE",
            "Module request is not cached",
        );
        return None;
    };
    let Some(linked_handle_id) = dependent.linked_requests.get(index as usize).copied() else {
        throw_code_error_local(
            state,
            scope,
            "ERR_VM_MODULE_LINK_FAILURE",
            "Module request is not cached",
        );
        return None;
    };
    let linked = state.module_wrap_handles.get(&linked_handle_id)?;
    Some(v8::Local::new(scope, &linked.module))
}

fn module_wrap_synthetic_evaluation_steps<'a>(
    context: v8::Local<'a, v8::Context>,
    module: v8::Local<'a, v8::Module>,
) -> Option<v8::Local<'a, v8::Value>> {
    let scope = &mut unsafe { v8::CallbackScope::new(context) };
    let isolate = unsafe { v8__Context__GetIsolate(&*context) };
    let state = state_from_isolate_ptr(isolate)?;
    let handle_id = module_wrap_handle_id_by_module(state, scope, module)?;
    let handle = state.module_wrap_handles.get(&handle_id)?;
    let callback = local_function(state, scope, handle.synthetic_eval_steps_id)?;
    let wrapper = local_value(state, scope, handle.wrapper_id)?;
    let mut try_catch = v8::TryCatch::new(scope);
    if callback.call(&mut try_catch, wrapper, &[]).is_none() {
        let _ = try_catch.rethrow();
        return None;
    }
    let resolver = v8::PromiseResolver::new(&mut try_catch)?;
    let undefined = v8::undefined(&mut try_catch).into();
    resolver.resolve(&mut try_catch, undefined)?;
    Some(resolver.get_promise(&mut try_catch).into())
}

extern "C" fn module_wrap_host_initialize_import_meta_object_callback(
    context: v8::Local<'_, v8::Context>,
    module: v8::Local<'_, v8::Module>,
    meta: v8::Local<'_, v8::Object>,
) {
    let scope = &mut unsafe { v8::CallbackScope::new(context) };
    let isolate = unsafe { v8__Context__GetIsolate(&*context) };
    let Some(state) = state_from_isolate_ptr(isolate) else {
        return;
    };
    let Some(callback_id) = state.module_wrap_initialize_import_meta_callback else {
        return;
    };
    let Some(handle_id) = module_wrap_handle_id_by_module(state, scope, module) else {
        return;
    };
    let Some(handle) = state.module_wrap_handles.get(&handle_id) else {
        return;
    };
    let Some(callback) = local_function(state, scope, callback_id) else {
        return;
    };
    let Some(wrapper) = local_value(state, scope, handle.wrapper_id) else {
        return;
    };
    let Some(id_value) = local_value(state, scope, handle.host_defined_option_id) else {
        return;
    };
    let recv = context.global(scope);
    let args = [id_value, meta.into(), wrapper];
    let mut try_catch = v8::TryCatch::new(scope);
    if callback
        .call(&mut try_catch, recv.into(), &args)
        .is_none()
    {
        let _ = try_catch.rethrow();
    }
}

extern "C" fn module_wrap_host_import_module_dynamically_callback(
    context: v8::Local<'_, v8::Context>,
    referrer: v8::Local<'_, v8::ScriptOrModule>,
    specifier: v8::Local<'_, v8::String>,
    import_assertions: v8::Local<'_, v8::FixedArray>,
) -> *mut v8::Promise {
    let scope = &mut unsafe { v8::CallbackScope::new(context) };
    let isolate = unsafe { v8__Context__GetIsolate(&*context) };
    let Some(state) = state_from_isolate_ptr(isolate) else {
        return ptr::null_mut();
    };
    let Some(callback_id) = state.module_wrap_import_module_dynamically_callback else {
        return ptr::null_mut();
    };
    let Some(callback) = local_function(state, scope, callback_id) else {
        return ptr::null_mut();
    };

    let id_value = {
        let options = referrer.get_host_defined_options();
        if options.length() > 8 {
            options.get(scope, 8).into()
        } else {
            let key = private_key(scope, "node:host_defined_option_symbol");
            context
                .global(scope)
                .get_private(scope, key)
                .unwrap_or_else(|| v8::undefined(scope).into())
        }
    };
    let phase = v8::Integer::new(scope, 2);
    let attrs = match create_dynamic_import_attributes_object(state, scope, import_assertions) {
        Ok(attrs) => attrs,
        Err(_) => return ptr::null_mut(),
    };
    let resource_name = referrer.get_resource_name();
    let recv = context.global(scope);
    let args = [
        id_value,
        specifier.into(),
        phase.into(),
        attrs.into(),
        resource_name,
    ];
    let mut try_catch = v8::TryCatch::new(scope);
    let Some(result) = callback.call(&mut try_catch, recv.into(), &args) else {
        let _ = try_catch.rethrow();
        return ptr::null_mut();
    };
    let Some(resolver) = v8::PromiseResolver::new(&mut try_catch) else {
        return ptr::null_mut();
    };
    if resolver.resolve(&mut try_catch, result).is_none() {
        return ptr::null_mut();
    }
    local_to_raw(resolver.get_promise(&mut try_catch)) as *mut v8::Promise
}

fn module_wrap_link_required_facade_original<'a>(
    context: v8::Local<'a, v8::Context>,
    specifier: v8::Local<'a, v8::String>,
    _import_assertions: v8::Local<'a, v8::FixedArray>,
    _referrer: v8::Local<'a, v8::Module>,
) -> Option<v8::Local<'a, v8::Module>> {
    let scope = &mut unsafe { v8::CallbackScope::new(context) };
    let isolate = unsafe { v8__Context__GetIsolate(&*context) };
    let state = state_from_isolate_ptr(isolate)?;
    if specifier.to_rust_string_lossy(scope) != "original" {
        return None;
    }
    let handle_id = state.temporary_required_module_facade_original?;
    let handle = state.module_wrap_handles.get(&handle_id)?;
    Some(v8::Local::new(scope, &handle.module))
}

fn store_callback_invocation<'s, 'a>(
    state: &mut SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    args: &v8::FunctionCallbackArguments<'a>,
    data_val: u64,
) -> u32 {
    let mut argv_ids = Vec::with_capacity(args.length() as usize);
    for i in 0..args.length() {
        argv_ids.push(store_global_value(state, scope, local_for_scope(args.get(i))));
    }
    let this_id = store_global_value(state, scope, local_for_scope(args.this().into()));
    let id = next_id(&mut state.next_callback_invocation_id);
    state.callback_invocations.insert(
        id,
        CallbackInvocation {
            argv_ids,
            this_id,
            data_val,
            new_target_id: 0,
        },
    );
    id
}

fn callback_binding_data<'s>(
    scope: &mut v8::HandleScope<'s>,
    binding: &CallbackBinding,
) -> v8::Local<'s, v8::External> {
    v8::External::new(scope, (binding as *const CallbackBinding).cast_mut().cast())
}

fn make_function_from_binding<'s>(
    state: &mut SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    name: Option<&str>,
    binding: Box<CallbackBinding>,
) -> Result<v8::Local<'s, v8::Function>, i32> {
    let data = callback_binding_data(scope, &binding);
    let function = v8::Function::builder(generic_callback_adapter)
        .data(data.into())
        .build(scope)
        .ok_or(NAPI_GENERIC_FAILURE)?;
    if let Some(name) = name
        && let Some(name) = v8::String::new(scope, name)
    {
        function.set_name(name);
    }
    state.callback_bindings.push(binding);
    Ok(function)
}

fn property_attr_from_napi(attrs: i32) -> v8::PropertyAttribute {
    let mut out = v8::NONE;
    if attrs & (1 << 0) == 0 {
        out = out + v8::READ_ONLY;
    }
    if attrs & (1 << 1) == 0 {
        out = out + v8::DONT_ENUM;
    }
    if attrs & (1 << 2) == 0 {
        out = out + v8::DONT_DELETE;
    }
    out
}

fn make_prop_name<'s>(
    state: &SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    name_ptr: *const i8,
    name_id: u32,
) -> Result<v8::Local<'s, v8::Name>, i32> {
    if !name_ptr.is_null() {
        let c_name = unsafe { CStr::from_ptr(name_ptr) }.to_bytes().to_vec();
        let key = v8::String::new_from_utf8(scope, &c_name, v8::NewStringType::Normal)
            .ok_or(NAPI_GENERIC_FAILURE)?;
        return Ok(key.into());
    }
    let value = local_value(state, scope, name_id).ok_or(NAPI_NAME_EXPECTED)?;
    value.try_into().map_err(|_| NAPI_NAME_EXPECTED)
}

fn prop_name_ptr_at(ptr: *const *const i8, index: usize) -> *const i8 {
    if ptr.is_null() {
        ptr::null()
    } else {
        unsafe { *ptr.add(index) }
    }
}

fn prop_u32_at(ptr: *const u32, index: usize) -> u32 {
    if ptr.is_null() {
        0
    } else {
        unsafe { *ptr.add(index) }
    }
}

fn prop_i32_at(ptr: *const i32, index: usize) -> i32 {
    if ptr.is_null() {
        0
    } else {
        unsafe { *ptr.add(index) }
    }
}

fn prop_name_string(ptr: *const i8) -> Option<String> {
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned())
    }
}

fn make_callback_function<'s>(
    env: SnapiEnv,
    state: &mut SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    reg_id: u32,
    kind: CallbackKind,
    name: Option<&str>,
) -> Result<v8::Local<'s, v8::Function>, i32> {
    if !state.cb_registry.contains_key(&reg_id) {
        return Err(NAPI_INVALID_ARG);
    }
    let binding = Box::new(CallbackBinding {
        state: env,
        reg_id,
        kind,
    });
    let binding_ptr = (&*binding) as *const CallbackBinding;
    let data = v8::External::new(scope, binding_ptr.cast_mut().cast());
    let function = v8::Function::builder(generic_callback_adapter)
        .data(data.into())
        .build(scope)
        .ok_or(NAPI_GENERIC_FAILURE)?;
    if let Some(name) = name
        && let Some(name) = v8::String::new(scope, name)
    {
        function.set_name(name);
    }
    state.callback_bindings.push(binding);
    Ok(function)
}

fn define_properties_on_target<'s>(
    env: SnapiEnv,
    state: &mut SnapiEnvState,
    scope: &mut v8::HandleScope<'s>,
    default_target: v8::Local<'s, v8::Object>,
    static_target: Option<v8::Local<'s, v8::Object>>,
    prop_count: u32,
    prop_names: *const *const i8,
    prop_name_ids: *const u32,
    prop_types: *const u32,
    prop_value_ids: *const u32,
    prop_method_reg_ids: *const u32,
    prop_getter_reg_ids: *const u32,
    prop_setter_reg_ids: *const u32,
    prop_attributes: *const i32,
) -> i32 {
    for index in 0..prop_count as usize {
        let attrs = prop_i32_at(prop_attributes, index);
        let name_ptr = prop_name_ptr_at(prop_names, index);
        let key = match make_prop_name(state, scope, name_ptr, prop_u32_at(prop_name_ids, index)) {
            Ok(key) => key,
            Err(status) => return status,
        };
        let target = if attrs & NAPI_STATIC != 0 {
            static_target.unwrap_or(default_target)
        } else {
            default_target
        };
        let prop_name = prop_name_string(name_ptr);
        match prop_u32_at(prop_types, index) {
            0 => {
                let value = match object_value_data(state, scope, prop_u32_at(prop_value_ids, index)) {
                    Ok(value) => value,
                    Err(status) => return status,
                };
                let mut tc = v8::TryCatch::new(scope);
                if !target
                    .define_own_property(&mut tc, key, value, property_attr_from_napi(attrs))
                    .unwrap_or(false)
                {
                    if let Some(exc) = tc.exception() {
                        set_pending_exception(state, &mut tc, exc);
                        return NAPI_PENDING_EXCEPTION;
                    }
                    return NAPI_GENERIC_FAILURE;
                }
            }
            1 => {
                let function = match make_callback_function(
                    env,
                    state,
                    scope,
                    prop_u32_at(prop_method_reg_ids, index),
                    CallbackKind::Method,
                    prop_name.as_deref(),
                ) {
                    Ok(function) => function,
                    Err(status) => return status,
                };
                let mut tc = v8::TryCatch::new(scope);
                if !target
                    .define_own_property(
                        &mut tc,
                        key,
                        function.into(),
                        property_attr_from_napi(attrs),
                    )
                    .unwrap_or(false)
                {
                    if let Some(exc) = tc.exception() {
                        set_pending_exception(state, &mut tc, exc);
                        return NAPI_PENDING_EXCEPTION;
                    }
                    return NAPI_GENERIC_FAILURE;
                }
            }
            2 | 3 | 4 => {
                let getter = if matches!(prop_u32_at(prop_types, index), 2 | 4) {
                    match make_callback_function(
                        env,
                        state,
                        scope,
                        prop_u32_at(prop_getter_reg_ids, index),
                        CallbackKind::Getter,
                        prop_name.as_deref(),
                    ) {
                        Ok(function) => Some(function),
                        Err(status) => return status,
                    }
                } else {
                    None
                };
                let setter = if prop_u32_at(prop_types, index) == 3 {
                    match make_callback_function(
                        env,
                        state,
                        scope,
                        prop_u32_at(prop_setter_reg_ids, index),
                        CallbackKind::Setter,
                        prop_name.as_deref(),
                    ) {
                        Ok(function) => Some(function),
                        Err(status) => return status,
                    }
                } else if prop_u32_at(prop_types, index) == 4 {
                    match make_callback_function(
                        env,
                        state,
                        scope,
                        prop_u32_at(prop_getter_reg_ids, index),
                        CallbackKind::Setter,
                        prop_name.as_deref(),
                    ) {
                        Ok(function) => Some(function),
                        Err(status) => return status,
                    }
                } else {
                    None
                };
                let getter_value = getter
                    .map(|getter| getter.into())
                    .unwrap_or_else(|| v8::undefined(scope).into());
                let setter_value = setter
                    .map(|setter| setter.into())
                    .unwrap_or_else(|| v8::undefined(scope).into());
                let enumerable = v8::Boolean::new(scope, attrs & (1 << 1) != 0);
                let configurable = v8::Boolean::new(scope, attrs & (1 << 2) != 0);
                if let Err(status) = call_helper(
                    state,
                    scope,
                    "defineAccessor",
                    &[
                        target.into(),
                        key.into(),
                        getter_value,
                        setter_value,
                        enumerable.into(),
                        configurable.into(),
                    ],
                ) {
                    return status;
                }
            }
            _ => return NAPI_INVALID_ARG,
        }
    }
    NAPI_OK
}

fn typed_array_ctor_name(array_type: i32) -> Option<&'static str> {
    match array_type {
        0 => Some("Int8Array"),
        1 => Some("Uint8Array"),
        2 => Some("Uint8ClampedArray"),
        3 => Some("Int16Array"),
        4 => Some("Uint16Array"),
        5 => Some("Int32Array"),
        6 => Some("Uint32Array"),
        7 => Some("Float32Array"),
        8 => Some("Float64Array"),
        9 => Some("BigInt64Array"),
        10 => Some("BigUint64Array"),
        11 => Some("Float16Array"),
        _ => None,
    }
}

fn typed_array_type_of(value: v8::Local<v8::Value>) -> Option<i32> {
    if value.is_int8_array() {
        Some(0)
    } else if value.is_uint8_array() {
        Some(1)
    } else if value.is_uint8_clamped_array() {
        Some(2)
    } else if value.is_int16_array() {
        Some(3)
    } else if value.is_uint16_array() {
        Some(4)
    } else if value.is_int32_array() {
        Some(5)
    } else if value.is_uint32_array() {
        Some(6)
    } else if value.is_float32_array() {
        Some(7)
    } else if value.is_float64_array() {
        Some(8)
    } else if value.is_big_int64_array() {
        Some(9)
    } else if value.is_big_uint64_array() {
        Some(10)
    } else {
        None
    }
}

fn typed_array_element_size(array_type: i32) -> Option<usize> {
    match array_type {
        0..=2 => Some(1),
        3..=4 => Some(2),
        5..=7 => Some(4),
        8..=10 => Some(8),
        _ => None,
    }
}

fn value_id_is_nullish(
    state: &SnapiEnvState,
    scope: &mut v8::HandleScope<'_>,
    id: u32,
) -> Result<bool, i32> {
    if id == 0 {
        return Ok(true);
    }
    let value = local_value(state, scope, id).ok_or(NAPI_INVALID_ARG)?;
    Ok(value.is_null_or_undefined())
}

fn generic_callback<'s, 'a>(
    scope: &mut v8::HandleScope<'s>,
    args: v8::FunctionCallbackArguments<'a>,
    mut rv: v8::ReturnValue,
) {
    bridge_lock!();
    eprintln!("[snapi] generic_callback enter");
    let Some(data) = args.data() else {
        eprintln!("[snapi] generic_callback missing data");
        return;
    };
    let Ok(external): Result<v8::Local<v8::External>, _> = data.try_into() else {
        eprintln!("[snapi] generic_callback bad external");
        return;
    };
    let binding = unsafe { &*(external.value() as *const CallbackBinding) };
    let Some(state) = state_mut(binding.state) else {
        eprintln!("[snapi] generic_callback missing state");
        return;
    };
    let _active_scope_guard = ActiveScopeGuard {
        state: state as *mut SnapiEnvState,
        prev_scope: state.active_scope,
    };
    state.active_scope = scope as *mut _ as *mut c_void;
    let Some(reg) = state.cb_registry.get(&binding.reg_id) else {
        eprintln!("[snapi] generic_callback missing reg {}", binding.reg_id);
        return;
    };
    let guest_env = reg.guest_env;
    let data_val = reg.data_val;
    let callback_id = match binding.kind {
        CallbackKind::Method | CallbackKind::Getter => reg.wasm_fn_ptr,
        CallbackKind::Setter => reg.wasm_setter_fn_ptr,
    };
    if callback_id == 0 {
        eprintln!("[snapi] generic_callback zero callback id");
        return;
    }
    let callback_ctx = state.active_callback_ctx.load(Ordering::SeqCst);
    if callback_ctx.is_null() {
        eprintln!("[snapi] generic_callback missing active callback ctx");
        return;
    }
    let invocation_id = store_callback_invocation(state, scope, &args, data_val);
    eprintln!("[snapi] generic_callback invoke {}", invocation_id);
    let ret_id = crate::guest::callback::snapi_host_invoke_wasm_callback(
        callback_ctx,
        guest_env,
        callback_id,
        invocation_id,
    );
    state.callback_invocations.remove(&invocation_id);
    if let Some(exception) = state.pending_exception.take() {
        let exception = v8::Local::new(scope, &exception);
        eprintln!("[snapi] generic_callback throw pending exception");
        scope.throw_exception(exception);
        return;
    }
    if ret_id == 0 {
        eprintln!("[snapi] generic_callback return 0");
        return;
    }
    if let Some(value) = local_value(state, scope, ret_id) {
        eprintln!("[snapi] generic_callback set return {}", ret_id);
        rv.set(value);
    }
}

fn generic_callback_adapter(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    rv: v8::ReturnValue,
) {
    eprintln!("[snapi] generic_callback_adapter enter");
    generic_callback(scope, args, rv);
}

pub unsafe fn snapi_bridge_init() -> i32 {
    init_v8();
    NAPI_OK
}

pub unsafe fn snapi_bridge_unofficial_set_flags_from_string(flags: *const i8, length: u32) -> i32 {
    let flags = cstr_string(flags, length);
    let filtered = filtered_v8_flags(&flags);
    if !filtered.is_empty() {
        v8::V8::set_flags_from_string(&filtered);
    }
    NAPI_OK
}

pub unsafe fn snapi_bridge_unofficial_create_env(
    _module_api_version: i32,
    env_out: *mut SnapiEnv,
) -> i32 {
    bridge_lock!();
    if env_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    init_v8();
    let runtime = new_runtime(None);
    let isolate_handle = runtime.isolate.thread_safe_handle();
    let mut state = Box::new(SnapiEnvState::new(runtime, isolate_handle));
    let env_ptr: SnapiEnv = &mut *state;
    register_live_env(env_ptr);
    *env_out = Box::into_raw(state);
    NAPI_OK
}

pub unsafe fn snapi_bridge_unofficial_create_env_with_options(
    module_api_version: i32,
    max_young_generation_size_in_bytes: u32,
    max_old_generation_size_in_bytes: u32,
    _code_range_size_in_bytes: u32,
    _stack_limit: u32,
    env_out: *mut SnapiEnv,
) -> i32 {
    bridge_lock!();
    if env_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    init_v8();
    let max_heap_size =
        (max_young_generation_size_in_bytes as usize) + (max_old_generation_size_in_bytes as usize);
    let runtime = new_runtime((max_heap_size > 0).then_some(max_heap_size));
    let isolate_handle = runtime.isolate.thread_safe_handle();
    let mut state = Box::new(SnapiEnvState::new(runtime, isolate_handle));
    let env_ptr: SnapiEnv = &mut *state;
    register_live_env(env_ptr);
    *env_out = Box::into_raw(state);
    let _ = module_api_version;
    NAPI_OK
}

pub unsafe fn snapi_bridge_unofficial_release_env(env: SnapiEnv) -> i32 {
    bridge_lock!();
    if env.is_null() {
        return NAPI_INVALID_ARG;
    }
    unregister_live_env(env);
    if let Some(state) = state_mut(env)
        && let Some(mut runtime) = state.runtime.take()
    {
        if state.near_heap_limit_callback.take().is_some() {
            runtime
                .isolate
                .remove_near_heap_limit_callback(snapi_near_heap_limit_callback, 0);
        }
        state.near_heap_limit_data = 0;
        unsafe {
            snapi_v8_set_fatal_error_handler(ptr::from_mut(&mut *runtime.isolate), None);
            snapi_v8_set_oom_error_handler(ptr::from_mut(&mut *runtime.isolate), None);
        }
        {
            let scope = &mut v8::HandleScope::new(&mut runtime.isolate);
            let context = v8::Local::new(scope, &runtime.context);
            unsafe {
                v8__Context__Exit(&*context);
            }
        }
        runtime.env_handle_scope = None;
        unsafe {
            runtime.isolate.exit();
        }
    }
    drop(Box::from_raw(env));
    NAPI_OK
}

pub unsafe fn snapi_bridge_unofficial_release_env_with_loop(env: SnapiEnv, _loop_id: u32) -> i32 {
    snapi_bridge_unofficial_release_env(env)
}

pub unsafe fn snapi_bridge_unofficial_low_memory_notification(env: SnapiEnv) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        scope.low_memory_notification();
    });
    NAPI_OK
}

pub unsafe fn snapi_bridge_unofficial_process_microtasks(env: SnapiEnv) -> i32 {
    eprintln!("[snapi] process_microtasks");
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        scope.perform_microtask_checkpoint();
    });
    NAPI_OK
}

pub unsafe fn snapi_bridge_unofficial_request_gc_for_testing(env: SnapiEnv) -> i32 {
    snapi_bridge_unofficial_low_memory_notification(env)
}

pub unsafe fn snapi_bridge_unofficial_cancel_terminate_execution(env: SnapiEnv) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    if state.isolate_handle.cancel_terminate_execution() {
        NAPI_OK
    } else {
        NAPI_GENERIC_FAILURE
    }
}

pub unsafe fn snapi_bridge_unofficial_terminate_execution(env: SnapiEnv) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    if state.isolate_handle.terminate_execution() {
        NAPI_OK
    } else {
        NAPI_GENERIC_FAILURE
    }
}

pub unsafe fn snapi_bridge_unofficial_request_interrupt(
    env: SnapiEnv,
    guest_env: u32,
    wasm_fn_ptr: u32,
    data: u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let ctx = state.active_callback_ctx.load(Ordering::SeqCst);
    let payload = Box::into_raw(Box::new((ctx, guest_env, wasm_fn_ptr, data))) as *mut c_void;
    extern "C" fn interrupt_cb(isolate: &mut v8::Isolate, data: *mut c_void) {
        let payload = unsafe { Box::from_raw(data as *mut (*mut c_void, u32, u32, u32)) };
        let _ = isolate;
        crate::guest::callback::snapi_host_invoke_wasm_callback(payload.0, payload.1, payload.2, payload.3);
    }
    if state.isolate_handle.request_interrupt(interrupt_cb, payload) {
        NAPI_OK
    } else {
        let _ = Box::from_raw(payload as *mut (*mut c_void, u32, u32, u32));
        NAPI_GENERIC_FAILURE
    }
}

pub unsafe fn snapi_bridge_swap_active_callback_ctx(
    env: SnapiEnv,
    callback_ctx: *mut c_void,
) -> *mut c_void {
    bridge_lock!();
    let Some(state) = state_mut(env) else {
        return ptr::null_mut();
    };
    state.active_callback_ctx.swap(callback_ctx, Ordering::SeqCst)
}

pub unsafe fn snapi_bridge_unofficial_free_buffer(data: *mut c_void) {
    if !data.is_null() {
        free(data);
    }
}

pub unsafe fn snapi_bridge_get_undefined(env: SnapiEnv, out_id: *mut u32) -> i32 {
    eprintln!("[snapi] get_undefined");
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = v8::undefined(scope);
        let id = store_global_value(state, scope, value.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_get_null(env: SnapiEnv, out_id: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = v8::null(scope);
        let id = store_global_value(state, scope, value.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_get_boolean(env: SnapiEnv, value: i32, out_id: *mut u32) -> i32 {
    eprintln!("[snapi] get_boolean value={value}");
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = v8::Boolean::new(scope, value != 0);
        let id = store_global_value(state, scope, value.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_get_global(env: SnapiEnv, out_id: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let global = scope.get_current_context().global(scope);
        let id = store_global_value(state, scope, global.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_string_utf8(
    env: SnapiEnv,
    str_ptr: *const i8,
    wasm_length: u32,
    out_id: *mut u32,
) -> i32 {
    eprintln!("[snapi] create_string_utf8 len={wasm_length}");
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let bytes = cstr_bytes(str_ptr, wasm_length);
    with_scope(state, |scope, state| {
        let Some(value) = v8::String::new_from_utf8(scope, &bytes, v8::NewStringType::Normal) else {
            return NAPI_GENERIC_FAILURE;
        };
        let id = store_global_value(state, scope, value.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_string_latin1(
    env: SnapiEnv,
    str_ptr: *const i8,
    wasm_length: u32,
    out_id: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let bytes = cstr_bytes(str_ptr, wasm_length);
    with_scope(state, |scope, state| {
        let Some(value) = v8::String::new_from_one_byte(scope, &bytes, v8::NewStringType::Normal) else {
            return NAPI_GENERIC_FAILURE;
        };
        let id = store_global_value(state, scope, value.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_string_utf16(
    env: SnapiEnv,
    str_ptr: *const u16,
    wasm_length: u32,
    out_id: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let bytes = utf16_bytes(str_ptr, wasm_length);
    with_scope(state, |scope, state| {
        let Some(value) = v8::String::new_from_two_byte(scope, &bytes, v8::NewStringType::Normal) else {
            return NAPI_GENERIC_FAILURE;
        };
        let id = store_global_value(state, scope, value.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_int32(env: SnapiEnv, value: i32, out_id: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = v8::Integer::new(scope, value);
        let id = store_global_value(state, scope, value.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_uint32(env: SnapiEnv, value: u32, out_id: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = v8::Integer::new_from_unsigned(scope, value);
        let id = store_global_value(state, scope, value.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_double(env: SnapiEnv, value: f64, out_id: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = v8::Number::new(scope, value);
        let id = store_global_value(state, scope, value.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_int64(env: SnapiEnv, value: i64, out_id: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = v8::Number::new(scope, value as f64);
        let id = store_global_value(state, scope, value.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_object(env: SnapiEnv, out_id: *mut u32) -> i32 {
    eprintln!("[snapi] create_object");
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = v8::Object::new(scope);
        let id = store_global_value(state, scope, value.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_array(env: SnapiEnv, out_id: *mut u32) -> i32 {
    snapi_bridge_create_array_with_length(env, 0, out_id)
}

pub unsafe fn snapi_bridge_create_array_with_length(
    env: SnapiEnv,
    length: u32,
    out_id: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = v8::Array::new(scope, length as i32);
        let id = store_global_value(state, scope, value.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_symbol(env: SnapiEnv, description_id: u32, out_id: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let description = if description_id == 0 {
            None
        } else {
            let value = try_status!(object_value_data(state, scope, description_id));
            value.to_string(scope)
        };
        let symbol = v8::Symbol::new(scope, description);
        let id = store_global_value(state, scope, symbol.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_bigint_int64(
    env: SnapiEnv,
    value: i64,
    out_id: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = v8::BigInt::new_from_i64(scope, value);
        let id = store_global_value(state, scope, value.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_bigint_uint64(
    env: SnapiEnv,
    value: u64,
    out_id: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = v8::BigInt::new_from_u64(scope, value);
        let id = store_global_value(state, scope, value.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_date(env: SnapiEnv, time: f64, out_id: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let Some(date) = v8::Date::new(scope, time) else {
            return NAPI_GENERIC_FAILURE;
        };
        let id = store_global_value(state, scope, date.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_external(env: SnapiEnv, data_val: u64, out_id: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let external = v8::External::new(scope, data_val as usize as *mut c_void);
        let id = store_global_value(state, scope, external.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_get_value_string_utf8(
    env: SnapiEnv,
    id: u32,
    buf: *mut i8,
    bufsize: usize,
    result: *mut usize,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let string = try_status!(string_value(scope, value));
        let rust = string.to_rust_string_lossy(scope);
        if !result.is_null() {
            unsafe { result.write(rust.len()) };
        }
        write_buffer_u8(buf.cast(), bufsize, rust.as_bytes());
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_get_value_string_latin1(
    env: SnapiEnv,
    id: u32,
    buf: *mut i8,
    bufsize: usize,
    result: *mut usize,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let string = try_status!(string_value(scope, value));
        let len = string.length();
        let mut data = vec![0u8; len];
        let written = string.write_one_byte(
            scope,
            &mut data,
            0,
            v8::WriteOptions::NO_NULL_TERMINATION,
        );
        data.truncate(written);
        if !result.is_null() {
            unsafe { result.write(data.len()) };
        }
        write_buffer_u8(buf.cast(), bufsize, &data);
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_get_value_string_utf16(
    env: SnapiEnv,
    id: u32,
    buf: *mut u16,
    bufsize: usize,
    result: *mut usize,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let string = try_status!(string_value(scope, value));
        let len = string.length();
        let mut data = vec![0u16; len];
        let written = string.write(scope, &mut data, 0, v8::WriteOptions::NO_NULL_TERMINATION);
        data.truncate(written);
        if !result.is_null() {
            unsafe { result.write(data.len()) };
        }
        write_buffer_u16(buf, bufsize, &data);
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_get_value_int32(env: SnapiEnv, id: u32, result: *mut i32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let Some(out) = value.int32_value(scope) else {
            return NAPI_NUMBER_EXPECTED;
        };
        write_out(result, out)
    })
}

pub unsafe fn snapi_bridge_get_value_uint32(env: SnapiEnv, id: u32, result: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let Some(out) = value.uint32_value(scope) else {
            return NAPI_NUMBER_EXPECTED;
        };
        write_out(result, out)
    })
}

pub unsafe fn snapi_bridge_get_value_double(env: SnapiEnv, id: u32, result: *mut f64) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let Some(out) = value.number_value(scope) else {
            return NAPI_NUMBER_EXPECTED;
        };
        write_out(result, out)
    })
}

pub unsafe fn snapi_bridge_get_value_int64(env: SnapiEnv, id: u32, result: *mut i64) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let Some(out) = value.integer_value(scope) else {
            return NAPI_NUMBER_EXPECTED;
        };
        write_out(result, out)
    })
}

pub unsafe fn snapi_bridge_get_value_bool(env: SnapiEnv, id: u32, result: *mut i32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        write_out(result, value.boolean_value(scope) as i32)
    })
}

pub unsafe fn snapi_bridge_get_value_bigint_int64(
    env: SnapiEnv,
    id: u32,
    value: *mut i64,
    lossless: *mut i32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let local = try_status!(object_value_data(state, scope, id));
        let Ok(bigint): Result<v8::Local<v8::BigInt>, _> = local.try_into() else {
            return NAPI_BIGINT_EXPECTED;
        };
        let (v, ok) = bigint.i64_value();
        let status = write_out(value, v);
        if status != NAPI_OK {
            return status;
        }
        write_out(lossless, ok as i32)
    })
}

pub unsafe fn snapi_bridge_get_value_bigint_uint64(
    env: SnapiEnv,
    id: u32,
    value: *mut u64,
    lossless: *mut i32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let local = try_status!(object_value_data(state, scope, id));
        let Ok(bigint): Result<v8::Local<v8::BigInt>, _> = local.try_into() else {
            return NAPI_BIGINT_EXPECTED;
        };
        let (v, ok) = bigint.u64_value();
        let status = write_out(value, v);
        if status != NAPI_OK {
            return status;
        }
        write_out(lossless, ok as i32)
    })
}

pub unsafe fn snapi_bridge_get_date_value(env: SnapiEnv, id: u32, result: *mut f64) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let local = try_status!(object_value_data(state, scope, id));
        let Ok(date): Result<v8::Local<v8::Date>, _> = local.try_into() else {
            return NAPI_DATE_EXPECTED;
        };
        write_out(result, date.value_of())
    })
}

pub unsafe fn snapi_bridge_get_value_external(
    env: SnapiEnv,
    id: u32,
    data_out: *mut u64,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let local = try_status!(object_value_data(state, scope, id));
        let Ok(external): Result<v8::Local<v8::External>, _> = local.try_into() else {
            return NAPI_INVALID_ARG;
        };
        write_out(data_out, external.value() as usize as u64)
    })
}

pub unsafe fn snapi_bridge_set_property(
    env: SnapiEnv,
    obj_id: u32,
    key_id: u32,
    val_id: u32,
) -> i32 {
    eprintln!("[snapi] set_property obj={obj_id} key={key_id} val={val_id}");
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let obj = try_status!(value_as_object(try_status!(object_value_data(state, scope, obj_id))));
        let key = try_status!(object_value_data(state, scope, key_id));
        let val = try_status!(object_value_data(state, scope, val_id));
        let mut tc = v8::TryCatch::new(scope);
        if obj.set(&mut tc, key, val).unwrap_or(false) {
            NAPI_OK
        } else if let Some(exc) = tc.exception() {
            set_pending_exception(state, &mut tc, exc);
            NAPI_PENDING_EXCEPTION
        } else {
            NAPI_GENERIC_FAILURE
        }
    })
}

pub unsafe fn snapi_bridge_get_property(
    env: SnapiEnv,
    obj_id: u32,
    key_id: u32,
    out_id: *mut u32,
) -> i32 {
    eprintln!("[snapi] get_property obj={obj_id} key={key_id}");
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let obj = try_status!(value_as_object(try_status!(object_value_data(state, scope, obj_id))));
        let key = try_status!(object_value_data(state, scope, key_id));
        let mut tc = v8::TryCatch::new(scope);
        let Some(value) = obj.get(&mut tc, key) else {
            if let Some(exc) = tc.exception() {
                set_pending_exception(state, &mut tc, exc);
                return NAPI_PENDING_EXCEPTION;
            }
            return NAPI_GENERIC_FAILURE;
        };
        let id = store_global_value(state, &mut tc, value);
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_has_property(
    env: SnapiEnv,
    obj_id: u32,
    key_id: u32,
    result: *mut i32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let obj = try_status!(value_as_object(try_status!(object_value_data(state, scope, obj_id))));
        let key = try_status!(object_value_data(state, scope, key_id));
        let has = try_status!(obj.has(scope, key).ok_or(NAPI_GENERIC_FAILURE));
        write_out(result, has as i32)
    })
}

pub unsafe fn snapi_bridge_has_own_property(
    env: SnapiEnv,
    obj_id: u32,
    key_id: u32,
    result: *mut i32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let obj = try_status!(object_value_data(state, scope, obj_id));
        let key = try_status!(object_value_data(state, scope, key_id));
        let has =
            try_status!(call_helper(state, scope, "hasOwn", &[obj, key])).boolean_value(scope) as i32;
        write_out(result, has)
    })
}

pub unsafe fn snapi_bridge_delete_property(
    env: SnapiEnv,
    obj_id: u32,
    key_id: u32,
    result: *mut i32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let obj = try_status!(value_as_object(try_status!(object_value_data(state, scope, obj_id))));
        let key = try_status!(object_value_data(state, scope, key_id));
        let deleted = try_status!(obj.delete(scope, key).ok_or(NAPI_GENERIC_FAILURE));
        write_out(result, deleted as i32)
    })
}

pub unsafe fn snapi_bridge_set_named_property(
    env: SnapiEnv,
    obj_id: u32,
    name: *const i8,
    val_id: u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    if name.is_null() {
        return NAPI_INVALID_ARG;
    }
    let key = unsafe { CStr::from_ptr(name) }.to_bytes().to_vec();
    eprintln!(
        "[snapi] set_named_property obj={obj_id} name={} val={val_id}",
        String::from_utf8_lossy(&key)
    );
    with_scope(state, |scope, state| {
        let obj = try_status!(value_as_object(try_status!(object_value_data(state, scope, obj_id))));
        let Some(key) = v8::String::new_from_utf8(scope, &key, v8::NewStringType::Normal) else {
            return NAPI_GENERIC_FAILURE;
        };
        let val = try_status!(object_value_data(state, scope, val_id));
        let mut tc = v8::TryCatch::new(scope);
        if obj.set(&mut tc, key.into(), val).unwrap_or(false) {
            eprintln!("[snapi] set_named_property ok");
            NAPI_OK
        } else if let Some(exc) = tc.exception() {
            set_pending_exception(state, &mut tc, exc);
            NAPI_PENDING_EXCEPTION
        } else {
            NAPI_GENERIC_FAILURE
        }
    })
}

pub unsafe fn snapi_bridge_get_named_property(
    env: SnapiEnv,
    obj_id: u32,
    name: *const i8,
    out_id: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    if name.is_null() {
        return NAPI_INVALID_ARG;
    }
    let key = unsafe { CStr::from_ptr(name) }.to_bytes().to_vec();
    eprintln!(
        "[snapi] get_named_property obj={obj_id} name={}",
        String::from_utf8_lossy(&key)
    );
    with_scope(state, |scope, state| {
        let obj = try_status!(value_as_object(try_status!(object_value_data(state, scope, obj_id))));
        let Some(key) = v8::String::new_from_utf8(scope, &key, v8::NewStringType::Normal) else {
            return NAPI_GENERIC_FAILURE;
        };
        let mut tc = v8::TryCatch::new(scope);
        let Some(value) = obj.get(&mut tc, key.into()) else {
            if let Some(exc) = tc.exception() {
                set_pending_exception(state, &mut tc, exc);
                return NAPI_PENDING_EXCEPTION;
            }
            return NAPI_GENERIC_FAILURE;
        };
        let id = store_global_value(state, &mut tc, value);
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_has_named_property(
    env: SnapiEnv,
    obj_id: u32,
    name: *const i8,
    result: *mut i32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    if name.is_null() {
        return NAPI_INVALID_ARG;
    }
    let key = unsafe { CStr::from_ptr(name) }.to_bytes().to_vec();
    with_scope(state, |scope, state| {
        let obj = try_status!(value_as_object(try_status!(object_value_data(state, scope, obj_id))));
        let Some(key) = v8::String::new_from_utf8(scope, &key, v8::NewStringType::Normal) else {
            return NAPI_GENERIC_FAILURE;
        };
        let has = try_status!(obj.has(scope, key.into()).ok_or(NAPI_GENERIC_FAILURE));
        write_out(result, has as i32)
    })
}

pub unsafe fn snapi_bridge_set_element(
    env: SnapiEnv,
    obj_id: u32,
    index: u32,
    val_id: u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let obj = try_status!(value_as_object(try_status!(object_value_data(state, scope, obj_id))));
        let val = try_status!(object_value_data(state, scope, val_id));
        let mut tc = v8::TryCatch::new(scope);
        if obj.set_index(&mut tc, index, val).unwrap_or(false) {
            NAPI_OK
        } else if let Some(exc) = tc.exception() {
            set_pending_exception(state, &mut tc, exc);
            NAPI_PENDING_EXCEPTION
        } else {
            NAPI_GENERIC_FAILURE
        }
    })
}

pub unsafe fn snapi_bridge_get_element(
    env: SnapiEnv,
    obj_id: u32,
    index: u32,
    out_id: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let obj = try_status!(value_as_object(try_status!(object_value_data(state, scope, obj_id))));
        let mut tc = v8::TryCatch::new(scope);
        let Some(value) = obj.get_index(&mut tc, index) else {
            if let Some(exc) = tc.exception() {
                set_pending_exception(state, &mut tc, exc);
                return NAPI_PENDING_EXCEPTION;
            }
            return NAPI_GENERIC_FAILURE;
        };
        let id = store_global_value(state, &mut tc, value);
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_has_element(
    env: SnapiEnv,
    obj_id: u32,
    index: u32,
    result: *mut i32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let obj = try_status!(value_as_object(try_status!(object_value_data(state, scope, obj_id))));
        let has = try_status!(obj.has_index(scope, index).ok_or(NAPI_GENERIC_FAILURE));
        write_out(result, has as i32)
    })
}

pub unsafe fn snapi_bridge_delete_element(
    env: SnapiEnv,
    obj_id: u32,
    index: u32,
    result: *mut i32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let obj = try_status!(value_as_object(try_status!(object_value_data(state, scope, obj_id))));
        let deleted = try_status!(obj.delete_index(scope, index).ok_or(NAPI_GENERIC_FAILURE));
        write_out(result, deleted as i32)
    })
}

pub unsafe fn snapi_bridge_get_array_length(env: SnapiEnv, arr_id: u32, result: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let arr = try_status!(value_as_array(try_status!(object_value_data(state, scope, arr_id))));
        write_out(result, arr.length())
    })
}

pub unsafe fn snapi_bridge_get_property_names(
    env: SnapiEnv,
    obj_id: u32,
    out_id: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let obj = try_status!(value_as_object(try_status!(object_value_data(state, scope, obj_id))));
        let mut tc = v8::TryCatch::new(scope);
        let Some(names) = obj.get_property_names(&mut tc) else {
            if let Some(exc) = tc.exception() {
                set_pending_exception(state, &mut tc, exc);
                return NAPI_PENDING_EXCEPTION;
            }
            return NAPI_GENERIC_FAILURE;
        };
        let id = store_global_value(state, &mut tc, names.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_get_all_property_names(
    env: SnapiEnv,
    obj_id: u32,
    _mode: i32,
    _filter: i32,
    _conversion: i32,
    out_id: *mut u32,
) -> i32 {
    if out_id.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, obj_id));
        let obj = try_status!(value_as_object(value));
        let mode = v8::Integer::new(scope, _mode);
        let filter = v8::Integer::new(scope, _filter);
        let conversion = v8::Integer::new(scope, _conversion);
        let result = match call_helper(
            state,
            scope,
            "allProps",
            &[obj.into(), mode.into(), filter.into(), conversion.into()],
        ) {
            Ok(value) => value,
            Err(status) => return status,
        };
        let id = store_global_value(state, scope, result);
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_get_prototype(
    env: SnapiEnv,
    obj_id: u32,
    out_id: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let obj = try_status!(value_as_object(try_status!(object_value_data(state, scope, obj_id))));
        let Some(proto) = obj.get_prototype(scope) else {
            return NAPI_GENERIC_FAILURE;
        };
        let id = store_global_value(state, scope, proto);
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_strict_equals(
    env: SnapiEnv,
    a_id: u32,
    b_id: u32,
    result: *mut i32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let a = try_status!(object_value_data(state, scope, a_id));
        let b = try_status!(object_value_data(state, scope, b_id));
        write_out(result, a.strict_equals(b) as i32)
    })
}

pub unsafe fn snapi_bridge_typeof(env: SnapiEnv, id: u32, result: *mut i32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let value_type = if value.is_undefined() {
            NAPI_TYPEOF_UNDEFINED
        } else if value.is_null() {
            NAPI_TYPEOF_NULL
        } else if value.is_boolean() {
            NAPI_TYPEOF_BOOLEAN
        } else if value.is_number() {
            NAPI_TYPEOF_NUMBER
        } else if value.is_string() {
            NAPI_TYPEOF_STRING
        } else if value.is_symbol() {
            NAPI_TYPEOF_SYMBOL
        } else if value.is_function() {
            NAPI_TYPEOF_FUNCTION
        } else if value.is_big_int() {
            NAPI_TYPEOF_BIGINT
        } else if value.is_external() {
            NAPI_TYPEOF_EXTERNAL
        } else {
            NAPI_TYPEOF_OBJECT
        };
        write_out(result, value_type)
    })
}

pub unsafe fn snapi_bridge_call_function(
    env: SnapiEnv,
    recv_id: u32,
    func_id: u32,
    argc: u32,
    argv_ids: *const u32,
    out_id: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let arg_ids = if argc == 0 || argv_ids.is_null() {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(argv_ids, argc as usize) }.to_vec()
    };
    with_scope(state, |scope, state| {
        let recv = if recv_id == 0 {
            v8::undefined(scope).into()
        } else {
            try_status!(object_value_data(state, scope, recv_id))
        };
        let func = try_status!(value_as_function(try_status!(object_value_data(state, scope, func_id))));
        let mut args = Vec::with_capacity(arg_ids.len());
        for arg_id in &arg_ids {
            args.push(try_status!(object_value_data(state, scope, *arg_id)));
        }
        let mut tc = v8::TryCatch::new(scope);
        let Some(result) = func.call(&mut tc, recv, &args) else {
            if let Some(exc) = tc.exception() {
                set_pending_exception(state, &mut tc, exc);
                return NAPI_PENDING_EXCEPTION;
            }
            return NAPI_GENERIC_FAILURE;
        };
        let id = store_global_value(state, &mut tc, result);
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_coerce_to_bool(env: SnapiEnv, id: u32, out_id: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let bool_value = value.boolean_value(scope);
        let coerced = v8::Boolean::new(scope, bool_value);
        let out = store_global_value(state, scope, coerced.into());
        write_out(out_id, out)
    })
}

pub unsafe fn snapi_bridge_coerce_to_number(env: SnapiEnv, id: u32, out_id: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let mut tc = v8::TryCatch::new(scope);
        let Some(coerced) = value.to_number(&mut tc) else {
            if let Some(exc) = tc.exception() {
                set_pending_exception(state, &mut tc, exc);
                return NAPI_PENDING_EXCEPTION;
            }
            return NAPI_GENERIC_FAILURE;
        };
        let out = store_global_value(state, &mut tc, coerced.into());
        write_out(out_id, out)
    })
}

pub unsafe fn snapi_bridge_coerce_to_string(env: SnapiEnv, id: u32, out_id: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let mut tc = v8::TryCatch::new(scope);
        let Some(coerced) = value.to_string(&mut tc) else {
            if let Some(exc) = tc.exception() {
                set_pending_exception(state, &mut tc, exc);
                return NAPI_PENDING_EXCEPTION;
            }
            return NAPI_GENERIC_FAILURE;
        };
        let out = store_global_value(state, &mut tc, coerced.into());
        write_out(out_id, out)
    })
}

pub unsafe fn snapi_bridge_coerce_to_object(env: SnapiEnv, id: u32, out_id: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let mut tc = v8::TryCatch::new(scope);
        let Some(coerced) = value.to_object(&mut tc) else {
            if let Some(exc) = tc.exception() {
                set_pending_exception(state, &mut tc, exc);
                return NAPI_PENDING_EXCEPTION;
            }
            return NAPI_GENERIC_FAILURE;
        };
        let out = store_global_value(state, &mut tc, coerced.into());
        write_out(out_id, out)
    })
}

pub unsafe fn snapi_bridge_run_script(env: SnapiEnv, script_id: u32, out_value_id: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let source_value = try_status!(object_value_data(state, scope, script_id));
        let Ok(source): Result<v8::Local<v8::String>, _> = source_value.try_into() else {
            return NAPI_STRING_EXPECTED;
        };
        let mut tc = v8::TryCatch::new(scope);
        let Some(compiled) = v8::Script::compile(&mut tc, source, None) else {
            if let Some(exc) = tc.exception() {
                set_pending_exception(state, &mut tc, exc);
                return NAPI_PENDING_EXCEPTION;
            }
            return NAPI_GENERIC_FAILURE;
        };
        let Some(result) = compiled.run(&mut tc) else {
            if let Some(exc) = tc.exception() {
                set_pending_exception(state, &mut tc, exc);
                return NAPI_PENDING_EXCEPTION;
            }
            return NAPI_GENERIC_FAILURE;
        };
        let out_id = store_global_value(state, &mut tc, result);
        write_out(out_value_id, out_id)
    })
}

pub unsafe fn snapi_bridge_is_buffer(env: SnapiEnv, id: u32, result: *mut i32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        write_out(result, value.is_array_buffer_view() as i32)
    })
}

pub unsafe fn snapi_bridge_object_freeze(env: SnapiEnv, obj_id: u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let obj = try_status!(value_as_object(try_status!(object_value_data(state, scope, obj_id))));
        match call_helper(state, scope, "freeze", &[obj.into()]) {
            Ok(_) => NAPI_OK,
            Err(status) => status,
        }
    })
}

pub unsafe fn snapi_bridge_get_buffer_info(
    env: SnapiEnv,
    id: u32,
    data_out: *mut u64,
    length_out: *mut u32,
    backing_store_token_out: *mut u64,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let view: v8::Local<v8::ArrayBufferView> = match value.try_into() {
            Ok(value) => value,
            Err(_) => return NAPI_INVALID_ARG,
        };
        let arraybuffer = try_status!(view.buffer(scope).ok_or(NAPI_GENERIC_FAILURE));
        let byte_offset = view.byte_offset();
        let byte_length = view.byte_length();
        if !data_out.is_null() {
            let base = arraybuffer.get_backing_store().data() as usize;
            unsafe {
                data_out.write(base.saturating_add(byte_offset) as u64);
            }
        }
        if !length_out.is_null() {
            unsafe {
                length_out.write(byte_length as u32);
            }
        }
        if !backing_store_token_out.is_null() {
            unsafe {
                backing_store_token_out.write(arraybuffer.get_backing_store().data() as u64);
            }
        }
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_is_exception_pending(env: SnapiEnv, result: *mut i32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    write_out(result, state.pending_exception.is_some() as i32)
}

pub unsafe fn snapi_bridge_get_and_clear_last_exception(env: SnapiEnv, out_id: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let Some(exception) = state.pending_exception.take() else {
        return NAPI_GENERIC_FAILURE;
    };
    with_scope(state, |scope, state| {
        let value = v8::Local::new(scope, &exception);
        eprintln!("[snapi] clear_last_exception {}", value.to_rust_string_lossy(scope));
        let id = store_global_value(state, scope, value);
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_function(
    env: SnapiEnv,
    utf8name: *const i8,
    name_len: u32,
    reg_id: u32,
    out_id: *mut u32,
) -> i32 {
    eprintln!("[snapi] create_function reg_id={reg_id}");
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let name = if utf8name.is_null() {
        None
    } else if name_len == u32::MAX {
        Some(unsafe { CStr::from_ptr(utf8name) }.to_string_lossy().into_owned())
    } else {
        Some(cstr_string(utf8name, name_len))
    };
    with_scope(state, |scope, state| {
        if !state.cb_registry.contains_key(&reg_id) {
            return NAPI_INVALID_ARG;
        }
        let binding = Box::new(CallbackBinding {
            state: env,
            reg_id,
            kind: CallbackKind::Method,
        });
        let binding_ptr = (&*binding) as *const CallbackBinding;
        let data = v8::External::new(scope, binding_ptr.cast_mut().cast());
        let Some(function) = v8::Function::builder(generic_callback_adapter)
        .data(data.into())
        .build(scope)
        else {
            return NAPI_GENERIC_FAILURE;
        };
        if let Some(name) = name.as_deref()
            && let Some(name) = v8::String::new(scope, name)
        {
            function.set_name(name);
        }
        if reg_id == 1 && std::env::var_os("SNAPI_SELFTEST_CALLBACKS").is_some() {
            eprintln!("[snapi] selftest invoking reg_id={reg_id}");
            let mut tc = v8::TryCatch::new(scope);
            let recv = v8::undefined(&mut tc);
            let _ = function.call(&mut tc, recv.into(), &[]);
        }
        state.callback_bindings.push(binding);
        let id = store_global_value(state, scope, function.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_get_cb_info(
    env: SnapiEnv,
    cbinfo_id: u32,
    argc_ptr: *mut u32,
    argv_out: *mut u32,
    max_argv: u32,
    this_out: *mut u32,
    data_out: *mut u64,
) -> i32 {
    bridge_lock!();
    eprintln!("[snapi] get_cb_info cbinfo_id={cbinfo_id}");
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let Some(invocation) = state.callback_invocations.get(&cbinfo_id) else {
        return NAPI_INVALID_ARG;
    };
    let actual_argc = invocation.argv_ids.len() as u32;
    if !argc_ptr.is_null() {
        unsafe {
            argc_ptr.write(actual_argc);
        }
    }
    if !argv_out.is_null() {
        let to_write = max_argv.min(actual_argc) as usize;
        unsafe {
            ptr::copy_nonoverlapping(invocation.argv_ids.as_ptr(), argv_out, to_write);
        }
    }
    if !this_out.is_null() {
        unsafe {
            this_out.write(invocation.this_id);
        }
    }
    if !data_out.is_null() {
        unsafe {
            data_out.write(invocation.data_val);
        }
    }
    NAPI_OK
}

pub unsafe fn snapi_bridge_get_new_target(
    env: SnapiEnv,
    cbinfo_id: u32,
    out_id: *mut u32,
) -> i32 {
    bridge_lock!();
    eprintln!("[snapi] get_new_target cbinfo_id={cbinfo_id}");
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let Some(invocation) = state.callback_invocations.get(&cbinfo_id) else {
        return NAPI_INVALID_ARG;
    };
    write_out(out_id, invocation.new_target_id)
}

pub unsafe fn snapi_bridge_new_instance(
    env: SnapiEnv,
    ctor_id: u32,
    argc: u32,
    argv_ids: *const u32,
    out_id: *mut u32,
) -> i32 {
    eprintln!("[snapi] new_instance ctor_id={ctor_id} argc={argc}");
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let arg_ids = if argc == 0 || argv_ids.is_null() {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(argv_ids, argc as usize) }.to_vec()
    };
    with_scope(state, |scope, state| {
        eprintln!("[snapi] new_instance resolving ctor");
        let ctor = try_status!(value_as_function(try_status!(object_value_data(state, scope, ctor_id))));
        eprintln!("[snapi] new_instance resolving args");
        let mut args = Vec::with_capacity(arg_ids.len());
        for arg_id in &arg_ids {
            args.push(try_status!(object_value_data(state, scope, *arg_id)));
        }
        if args.len() == 2 {
            let global = scope.get_current_context().global(scope);
            if let Some(proxy_name) = v8::String::new(scope, "Proxy")
                && let Some(proxy_ctor) = global.get(scope, proxy_name.into())
                && proxy_ctor.strict_equals(ctor.into())
            {
                eprintln!("[snapi] new_instance using Proxy::new fast path");
                let target = try_status!(value_as_object(args[0]));
                let handler = try_status!(value_as_object(args[1]));
                let Some(proxy) = v8::Proxy::new(scope, target, handler) else {
                    eprintln!("[snapi] Proxy::new returned none");
                    return NAPI_GENERIC_FAILURE;
                };
                if std::env::var_os("SNAPI_SELFTEST_PROXY").is_some() {
                    eprintln!("[snapi] selftest reading proxy property");
                    let proxy_value: v8::Local<v8::Value> = proxy.into();
                    let Some(proxy_obj) = proxy_value.to_object(scope) else {
                        eprintln!("[snapi] selftest proxy object conversion failed");
                        return NAPI_GENERIC_FAILURE;
                    };
                    let Some(probe_key) = v8::String::new(scope, "PATH") else {
                        return NAPI_GENERIC_FAILURE;
                    };
                    {
                        let mut tc = v8::TryCatch::new(scope);
                        let _ = proxy_obj.get(&mut tc, probe_key.into());
                    }
                    eprintln!("[snapi] selftest checking proxy has");
                    {
                        let mut tc = v8::TryCatch::new(scope);
                        let _ = proxy_obj.has(&mut tc, probe_key.into());
                    }
                    let Some(probe_value) = v8::String::new(scope, "snapi-probe") else {
                        return NAPI_GENERIC_FAILURE;
                    };
                    eprintln!("[snapi] selftest setting proxy property");
                    let _ = call_helper(
                        state,
                        scope,
                        "setProp",
                        &[proxy_obj.into(), probe_key.into(), probe_value.into()],
                    );
                    eprintln!("[snapi] selftest defining proxy property");
                    let _ = call_helper(
                        state,
                        scope,
                        "defineProp",
                        &[proxy_obj.into(), probe_key.into(), probe_value.into()],
                    );
                    eprintln!("[snapi] selftest reading proxy descriptor");
                    let _ = call_helper(state, scope, "getOwnPropDesc", &[proxy_obj.into(), probe_key.into()]);
                    eprintln!("[snapi] selftest deleting proxy property");
                    {
                        let mut tc = v8::TryCatch::new(scope);
                        let _ = proxy_obj.delete(&mut tc, probe_key.into());
                    }
                    eprintln!("[snapi] selftest enumerating proxy keys");
                    {
                        let mut tc = v8::TryCatch::new(scope);
                        let _ = proxy_obj.get_property_names(&mut tc);
                    }
                }
                let id = store_global_value(state, scope, proxy.into());
                return write_out(out_id, id);
            }
        }
        eprintln!("[snapi] new_instance invoking ctor");
        let mut tc = v8::TryCatch::new(scope);
        let Some(instance) = ctor.new_instance(&mut tc, &args) else {
            eprintln!("[snapi] new_instance ctor returned none");
            if let Some(exc) = tc.exception() {
                set_pending_exception(state, &mut tc, exc);
                return NAPI_PENDING_EXCEPTION;
            }
            return NAPI_GENERIC_FAILURE;
        };
        eprintln!("[snapi] new_instance storing instance");
        let id = store_global_value(state, &mut tc, instance.into());
        write_out(out_id, id)
    })
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn snapi_bridge_define_properties(
    env: SnapiEnv,
    obj_id: u32,
    prop_count: u32,
    prop_names: *const *const i8,
    prop_name_ids: *const u32,
    prop_types: *const u32,
    prop_value_ids: *const u32,
    prop_method_reg_ids: *const u32,
    prop_getter_reg_ids: *const u32,
    prop_setter_reg_ids: *const u32,
    prop_attributes: *const i32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let obj = try_status!(value_as_object(try_status!(object_value_data(state, scope, obj_id))));
        define_properties_on_target(
            env,
            state,
            scope,
            obj,
            None,
            prop_count,
            prop_names,
            prop_name_ids,
            prop_types,
            prop_value_ids,
            prop_method_reg_ids,
            prop_getter_reg_ids,
            prop_setter_reg_ids,
            prop_attributes,
        )
    })
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn snapi_bridge_define_class(
    env: SnapiEnv,
    utf8name: *const i8,
    name_len: u32,
    ctor_reg_id: u32,
    prop_count: u32,
    prop_names: *const *const i8,
    prop_name_ids: *const u32,
    prop_types: *const u32,
    prop_value_ids: *const u32,
    prop_method_reg_ids: *const u32,
    prop_getter_reg_ids: *const u32,
    prop_setter_reg_ids: *const u32,
    prop_attributes: *const i32,
    out_id: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let name = if utf8name.is_null() {
        None
    } else if name_len == u32::MAX {
        Some(unsafe { CStr::from_ptr(utf8name) }.to_string_lossy().into_owned())
    } else {
        Some(cstr_string(utf8name, name_len))
    };
    with_scope(state, |scope, state| {
        let ctor = try_status!(make_callback_function(
            env,
            state,
            scope,
            ctor_reg_id,
            CallbackKind::Method,
            name.as_deref(),
        ));
        let ctor_obj = try_status!(value_as_object(ctor.into()));
        let Some(prototype_key) = v8::String::new(scope, "prototype") else {
            return NAPI_GENERIC_FAILURE;
        };
        let Some(proto_value) = ctor_obj.get(scope, prototype_key.into()) else {
            return NAPI_GENERIC_FAILURE;
        };
        let proto = try_status!(value_as_object(proto_value));
        let status = define_properties_on_target(
            env,
            state,
            scope,
            proto,
            Some(ctor_obj),
            prop_count,
            prop_names,
            prop_name_ids,
            prop_types,
            prop_value_ids,
            prop_method_reg_ids,
            prop_getter_reg_ids,
            prop_setter_reg_ids,
            prop_attributes,
        );
        if status != NAPI_OK {
            return status;
        }
        let id = store_global_value(state, scope, ctor.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_is_array(env: SnapiEnv, id: u32, result: *mut i32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        write_out(result, value.is_array() as i32)
    })
}

pub unsafe fn snapi_bridge_is_typedarray(env: SnapiEnv, id: u32, result: *mut i32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        write_out(result, value.is_typed_array() as i32)
    })
}

pub unsafe fn snapi_bridge_create_external_arraybuffer(
    env: SnapiEnv,
    data_addr: u64,
    byte_length: u32,
    backing_store_token_out: *mut u64,
    out_id: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let array_buffer = if data_addr == 0 && byte_length == 0 {
            let empty = v8::ArrayBuffer::new(scope, 0);
            empty.detach();
            empty
        } else {
            if data_addr == 0 {
                return NAPI_INVALID_ARG;
            }
            let backing_store = unsafe {
                v8::UniquePtr::from_raw(v8__ArrayBuffer__NewBackingStore__with_data(
                    data_addr as *mut c_void,
                    byte_length as usize,
                    noop_backing_store_deleter,
                    ptr::null_mut(),
                ))
            }
            .make_shared()
            .unwrap();
            v8::ArrayBuffer::with_backing_store(scope, &backing_store)
        };
        if !backing_store_token_out.is_null() {
            let token = if byte_length == 0 {
                0
            } else {
                array_buffer.get_backing_store().data() as u64
            };
            unsafe {
                backing_store_token_out.write(token);
            }
        }
        let id = store_global_value(state, scope, array_buffer.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_typedarray(
    env: SnapiEnv,
    array_type: i32,
    length: u32,
    arraybuffer_id: u32,
    byte_offset: u32,
    out_id: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let ctor_name = match typed_array_ctor_name(array_type) {
            Some(name) => name,
            None => return NAPI_INVALID_ARG,
        };
        let arraybuffer =
            try_status!(object_value_data(state, scope, arraybuffer_id)).try_into().map_err(|_| NAPI_INVALID_ARG);
        let arraybuffer: v8::Local<v8::ArrayBuffer> = try_status!(arraybuffer);
        let Some(ctor_key) = v8::String::new(scope, ctor_name) else {
            return NAPI_GENERIC_FAILURE;
        };
        let Some(ctor_value) = scope.get_current_context().global(scope).get(scope, ctor_key.into()) else {
            return NAPI_GENERIC_FAILURE;
        };
        let ctor = try_status!(value_as_function(ctor_value));
        let offset = v8::Integer::new_from_unsigned(scope, byte_offset);
        let length = v8::Integer::new_from_unsigned(scope, length);
        let args = [arraybuffer.into(), offset.into(), length.into()];
        let view = {
            let mut tc = v8::TryCatch::new(scope);
            let Some(view) = ctor.new_instance(&mut tc, &args) else {
                if let Some(exc) = tc.exception() {
                    set_pending_exception(state, &mut tc, exc);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            };
            view
        };
        let id = store_global_value(state, scope, view.into());
        write_out(out_id, id)
    })
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn snapi_bridge_get_typedarray_info(
    env: SnapiEnv,
    id: u32,
    type_out: *mut i32,
    length_out: *mut u32,
    data_out: *mut u64,
    arraybuffer_out: *mut u32,
    byte_offset_out: *mut u32,
    backing_store_token_out: *mut u64,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let typed_array: v8::Local<v8::TypedArray> = match value.try_into() {
            Ok(value) => value,
            Err(_) => return NAPI_INVALID_ARG,
        };
        let array_type = match typed_array_type_of(value) {
            Some(array_type) => array_type,
            None => return NAPI_GENERIC_FAILURE,
        };
        let byte_offset = typed_array.byte_offset();
        let byte_length = typed_array.byte_length();
        let element_size = typed_array_element_size(array_type).ok_or(NAPI_GENERIC_FAILURE);
        let element_size = try_status!(element_size);
        let arraybuffer = typed_array.buffer(scope).ok_or(NAPI_GENERIC_FAILURE);
        let arraybuffer = try_status!(arraybuffer);
        if !type_out.is_null() {
            unsafe {
                type_out.write(array_type);
            }
        }
        if !length_out.is_null() {
            unsafe {
                length_out.write((byte_length / element_size) as u32);
            }
        }
        if !data_out.is_null() {
            let base = arraybuffer.get_backing_store().data() as usize;
            unsafe {
                data_out.write(base.saturating_add(byte_offset) as u64);
            }
        }
        if !arraybuffer_out.is_null() {
            let arraybuffer_id = store_global_value(state, scope, arraybuffer.into());
            unsafe {
                arraybuffer_out.write(arraybuffer_id);
            }
        }
        if !byte_offset_out.is_null() {
            unsafe {
                byte_offset_out.write(byte_offset as u32);
            }
        }
        if !backing_store_token_out.is_null() {
            unsafe {
                backing_store_token_out.write(arraybuffer.get_backing_store().data() as u64);
            }
        }
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_wrap(
    env: SnapiEnv,
    obj_id: u32,
    native_data: u64,
    ref_out: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let obj = try_status!(value_as_object(try_status!(object_value_data(state, scope, obj_id))));
        let wrap_key = private_key(scope, WRAP_PRIVATE_KEY);
        if let Some(existing) = obj.get_private(scope, wrap_key)
            && existing.is_external()
        {
            return NAPI_INVALID_ARG;
        }
        let wrap_key = private_key(scope, WRAP_PRIVATE_KEY);
        let external = v8::External::new(scope, native_data as *mut c_void);
        {
            let mut tc = v8::TryCatch::new(scope);
            if !obj.set_private(&mut tc, wrap_key, external.into()).unwrap_or(false) {
                if let Some(exc) = tc.exception() {
                    set_pending_exception(state, &mut tc, exc);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            }
        }
        if !ref_out.is_null() {
            let ref_id = create_ref(state, scope, obj.into(), 0);
            unsafe {
                ref_out.write(ref_id);
            }
        }
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_unwrap(env: SnapiEnv, obj_id: u32, data_out: *mut u64) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let obj = try_status!(value_as_object(try_status!(object_value_data(state, scope, obj_id))));
        let wrap_key = private_key(scope, WRAP_PRIVATE_KEY);
        let Some(value) = obj.get_private(scope, wrap_key) else {
            return NAPI_INVALID_ARG;
        };
        let Ok(value): Result<v8::Local<v8::External>, _> = value.try_into() else {
            return NAPI_INVALID_ARG;
        };
        write_out(data_out, value.value() as u64)
    })
}

pub unsafe fn snapi_bridge_remove_wrap(env: SnapiEnv, obj_id: u32, data_out: *mut u64) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let obj = try_status!(value_as_object(try_status!(object_value_data(state, scope, obj_id))));
        let wrap_key = private_key(scope, WRAP_PRIVATE_KEY);
        let Some(value) = obj.get_private(scope, wrap_key) else {
            return NAPI_INVALID_ARG;
        };
        let Ok(value): Result<v8::Local<v8::External>, _> = value.try_into() else {
            return NAPI_INVALID_ARG;
        };
        let mut tc = v8::TryCatch::new(scope);
        if !obj.delete_private(&mut tc, wrap_key).unwrap_or(false) {
            if let Some(exc) = tc.exception() {
                set_pending_exception(state, &mut tc, exc);
                return NAPI_PENDING_EXCEPTION;
            }
            return NAPI_GENERIC_FAILURE;
        }
        write_out(data_out, value.value() as u64)
    })
}

pub unsafe fn snapi_bridge_create_reference(
    env: SnapiEnv,
    value_id: u32,
    initial_refcount: u32,
    ref_out: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, value_id));
        let ref_id = create_ref(state, scope, value, initial_refcount);
        write_out(ref_out, ref_id)
    })
}

pub unsafe fn snapi_bridge_delete_reference(env: SnapiEnv, ref_id: u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    state.refs.remove(&ref_id);
    NAPI_OK
}

pub unsafe fn snapi_bridge_reference_ref(env: SnapiEnv, ref_id: u32, result: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let Some(reference) = state.refs.get_mut(&ref_id) else {
        return NAPI_INVALID_ARG;
    };
    reference.refcount = reference.refcount.saturating_add(1);
    if !result.is_null() {
        unsafe {
            result.write(reference.refcount);
        }
    }
    NAPI_OK
}

pub unsafe fn snapi_bridge_reference_unref(env: SnapiEnv, ref_id: u32, result: *mut u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let Some(reference) = state.refs.get_mut(&ref_id) else {
        return NAPI_INVALID_ARG;
    };
    if reference.refcount > 0 {
        reference.refcount -= 1;
    }
    if !result.is_null() {
        unsafe {
            result.write(reference.refcount);
        }
    }
    NAPI_OK
}

pub unsafe fn snapi_bridge_get_reference_value(
    env: SnapiEnv,
    ref_id: u32,
    out_id: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let Some(reference) = state.refs.get(&ref_id) else {
            return NAPI_INVALID_ARG;
        };
        let value = v8::Local::new(scope, &reference.value);
        let id = store_global_value(state, scope, value);
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_unofficial_create_private_symbol(
    env: SnapiEnv,
    str_ptr: *const i8,
    wasm_length: u32,
    out_id: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    if str_ptr.is_null() && wasm_length > 0 {
        return NAPI_INVALID_ARG;
    }
    let description = if str_ptr.is_null() {
        String::new()
    } else {
        cstr_string(str_ptr, wasm_length)
    };
    with_scope(state, |scope, state| {
        let Some(desc) = v8::String::new(scope, &description) else {
            return NAPI_GENERIC_FAILURE;
        };
        let private = v8::Private::for_api(scope, Some(desc));
        let tmpl = v8::ObjectTemplate::new(scope);
        let Some(value_key) = v8::String::new(scope, "value") else {
            return NAPI_GENERIC_FAILURE;
        };
        tmpl.set(value_key.into(), private.into());
        let Some(holder) = tmpl.new_instance(scope) else {
            return NAPI_GENERIC_FAILURE;
        };
        let Some(symbol_value) = holder.get(scope, value_key.into()) else {
            return NAPI_GENERIC_FAILURE;
        };
        let id = store_global_value(state, scope, symbol_value);
        write_out(out_id, id)
    })
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn snapi_bridge_unofficial_contextify_contains_module_syntax(
    env: SnapiEnv,
    code_id: u32,
    filename_id: u32,
    resource_name_id: u32,
    cjs_var_in_scope: i32,
    result_out: *mut i32,
) -> i32 {
    if result_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let code_value = try_status!(object_value_data(state, scope, code_id));
        let code = try_status!(string_value(scope, code_value));
        let filename_value = try_status!(object_value_data(state, scope, filename_id));
        let filename = try_status!(string_value(scope, filename_value));
        let resource_name = if try_status!(value_id_is_nullish(state, scope, resource_name_id)) {
            filename
        } else {
            let resource_value = try_status!(object_value_data(state, scope, resource_name_id));
            try_status!(string_value(scope, resource_value))
        };

        let cjs_params = if cjs_var_in_scope != 0 {
            vec![
                v8::String::new(scope, "exports").ok_or(NAPI_GENERIC_FAILURE),
                v8::String::new(scope, "require").ok_or(NAPI_GENERIC_FAILURE),
                v8::String::new(scope, "module").ok_or(NAPI_GENERIC_FAILURE),
                v8::String::new(scope, "__filename").ok_or(NAPI_GENERIC_FAILURE),
                v8::String::new(scope, "__dirname").ok_or(NAPI_GENERIC_FAILURE),
            ]
        } else {
            Vec::new()
        };
        let mut params = Vec::with_capacity(cjs_params.len());
        for param in cjs_params {
            params.push(try_status!(param));
        }

        let undefined = v8::undefined(scope);
        let origin = v8::ScriptOrigin::new(
            scope,
            resource_name.into(),
            0,
            0,
            true,
            -1,
            undefined.into(),
            false,
            false,
            false,
        );
        let cjs_source = v8::script_compiler::Source::new(code, Some(&origin));
        let cjs_ok = {
            let mut try_catch = v8::TryCatch::new(scope);
            v8::script_compiler::compile_function_in_context(
                &mut try_catch,
                cjs_source,
                &params,
                &[],
                v8::script_compiler::CompileOptions::NoCompileOptions,
                v8::script_compiler::NoCacheReason::NoReason,
            )
            .is_some()
        };
        if cjs_ok {
            return write_out(result_out, 0);
        }

        let esm_source = v8::script_compiler::Source::new(code, Some(&origin));
        let esm_ok = {
            let mut try_catch = v8::TryCatch::new(scope);
            v8::script_compiler::compile_module(&mut try_catch, esm_source).is_some()
        };
        write_out(result_out, esm_ok as i32)
    })
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn snapi_bridge_unofficial_contextify_make_context(
    env: SnapiEnv,
    sandbox_or_symbol_id: u32,
    _name_id: u32,
    _origin_id: u32,
    _allow_code_gen_strings: i32,
    _allow_code_gen_wasm: i32,
    _own_microtask_queue: i32,
    host_defined_option_id: u32,
    result_out: *mut u32,
) -> i32 {
    if result_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let sandbox_value = try_status!(object_value_data(state, scope, sandbox_or_symbol_id));
        let vanilla = sandbox_value.is_symbol();
        if !vanilla && !sandbox_value.is_object() {
            return NAPI_INVALID_ARG;
        }
        if resolve_context_from_key(state, scope, sandbox_or_symbol_id).is_some() {
            return NAPI_INVALID_ARG;
        }

        let global_ptr = if vanilla {
            ptr::null()
        } else {
            &*sandbox_value as *const v8::Value
        };
        let isolate = unsafe { v8__Context__GetIsolate(&*scope.get_current_context()) };
        let Some(context) = local_from_raw(unsafe { v8__Context__New(isolate, ptr::null(), global_ptr) })
        else {
            return NAPI_PENDING_EXCEPTION;
        };

        let key_object = if vanilla {
            context.global(scope)
        } else {
            try_status!(value_as_object(sandbox_value))
        };
        let key_id = store_global_context(state, scope, key_object.into(), context);

        let context_key = private_key(scope, "node:contextify:context");
        let _ = key_object.set_private(scope, context_key, key_object.into());
        let host_key = private_key(scope, "node:host_defined_option_symbol");
        let host_value = if try_status!(value_id_is_nullish(state, scope, host_defined_option_id)) {
            v8::undefined(scope).into()
        } else {
            try_status!(object_value_data(state, scope, host_defined_option_id))
        };
        let _ = key_object.set_private(scope, host_key, host_value);

        write_out(result_out, key_id)
    })
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn snapi_bridge_unofficial_contextify_run_script(
    env: SnapiEnv,
    sandbox_or_null_id: u32,
    source_id: u32,
    filename_id: u32,
    line_offset: i32,
    column_offset: i32,
    _timeout: i64,
    _display_errors: i32,
    _break_on_sigint: i32,
    _break_on_first_line: i32,
    _host_defined_option_id: u32,
    result_out: *mut u32,
) -> i32 {
    if result_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let target_context = if try_status!(value_id_is_nullish(state, scope, sandbox_or_null_id)) {
            scope.get_current_context()
        } else {
            match resolve_context_from_key(state, scope, sandbox_or_null_id) {
                Some(context) => context,
                None => return NAPI_INVALID_ARG,
            }
        };
        let source_value = try_status!(object_value_data(state, scope, source_id));
        let source = try_status!(string_value(scope, source_value));
        let filename_value = try_status!(object_value_data(state, scope, filename_id));
        let filename = try_status!(string_value(scope, filename_value));
        let undefined = v8::undefined(scope);
        let origin = v8::ScriptOrigin::new(
            scope,
            filename.into(),
            line_offset,
            column_offset,
            true,
            -1,
            undefined.into(),
            false,
            false,
            false,
        );

        let result = {
            let mut target_scope = v8::ContextScope::new(scope, target_context);
            let mut try_catch = v8::TryCatch::new(&mut target_scope);
            let Some(script) = v8::Script::compile(&mut try_catch, source, Some(&origin)) else {
                if let Some(exception) = try_catch.exception() {
                    set_pending_exception(state, &mut try_catch, exception);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            };
            let Some(result) = script.run(&mut try_catch) else {
                if let Some(exception) = try_catch.exception() {
                    set_pending_exception(state, &mut try_catch, exception);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            };
            local_for_scope(result)
        };
        let id = store_global_value(state, scope, result);
        write_out(result_out, id)
    })
}

pub unsafe fn snapi_bridge_unofficial_contextify_dispose_context(
    env: SnapiEnv,
    sandbox_or_context_global_id: u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let key = try_status!(object_value_data(state, scope, sandbox_or_context_global_id));
        let mut remove_id = state
            .contexts
            .contains_key(&sandbox_or_context_global_id)
            .then_some(sandbox_or_context_global_id);
        if remove_id.is_none() {
            for candidate_id in state.contexts.keys().copied().collect::<Vec<_>>() {
                let Some(candidate) = local_value(state, scope, candidate_id) else {
                    continue;
                };
                if candidate.strict_equals(key) {
                    remove_id = Some(candidate_id);
                    break;
                }
            }
        }
        if let Some(candidate_id) = remove_id {
            state.contexts.remove(&candidate_id);
        }
        NAPI_OK
    })
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn snapi_bridge_unofficial_contextify_create_cached_data(
    env: SnapiEnv,
    code_id: u32,
    filename_id: u32,
    line_offset: i32,
    column_offset: i32,
    _host_defined_option_id: u32,
    result_out: *mut u32,
) -> i32 {
    if result_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let code_value = try_status!(object_value_data(state, scope, code_id));
        let code = try_status!(string_value(scope, code_value));
        let filename_value = try_status!(object_value_data(state, scope, filename_id));
        let filename = try_status!(string_value(scope, filename_value));
        let undefined = v8::undefined(scope);
        let origin = v8::ScriptOrigin::new(
            scope,
            filename.into(),
            line_offset,
            column_offset,
            true,
            -1,
            undefined.into(),
            false,
            false,
            false,
        );
        let source = v8::script_compiler::Source::new(code, Some(&origin));
        let buffer = {
            let mut try_catch = v8::TryCatch::new(scope);
            let Some(script) = v8::script_compiler::compile_unbound_script(
                &mut try_catch,
                source,
                v8::script_compiler::CompileOptions::NoCompileOptions,
                v8::script_compiler::NoCacheReason::NoReason,
            ) else {
                if let Some(exception) = try_catch.exception() {
                    set_pending_exception(state, &mut try_catch, exception);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            };
            let Some(cache) = script.create_code_cache() else {
                return NAPI_GENERIC_FAILURE;
            };
            let cache_bytes = cache.to_vec();
            let cache_len = cache_bytes.len();
            let backing =
                v8::ArrayBuffer::new_backing_store_from_boxed_slice(cache_bytes.into_boxed_slice());
            let shared = backing.make_shared();
            let ab = v8::ArrayBuffer::with_backing_store(&mut try_catch, &shared);
            let buffer =
                match node_buffer_from_arraybuffer(state, &mut try_catch, ab.into(), 0, cache_len) {
                    Ok(buffer) => buffer,
                    Err(status) => return status,
                };
            local_for_scope(buffer)
        };
        let id = store_global_value(state, scope, buffer);
        write_out(result_out, id)
    })
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn snapi_bridge_unofficial_contextify_compile_function(
    env: SnapiEnv,
    code_id: u32,
    filename_id: u32,
    line_offset: i32,
    column_offset: i32,
    cached_data_id: u32,
    produce_cached_data: i32,
    parsing_context_id: u32,
    context_extensions_id: u32,
    params_id: u32,
    host_defined_option_id: u32,
    result_out: *mut u32,
) -> i32 {
    eprintln!(
        "[snapi] contextify_compile_function code={} filename={} cached={} produce={} parse_ctx={} ctx_ext={} params={}",
        code_id, filename_id, cached_data_id, produce_cached_data, parsing_context_id, context_extensions_id, params_id
    );
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let code_value = try_status!(object_value_data(state, scope, code_id));
        let code = try_status!(string_value(scope, code_value));
        let filename_value = try_status!(object_value_data(state, scope, filename_id));
        let filename = try_status!(string_value(scope, filename_value));
        let cached_data_is_nullish = try_status!(value_id_is_nullish(state, scope, cached_data_id));
        let parsing_context = if try_status!(value_id_is_nullish(state, scope, parsing_context_id)) {
            scope.get_current_context()
        } else {
            let parsing_value = try_status!(object_value_data(state, scope, parsing_context_id));
            let parsing_obj = try_status!(value_as_object(parsing_value));
            parsing_obj.creation_context(scope)
        };

        let mut context_extensions = Vec::new();
        if !try_status!(value_id_is_nullish(state, scope, context_extensions_id)) {
            let array = try_status!(value_as_array(try_status!(object_value_data(
                state,
                scope,
                context_extensions_id
            ))));
            for index in 0..array.length() {
                let Some(item) = array.get_index(scope, index) else {
                    return NAPI_INVALID_ARG;
                };
                context_extensions.push(try_status!(value_as_object(item)));
            }
        }

        let mut params = Vec::new();
        if !try_status!(value_id_is_nullish(state, scope, params_id)) {
            let array = try_status!(value_as_array(try_status!(object_value_data(
                state, scope, params_id
            ))));
            for index in 0..array.length() {
                let Some(item) = array.get_index(scope, index) else {
                    return NAPI_INVALID_ARG;
                };
                let Ok(item): Result<v8::Local<v8::String>, _> = item.try_into() else {
                    return NAPI_INVALID_ARG;
                };
                params.push(item);
            }
        }

        let mut cached_data_bytes = Vec::new();
        let source = {
            let undefined = v8::undefined(scope);
            let origin = v8::ScriptOrigin::new(
                scope,
                filename.into(),
                line_offset,
                column_offset,
                true,
                -1,
                undefined.into(),
                false,
                false,
                false,
            );
            if !cached_data_is_nullish {
                let cached_value = try_status!(object_value_data(state, scope, cached_data_id));
                let Ok(cached_view): Result<v8::Local<v8::ArrayBufferView>, _> = cached_value.try_into()
                else {
                    return NAPI_INVALID_ARG;
                };
                cached_data_bytes.resize(cached_view.byte_length(), 0);
                cached_view.copy_contents(&mut cached_data_bytes);
                v8::script_compiler::Source::new_with_cached_data(
                    code,
                    Some(&origin),
                    v8::CachedData::new(&cached_data_bytes),
                )
            } else {
                v8::script_compiler::Source::new(code, Some(&origin))
            }
        };

        let function = {
            let mut parsing_scope = v8::ContextScope::new(scope, parsing_context);
            let options = if !cached_data_is_nullish {
                v8::script_compiler::CompileOptions::ConsumeCodeCache
            } else {
                v8::script_compiler::CompileOptions::NoCompileOptions
            };
            let mut tc = v8::TryCatch::new(&mut parsing_scope);
            let Some(function) = v8::script_compiler::compile_function_in_context(
                &mut tc,
                source,
                &params,
                &context_extensions,
                options,
                v8::script_compiler::NoCacheReason::NoReason,
            ) else {
                if let Some(exc) = tc.exception() {
                    set_pending_exception(state, &mut tc, exc);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            };
            local_for_scope(function)
        };

        {
            let host_value = if try_status!(value_id_is_nullish(state, scope, host_defined_option_id)) {
                v8::undefined(scope).into()
            } else {
                try_status!(object_value_data(state, scope, host_defined_option_id))
            };
            let key = private_key(scope, "node:host_defined_option_symbol");
            let mut tc = v8::TryCatch::new(scope);
            if !function
                .set_private(&mut tc, key, host_value)
                .unwrap_or(false)
            {
                if let Some(exc) = tc.exception() {
                    set_pending_exception(state, &mut tc, exc);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            }
        }

        let out = v8::Object::new(scope);
        let set_named = |scope: &mut v8::HandleScope<'_>,
                         state: &mut SnapiEnvState,
                         obj: v8::Local<'_, v8::Object>,
                         name: &str,
                         value: v8::Local<'_, v8::Value>|
         -> i32 {
            let Some(key) = v8::String::new(scope, name) else {
                return NAPI_GENERIC_FAILURE;
            };
            let mut tc = v8::TryCatch::new(scope);
            if !obj.set(&mut tc, key.into(), value).unwrap_or(false) {
                if let Some(exc) = tc.exception() {
                    set_pending_exception(state, &mut tc, exc);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            }
            NAPI_OK
        };
        let status = set_named(scope, state, out, "function", function.into());
        if status != NAPI_OK {
            return status;
        }
        let status = set_named(scope, state, out, "sourceURL", filename.into());
        if status != NAPI_OK {
            return status;
        }
        let undefined = v8::undefined(scope);
        let status = set_named(scope, state, out, "sourceMapURL", undefined.into());
        if status != NAPI_OK {
            return status;
        }
        if !cached_data_is_nullish {
            let rejected = v8::Boolean::new(scope, false);
            let status = set_named(
                scope,
                state,
                out,
                "cachedDataRejected",
                rejected.into(),
            );
            if status != NAPI_OK {
                return status;
            }
        }
        if produce_cached_data != 0 {
            let produced = v8::Boolean::new(scope, false);
            let status = set_named(
                scope,
                state,
                out,
                "cachedDataProduced",
                produced.into(),
            );
            if status != NAPI_OK {
                return status;
            }
        }
        let out_id = store_global_value(state, scope, out.into());
        eprintln!("[snapi] contextify_compile_function ok out={out_id}");
        write_out(result_out, out_id)
    })
}

pub unsafe fn snapi_bridge_unofficial_contextify_compile_function_for_cjs_loader(
    env: SnapiEnv,
    code_id: u32,
    filename_id: u32,
    _is_sea_main: i32,
    _should_detect_module: i32,
    result_out: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let code_value = try_status!(object_value_data(state, scope, code_id));
        let code = try_status!(string_value(scope, code_value));
        let filename_value = try_status!(object_value_data(state, scope, filename_id));
        let filename = try_status!(string_value(scope, filename_value));
        let undefined = v8::undefined(scope);
        let origin = v8::ScriptOrigin::new(
            scope,
            filename.into(),
            0,
            0,
            true,
            -1,
            undefined.into(),
            false,
            false,
            false,
        );
        let source = v8::script_compiler::Source::new(code, Some(&origin));
        let params = [
            v8::String::new(scope, "exports").ok_or(NAPI_GENERIC_FAILURE),
            v8::String::new(scope, "require").ok_or(NAPI_GENERIC_FAILURE),
            v8::String::new(scope, "module").ok_or(NAPI_GENERIC_FAILURE),
            v8::String::new(scope, "__filename").ok_or(NAPI_GENERIC_FAILURE),
            v8::String::new(scope, "__dirname").ok_or(NAPI_GENERIC_FAILURE),
        ];
        let mut params_vec = Vec::with_capacity(params.len());
        for param in params {
            params_vec.push(try_status!(param));
        }
        let function = {
            let mut tc = v8::TryCatch::new(scope);
            let Some(function) = v8::script_compiler::compile_function_in_context(
                &mut tc,
                source,
                &params_vec,
                &[],
                v8::script_compiler::CompileOptions::NoCompileOptions,
                v8::script_compiler::NoCacheReason::NoReason,
            ) else {
                if let Some(exc) = tc.exception() {
                    set_pending_exception(state, &mut tc, exc);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            };
            local_for_scope(function)
        };

        let out = v8::Object::new(scope);
        let set_named = |scope: &mut v8::HandleScope<'_>,
                         state: &mut SnapiEnvState,
                         obj: v8::Local<'_, v8::Object>,
                         name: &str,
                         value: v8::Local<'_, v8::Value>|
         -> i32 {
            let Some(key) = v8::String::new(scope, name) else {
                return NAPI_GENERIC_FAILURE;
            };
            let mut tc = v8::TryCatch::new(scope);
            if !obj.set(&mut tc, key.into(), value).unwrap_or(false) {
                if let Some(exc) = tc.exception() {
                    set_pending_exception(state, &mut tc, exc);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            }
            NAPI_OK
        };

        let rejected = v8::Boolean::new(scope, false);
        let status = set_named(scope, state, out, "cachedDataRejected", rejected.into());
        if status != NAPI_OK {
            return status;
        }
        let can_parse_as_esm = v8::Boolean::new(scope, false);
        let status = set_named(scope, state, out, "canParseAsESM", can_parse_as_esm.into());
        if status != NAPI_OK {
            return status;
        }
        let status = set_named(scope, state, out, "sourceMapURL", undefined.into());
        if status != NAPI_OK {
            return status;
        }
        let status = set_named(scope, state, out, "sourceURL", filename.into());
        if status != NAPI_OK {
            return status;
        }
        let status = set_named(scope, state, out, "function", function.into());
        if status != NAPI_OK {
            return status;
        }
        let out_id = store_global_value(state, scope, out.into());
        write_out(result_out, out_id)
    })
}

pub unsafe fn snapi_bridge_unofficial_get_continuation_preserved_embedder_data(
    env: SnapiEnv,
    out_id: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    if let Some(id) = state.continuation_preserved_embedder_data {
        return write_out(out_id, id);
    }
    with_scope(state, |scope, state| {
        let undefined = v8::undefined(scope);
        let id = store_global_value(state, scope, undefined.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_unofficial_set_continuation_preserved_embedder_data(
    env: SnapiEnv,
    value_id: u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    state.continuation_preserved_embedder_data = if value_id == 0 {
        None
    } else {
        Some(value_id)
    };
    NAPI_OK
}

pub unsafe fn snapi_bridge_unofficial_get_own_non_index_properties(
    env: SnapiEnv,
    value_id: u32,
    _filter_bits: u32,
    out_id: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, value_id));
        let obj = try_status!(value_as_object(value));
        let filter_bits = v8::Integer::new_from_unsigned(scope, _filter_bits);
        let result = match call_helper(state, scope, "ownNonIndex", &[obj.into(), filter_bits.into()])
        {
            Ok(value) => value,
            Err(status) => return status,
        };
        let id = store_global_value(state, scope, result);
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_unofficial_get_proxy_details(
    env: SnapiEnv,
    proxy_id: u32,
    target_out: *mut u32,
    handler_out: *mut u32,
) -> i32 {
    if target_out.is_null() || handler_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, proxy_id));
        let proxy: v8::Local<v8::Proxy> = match value.try_into() {
            Ok(proxy) => proxy,
            Err(_) => return NAPI_INVALID_ARG,
        };

        let Some(target) = local_from_raw(unsafe { v8__Proxy__GetTarget(&*proxy) }) else {
            return NAPI_GENERIC_FAILURE;
        };
        let Some(handler) = local_from_raw(unsafe { v8__Proxy__GetHandler(&*proxy) }) else {
            return NAPI_GENERIC_FAILURE;
        };

        let target_id = store_global_value(state, scope, target);
        let handler_id = store_global_value(state, scope, handler);
        let status = write_out(target_out, target_id);
        if status != NAPI_OK {
            return status;
        }
        write_out(handler_out, handler_id)
    })
}

pub unsafe fn snapi_bridge_node_api_set_prototype(
    env: SnapiEnv,
    object_id: u32,
    prototype_id: u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let object = try_status!(value_as_object(try_status!(object_value_data(state, scope, object_id))));
        let prototype = try_status!(object_value_data(state, scope, prototype_id));
        let mut tc = v8::TryCatch::new(scope);
        if !object.set_prototype(&mut tc, prototype).unwrap_or(false) {
            if let Some(exc) = tc.exception() {
                set_pending_exception(state, &mut tc, exc);
                return NAPI_PENDING_EXCEPTION;
            }
            return NAPI_GENERIC_FAILURE;
        }
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_alloc_cb_reg_id(env: SnapiEnv) -> u32 {
    bridge_lock!();
    let Some(state) = state_mut(env) else {
        return 0;
    };
    let reg_id = next_id(&mut state.next_cb_reg_id);
    state.cb_registry.entry(reg_id).or_insert(CbRegistration {
        guest_env: 0,
        wasm_fn_ptr: 0,
        wasm_setter_fn_ptr: 0,
        data_val: 0,
    });
    reg_id
}

pub unsafe fn snapi_bridge_register_callback(
    env: SnapiEnv,
    reg_id: u32,
    guest_env: u32,
    wasm_fn_ptr: u32,
    data_val: u64,
) {
    bridge_lock!();
    let Some(state) = state_mut(env) else {
        return;
    };
    state.cb_registry.insert(
        reg_id,
        CbRegistration {
            guest_env,
            wasm_fn_ptr,
            wasm_setter_fn_ptr: 0,
            data_val,
        },
    );
}

pub unsafe fn snapi_bridge_register_callback_pair(
    env: SnapiEnv,
    reg_id: u32,
    guest_env: u32,
    wasm_getter_fn_ptr: u32,
    wasm_setter_fn_ptr: u32,
    data_val: u64,
) {
    bridge_lock!();
    let Some(state) = state_mut(env) else {
        return;
    };
    state.cb_registry.insert(
        reg_id,
        CbRegistration {
            guest_env,
            wasm_fn_ptr: wasm_getter_fn_ptr,
            wasm_setter_fn_ptr,
            data_val,
        },
    );
}

pub unsafe fn snapi_bridge_snapshot_value_bytes(
    env: SnapiEnv,
    id: u32,
    data_out: *mut u64,
    byte_length_out: *mut u32,
) -> i32 {
    if data_out.is_null() || byte_length_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let bytes = try_status!(snapshot_value_bytes_local(scope, value));
        let len = bytes.len();
        let ptr = if len == 0 {
            ptr::null_mut()
        } else {
            let ptr = unsafe { libc::malloc(len) as *mut u8 };
            if ptr.is_null() {
                return NAPI_GENERIC_FAILURE;
            }
            unsafe {
                ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, len);
            }
            ptr
        };
        let status = write_out(data_out, ptr as u64);
        if status != NAPI_OK {
            if !ptr.is_null() {
                unsafe {
                    libc::free(ptr as *mut c_void);
                }
            }
            return status;
        }
        write_out(byte_length_out, len as u32)
    })
}

pub unsafe fn snapi_bridge_overwrite_value_bytes(
    env: SnapiEnv,
    id: u32,
    data: *const c_void,
    byte_length: u32,
) -> i32 {
    if data.is_null() && byte_length != 0 {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let bytes = if byte_length == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(data as *const u8, byte_length as usize) }
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        match overwrite_value_bytes_local(scope, value, bytes) {
            Ok(()) => NAPI_OK,
            Err(status) => status,
        }
    })
}

pub unsafe fn snapi_bridge_unofficial_get_promise_details(
    env: SnapiEnv,
    promise_id: u32,
    state_out: *mut i32,
    result_out: *mut u32,
    has_result_out: *mut i32,
) -> i32 {
    if state_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, promise_id));
        let promise: v8::Local<v8::Promise> = match value.try_into() {
            Ok(promise) => promise,
            Err(_) => return NAPI_INVALID_ARG,
        };
        let promise_state = match promise.state() {
            v8::PromiseState::Pending => 0,
            v8::PromiseState::Fulfilled => 1,
            v8::PromiseState::Rejected => 2,
        };
        let status = write_out(state_out, promise_state);
        if status != NAPI_OK {
            return status;
        }
        let has_result = promise_state != 0;
        if !has_result_out.is_null() {
            let status = write_out(has_result_out, has_result as i32);
            if status != NAPI_OK {
                return status;
            }
        }
        if !result_out.is_null() {
            let result_id = if has_result {
                let promise_result = promise.result(scope);
                store_global_value(state, scope, promise_result)
            } else {
                0
            };
            let status = write_out(result_out, result_id);
            if status != NAPI_OK {
                return status;
            }
        }
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_unofficial_preview_entries(
    env: SnapiEnv,
    value_id: u32,
    entries_out: *mut u32,
    is_key_value_out: *mut i32,
) -> i32 {
    if entries_out.is_null() || is_key_value_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, value_id));
        let result = match call_helper(state, scope, "previewEntries", &[value]) {
            Ok(value) => value,
            Err(status) => return status,
        };
        let array = try_status!(value_as_array(result));
        let entries = array
            .get_index(scope, 0)
            .unwrap_or_else(|| v8::Array::new(scope, 0).into());
        let is_key_value = array
            .get_index(scope, 1)
            .unwrap_or_else(|| v8::Boolean::new(scope, false).into())
            .boolean_value(scope) as i32;
        let entries_id = store_global_value(state, scope, entries);
        let status = write_out(entries_out, entries_id);
        if status != NAPI_OK {
            return status;
        }
        write_out(is_key_value_out, is_key_value)
    })
}

pub unsafe fn snapi_bridge_unofficial_arraybuffer_view_has_buffer(
    env: SnapiEnv,
    value_id: u32,
    result_out: *mut i32,
) -> i32 {
    if result_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, value_id));
        let view: v8::Local<v8::ArrayBufferView> = match value.try_into() {
            Ok(view) => view,
            Err(_) => return NAPI_INVALID_ARG,
        };
        let has_buffer = view.buffer(scope).is_some() as i32;
        write_out(result_out, has_buffer)
    })
}

pub unsafe fn snapi_bridge_unofficial_get_constructor_name(
    env: SnapiEnv,
    value_id: u32,
    name_out: *mut u32,
) -> i32 {
    if name_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, value_id));
        if !value.is_object() {
            return NAPI_INVALID_ARG;
        }
        let name = match call_helper(state, scope, "ctorName", &[value]) {
            Ok(value) => value,
            Err(status) => return status,
        };
        let id = store_global_value(state, scope, name);
        write_out(name_out, id)
    })
}

pub unsafe fn snapi_bridge_create_error(
    env: SnapiEnv,
    code_id: u32,
    msg_id: u32,
    out_id: *mut u32,
) -> i32 {
    if out_id.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let id = try_status!(create_error_common(
            state,
            scope,
            code_id,
            msg_id,
            0,
        ));
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_type_error(
    env: SnapiEnv,
    code_id: u32,
    msg_id: u32,
    out_id: *mut u32,
) -> i32 {
    if out_id.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let id = try_status!(create_error_common(
            state,
            scope,
            code_id,
            msg_id,
            1,
        ));
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_range_error(
    env: SnapiEnv,
    code_id: u32,
    msg_id: u32,
    out_id: *mut u32,
) -> i32 {
    if out_id.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let id = try_status!(create_error_common(
            state,
            scope,
            code_id,
            msg_id,
            2,
        ));
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_is_error(env: SnapiEnv, id: u32, result: *mut i32) -> i32 {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        write_out(result, value.is_native_error() as i32)
    })
}

pub unsafe fn snapi_bridge_is_arraybuffer(env: SnapiEnv, id: u32, result: *mut i32) -> i32 {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        write_out(result, value.is_array_buffer() as i32)
    })
}

pub unsafe fn snapi_bridge_is_dataview(env: SnapiEnv, id: u32, result: *mut i32) -> i32 {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        write_out(result, value.is_data_view() as i32)
    })
}

pub unsafe fn snapi_bridge_is_date(env: SnapiEnv, id: u32, result: *mut i32) -> i32 {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        write_out(result, value.is_date() as i32)
    })
}

pub unsafe fn snapi_bridge_is_promise(env: SnapiEnv, id: u32, result: *mut i32) -> i32 {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        write_out(result, value.is_promise() as i32)
    })
}

pub unsafe fn snapi_bridge_instanceof(
    env: SnapiEnv,
    obj_id: u32,
    ctor_id: u32,
    result: *mut i32,
) -> i32 {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let obj = try_status!(object_value_data(state, scope, obj_id));
        let ctor = try_status!(object_value_data(state, scope, ctor_id));
        let value = match call_helper(state, scope, "instanceofFn", &[obj, ctor]) {
            Ok(value) => value,
            Err(status) => return status,
        };
        write_out(result, value.boolean_value(scope) as i32)
    })
}

pub unsafe fn snapi_bridge_object_seal(env: SnapiEnv, obj_id: u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, obj_id));
        let obj = try_status!(value_as_object(value));
        match call_helper(state, scope, "seal", &[obj.into()]) {
            Ok(_) => NAPI_OK,
            Err(status) => status,
        }
    })
}

pub unsafe fn snapi_bridge_throw(env: SnapiEnv, error_id: u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let error = try_status!(object_value_data(state, scope, error_id));
        set_pending_exception(state, scope, error);
        scope.throw_exception(error);
        NAPI_PENDING_EXCEPTION
    })
}

pub unsafe fn snapi_bridge_throw_error(env: SnapiEnv, code: *const i8, msg: *const i8) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let msg = cstr_string(msg, u32::MAX);
        let message =
            v8::String::new(scope, if msg.is_empty() { "N-API error" } else { &msg }).ok_or(
                NAPI_GENERIC_FAILURE,
            );
        let message = try_status!(message);
        let error = v8::Exception::error(scope, message);
        if !code.is_null() {
            let code_key = v8::String::new(scope, "code").ok_or(NAPI_GENERIC_FAILURE);
            let code_key = try_status!(code_key);
            let code_val =
                v8::String::new(scope, &cstr_string(code, u32::MAX)).ok_or(NAPI_GENERIC_FAILURE);
            let code_val = try_status!(code_val);
            let err_obj = try_status!(value_as_object(error));
            if !err_obj
                .set(scope, code_key.into(), code_val.into())
                .unwrap_or(false)
            {
                return NAPI_GENERIC_FAILURE;
            }
        }
        set_pending_exception(state, scope, error);
        scope.throw_exception(error);
        NAPI_PENDING_EXCEPTION
    })
}

pub unsafe fn snapi_bridge_throw_type_error(
    env: SnapiEnv,
    code: *const i8,
    msg: *const i8,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let msg = cstr_string(msg, u32::MAX);
        let message =
            v8::String::new(scope, if msg.is_empty() { "Type error" } else { &msg }).ok_or(
                NAPI_GENERIC_FAILURE,
            );
        let message = try_status!(message);
        let error = v8::Exception::type_error(scope, message);
        if !code.is_null() {
            let code_key = v8::String::new(scope, "code").ok_or(NAPI_GENERIC_FAILURE);
            let code_key = try_status!(code_key);
            let code_val =
                v8::String::new(scope, &cstr_string(code, u32::MAX)).ok_or(NAPI_GENERIC_FAILURE);
            let code_val = try_status!(code_val);
            let err_obj = try_status!(value_as_object(error));
            if !err_obj
                .set(scope, code_key.into(), code_val.into())
                .unwrap_or(false)
            {
                return NAPI_GENERIC_FAILURE;
            }
        }
        set_pending_exception(state, scope, error);
        scope.throw_exception(error);
        NAPI_PENDING_EXCEPTION
    })
}

pub unsafe fn snapi_bridge_throw_range_error(
    env: SnapiEnv,
    code: *const i8,
    msg: *const i8,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let msg = cstr_string(msg, u32::MAX);
        let message =
            v8::String::new(scope, if msg.is_empty() { "Range error" } else { &msg }).ok_or(
                NAPI_GENERIC_FAILURE,
            );
        let message = try_status!(message);
        let error = v8::Exception::range_error(scope, message);
        if !code.is_null() {
            let code_key = v8::String::new(scope, "code").ok_or(NAPI_GENERIC_FAILURE);
            let code_key = try_status!(code_key);
            let code_val =
                v8::String::new(scope, &cstr_string(code, u32::MAX)).ok_or(NAPI_GENERIC_FAILURE);
            let code_val = try_status!(code_val);
            let err_obj = try_status!(value_as_object(error));
            if !err_obj
                .set(scope, code_key.into(), code_val.into())
                .unwrap_or(false)
            {
                return NAPI_GENERIC_FAILURE;
            }
        }
        set_pending_exception(state, scope, error);
        scope.throw_exception(error);
        NAPI_PENDING_EXCEPTION
    })
}

pub unsafe fn snapi_bridge_create_promise(
    env: SnapiEnv,
    deferred_out: *mut u32,
    out_id: *mut u32,
) -> i32 {
    if deferred_out.is_null() || out_id.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let resolver = try_status!(v8::PromiseResolver::new(scope).ok_or(NAPI_GENERIC_FAILURE));
        let promise = resolver.get_promise(scope);
        let deferred_id = next_id(&mut state.next_deferred_id);
        state.deferreds.insert(
            deferred_id,
            DeferredEntry {
                resolver: v8::Global::new(scope, resolver),
            },
        );
        let promise_id = store_global_value(state, scope, promise.into());
        let status = write_out(deferred_out, deferred_id);
        if status != NAPI_OK {
            return status;
        }
        write_out(out_id, promise_id)
    })
}

pub unsafe fn snapi_bridge_resolve_deferred(env: SnapiEnv, deferred_id: u32, value_id: u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let Some(entry) = state.deferreds.remove(&deferred_id) else {
            return NAPI_INVALID_ARG;
        };
        let value = try_status!(object_value_data(state, scope, value_id));
        let resolver = v8::Local::new(scope, &entry.resolver);
        let mut tc = v8::TryCatch::new(scope);
        match resolver.resolve(&mut tc, value) {
            Some(_) => NAPI_OK,
            None => {
                if let Some(exc) = tc.exception() {
                    set_pending_exception(state, &mut tc, exc);
                    return NAPI_PENDING_EXCEPTION;
                }
                NAPI_GENERIC_FAILURE
            }
        }
    })
}

pub unsafe fn snapi_bridge_reject_deferred(env: SnapiEnv, deferred_id: u32, value_id: u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let Some(entry) = state.deferreds.remove(&deferred_id) else {
            return NAPI_INVALID_ARG;
        };
        let value = try_status!(object_value_data(state, scope, value_id));
        let resolver = v8::Local::new(scope, &entry.resolver);
        let mut tc = v8::TryCatch::new(scope);
        match resolver.reject(&mut tc, value) {
            Some(_) => NAPI_OK,
            None => {
                if let Some(exc) = tc.exception() {
                    set_pending_exception(state, &mut tc, exc);
                    return NAPI_PENDING_EXCEPTION;
                }
                NAPI_GENERIC_FAILURE
            }
        }
    })
}

pub unsafe fn snapi_bridge_create_arraybuffer(
    env: SnapiEnv,
    byte_length: u32,
    out_id: *mut u32,
) -> i32 {
    if out_id.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let ab = v8::ArrayBuffer::new(scope, byte_length as usize);
        let id = store_global_value(state, scope, ab.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_external_buffer(
    env: SnapiEnv,
    data_addr: u64,
    byte_length: u32,
    backing_store_token_out: *mut u64,
    out_id: *mut u32,
) -> i32 {
    if out_id.is_null() {
        return NAPI_INVALID_ARG;
    }
    if data_addr == 0 && byte_length != 0 {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let ab = if data_addr == 0 && byte_length == 0 {
            v8::ArrayBuffer::new(scope, 0)
        } else {
            let backing = unsafe {
                unique_ref_from_raw(v8__ArrayBuffer__NewBackingStore__with_data(
                    data_addr as *mut c_void,
                    byte_length as usize,
                    noop_backing_store_deleter,
                    ptr::null_mut(),
                ))
            };
            let backing = try_status!(backing.ok_or(NAPI_GENERIC_FAILURE));
            let shared = v8::SharedRef::from(backing);
            v8::ArrayBuffer::with_backing_store(scope, &shared)
        };
        let token = backing_store_token(ab.get_backing_store().data());
        let buffer = match node_buffer_from_arraybuffer(
            state,
            scope,
            ab.into(),
            0,
            byte_length as usize,
        ) {
            Ok(value) => value,
            Err(status) => return status,
        };
        let id = store_global_value(state, scope, buffer);
        if !backing_store_token_out.is_null() {
            let status = write_out(backing_store_token_out, token);
            if status != NAPI_OK {
                return status;
            }
        }
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_get_arraybuffer_info(
    env: SnapiEnv,
    id: u32,
    data_out: *mut u64,
    byte_length: *mut u32,
    backing_store_token_out: *mut u64,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let buffer: v8::Local<v8::ArrayBuffer> = match value.try_into() {
            Ok(buffer) => buffer,
            Err(_) => return NAPI_INVALID_ARG,
        };
        let backing = buffer.get_backing_store();
        if !data_out.is_null() {
            let status = write_out(data_out, backing.data() as u64);
            if status != NAPI_OK {
                return status;
            }
        }
        if !byte_length.is_null() {
            let status = write_out(byte_length, buffer.byte_length() as u32);
            if status != NAPI_OK {
                return status;
            }
        }
        if !backing_store_token_out.is_null() {
            let status = write_out(backing_store_token_out, backing_store_token(backing.data()));
            if status != NAPI_OK {
                return status;
            }
        }
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_detach_arraybuffer(env: SnapiEnv, id: u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let buffer: v8::Local<v8::ArrayBuffer> = match value.try_into() {
            Ok(buffer) => buffer,
            Err(_) => return NAPI_INVALID_ARG,
        };
        buffer.detach();
        state.detached_arraybuffers.push(v8::Global::new(scope, value));
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_is_detached_arraybuffer(
    env: SnapiEnv,
    id: u32,
    result: *mut i32,
) -> i32 {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        if !value.is_array_buffer() {
            return NAPI_INVALID_ARG;
        }
        let detached = state
            .detached_arraybuffers
            .iter()
            .any(|entry| v8::Local::new(scope, entry).strict_equals(value));
        write_out(result, detached as i32)
    })
}

pub unsafe fn snapi_bridge_is_sharedarraybuffer(
    env: SnapiEnv,
    id: u32,
    result: *mut i32,
) -> i32 {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        write_out(result, value.is_shared_array_buffer() as i32)
    })
}

pub unsafe fn snapi_bridge_create_sharedarraybuffer(
    env: SnapiEnv,
    byte_length: u32,
    data_out: *mut u64,
    out_id: *mut u32,
) -> i32 {
    if out_id.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let sab = try_status!(v8::SharedArrayBuffer::new(scope, byte_length as usize)
            .ok_or(NAPI_GENERIC_FAILURE));
        let backing = sab.get_backing_store();
        if !data_out.is_null() {
            let status = write_out(data_out, backing.data() as u64);
            if status != NAPI_OK {
                return status;
            }
        }
        let id = store_global_value(state, scope, sab.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_dataview(
    env: SnapiEnv,
    byte_length: u32,
    arraybuffer_id: u32,
    byte_offset: u32,
    out_id: *mut u32,
) -> i32 {
    if out_id.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, arraybuffer_id));
        let buffer: v8::Local<v8::ArrayBuffer> = match value.try_into() {
            Ok(buffer) => buffer,
            Err(_) => return NAPI_INVALID_ARG,
        };
        let global = scope.get_current_context().global(scope);
        let ctor_key = v8::String::new(scope, "DataView").ok_or(NAPI_GENERIC_FAILURE);
        let ctor_key = try_status!(ctor_key);
        let ctor_value = global.get(scope, ctor_key.into()).ok_or(NAPI_GENERIC_FAILURE);
        let ctor = try_status!(value_as_function(try_status!(ctor_value)));
        let args = [
            buffer.into(),
            v8::Integer::new_from_unsigned(scope, byte_offset).into(),
            v8::Integer::new_from_unsigned(scope, byte_length).into(),
        ];
        let view = match ctor.new_instance(scope, &args) {
            Some(view) => view,
            None => return NAPI_GENERIC_FAILURE,
        };
        let id = store_global_value(state, scope, view.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_get_dataview_info(
    env: SnapiEnv,
    id: u32,
    byte_length_out: *mut u32,
    data_out: *mut u64,
    arraybuffer_out: *mut u32,
    byte_offset_out: *mut u32,
    backing_store_token_out: *mut u64,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let view: v8::Local<v8::DataView> = match value.try_into() {
            Ok(view) => view,
            Err(_) => return NAPI_INVALID_ARG,
        };
        let buffer = match view.buffer(scope) {
            Some(buffer) => buffer,
            None => return NAPI_GENERIC_FAILURE,
        };
        let backing = buffer.get_backing_store();
        if !byte_length_out.is_null() {
            let status = write_out(byte_length_out, view.byte_length() as u32);
            if status != NAPI_OK {
                return status;
            }
        }
        if !data_out.is_null() {
            let data = if backing.data().is_null() {
                0
            } else {
                unsafe { (backing.data() as *mut u8).add(view.byte_offset()) as u64 }
            };
            let status = write_out(data_out, data);
            if status != NAPI_OK {
                return status;
            }
        }
        if !arraybuffer_out.is_null() {
            let arraybuffer_id = store_global_value(state, scope, buffer.into());
            let status = write_out(arraybuffer_out, arraybuffer_id);
            if status != NAPI_OK {
                return status;
            }
        }
        if !byte_offset_out.is_null() {
            let status = write_out(byte_offset_out, view.byte_offset() as u32);
            if status != NAPI_OK {
                return status;
            }
        }
        if !backing_store_token_out.is_null() {
            let status = write_out(backing_store_token_out, backing_store_token(backing.data()));
            if status != NAPI_OK {
                return status;
            }
        }
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_open_escapable_handle_scope(
    env: SnapiEnv,
    scope_out: *mut u32,
) -> i32 {
    if scope_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    bridge_lock!();
    let id = next_id(&mut state.next_esc_scope_id);
    state.esc_scopes.insert(id, EscapableScopeState { escaped: false });
    write_out(scope_out, id)
}

pub unsafe fn snapi_bridge_close_escapable_handle_scope(env: SnapiEnv, scope_id: u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    bridge_lock!();
    if state.esc_scopes.remove(&scope_id).is_some() {
        NAPI_OK
    } else {
        NAPI_INVALID_ARG
    }
}

pub unsafe fn snapi_bridge_escape_handle(
    env: SnapiEnv,
    scope_id: u32,
    escapee_id: u32,
    out_id: *mut u32,
) -> i32 {
    if out_id.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    bridge_lock!();
    let Some(scope_state) = state.esc_scopes.get_mut(&scope_id) else {
        return NAPI_INVALID_ARG;
    };
    if scope_state.escaped {
        return NAPI_ESCAPE_CALLED_TWICE;
    }
    if !state.values.contains_key(&escapee_id) {
        return NAPI_INVALID_ARG;
    }
    scope_state.escaped = true;
    write_out(out_id, escapee_id)
}

pub unsafe fn snapi_bridge_type_tag_object(
    env: SnapiEnv,
    obj_id: u32,
    tag_lower: u64,
    tag_upper: u64,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, obj_id));
        if !value.is_object() && !value.is_external() {
            return NAPI_INVALID_ARG;
        }
        for entry in &mut state.type_tags {
            if v8::Local::new(scope, &entry.value).strict_equals(value) {
                entry.lower = tag_lower;
                entry.upper = tag_upper;
                return NAPI_OK;
            }
        }
        state.type_tags.push(TypeTagEntry {
            value: v8::Global::new(scope, value),
            lower: tag_lower,
            upper: tag_upper,
        });
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_check_object_type_tag(
    env: SnapiEnv,
    obj_id: u32,
    tag_lower: u64,
    tag_upper: u64,
    result: *mut i32,
) -> i32 {
    if result.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, obj_id));
        if !value.is_object() && !value.is_external() {
            return write_out(result, 0);
        }
        let matches = state.type_tags.iter().any(|entry| {
            let tagged = v8::Local::new(scope, &entry.value);
            tagged.strict_equals(value) && entry.lower == tag_lower && entry.upper == tag_upper
        });
        write_out(result, matches as i32)
    })
}

pub unsafe fn snapi_bridge_create_bigint_words(
    env: SnapiEnv,
    sign_bit: i32,
    word_count: u32,
    words: *const u64,
    out_id: *mut u32,
) -> i32 {
    if out_id.is_null() || (words.is_null() && word_count != 0) {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let words = if word_count == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(words, word_count as usize) }
    };
    with_scope(state, |scope, state| {
        let bigint = try_status!(v8::BigInt::new_from_words(scope, sign_bit != 0, words)
            .ok_or(NAPI_GENERIC_FAILURE));
        let id = store_global_value(state, scope, bigint.into());
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_get_value_bigint_words(
    env: SnapiEnv,
    id: u32,
    sign_bit: *mut i32,
    word_count: *mut usize,
    words: *mut u64,
) -> i32 {
    if sign_bit.is_null() || word_count.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, id));
        let bigint = match value.to_big_int(scope) {
            Some(bigint) => bigint,
            None => return NAPI_BIGINT_EXPECTED,
        };
        let available = bigint.word_count();
        let requested = unsafe { *word_count };
        let (negative, _) = bigint.to_words_array(&mut []);
        if words.is_null() || requested == 0 {
            let status = write_out(sign_bit, negative as i32);
            if status != NAPI_OK {
                return status;
            }
            return write_out(word_count, available);
        }
        let words_slice = unsafe { std::slice::from_raw_parts_mut(words, requested.min(available)) };
        let (negative, written) = bigint.to_words_array(words_slice);
        let status = write_out(sign_bit, negative as i32);
        if status != NAPI_OK {
            return status;
        }
        write_out(word_count, written.len())
    })
}

pub unsafe fn snapi_bridge_set_instance_data(env: SnapiEnv, data_val: u64) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    bridge_lock!();
    state.instance_data = data_val;
    NAPI_OK
}

pub unsafe fn snapi_bridge_get_instance_data(env: SnapiEnv, data_out: *mut u64) -> i32 {
    if data_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    bridge_lock!();
    write_out(data_out, state.instance_data)
}

pub unsafe fn snapi_bridge_adjust_external_memory(
    env: SnapiEnv,
    change: i64,
    adjusted: *mut i64,
) -> i32 {
    if adjusted.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    bridge_lock!();
    state.adjusted_external_memory = state.adjusted_external_memory.saturating_add(change);
    write_out(adjusted, state.adjusted_external_memory)
}

pub unsafe fn snapi_bridge_create_buffer(
    env: SnapiEnv,
    length: u32,
    data_out: *mut u64,
    out_id: *mut u32,
) -> i32 {
    if out_id.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let ab = v8::ArrayBuffer::new(scope, length as usize);
        let backing = ab.get_backing_store();
        if !data_out.is_null() {
            let status = write_out(data_out, backing.data() as u64);
            if status != NAPI_OK {
                return status;
            }
        }
        let buffer = match node_buffer_from_arraybuffer(state, scope, ab.into(), 0, length as usize) {
            Ok(value) => value,
            Err(status) => return status,
        };
        let id = store_global_value(state, scope, buffer);
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_create_buffer_copy(
    env: SnapiEnv,
    length: u32,
    src_data: *const u8,
    result_data_out: *mut u64,
    out_id: *mut u32,
) -> i32 {
    if out_id.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let bytes = if src_data.is_null() || length == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(src_data, length as usize) }
    };
    with_scope(state, |scope, state| {
        let ab = v8::ArrayBuffer::new(scope, length as usize);
        let backing = ab.get_backing_store();
        if !bytes.is_empty() {
            unsafe {
                ptr::copy_nonoverlapping(bytes.as_ptr(), backing.data() as *mut u8, bytes.len());
            }
        }
        if !result_data_out.is_null() {
            let status = write_out(result_data_out, backing.data() as u64);
            if status != NAPI_OK {
                return status;
            }
        }
        let buffer = match node_buffer_from_arraybuffer(state, scope, ab.into(), 0, length as usize) {
            Ok(value) => value,
            Err(status) => return status,
        };
        let id = store_global_value(state, scope, buffer);
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_get_node_version(
    env: SnapiEnv,
    major: *mut u32,
    minor: *mut u32,
    patch: *mut u32,
) -> i32 {
    let Some(_state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    if !major.is_null() {
        let status = write_out(major, 22);
        if status != NAPI_OK {
            return status;
        }
    }
    if !minor.is_null() {
        let status = write_out(minor, 0);
        if status != NAPI_OK {
            return status;
        }
    }
    if !patch.is_null() {
        let status = write_out(patch, 0);
        if status != NAPI_OK {
            return status;
        }
    }
    NAPI_OK
}

pub unsafe fn snapi_bridge_add_finalizer(
    env: SnapiEnv,
    obj_id: u32,
    _data_val: u64,
    ref_out: *mut u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, obj_id));
        let obj = try_status!(value_as_object(value));
        if ref_out.is_null() {
            return NAPI_OK;
        }
        let ref_id = create_ref(state, scope, obj.into(), 0);
        write_out(ref_out, ref_id)
    })
}

pub unsafe fn snapi_bridge_unofficial_get_call_sites(
    env: SnapiEnv,
    frames: u32,
    callsites_out: *mut u32,
) -> i32 {
    if callsites_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let callsites = try_status!(build_call_site_array(state, scope, frames, 1));
        let id = store_global_value(state, scope, callsites.into());
        write_out(callsites_out, id)
    })
}

pub unsafe fn snapi_bridge_unofficial_get_current_stack_trace(
    env: SnapiEnv,
    frames: u32,
    callsites_out: *mut u32,
) -> i32 {
    if callsites_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let callsites = try_status!(build_call_site_array(state, scope, frames, 0));
        let id = store_global_value(state, scope, callsites.into());
        write_out(callsites_out, id)
    })
}

pub unsafe fn snapi_bridge_unofficial_get_caller_location(
    env: SnapiEnv,
    location_out: *mut u32,
) -> i32 {
    if location_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let location = try_status!(call_helper(state, scope, "getCallerLocation", &[]));
        if location.is_undefined() {
            return write_out(location_out, 0);
        }
        let id = store_global_value(state, scope, location);
        write_out(location_out, id)
    })
}

pub unsafe fn snapi_bridge_unofficial_structured_clone(
    env: SnapiEnv,
    value_id: u32,
    out_id: *mut u32,
) -> i32 {
    if out_id.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, value_id));
        let cloned = try_status!(structured_clone_local(scope, value, None));
        let id = store_global_value(state, scope, cloned);
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_unofficial_structured_clone_with_transfer(
    env: SnapiEnv,
    value_id: u32,
    transfer_list_id: u32,
    out_id: *mut u32,
) -> i32 {
    if out_id.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, value_id));
        let transfer_list = try_status!(object_value_data(state, scope, transfer_list_id));
        let cloned = try_status!(structured_clone_local(scope, value, Some(transfer_list)));
        let id = store_global_value(state, scope, cloned);
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_unofficial_enqueue_microtask(env: SnapiEnv, callback_id: u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let callback = try_status!(value_as_function(try_status!(object_value_data(
            state,
            scope,
            callback_id
        ))));
        let isolate = unsafe { &mut *v8__Context__GetIsolate(&*scope.get_current_context()) };
        isolate.enqueue_microtask(callback);
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_unofficial_get_process_memory_info(
    env: SnapiEnv,
    heap_total_out: *mut f64,
    heap_used_out: *mut f64,
    external_out: *mut f64,
    array_buffers_out: *mut f64,
) -> i32 {
    if heap_total_out.is_null()
        || heap_used_out.is_null()
        || external_out.is_null()
        || array_buffers_out.is_null()
    {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let Some(runtime) = state.runtime.as_mut() else {
        return NAPI_INVALID_ARG;
    };
    let mut stats = v8::HeapStatistics::default();
    runtime.isolate.get_heap_statistics(&mut stats);
    let status = write_out(heap_total_out, stats.total_heap_size() as f64);
    if status != NAPI_OK {
        return status;
    }
    let status = write_out(heap_used_out, stats.used_heap_size() as f64);
    if status != NAPI_OK {
        return status;
    }
    let status = write_out(external_out, stats.external_memory() as f64);
    if status != NAPI_OK {
        return status;
    }
    write_out(array_buffers_out, 0f64)
}

pub unsafe fn snapi_bridge_unofficial_get_hash_seed(
    env: SnapiEnv,
    hash_seed_out: *mut u64,
) -> i32 {
    if hash_seed_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    write_out(hash_seed_out, state.hash_seed)
}

pub unsafe fn snapi_bridge_unofficial_get_error_source_positions(
    env: SnapiEnv,
    error_id: u32,
    source_line_out: *mut u32,
    script_resource_name_out: *mut u32,
    line_number_out: *mut i32,
    start_column_out: *mut i32,
    end_column_out: *mut i32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let error = try_status!(object_value_data(state, scope, error_id));
        let message = v8::Exception::create_message(scope, error);
        if !source_line_out.is_null() {
            let source_line = message
                .get_source_line(scope)
                .map(|line| store_global_value(state, scope, line.into()))
                .unwrap_or(0);
            let status = write_out(source_line_out, source_line);
            if status != NAPI_OK {
                return status;
            }
        }
        if !script_resource_name_out.is_null() {
            let resource_name = message
                .get_script_resource_name(scope)
                .map(|name| store_global_value(state, scope, name))
                .unwrap_or(0);
            let status = write_out(script_resource_name_out, resource_name);
            if status != NAPI_OK {
                return status;
            }
        }
        if !line_number_out.is_null() {
            let status = write_out(line_number_out, message.get_line_number(scope).unwrap_or(0) as i32);
            if status != NAPI_OK {
                return status;
            }
        }
        if !start_column_out.is_null() {
            let status = write_out(start_column_out, message.get_start_column() as i32);
            if status != NAPI_OK {
                return status;
            }
        }
        if !end_column_out.is_null() {
            let status = write_out(end_column_out, message.get_end_column() as i32);
            if status != NAPI_OK {
                return status;
            }
        }
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_unofficial_get_heap_statistics(
    env: SnapiEnv,
    stats_out: *mut SnapiUnofficialHeapStatistics,
) -> i32 {
    if stats_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let Some(runtime) = state.runtime.as_mut() else {
        return NAPI_INVALID_ARG;
    };
    let mut stats = v8::HeapStatistics::default();
    runtime.isolate.get_heap_statistics(&mut stats);
    unsafe {
        stats_out.write(SnapiUnofficialHeapStatistics {
            total_heap_size: stats.total_heap_size() as u64,
            total_heap_size_executable: stats.total_heap_size_executable() as u64,
            total_physical_size: stats.total_physical_size() as u64,
            total_available_size: stats.total_available_size() as u64,
            used_heap_size: stats.used_heap_size() as u64,
            heap_size_limit: stats.heap_size_limit() as u64,
            does_zap_garbage: stats.does_zap_garbage() as u64,
            malloced_memory: stats.malloced_memory() as u64,
            peak_malloced_memory: stats.peak_malloced_memory() as u64,
            number_of_native_contexts: stats.number_of_native_contexts() as u64,
            number_of_detached_contexts: stats.number_of_detached_contexts() as u64,
            total_global_handles_size: stats.total_global_handles_size() as u64,
            used_global_handles_size: stats.used_global_handles_size() as u64,
            external_memory: stats.external_memory() as u64,
        });
    }
    NAPI_OK
}

pub unsafe fn snapi_bridge_unofficial_take_heap_snapshot(
    env: SnapiEnv,
    _expose_internals: i32,
    _expose_numeric_values: i32,
    json_out: *mut u64,
    json_len_out: *mut u32,
) -> i32 {
    if json_out.is_null() || json_len_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let Some(runtime) = state.runtime.as_mut() else {
        return NAPI_INVALID_ARG;
    };
    let mut bytes = Vec::new();
    runtime.isolate.take_heap_snapshot(|chunk| {
        bytes.extend_from_slice(chunk);
        true
    });
    let len = bytes.len();
    let ptr = if len == 0 {
        0u64
    } else {
        let raw = unsafe { libc::malloc(len) as *mut u8 };
        if raw.is_null() {
            return NAPI_GENERIC_FAILURE;
        }
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), raw, len);
        }
        raw as u64
    };
    let status = write_out(json_out, ptr);
    if status != NAPI_OK {
        if ptr != 0 {
            unsafe {
                free(ptr as *mut c_void);
            }
        }
        return status;
    }
    write_out(json_len_out, len as u32)
}

fn malloc_bytes(bytes: &[u8], ptr_out: *mut u64, len_out: *mut u32) -> i32 {
    let len = bytes.len();
    let ptr = if len == 0 {
        0u64
    } else {
        let raw = unsafe { libc::malloc(len) as *mut u8 };
        if raw.is_null() {
            return NAPI_GENERIC_FAILURE;
        }
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), raw, len);
        }
        raw as u64
    };
    let status = write_out(ptr_out, ptr);
    if status != NAPI_OK {
        if ptr != 0 {
            unsafe {
                free(ptr as *mut c_void);
            }
        }
        return status;
    }
    write_out(len_out, len as u32)
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn snapi_bridge_unofficial_module_wrap_create_source_text(
    env: SnapiEnv,
    wrapper_id: u32,
    url_id: u32,
    context_id: u32,
    source_id: u32,
    line_offset: i32,
    column_offset: i32,
    cached_data_or_id: u32,
    handle_out: *mut u32,
) -> i32 {
    if handle_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let wrapper = try_status!(value_as_object(try_status!(object_value_data(
            state, scope, wrapper_id
        ))));
        let url_value = try_status!(object_value_data(state, scope, url_id));
        let url = try_status!(string_value(scope, url_value));
        let source_value = try_status!(object_value_data(state, scope, source_id));
        let source = try_status!(string_value(scope, source_value));
        let context = if try_status!(value_id_is_nullish(state, scope, context_id)) {
            scope.get_current_context()
        } else {
            match resolve_context_from_key(state, scope, context_id) {
                Some(context) => context,
                None => return NAPI_INVALID_ARG,
            }
        };

        let mut host_defined_option_id = 0u32;
        let mut cached_data_bytes = Vec::new();
        if !try_status!(value_id_is_nullish(state, scope, cached_data_or_id)) {
            let raw = try_status!(object_value_data(state, scope, cached_data_or_id));
            if raw.is_symbol() {
                host_defined_option_id = cached_data_or_id;
            } else if raw.is_array_buffer_view() {
                let view: v8::Local<v8::ArrayBufferView> =
                    raw.try_into().map_err(|_| NAPI_INVALID_ARG).unwrap();
                cached_data_bytes.resize(view.byte_length(), 0);
                view.copy_contents(&mut cached_data_bytes);
            } else {
                return NAPI_INVALID_ARG;
            }
        }
        if host_defined_option_id == 0 {
            let symbol = v8::Symbol::new(scope, Some(url));
            host_defined_option_id = store_global_value(state, scope, symbol.into());
        }

        let undefined = v8::undefined(scope);
        let origin = v8::ScriptOrigin::new(
            scope,
            url.into(),
            line_offset,
            column_offset,
            true,
            -1,
            undefined.into(),
            false,
            false,
            true,
        );
        let source = if cached_data_bytes.is_empty() {
            v8::script_compiler::Source::new(source, Some(&origin))
        } else {
            v8::script_compiler::Source::new_with_cached_data(
                source,
                Some(&origin),
                v8::CachedData::new(&cached_data_bytes),
            )
        };
        let compile_options = if cached_data_bytes.is_empty() {
            v8::script_compiler::CompileOptions::NoCompileOptions
        } else {
            v8::script_compiler::CompileOptions::ConsumeCodeCache
        };
        let module = {
            let mut context_scope = v8::ContextScope::new(scope, context);
            let mut try_catch = v8::TryCatch::new(&mut context_scope);
            let Some(module) = v8::script_compiler::compile_module2(
                &mut try_catch,
                source,
                compile_options,
                v8::script_compiler::NoCacheReason::NoReason,
            ) else {
                if let Some(exception) = try_catch.exception() {
                    set_pending_exception(state, &mut try_catch, exception);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            };
            local_for_scope(module)
        };
        let (module_requests, resolve_cache) = try_status!(populate_module_requests(scope, module));

        let host_value = try_status!(object_value_data(state, scope, host_defined_option_id));
        let host_key = private_key(scope, "node:host_defined_option_symbol");
        let _ = wrapper.set_private(scope, host_key, host_value);
        let global = context.global(scope);
        let _ = global.set_private(scope, host_key, host_value);

        let has_top_level_await = v8::Boolean::new(scope, false);
        if let Err(status) = set_named_property(
            scope,
            wrapper,
            "hasTopLevelAwait",
            has_top_level_await.into(),
        ) {
            return status;
        }
        if let Err(status) = set_named_property(scope, wrapper, "sourceURL", url.into()) {
            return status;
        }
        let undefined = v8::undefined(scope);
        if let Err(status) = set_named_property(scope, wrapper, "sourceMapURL", undefined.into()) {
            return status;
        }

        let handle_id = next_id(&mut state.next_module_wrap_handle_id);
        state.module_wrap_handles.insert(
            handle_id,
            ModuleWrapHandle {
                wrapper_id,
                synthetic_eval_steps_id: 0,
                source_object_id: 0,
                host_defined_option_id,
                context: v8::Global::new(scope, context),
                module: v8::Global::new(scope, module),
                module_requests,
                resolve_cache,
                linked_requests: Vec::new(),
                has_top_level_await: false,
                last_evaluation_promise: None,
            },
        );
        write_out(handle_out, handle_id)
    })
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_create_synthetic(
    env: SnapiEnv,
    wrapper_id: u32,
    url_id: u32,
    context_id: u32,
    export_names_id: u32,
    synthetic_eval_steps_id: u32,
    handle_out: *mut u32,
) -> i32 {
    if handle_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let _wrapper = try_status!(value_as_object(try_status!(object_value_data(
            state, scope, wrapper_id
        ))));
        let url_value = try_status!(object_value_data(state, scope, url_id));
        let url = try_status!(string_value(scope, url_value));
        let _callback = try_status!(value_as_function(try_status!(object_value_data(
            state,
            scope,
            synthetic_eval_steps_id
        ))));
        let context = if try_status!(value_id_is_nullish(state, scope, context_id)) {
            scope.get_current_context()
        } else {
            match resolve_context_from_key(state, scope, context_id) {
                Some(context) => context,
                None => return NAPI_INVALID_ARG,
            }
        };
        let export_names_array = try_status!(value_as_array(try_status!(object_value_data(
            state,
            scope,
            export_names_id
        ))));
        let mut export_names = Vec::with_capacity(export_names_array.length() as usize);
        for index in 0..export_names_array.length() {
            let Some(name) = export_names_array.get_index(scope, index) else {
                return NAPI_INVALID_ARG;
            };
            let Ok(name): Result<v8::Local<v8::String>, _> = name.try_into() else {
                return NAPI_INVALID_ARG;
            };
            export_names.push(name);
        }
        let module = {
            let mut context_scope = v8::ContextScope::new(scope, context);
            let module = v8::Module::create_synthetic_module(
                &mut context_scope,
                url,
                &export_names,
                module_wrap_synthetic_evaluation_steps,
            );
            local_for_scope(module)
        };
        let handle_id = next_id(&mut state.next_module_wrap_handle_id);
        state.module_wrap_handles.insert(
            handle_id,
            ModuleWrapHandle {
                wrapper_id,
                synthetic_eval_steps_id,
                source_object_id: 0,
                host_defined_option_id: 0,
                context: v8::Global::new(scope, context),
                module: v8::Global::new(scope, module),
                module_requests: Vec::new(),
                resolve_cache: HashMap::new(),
                linked_requests: Vec::new(),
                has_top_level_await: false,
                last_evaluation_promise: None,
            },
        );
        write_out(handle_out, handle_id)
    })
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_destroy(env: SnapiEnv, handle_id: u32) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    state.module_wrap_handles.remove(&handle_id);
    NAPI_OK
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_get_module_requests(
    env: SnapiEnv,
    handle_id: u32,
    result_out: *mut u32,
) -> i32 {
    if result_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let requests = match state.module_wrap_handles.get(&handle_id) {
            Some(handle) => &handle.module_requests,
            None => return NAPI_INVALID_ARG,
        };
        let result = v8::Array::new(scope, requests.len() as i32);
        for (index, request) in requests.iter().enumerate() {
            let attrs = v8::Object::new(scope);
            for attr in &request.attributes {
                let Some(key) = v8::String::new(scope, &attr.key) else {
                    return NAPI_GENERIC_FAILURE;
                };
                let Some(value) = v8::String::new(scope, &attr.value) else {
                    return NAPI_GENERIC_FAILURE;
                };
                if !attrs.set(scope, key.into(), value.into()).unwrap_or(false) {
                    return NAPI_GENERIC_FAILURE;
                }
            }
            let request_obj = v8::Object::new(scope);
            let Some(specifier_key) = v8::String::new(scope, "specifier") else {
                return NAPI_GENERIC_FAILURE;
            };
            let Some(attributes_key) = v8::String::new(scope, "attributes") else {
                return NAPI_GENERIC_FAILURE;
            };
            let Some(phase_key) = v8::String::new(scope, "phase") else {
                return NAPI_GENERIC_FAILURE;
            };
            let Some(specifier) = v8::String::new(scope, &request.specifier) else {
                return NAPI_GENERIC_FAILURE;
            };
            let phase_value = v8::Integer::new(scope, request.phase);
            if !request_obj
                .set(scope, specifier_key.into(), specifier.into())
                .unwrap_or(false)
                || !request_obj
                    .set(scope, attributes_key.into(), attrs.into())
                    .unwrap_or(false)
                || !request_obj
                    .set(
                        scope,
                        phase_key.into(),
                        phase_value.into(),
                    )
                    .unwrap_or(false)
                || !result
                    .set_index(scope, index as u32, request_obj.into())
                    .unwrap_or(false)
            {
                return NAPI_GENERIC_FAILURE;
            }
        }
        let id = store_global_value(state, scope, result.into());
        write_out(result_out, id)
    })
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_link(
    env: SnapiEnv,
    handle_id: u32,
    count: u32,
    linked_handle_ids: *const u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let linked_ids = if count == 0 {
        Vec::new()
    } else if linked_handle_ids.is_null() {
        return NAPI_INVALID_ARG;
    } else {
        unsafe { std::slice::from_raw_parts(linked_handle_ids, count as usize) }.to_vec()
    };
    with_scope(state, |scope, state| {
        let Some(handle) = state.module_wrap_handles.get(&handle_id) else {
            return NAPI_INVALID_ARG;
        };
        if count as usize != handle.module_requests.len() {
            throw_code_error_local(
                state,
                scope,
                "ERR_VM_MODULE_LINK_FAILURE",
                "linked modules array length mismatch",
            );
            return NAPI_PENDING_EXCEPTION;
        }
        let module_requests = handle.module_requests.clone();
        let resolve_cache = handle.resolve_cache.clone();
        for (index, request) in module_requests.iter().enumerate() {
            let Some(linked_handle_id) = linked_ids.get(index).copied() else {
                throw_code_error_local(
                    state,
                    scope,
                    "ERR_VM_MODULE_LINK_FAILURE",
                    "linked module missing",
                );
                return NAPI_PENDING_EXCEPTION;
            };
            if !state.module_wrap_handles.contains_key(&linked_handle_id) {
                throw_code_error_local(
                    state,
                    scope,
                    "ERR_VM_MODULE_LINK_FAILURE",
                    "linked module missing",
                );
                return NAPI_PENDING_EXCEPTION;
            }
            let key = module_request_key(&request.specifier, &request.attributes);
            if let Some(existing) = resolve_cache.get(&key).copied()
                && existing < index as u32
                && linked_ids[existing as usize] != linked_handle_id
            {
                throw_code_error_local(
                    state,
                    scope,
                    "ERR_MODULE_LINK_MISMATCH",
                    &format!(
                        "Module request '{}' must be linked to the same module",
                        request.specifier
                    ),
                );
                return NAPI_PENDING_EXCEPTION;
            }
        }
        if let Some(handle) = state.module_wrap_handles.get_mut(&handle_id) {
            handle.linked_requests = linked_ids;
        }
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_instantiate(
    env: SnapiEnv,
    handle_id: u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let (context, module) = {
            let Some(handle) = state.module_wrap_handles.get(&handle_id) else {
                return NAPI_INVALID_ARG;
            };
            (
                local_for_scope(v8::Local::new(scope, &handle.context)),
                local_for_scope(v8::Local::new(scope, &handle.module)),
            )
        };
        let mut context_scope = v8::ContextScope::new(scope, context);
        let mut try_catch = v8::TryCatch::new(&mut context_scope);
        match module.instantiate_module(&mut try_catch, module_wrap_resolve_callback) {
            Some(_) => NAPI_OK,
            None => {
                if let Some(exception) = try_catch.exception() {
                    set_pending_exception(state, &mut try_catch, exception);
                    NAPI_PENDING_EXCEPTION
                } else {
                    NAPI_GENERIC_FAILURE
                }
            }
        }
    })
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_evaluate(
    env: SnapiEnv,
    handle_id: u32,
    _timeout: i64,
    _break_on_sigint: i32,
    result_out: *mut u32,
) -> i32 {
    if result_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let (context, module) = {
            let Some(handle) = state.module_wrap_handles.get(&handle_id) else {
                return NAPI_INVALID_ARG;
            };
            (
                local_for_scope(v8::Local::new(scope, &handle.context)),
                local_for_scope(v8::Local::new(scope, &handle.module)),
            )
        };
        let (result, promise) = {
            let mut context_scope = v8::ContextScope::new(scope, context);
            let mut try_catch = v8::TryCatch::new(&mut context_scope);
            let Some(result) = module.evaluate(&mut try_catch) else {
                if let Some(exception) = try_catch.exception() {
                    set_pending_exception(state, &mut try_catch, exception);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            };
            let promise = v8::Local::<v8::Promise>::try_from(result)
                .ok()
                .map(local_for_scope);
            (local_for_scope(result), promise)
        };
        if let Some(handle) = state.module_wrap_handles.get_mut(&handle_id) {
            handle.last_evaluation_promise = promise.map(|promise| v8::Global::new(scope, promise));
        }
        let result_id = store_global_value(state, scope, result);
        write_out(result_out, result_id)
    })
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_evaluate_sync(
    env: SnapiEnv,
    handle_id: u32,
    _filename_id: u32,
    _parent_filename_id: u32,
    result_out: *mut u32,
) -> i32 {
    if result_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let (context, module) = {
            let Some(handle) = state.module_wrap_handles.get(&handle_id) else {
                return NAPI_INVALID_ARG;
            };
            (
                local_for_scope(v8::Local::new(scope, &handle.context)),
                local_for_scope(v8::Local::new(scope, &handle.module)),
            )
        };
        let promise = {
            let mut context_scope = v8::ContextScope::new(scope, context);
            let mut try_catch = v8::TryCatch::new(&mut context_scope);
            let Some(result) = module.evaluate(&mut try_catch) else {
                if let Some(exception) = try_catch.exception() {
                    set_pending_exception(state, &mut try_catch, exception);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            };
            let Ok(promise) = v8::Local::<v8::Promise>::try_from(result) else {
                return NAPI_GENERIC_FAILURE;
            };
            local_for_scope(promise)
        };
        if let Some(handle) = state.module_wrap_handles.get_mut(&handle_id) {
            handle.last_evaluation_promise = Some(v8::Global::new(scope, promise));
        }
        match promise.state() {
            v8::PromiseState::Rejected => {
                let exception = promise.result(scope);
                set_pending_exception(state, scope, exception);
                scope.throw_exception(exception);
                NAPI_PENDING_EXCEPTION
            }
            v8::PromiseState::Pending => {
                throw_code_error_local(
                    state,
                    scope,
                    "ERR_REQUIRE_ASYNC_MODULE",
                    "require() cannot be used on an ESM graph with top-level await",
                );
                NAPI_PENDING_EXCEPTION
            }
            v8::PromiseState::Fulfilled => {
                let namespace = local_for_scope(module.get_module_namespace());
                let id = store_global_value(state, scope, namespace);
                write_out(result_out, id)
            }
        }
    })
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_get_namespace(
    env: SnapiEnv,
    handle_id: u32,
    result_out: *mut u32,
) -> i32 {
    if result_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let module = {
            let Some(handle) = state.module_wrap_handles.get(&handle_id) else {
                return NAPI_INVALID_ARG;
            };
            local_for_scope(v8::Local::new(scope, &handle.module))
        };
        if module.get_status() == v8::ModuleStatus::Uninstantiated
            || module.get_status() == v8::ModuleStatus::Instantiating
        {
            throw_code_error_local(
                state,
                scope,
                "ERR_MODULE_NOT_INSTANTIATED",
                "Module is not instantiated",
            );
            return NAPI_PENDING_EXCEPTION;
        }
        let namespace = local_for_scope(module.get_module_namespace());
        let id = store_global_value(state, scope, namespace);
        write_out(result_out, id)
    })
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_get_status(
    env: SnapiEnv,
    handle_id: u32,
    status_out: *mut i32,
) -> i32 {
    if status_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let module = {
            let Some(handle) = state.module_wrap_handles.get(&handle_id) else {
                return NAPI_INVALID_ARG;
            };
            local_for_scope(v8::Local::new(scope, &handle.module))
        };
        write_out(status_out, module.get_status() as i32)
    })
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_get_error(
    env: SnapiEnv,
    handle_id: u32,
    result_out: *mut u32,
) -> i32 {
    if result_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let module = {
            let Some(handle) = state.module_wrap_handles.get(&handle_id) else {
                return NAPI_INVALID_ARG;
            };
            local_for_scope(v8::Local::new(scope, &handle.module))
        };
        let exception = local_for_scope(module.get_exception());
        let id = store_global_value(state, scope, exception);
        write_out(result_out, id)
    })
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_has_top_level_await(
    env: SnapiEnv,
    handle_id: u32,
    result_out: *mut i32,
) -> i32 {
    if result_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let Some(handle) = state.module_wrap_handles.get(&handle_id) else {
        return NAPI_INVALID_ARG;
    };
    write_out(result_out, handle.has_top_level_await as i32)
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_has_async_graph(
    env: SnapiEnv,
    handle_id: u32,
    result_out: *mut i32,
) -> i32 {
    if result_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let Some(handle) = state.module_wrap_handles.get(&handle_id) else {
            return NAPI_INVALID_ARG;
        };
        let async_graph = handle
            .last_evaluation_promise
            .as_ref()
            .map(|promise| v8::Local::new(scope, promise).state() != v8::PromiseState::Fulfilled)
            .unwrap_or(false);
        write_out(result_out, async_graph as i32)
    })
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_check_unsettled_top_level_await(
    env: SnapiEnv,
    module_wrap_id: u32,
    warnings: i32,
    settled_out: *mut i32,
) -> i32 {
    if settled_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let Some(handle) = state.module_wrap_handles.get(&module_wrap_id) else {
            return write_out(settled_out, 1);
        };
        let settled = handle
            .last_evaluation_promise
            .as_ref()
            .map(|promise| v8::Local::new(scope, promise).state() != v8::PromiseState::Pending)
            .unwrap_or(true);
        if !settled && warnings != 0 {
            eprintln!("Warning: Detected unsettled top-level await");
        }
        write_out(settled_out, settled as i32)
    })
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_set_export(
    env: SnapiEnv,
    handle_id: u32,
    export_name_id: u32,
    export_value_id: u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let module = {
            let Some(handle) = state.module_wrap_handles.get(&handle_id) else {
                return NAPI_INVALID_ARG;
            };
            local_for_scope(v8::Local::new(scope, &handle.module))
        };
        let export_name_value = try_status!(object_value_data(state, scope, export_name_id));
        let export_name = try_status!(string_value(scope, export_name_value));
        let export_value = if export_value_id == 0 {
            v8::undefined(scope).into()
        } else {
            try_status!(object_value_data(state, scope, export_value_id))
        };
        let mut try_catch = v8::TryCatch::new(scope);
        match module.set_synthetic_module_export(&mut try_catch, export_name, export_value) {
            Some(true) => NAPI_OK,
            Some(false) => NAPI_GENERIC_FAILURE,
            None => {
                if let Some(exception) = try_catch.exception() {
                    set_pending_exception(state, &mut try_catch, exception);
                    NAPI_PENDING_EXCEPTION
                } else {
                    NAPI_GENERIC_FAILURE
                }
            }
        }
    })
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_set_module_source_object(
    env: SnapiEnv,
    handle_id: u32,
    source_object_id: u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let Some(handle) = state.module_wrap_handles.get_mut(&handle_id) else {
        return NAPI_INVALID_ARG;
    };
    handle.source_object_id = source_object_id;
    NAPI_OK
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_get_module_source_object(
    env: SnapiEnv,
    handle_id: u32,
    result_out: *mut u32,
) -> i32 {
    if result_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let Some(handle) = state.module_wrap_handles.get(&handle_id) else {
        return NAPI_INVALID_ARG;
    };
    write_out(result_out, handle.source_object_id)
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_create_cached_data(
    env: SnapiEnv,
    handle_id: u32,
    result_out: *mut u32,
) -> i32 {
    if result_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let module = {
            let Some(handle) = state.module_wrap_handles.get(&handle_id) else {
                return NAPI_INVALID_ARG;
            };
            local_for_scope(v8::Local::new(scope, &handle.module))
        };
        if !module.is_source_text_module() {
            let empty = v8::ArrayBuffer::new(scope, 0);
            let buffer = try_status!(node_buffer_from_arraybuffer(state, scope, empty.into(), 0, 0));
            let id = store_global_value(state, scope, buffer);
            return write_out(result_out, id);
        }
        let cache = module
            .get_unbound_module_script(scope)
            .create_code_cache()
            .ok_or(NAPI_GENERIC_FAILURE);
        let cache = try_status!(cache);
        let bytes = cache.to_vec();
        let len = bytes.len();
        let backing = v8::ArrayBuffer::new_backing_store_from_boxed_slice(bytes.into_boxed_slice());
        let shared = backing.make_shared();
        let ab = v8::ArrayBuffer::with_backing_store(scope, &shared);
        let buffer = try_status!(node_buffer_from_arraybuffer(state, scope, ab.into(), 0, len));
        let id = store_global_value(state, scope, buffer);
        write_out(result_out, id)
    })
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_set_import_module_dynamically_callback(
    env: SnapiEnv,
    callback_id: u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        if callback_id != 0 {
            let callback = try_status!(object_value_data(state, scope, callback_id));
            if !callback.is_function() {
                return NAPI_FUNCTION_EXPECTED;
            }
        }
        state.module_wrap_import_module_dynamically_callback = (callback_id != 0).then_some(callback_id);
        if let Some(runtime) = state.runtime.as_mut() {
            runtime
                .isolate
                .set_host_import_module_dynamically_callback(
                    module_wrap_host_import_module_dynamically_callback,
                );
        }
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_set_initialize_import_meta_object_callback(
    env: SnapiEnv,
    callback_id: u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        if callback_id != 0 {
            let callback = try_status!(object_value_data(state, scope, callback_id));
            if !callback.is_function() {
                return NAPI_FUNCTION_EXPECTED;
            }
        }
        state.module_wrap_initialize_import_meta_callback = (callback_id != 0).then_some(callback_id);
        if let Some(runtime) = state.runtime.as_mut() {
            runtime
                .isolate
                .set_host_initialize_import_meta_object_callback(
                    module_wrap_host_initialize_import_meta_object_callback,
                );
        }
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_import_module_dynamically(
    env: SnapiEnv,
    argc: u32,
    argv_ids: *const u32,
    result_out: *mut u32,
) -> i32 {
    if result_out.is_null() || (argc > 0 && argv_ids.is_null()) {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let argv_ids = if argc == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(argv_ids, argc as usize) }.to_vec()
    };
    with_scope(state, |scope, state| {
        let Some(callback_id) = state.module_wrap_import_module_dynamically_callback else {
            return NAPI_INVALID_ARG;
        };
        let Some(callback) = local_function(state, scope, callback_id) else {
            return NAPI_INVALID_ARG;
        };
        let recv = scope.get_current_context().global(scope);
        let args = if argc >= 5 {
            let mut args = Vec::with_capacity(5);
            for id in argv_ids.iter().take(5) {
                args.push(try_status!(object_value_data(state, scope, *id)));
            }
            args
        } else {
            if argc == 0 {
                return NAPI_INVALID_ARG;
            }
            let specifier = try_status!(object_value_data(state, scope, argv_ids[0]));
            let referrer_name = if argc >= 2 {
                try_status!(object_value_data(state, scope, argv_ids[1]))
            } else {
                v8::undefined(scope).into()
            };
            let attrs = v8::Object::new(scope);
            vec![
                v8::undefined(scope).into(),
                specifier,
                v8::Integer::new(scope, 2).into(),
                attrs.into(),
                referrer_name,
            ]
        };
        let result = {
            let mut try_catch = v8::TryCatch::new(scope);
            let Some(result) = callback.call(&mut try_catch, recv.into(), &args) else {
                if let Some(exception) = try_catch.exception() {
                    set_pending_exception(state, &mut try_catch, exception);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            };
            local_for_scope(result)
        };
        let id = store_global_value(state, scope, result);
        write_out(result_out, id)
    })
}

pub unsafe fn snapi_bridge_unofficial_module_wrap_create_required_module_facade(
    env: SnapiEnv,
    handle_id: u32,
    result_out: *mut u32,
) -> i32 {
    if result_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        if !state.module_wrap_handles.contains_key(&handle_id) {
            return NAPI_INVALID_ARG;
        }
        let facade_url = v8::String::new(scope, "node:internal/require_module_default_facade")
            .ok_or(NAPI_GENERIC_FAILURE);
        let facade_url = try_status!(facade_url);
        let facade_source = v8::String::new(
            scope,
            "export * from 'original'; export { default } from 'original'; export const __esModule = true;",
        )
        .ok_or(NAPI_GENERIC_FAILURE);
        let facade_source = try_status!(facade_source);
        let undefined = v8::undefined(scope);
        let origin = v8::ScriptOrigin::new(
            scope,
            facade_url.into(),
            0,
            0,
            true,
            -1,
            undefined.into(),
            false,
            false,
            true,
        );
        let source = v8::script_compiler::Source::new(facade_source, Some(&origin));
        let facade = {
            let mut try_catch = v8::TryCatch::new(scope);
            let Some(facade) = v8::script_compiler::compile_module(&mut try_catch, source) else {
                if let Some(exception) = try_catch.exception() {
                    set_pending_exception(state, &mut try_catch, exception);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            };
            local_for_scope(facade)
        };
        state.temporary_required_module_facade_original = Some(handle_id);
        let instantiated = {
            let mut try_catch = v8::TryCatch::new(scope);
            let instantiated =
                facade.instantiate_module(&mut try_catch, module_wrap_link_required_facade_original);
            if instantiated.is_none() {
                if let Some(exception) = try_catch.exception() {
                    set_pending_exception(state, &mut try_catch, exception);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            }
            instantiated
        };
        let _ = instantiated;
        state.temporary_required_module_facade_original = None;
        let namespace = {
            let mut try_catch = v8::TryCatch::new(scope);
            if facade.evaluate(&mut try_catch).is_none() {
                if let Some(exception) = try_catch.exception() {
                    set_pending_exception(state, &mut try_catch, exception);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            }
            local_for_scope(facade.get_module_namespace())
        };
        let id = store_global_value(state, scope, namespace);
        write_out(result_out, id)
    })
}

pub unsafe fn snapi_bridge_unofficial_set_enqueue_foreground_task_callback(env: SnapiEnv) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    state.enqueue_foreground_task_callback = None;
    NAPI_OK
}

pub unsafe fn snapi_bridge_unofficial_set_near_heap_limit_callback(
    env: SnapiEnv,
    callback_id: u32,
    data: u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let Some(runtime) = state.runtime.as_mut() else {
        return NAPI_INVALID_ARG;
    };
    if state.near_heap_limit_callback.is_some() {
        runtime
            .isolate
            .remove_near_heap_limit_callback(snapi_near_heap_limit_callback, 0);
    }
    state.near_heap_limit_callback = (callback_id != 0).then_some(callback_id);
    state.near_heap_limit_data = data;
    if state.near_heap_limit_callback.is_some() {
        runtime
            .isolate
            .add_near_heap_limit_callback(snapi_near_heap_limit_callback, env as *mut c_void);
    }
    NAPI_OK
}

pub unsafe fn snapi_bridge_unofficial_remove_near_heap_limit_callback(
    env: SnapiEnv,
    heap_limit: u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let Some(runtime) = state.runtime.as_mut() else {
        return NAPI_INVALID_ARG;
    };
    if state.near_heap_limit_callback.take().is_some() {
        runtime
            .isolate
            .remove_near_heap_limit_callback(snapi_near_heap_limit_callback, heap_limit as usize);
    }
    state.near_heap_limit_data = 0;
    NAPI_OK
}

pub unsafe fn snapi_bridge_unofficial_set_fatal_error_callbacks(
    env: SnapiEnv,
    fatal_callback_id: u32,
    oom_callback_id: u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    state.fatal_error_callback = (fatal_callback_id != 0).then_some(fatal_callback_id);
    state.oom_error_callback = (oom_callback_id != 0).then_some(oom_callback_id);
    let Some(runtime) = state.runtime.as_mut() else {
        return NAPI_INVALID_ARG;
    };
    unsafe {
        snapi_v8_set_fatal_error_handler(
            ptr::from_mut(&mut *runtime.isolate),
            state.fatal_error_callback.map(|_| snapi_fatal_error_callback as extern "C" fn(*const i8, *const i8)),
        );
        snapi_v8_set_oom_error_handler(
            ptr::from_mut(&mut *runtime.isolate),
            state.oom_error_callback.map(|_| snapi_oom_error_callback as extern "C" fn(*const i8, bool)),
        );
    }
    NAPI_OK
}

pub unsafe fn snapi_bridge_unofficial_notify_datetime_configuration_change(env: SnapiEnv) -> i32 {
    let Some(_state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    NAPI_OK
}

pub unsafe fn snapi_bridge_unofficial_get_heap_space_count(
    env: SnapiEnv,
    count_out: *mut u32,
) -> i32 {
    if count_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let Some(_runtime) = state.runtime.as_mut() else {
        return NAPI_INVALID_ARG;
    };
    write_out(count_out, 1)
}

pub unsafe fn snapi_bridge_unofficial_get_heap_space_statistics(
    env: SnapiEnv,
    space_index: u32,
    stats_out: *mut SnapiUnofficialHeapSpaceStatistics,
) -> i32 {
    if stats_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let Some(runtime) = state.runtime.as_mut() else {
        return NAPI_INVALID_ARG;
    };
    if space_index != 0 {
        return NAPI_INVALID_ARG;
    }
    let mut heap = v8::HeapStatistics::default();
    runtime.isolate.get_heap_statistics(&mut heap);
    let mut stats = SnapiUnofficialHeapSpaceStatistics {
        space_name: [0; 64],
        space_size: heap.total_heap_size() as u64,
        space_used_size: heap.used_heap_size() as u64,
        space_available_size: heap.total_available_size() as u64,
        physical_space_size: heap.total_physical_size() as u64,
    };
    copy_utf8_into(&mut stats.space_name, "heap");
    unsafe {
        *stats_out = stats;
    }
    NAPI_OK
}

pub unsafe fn snapi_bridge_unofficial_get_heap_code_statistics(
    env: SnapiEnv,
    stats_out: *mut SnapiUnofficialHeapCodeStatistics,
) -> i32 {
    if stats_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let Some(runtime) = state.runtime.as_mut() else {
        return NAPI_INVALID_ARG;
    };
    let mut heap = v8::HeapStatistics::default();
    runtime.isolate.get_heap_statistics(&mut heap);
    unsafe {
        *stats_out = SnapiUnofficialHeapCodeStatistics {
            code_and_metadata_size: 0,
            bytecode_and_metadata_size: 0,
            external_script_source_size: heap.external_memory() as u64,
            cpu_profiler_metadata_size: 0,
        };
    }
    NAPI_OK
}

pub unsafe fn snapi_bridge_unofficial_start_cpu_profile(
    env: SnapiEnv,
    result_out: *mut i32,
    profile_id_out: *mut u32,
) -> i32 {
    if result_out.is_null() || profile_id_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let profile_id = next_id(&mut state.next_cpu_profile_id);
    state.active_cpu_profiles.push(profile_id);
    let status = write_out(result_out, 0);
    if status != NAPI_OK {
        return status;
    }
    write_out(profile_id_out, profile_id)
}

pub unsafe fn snapi_bridge_unofficial_stop_cpu_profile(
    env: SnapiEnv,
    profile_id: u32,
    found_out: *mut i32,
    json_out: *mut u64,
    json_len_out: *mut u32,
) -> i32 {
    if found_out.is_null() || json_out.is_null() || json_len_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let Some(index) = state.active_cpu_profiles.iter().position(|id| *id == profile_id) else {
        let status = write_out(found_out, 0);
        if status != NAPI_OK {
            return status;
        }
        let status = write_out(json_out, 0);
        if status != NAPI_OK {
            return status;
        }
        return write_out(json_len_out, 0);
    };
    state.active_cpu_profiles.remove(index);
    let bytes = br#"{"nodes":[],"samples":[],"timeDeltas":[],"startTime":0,"endTime":0}"#;
    let status = write_out(found_out, 1);
    if status != NAPI_OK {
        return status;
    }
    malloc_bytes(bytes, json_out, json_len_out)
}

pub unsafe fn snapi_bridge_unofficial_start_heap_profile(
    env: SnapiEnv,
    started_out: *mut i32,
) -> i32 {
    if started_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    let started = !state.heap_profile_started;
    state.heap_profile_started = true;
    write_out(started_out, started as i32)
}

pub unsafe fn snapi_bridge_unofficial_stop_heap_profile(
    env: SnapiEnv,
    found_out: *mut i32,
    json_out: *mut u64,
    json_len_out: *mut u32,
) -> i32 {
    if found_out.is_null() || json_out.is_null() || json_len_out.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    if !state.heap_profile_started {
        let status = write_out(found_out, 0);
        if status != NAPI_OK {
            return status;
        }
        let status = write_out(json_out, 0);
        if status != NAPI_OK {
            return status;
        }
        return write_out(json_len_out, 0);
    }
    let Some(runtime) = state.runtime.as_mut() else {
        return NAPI_INVALID_ARG;
    };
    let mut bytes = Vec::new();
    runtime.isolate.take_heap_snapshot(|chunk| {
        bytes.extend_from_slice(chunk);
        true
    });
    state.heap_profile_started = false;
    let status = write_out(found_out, 1);
    if status != NAPI_OK {
        return status;
    }
    malloc_bytes(&bytes, json_out, json_len_out)
}

pub unsafe fn snapi_bridge_unofficial_create_serdes_binding(
    env: SnapiEnv,
    out_id: *mut u32,
) -> i32 {
    if out_id.is_null() {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let source = r#"
(() => {
  const cloneValue = (value) => {
    if (typeof structuredClone === 'function') return structuredClone(value);
    return JSON.parse(JSON.stringify(value));
  };
  const encoder = typeof TextEncoder === 'function' ? new TextEncoder() : null;
  const decoder = typeof TextDecoder === 'function' ? new TextDecoder() : null;
  const encode = (text) => {
    if (encoder) return encoder.encode(text);
    const out = new Uint8Array(text.length);
    for (let i = 0; i < text.length; i++) out[i] = text.charCodeAt(i) & 0xff;
    return out;
  };
  const decode = (bytes) => {
    if (decoder) return decoder.decode(bytes);
    let out = '';
    for (let i = 0; i < bytes.length; i++) out += String.fromCharCode(bytes[i]);
    return out;
  };
  class Serializer {
    constructor() {
      this._chunks = [];
      this._transfers = new Map();
      this._treatArrayBufferViewsAsHostObjects = false;
    }
    writeHeader() { this.writeUint32(0xfeedface >>> 0); }
    writeValue(value) {
      const payload = encode(JSON.stringify(cloneValue(value)));
      this.writeUint32(payload.byteLength);
      this.writeRawBytes(payload);
    }
    releaseBuffer() {
      const total = this._chunks.reduce((sum, chunk) => sum + chunk.byteLength, 0);
      const out = new Uint8Array(total);
      let offset = 0;
      for (const chunk of this._chunks) {
        out.set(chunk, offset);
        offset += chunk.byteLength;
      }
      this._chunks.length = 0;
      if (typeof Buffer === 'function' && typeof Buffer.from === 'function') {
        return Buffer.from(out.buffer, out.byteOffset, out.byteLength);
      }
      return out;
    }
    transferArrayBuffer(id, arrayBuffer) { this._transfers.set(id, arrayBuffer); }
    writeUint32(value) {
      const bytes = new Uint8Array(4);
      new DataView(bytes.buffer).setUint32(0, value >>> 0, true);
      this._chunks.push(bytes);
    }
    writeUint64(value) {
      const bytes = new Uint8Array(8);
      const view = new DataView(bytes.buffer);
      if (typeof view.setBigUint64 === 'function') {
        view.setBigUint64(0, BigInt(value), true);
      } else {
        const number = Number(value);
        view.setUint32(0, number >>> 0, true);
        view.setUint32(4, Math.floor(number / 0x100000000), true);
      }
      this._chunks.push(bytes);
    }
    writeDouble(value) {
      const bytes = new Uint8Array(8);
      new DataView(bytes.buffer).setFloat64(0, value, true);
      this._chunks.push(bytes);
    }
    writeRawBytes(value) {
      if (value instanceof Uint8Array) {
        this._chunks.push(new Uint8Array(value));
        return;
      }
      if (ArrayBuffer.isView(value)) {
        this._chunks.push(new Uint8Array(value.buffer, value.byteOffset, value.byteLength));
        return;
      }
      this._chunks.push(new Uint8Array(value));
    }
    _setTreatArrayBufferViewsAsHostObjects(flag) {
      this._treatArrayBufferViewsAsHostObjects = !!flag;
    }
  }
  class Deserializer {
    constructor(buffer) {
      if (buffer instanceof Uint8Array) {
        this._bytes = buffer;
      } else if (ArrayBuffer.isView(buffer)) {
        this._bytes = new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
      } else {
        this._bytes = new Uint8Array(buffer);
      }
      this._offset = 0;
      this._transfers = new Map();
    }
    readHeader() {
      if (this._bytes.byteLength - this._offset >= 4) this._offset += 4;
      return true;
    }
    readValue() {
      const len = this.readUint32();
      return JSON.parse(decode(this._readRawBytes(len)));
    }
    getWireFormatVersion() { return 1; }
    transferArrayBuffer(id, arrayBuffer) { this._transfers.set(id, arrayBuffer); }
    readUint32() {
      const value = new DataView(
        this._bytes.buffer,
        this._bytes.byteOffset + this._offset,
        4,
      ).getUint32(0, true);
      this._offset += 4;
      return value;
    }
    readUint64() {
      const view = new DataView(
        this._bytes.buffer,
        this._bytes.byteOffset + this._offset,
        8,
      );
      this._offset += 8;
      if (typeof view.getBigUint64 === 'function') {
        return Number(view.getBigUint64(0, true));
      }
      return view.getUint32(0, true) + view.getUint32(4, true) * 0x100000000;
    }
    readDouble() {
      const value = new DataView(
        this._bytes.buffer,
        this._bytes.byteOffset + this._offset,
        8,
      ).getFloat64(0, true);
      this._offset += 8;
      return value;
    }
    _readRawBytes(length) {
      const end = this._offset + length;
      const out = this._bytes.slice(this._offset, end);
      this._offset = end;
      return out;
    }
  }
  return { Serializer, Deserializer };
})()
        "#;
        let source = v8::String::new(scope, source).ok_or(NAPI_GENERIC_FAILURE);
        let source = try_status!(source);
        let binding = {
            let mut try_catch = v8::TryCatch::new(scope);
            let Some(script) = v8::Script::compile(&mut try_catch, source, None) else {
                if let Some(exception) = try_catch.exception() {
                    set_pending_exception(state, &mut try_catch, exception);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            };
            let Some(binding) = script.run(&mut try_catch) else {
                if let Some(exception) = try_catch.exception() {
                    set_pending_exception(state, &mut try_catch, exception);
                    return NAPI_PENDING_EXCEPTION;
                }
                return NAPI_GENERIC_FAILURE;
            };
            local_for_scope(binding)
        };
        let id = store_global_value(state, scope, binding);
        write_out(out_id, id)
    })
}

pub unsafe fn snapi_bridge_unofficial_set_prepare_stack_trace_callback(
    env: SnapiEnv,
    callback_id: u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        if callback_id != 0 {
            let callback = try_status!(object_value_data(state, scope, callback_id));
            if !callback.is_function() {
                return NAPI_INVALID_ARG;
            }
        }
        state.prepare_stack_trace_callback = (callback_id != 0).then_some(callback_id);
        if let Some(runtime) = state.runtime.as_mut() {
            runtime
                .isolate
                .set_prepare_stack_trace_callback(snapi_prepare_stack_trace_callback);
        }
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_unofficial_set_promise_reject_callback(
    env: SnapiEnv,
    callback_id: u32,
) -> i32 {
    if callback_id == 0 {
        return NAPI_INVALID_ARG;
    }
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let callback = try_status!(object_value_data(state, scope, callback_id));
        if !callback.is_function() {
            return NAPI_FUNCTION_EXPECTED;
        }
        state.promise_reject_callback = Some(callback_id);
        if let Some(runtime) = state.runtime.as_mut() {
            runtime
                .isolate
                .set_promise_reject_callback(snapi_promise_reject_callback);
        }
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_unofficial_set_promise_hooks(
    env: SnapiEnv,
    init_callback_id: u32,
    before_callback_id: u32,
    after_callback_id: u32,
    resolve_callback_id: u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        for callback_id in [
            init_callback_id,
            before_callback_id,
            after_callback_id,
            resolve_callback_id,
        ] {
            if callback_id == 0 {
                continue;
            }
            let callback = try_status!(object_value_data(state, scope, callback_id));
            if !callback.is_function() {
                return NAPI_FUNCTION_EXPECTED;
            }
        }
        state.promise_hook_callbacks = [
            init_callback_id,
            before_callback_id,
            after_callback_id,
            resolve_callback_id,
        ];
        if let Some(runtime) = state.runtime.as_mut() {
            runtime.isolate.set_promise_hook(snapi_promise_hook_callback);
        }
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_unofficial_preserve_error_source_message(
    env: SnapiEnv,
    error_id: u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let error = try_status!(object_value_data(state, scope, error_id));
        let error_obj: v8::Local<v8::Object> = match error.try_into() {
            Ok(object) => object,
            Err(_) => return NAPI_INVALID_ARG,
        };
        let message = v8::Exception::create_message(scope, error);
        let arrow = match build_arrow_message(scope, message) {
            Some(arrow) => arrow,
            None => return NAPI_OK,
        };
        let arrow_key = try_status!(private_for_api(scope, "node:arrowMessage"));
        if let Some(existing) = error_obj.get_private(scope, arrow_key)
            && existing.is_string()
        {
            return NAPI_OK;
        }
        let arrow_value = v8::String::new(scope, &arrow).ok_or(NAPI_GENERIC_FAILURE);
        let arrow_value = match arrow_value {
            Ok(value) => value,
            Err(status) => return status,
        };
        match error_obj.set_private(scope, arrow_key, arrow_value.into()) {
            Some(true) => NAPI_OK,
            Some(false) => NAPI_GENERIC_FAILURE,
            None => NAPI_PENDING_EXCEPTION,
        }
    })
}

pub unsafe fn snapi_bridge_unofficial_mark_promise_as_handled(
    env: SnapiEnv,
    promise_id: u32,
) -> i32 {
    let Some(state) = state_mut(env) else {
        return NAPI_INVALID_ARG;
    };
    with_scope(state, |scope, state| {
        let value = try_status!(object_value_data(state, scope, promise_id));
        let promise: v8::Local<v8::Promise> = match value.try_into() {
            Ok(promise) => promise,
            Err(_) => return NAPI_INVALID_ARG,
        };
        if promise.state() != v8::PromiseState::Rejected {
            return NAPI_OK;
        }
        let Some(callback_id) = state.promise_reject_callback else {
            return NAPI_OK;
        };
        let Some(callback) = local_function(state, scope, callback_id) else {
            return NAPI_OK;
        };

        let undefined = v8::undefined(scope).into();
        let event = v8::Integer::new(
            scope,
            v8::PromiseRejectEvent::PromiseHandlerAddedAfterReject as i32,
        )
        .into();
        let promise_value: v8::Local<v8::Value> = promise.into();
        let args = [event, promise_value, undefined];

        let mut try_catch = v8::TryCatch::new(scope);
        if callback.call(&mut try_catch, undefined, &args).is_none() {
            if let Some(exception) = try_catch.exception() {
                state.pending_exception = Some(v8::Global::new(&mut try_catch, exception));
                return NAPI_PENDING_EXCEPTION;
            }
            return NAPI_GENERIC_FAILURE;
        }
        NAPI_OK
    })
}

pub unsafe fn snapi_bridge_dispose() {
    for env in snapshot_live_envs() {
        let _ = snapi_bridge_unofficial_release_env(env);
    }
}

