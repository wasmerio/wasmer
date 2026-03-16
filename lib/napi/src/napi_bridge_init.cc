// N-API bridge for the WASM host.
//
// Uses unofficial_napi_create_env() from napi-v8 to obtain a proper
// napi_env with all V8 scopes managed correctly.  Each N-API function
// is wrapped with an extern "C" bridge that takes/returns u32 handle IDs
// instead of opaque pointers, so the Rust host can translate between
// WASM guest memory and native N-API calls.

#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <mutex>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#ifndef NAPI_EXPERIMENTAL
#define NAPI_EXPERIMENTAL
#endif

#include "node_api.h"
#include "unofficial_napi.h"

namespace {

std::recursive_mutex g_mu;
struct CbContext {
  uint32_t this_id;
  uint32_t argc;
  uint32_t argv_ids[64];
  uint64_t data_val;
  napi_callback_info original_info;
};

struct CbRegistration {
  uint32_t wasm_fn_ptr;
  uint32_t wasm_setter_fn_ptr;
  uint64_t data_val;
};

struct CallbackBinding;

struct SnapiEnvState {
  napi_env env = nullptr;
  void* scope = nullptr;

  // Handle table: maps u32 IDs to persistent napi_value references.
  //
  // Raw napi_value handles are only valid within the originating handle scope.
  // The guest stores these IDs across calls, so we must hold a reference on the
  // host side and re-resolve it when loading the value again.
  std::unordered_map<uint32_t, napi_ref> values;
  uint32_t next_value_id = 1;

  // Handle table for napi_ref (references).
  std::unordered_map<uint32_t, napi_ref> refs;
  uint32_t next_ref_id = 1;

  // Handle table for napi_deferred (promise deferreds).
  std::unordered_map<uint32_t, napi_deferred> deferreds;
  uint32_t next_deferred_id = 1;

  // Handle table for napi_escapable_handle_scope.
  std::unordered_map<uint32_t, napi_escapable_handle_scope> esc_scopes;
  uint32_t next_esc_scope_id = 1;

  // Handle table for opaque module_wrap handles.
  std::unordered_map<uint32_t, void*> module_wrap_handles;
  uint32_t next_module_wrap_handle_id = 1;

  std::vector<CbContext> cb_stack;
  std::unordered_map<uint32_t, CbRegistration> cb_registry;
  uint32_t next_cb_reg_id = 1;
  std::vector<std::unique_ptr<CallbackBinding>> callback_bindings;
};

struct CallbackBinding {
  SnapiEnvState* state = nullptr;
  uint32_t reg_id = 0;
};

std::unordered_set<SnapiEnvState*> g_envs;

CallbackBinding* RegisterCallbackBinding(SnapiEnvState* state, uint32_t reg_id) {
  if (state == nullptr || reg_id == 0) return nullptr;
  state->callback_bindings.push_back(
      std::make_unique<CallbackBinding>(CallbackBinding{state, reg_id}));
  return state->callback_bindings.back().get();
}

SnapiEnvState* LookupEnvState(SnapiEnvState* env_state) {
  if (env_state == nullptr || env_state->env == nullptr) return nullptr;
  return env_state;
}

uint32_t StoreValue(SnapiEnvState& state, napi_value val) {
  if (val == nullptr) return 0;
  if (state.env == nullptr) return 0;
  napi_ref ref = nullptr;
  if (napi_create_reference(state.env, val, 1, &ref) != napi_ok || ref == nullptr) {
    return 0;
  }
  uint32_t id = state.next_value_id++;
  state.values[id] = ref;
  return id;
}

napi_value LoadValue(SnapiEnvState& state, uint32_t id) {
  if (id == 0) return nullptr;
  if (state.env == nullptr) return nullptr;
  auto it = state.values.find(id);
  if (it == state.values.end() || it->second == nullptr) return nullptr;
  napi_value value = nullptr;
  if (napi_get_reference_value(state.env, it->second, &value) != napi_ok || value == nullptr) {
    return nullptr;
  }
  return value;
}

uint32_t StoreRef(SnapiEnvState& state, napi_ref ref) {
  if (ref == nullptr) return 0;
  uint32_t id = state.next_ref_id++;
  state.refs[id] = ref;
  return id;
}

napi_ref LoadRef(SnapiEnvState& state, uint32_t id) {
  if (id == 0) return nullptr;
  auto it = state.refs.find(id);
  return it != state.refs.end() ? it->second : nullptr;
}

void RemoveRef(SnapiEnvState& state, uint32_t id) {
  state.refs.erase(id);
}

uint32_t StoreDeferred(SnapiEnvState& state, napi_deferred d) {
  if (d == nullptr) return 0;
  uint32_t id = state.next_deferred_id++;
  state.deferreds[id] = d;
  return id;
}

napi_deferred LoadDeferred(SnapiEnvState& state, uint32_t id) {
  if (id == 0) return nullptr;
  auto it = state.deferreds.find(id);
  return it != state.deferreds.end() ? it->second : nullptr;
}

void RemoveDeferred(SnapiEnvState& state, uint32_t id) {
  state.deferreds.erase(id);
}

uint32_t StoreEscScope(SnapiEnvState& state, napi_escapable_handle_scope s) {
  if (s == nullptr) return 0;
  uint32_t id = state.next_esc_scope_id++;
  state.esc_scopes[id] = s;
  return id;
}

napi_escapable_handle_scope LoadEscScope(SnapiEnvState& state, uint32_t id) {
  if (id == 0) return nullptr;
  auto it = state.esc_scopes.find(id);
  return it != state.esc_scopes.end() ? it->second : nullptr;
}

void RemoveEscScope(SnapiEnvState& state, uint32_t id) {
  state.esc_scopes.erase(id);
}

uint32_t StoreModuleWrapHandle(SnapiEnvState& state, void* handle) {
  if (handle == nullptr) return 0;
  uint32_t id = state.next_module_wrap_handle_id++;
  state.module_wrap_handles[id] = handle;
  return id;
}

void* LoadModuleWrapHandle(SnapiEnvState& state, uint32_t id) {
  if (id == 0) return nullptr;
  auto it = state.module_wrap_handles.find(id);
  return it != state.module_wrap_handles.end() ? it->second : nullptr;
}

void RemoveModuleWrapHandle(SnapiEnvState& state, uint32_t id) {
  state.module_wrap_handles.erase(id);
}

SnapiEnvState* RequireEnvState(SnapiEnvState* env_state) {
  auto* bridge_state = LookupEnvState(env_state);
  if (bridge_state == nullptr || bridge_state->env == nullptr) {
    return nullptr;
  }
  return bridge_state;
}

napi_status DisposeBridgeStateLocked(SnapiEnvState* state) {
  if (state == nullptr) return napi_ok;
  for (auto& entry : state->values) {
    if (entry.second != nullptr) {
      napi_delete_reference(state->env, entry.second);
    }
  }
  state->values.clear();
  state->next_value_id = 1;
  state->refs.clear();
  state->next_ref_id = 1;
  state->deferreds.clear();
  state->next_deferred_id = 1;
  state->esc_scopes.clear();
  state->next_esc_scope_id = 1;
  state->module_wrap_handles.clear();
  state->next_module_wrap_handle_id = 1;
  state->cb_stack.clear();
  state->cb_registry.clear();
  state->next_cb_reg_id = 1;
  state->callback_bindings.clear();
  if (state->scope != nullptr) {
    napi_status s = unofficial_napi_release_env(state->scope);
    state->scope = nullptr;
    state->env = nullptr;
    g_envs.erase(state);
    delete state;
    return s;
  }
  state->env = nullptr;
  g_envs.erase(state);
  delete state;
  return napi_ok;
}

}  // namespace

// ============================================================
// Initialization
// ============================================================

extern "C" int snapi_bridge_init() {
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  // Intentionally do not create a N-API env here.
  // Env creation is deferred until the guest explicitly calls
  // `unofficial_napi_create_env`, so init happens on the execution thread.
  (void)lock;
  return 1;
}

// ============================================================
// Value creation
// ============================================================

extern "C" int snapi_bridge_get_undefined(SnapiEnvState* env_state, uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value result;
  napi_status s = napi_get_undefined(env, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_get_null(SnapiEnvState* env_state, uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value result;
  napi_status s = napi_get_null(env, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_get_boolean(SnapiEnvState* env_state, int value, uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value result;
  napi_status s = napi_get_boolean(env, value != 0, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_get_global(SnapiEnvState* env_state, uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value result;
  napi_status s = napi_get_global(env, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_create_string_utf8(SnapiEnvState* env_state, const char* str,
                                               uint32_t wasm_length,
                                               uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  size_t length =
      (wasm_length == 0xFFFFFFFFu) ? NAPI_AUTO_LENGTH : (size_t)wasm_length;
  napi_value result;
  napi_status s = napi_create_string_utf8(env, str, length, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_create_string_latin1(SnapiEnvState* env_state, const char* str,
                                                 uint32_t wasm_length,
                                                 uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  size_t length =
      (wasm_length == 0xFFFFFFFFu) ? NAPI_AUTO_LENGTH : (size_t)wasm_length;
  napi_value result;
  napi_status s = napi_create_string_latin1(env, str, length, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_create_int32(SnapiEnvState* env_state, int32_t value, uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value result;
  napi_status s = napi_create_int32(env, value, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_create_uint32(SnapiEnvState* env_state, uint32_t value, uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value result;
  napi_status s = napi_create_uint32(env, value, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_create_double(SnapiEnvState* env_state, double value, uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value result;
  napi_status s = napi_create_double(env, value, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_create_int64(SnapiEnvState* env_state, int64_t value, uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value result;
  napi_status s = napi_create_int64(env, value, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_create_object(SnapiEnvState* env_state, uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value result;
  napi_status s = napi_create_object(env, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_create_array(SnapiEnvState* env_state, uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value result;
  napi_status s = napi_create_array(env, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_create_array_with_length(SnapiEnvState* env_state, uint32_t length,
                                                     uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value result;
  napi_status s = napi_create_array_with_length(env, (size_t)length, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

// ============================================================
// Value reading
// ============================================================

extern "C" int snapi_bridge_get_value_string_utf8(SnapiEnvState* env_state, uint32_t id, char* buf,
                                                  size_t bufsize,
                                                  size_t* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  return napi_get_value_string_utf8(env, val, buf, bufsize, result);
}

extern "C" int snapi_bridge_get_value_string_latin1(SnapiEnvState* env_state, uint32_t id, char* buf,
                                                    size_t bufsize,
                                                    size_t* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  return napi_get_value_string_latin1(env, val, buf, bufsize, result);
}

extern "C" int snapi_bridge_get_value_int32(SnapiEnvState* env_state, uint32_t id, int32_t* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  return napi_get_value_int32(env, val, result);
}

extern "C" int snapi_bridge_get_value_uint32(SnapiEnvState* env_state, uint32_t id, uint32_t* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  return napi_get_value_uint32(env, val, result);
}

extern "C" int snapi_bridge_get_value_double(SnapiEnvState* env_state, uint32_t id, double* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  return napi_get_value_double(env, val, result);
}

extern "C" int snapi_bridge_get_value_int64(SnapiEnvState* env_state, uint32_t id, int64_t* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  return napi_get_value_int64(env, val, result);
}

extern "C" int snapi_bridge_get_value_bool(SnapiEnvState* env_state, uint32_t id, int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  bool b;
  napi_status s = napi_get_value_bool(env, val, &b);
  if (s != napi_ok) return s;
  *result = b ? 1 : 0;
  return napi_ok;
}

// ============================================================
// Type checking
// ============================================================

extern "C" int snapi_bridge_typeof(SnapiEnvState* env_state, uint32_t id, int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  napi_valuetype vtype;
  napi_status s = napi_typeof(env, val, &vtype);
  if (s != napi_ok) return s;
  *result = (int)vtype;
  return napi_ok;
}

extern "C" int snapi_bridge_is_array(SnapiEnvState* env_state, uint32_t id, int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  bool is;
  napi_status s = napi_is_array(env, val, &is);
  if (s != napi_ok) return s;
  *result = is ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_is_error(SnapiEnvState* env_state, uint32_t id, int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  bool is;
  napi_status s = napi_is_error(env, val, &is);
  if (s != napi_ok) return s;
  *result = is ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_is_arraybuffer(SnapiEnvState* env_state, uint32_t id, int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  bool is;
  napi_status s = napi_is_arraybuffer(env, val, &is);
  if (s != napi_ok) return s;
  *result = is ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_is_typedarray(SnapiEnvState* env_state, uint32_t id, int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  bool is;
  napi_status s = napi_is_typedarray(env, val, &is);
  if (s != napi_ok) return s;
  *result = is ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_is_dataview(SnapiEnvState* env_state, uint32_t id, int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  bool is;
  napi_status s = napi_is_dataview(env, val, &is);
  if (s != napi_ok) return s;
  *result = is ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_is_date(SnapiEnvState* env_state, uint32_t id, int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  bool is;
  napi_status s = napi_is_date(env, val, &is);
  if (s != napi_ok) return s;
  *result = is ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_is_promise(SnapiEnvState* env_state, uint32_t id, int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  bool is;
  napi_status s = napi_is_promise(env, val, &is);
  if (s != napi_ok) return s;
  *result = is ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_instanceof(SnapiEnvState* env_state, uint32_t obj_id, uint32_t ctor_id,
                                       int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  napi_value ctor = LoadValue(*bridge_state, ctor_id);
  if (!obj || !ctor) return napi_invalid_arg;
  bool is;
  napi_status s = napi_instanceof(env, obj, ctor, &is);
  if (s != napi_ok) return s;
  *result = is ? 1 : 0;
  return napi_ok;
}

// ============================================================
// Coercion
// ============================================================

extern "C" int snapi_bridge_coerce_to_bool(SnapiEnvState* env_state, uint32_t id, uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  napi_value result;
  napi_status s = napi_coerce_to_bool(env, val, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_coerce_to_number(SnapiEnvState* env_state, uint32_t id, uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  napi_value result;
  napi_status s = napi_coerce_to_number(env, val, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_coerce_to_string(SnapiEnvState* env_state, uint32_t id, uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  napi_value result;
  napi_status s = napi_coerce_to_string(env, val, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_coerce_to_object(SnapiEnvState* env_state, uint32_t id, uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  napi_value result;
  napi_status s = napi_coerce_to_object(env, val, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

// ============================================================
// Object operations
// ============================================================

extern "C" int snapi_bridge_set_property(SnapiEnvState* env_state, uint32_t obj_id, uint32_t key_id,
                                         uint32_t val_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  napi_value key = LoadValue(*bridge_state, key_id);
  napi_value val = LoadValue(*bridge_state, val_id);
  if (!obj || !key || !val) return napi_invalid_arg;
  return napi_set_property(env, obj, key, val);
}

extern "C" int snapi_bridge_get_property(SnapiEnvState* env_state, uint32_t obj_id, uint32_t key_id,
                                         uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  napi_value key = LoadValue(*bridge_state, key_id);
  if (!obj || !key) return napi_invalid_arg;
  napi_value result;
  napi_status s = napi_get_property(env, obj, key, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_has_property(SnapiEnvState* env_state, uint32_t obj_id, uint32_t key_id,
                                         int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  napi_value key = LoadValue(*bridge_state, key_id);
  if (!obj || !key) return napi_invalid_arg;
  bool has;
  napi_status s = napi_has_property(env, obj, key, &has);
  if (s != napi_ok) return s;
  *result = has ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_has_own_property(SnapiEnvState* env_state, uint32_t obj_id, uint32_t key_id,
                                             int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  napi_value key = LoadValue(*bridge_state, key_id);
  if (!obj || !key) return napi_invalid_arg;
  bool has;
  napi_status s = napi_has_own_property(env, obj, key, &has);
  if (s != napi_ok) return s;
  *result = has ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_delete_property(SnapiEnvState* env_state, uint32_t obj_id, uint32_t key_id,
                                            int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  napi_value key = LoadValue(*bridge_state, key_id);
  if (!obj || !key) return napi_invalid_arg;
  bool deleted;
  napi_status s = napi_delete_property(env, obj, key, &deleted);
  if (s != napi_ok) return s;
  *result = deleted ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_set_named_property(SnapiEnvState* env_state, uint32_t obj_id,
                                               const char* name,
                                               uint32_t val_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  napi_value val = LoadValue(*bridge_state, val_id);
  if (!obj || !val || !name) return napi_invalid_arg;
  return napi_set_named_property(env, obj, name, val);
}

extern "C" int snapi_bridge_get_named_property(SnapiEnvState* env_state, uint32_t obj_id,
                                               const char* name,
                                               uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  if (!obj || !name) return napi_invalid_arg;
  napi_value result;
  napi_status s = napi_get_named_property(env, obj, name, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_has_named_property(SnapiEnvState* env_state, uint32_t obj_id,
                                               const char* name,
                                               int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  if (!obj || !name) return napi_invalid_arg;
  bool has;
  napi_status s = napi_has_named_property(env, obj, name, &has);
  if (s != napi_ok) return s;
  *result = has ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_set_element(SnapiEnvState* env_state, uint32_t obj_id, uint32_t index,
                                        uint32_t val_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  napi_value val = LoadValue(*bridge_state, val_id);
  if (!obj || !val) return napi_invalid_arg;
  return napi_set_element(env, obj, index, val);
}

extern "C" int snapi_bridge_get_element(SnapiEnvState* env_state, uint32_t obj_id, uint32_t index,
                                        uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  if (!obj) return napi_invalid_arg;
  napi_value result;
  napi_status s = napi_get_element(env, obj, index, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_has_element(SnapiEnvState* env_state, uint32_t obj_id, uint32_t index,
                                        int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  if (!obj) return napi_invalid_arg;
  bool has;
  napi_status s = napi_has_element(env, obj, index, &has);
  if (s != napi_ok) return s;
  *result = has ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_delete_element(SnapiEnvState* env_state, uint32_t obj_id, uint32_t index,
                                           int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  if (!obj) return napi_invalid_arg;
  bool deleted;
  napi_status s = napi_delete_element(env, obj, index, &deleted);
  if (s != napi_ok) return s;
  *result = deleted ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_get_array_length(SnapiEnvState* env_state, uint32_t arr_id,
                                             uint32_t* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value arr = LoadValue(*bridge_state, arr_id);
  if (!arr) return napi_invalid_arg;
  return napi_get_array_length(env, arr, result);
}

extern "C" int snapi_bridge_get_property_names(SnapiEnvState* env_state, uint32_t obj_id,
                                               uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  if (!obj) return napi_invalid_arg;
  napi_value result;
  napi_status s = napi_get_property_names(env, obj, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_get_all_property_names(SnapiEnvState* env_state, uint32_t obj_id,
                                                   int mode, int filter,
                                                   int conversion,
                                                   uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  if (!obj) return napi_invalid_arg;
  napi_value result;
  napi_status s = napi_get_all_property_names(
      env, obj, (napi_key_collection_mode)mode, (napi_key_filter)filter,
      (napi_key_conversion)conversion, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_get_prototype(SnapiEnvState* env_state, uint32_t obj_id, uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  if (!obj) return napi_invalid_arg;
  napi_value result;
  napi_status s = napi_get_prototype(env, obj, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_object_freeze(SnapiEnvState* env_state, uint32_t obj_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  if (!obj) return napi_invalid_arg;
  return napi_object_freeze(env, obj);
}

extern "C" int snapi_bridge_object_seal(SnapiEnvState* env_state, uint32_t obj_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  if (!obj) return napi_invalid_arg;
  return napi_object_seal(env, obj);
}

// ============================================================
// Comparison
// ============================================================

extern "C" int snapi_bridge_strict_equals(SnapiEnvState* env_state, uint32_t a_id, uint32_t b_id,
                                          int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value a = LoadValue(*bridge_state, a_id);
  napi_value b = LoadValue(*bridge_state, b_id);
  if (!a || !b) return napi_invalid_arg;
  bool eq;
  napi_status s = napi_strict_equals(env, a, b, &eq);
  if (s != napi_ok) return s;
  *result = eq ? 1 : 0;
  return napi_ok;
}

// ============================================================
// Error handling
// ============================================================

extern "C" int snapi_bridge_create_error(SnapiEnvState* env_state, uint32_t code_id, uint32_t msg_id,
                                         uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value code = LoadValue(*bridge_state, code_id);  // can be null (0)
  napi_value msg = LoadValue(*bridge_state, msg_id);
  if (!msg) return napi_invalid_arg;
  napi_value result;
  napi_status s = napi_create_error(env, code, msg, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_create_type_error(SnapiEnvState* env_state, uint32_t code_id,
                                              uint32_t msg_id,
                                              uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value code = LoadValue(*bridge_state, code_id);
  napi_value msg = LoadValue(*bridge_state, msg_id);
  if (!msg) return napi_invalid_arg;
  napi_value result;
  napi_status s = napi_create_type_error(env, code, msg, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_create_range_error(SnapiEnvState* env_state, uint32_t code_id,
                                               uint32_t msg_id,
                                               uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value code = LoadValue(*bridge_state, code_id);
  napi_value msg = LoadValue(*bridge_state, msg_id);
  if (!msg) return napi_invalid_arg;
  napi_value result;
  napi_status s = napi_create_range_error(env, code, msg, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_throw(SnapiEnvState* env_state, uint32_t error_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value error = LoadValue(*bridge_state, error_id);
  if (!error) return napi_invalid_arg;
  return napi_throw(env, error);
}

extern "C" int snapi_bridge_throw_error(SnapiEnvState* env_state, const char* code, const char* msg) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  return napi_throw_error(env, code, msg);
}

extern "C" int snapi_bridge_throw_type_error(SnapiEnvState* env_state, const char* code,
                                             const char* msg) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  return napi_throw_type_error(env, code, msg);
}

extern "C" int snapi_bridge_throw_range_error(SnapiEnvState* env_state, const char* code,
                                              const char* msg) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  return napi_throw_range_error(env, code, msg);
}

extern "C" int snapi_bridge_is_exception_pending(SnapiEnvState* env_state, int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  bool pending;
  napi_status s = napi_is_exception_pending(env, &pending);
  if (s != napi_ok) return s;
  *result = pending ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_get_and_clear_last_exception(SnapiEnvState* env_state, uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value result;
  napi_status s = napi_get_and_clear_last_exception(env, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

// ============================================================
// Symbol
// ============================================================

extern "C" int snapi_bridge_create_symbol(SnapiEnvState* env_state, uint32_t description_id,
                                          uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value description = LoadValue(*bridge_state, description_id);  // can be null (0)
  napi_value result;
  napi_status s = napi_create_symbol(env, description, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

// ============================================================
// BigInt
// ============================================================

extern "C" int snapi_bridge_create_bigint_int64(SnapiEnvState* env_state, int64_t value,
                                                uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value result;
  napi_status s = napi_create_bigint_int64(env, value, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_create_bigint_uint64(SnapiEnvState* env_state, uint64_t value,
                                                 uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value result;
  napi_status s = napi_create_bigint_uint64(env, value, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_get_value_bigint_int64(SnapiEnvState* env_state, uint32_t id, int64_t* value,
                                                   int* lossless) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  bool loss;
  napi_status s = napi_get_value_bigint_int64(env, val, value, &loss);
  if (s != napi_ok) return s;
  *lossless = loss ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_get_value_bigint_uint64(SnapiEnvState* env_state, uint32_t id,
                                                    uint64_t* value,
                                                    int* lossless) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  bool loss;
  napi_status s = napi_get_value_bigint_uint64(env, val, value, &loss);
  if (s != napi_ok) return s;
  *lossless = loss ? 1 : 0;
  return napi_ok;
}

// ============================================================
// Date
// ============================================================

extern "C" int snapi_bridge_create_date(SnapiEnvState* env_state, double time, uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value result;
  napi_status s = napi_create_date(env, time, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_get_date_value(SnapiEnvState* env_state, uint32_t id, double* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  return napi_get_date_value(env, val, result);
}

// ============================================================
// Promise
// ============================================================

extern "C" int snapi_bridge_create_promise(SnapiEnvState* env_state, uint32_t* deferred_out,
                                           uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_deferred deferred;
  napi_value promise;
  napi_status s = napi_create_promise(env, &deferred, &promise);
  if (s != napi_ok) return s;
  *deferred_out = StoreDeferred(*bridge_state, deferred);
  *out_id = StoreValue(*bridge_state, promise);
  return napi_ok;
}

extern "C" int snapi_bridge_resolve_deferred(SnapiEnvState* env_state, uint32_t deferred_id,
                                             uint32_t value_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_deferred d = LoadDeferred(*bridge_state, deferred_id);
  napi_value val = LoadValue(*bridge_state, value_id);
  if (!d || !val) return napi_invalid_arg;
  napi_status s = napi_resolve_deferred(env, d, val);
  if (s == napi_ok) RemoveDeferred(*bridge_state, deferred_id);
  return s;
}

extern "C" int snapi_bridge_reject_deferred(SnapiEnvState* env_state, uint32_t deferred_id,
                                            uint32_t value_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_deferred d = LoadDeferred(*bridge_state, deferred_id);
  napi_value val = LoadValue(*bridge_state, value_id);
  if (!d || !val) return napi_invalid_arg;
  napi_status s = napi_reject_deferred(env, d, val);
  if (s == napi_ok) RemoveDeferred(*bridge_state, deferred_id);
  return s;
}

// ============================================================
// ArrayBuffer
// ============================================================

extern "C" int snapi_bridge_create_arraybuffer(SnapiEnvState* env_state, uint32_t byte_length,
                                               uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  void* data;
  napi_value result;
  napi_status s =
      napi_create_arraybuffer(env, (size_t)byte_length, &data, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_create_external_arraybuffer(SnapiEnvState* env_state, uint64_t data_addr,
                                                        uint32_t byte_length,
                                                        uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  void* data = (void*)(uintptr_t)data_addr;
  napi_value result;
  napi_status s = napi_create_external_arraybuffer(
      env, data, (size_t)byte_length, nullptr, nullptr, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_create_external_buffer(SnapiEnvState* env_state, uint64_t data_addr,
                                                   uint32_t byte_length,
                                                   uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  void* data = (void*)(uintptr_t)data_addr;
  napi_value result;
  napi_status s = napi_create_external_buffer(
      env, (size_t)byte_length, data, nullptr, nullptr, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_is_sharedarraybuffer(SnapiEnvState* env_state, uint32_t id, int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  bool is_sab = false;
  napi_status s = node_api_is_sharedarraybuffer(env, val, &is_sab);
  if (s != napi_ok) return s;
  *result = is_sab ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_create_sharedarraybuffer(SnapiEnvState* env_state, uint32_t byte_length,
                                                     uint64_t* data_out,
                                                     uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  void* data = nullptr;
  napi_value result;
  napi_status s =
      node_api_create_sharedarraybuffer(env, (size_t)byte_length, &data, &result);
  if (s != napi_ok) return s;
  if (data_out) *data_out = (uint64_t)(uintptr_t)data;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_node_api_set_prototype(SnapiEnvState* env_state, uint32_t object_id,
                                                   uint32_t prototype_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value object = LoadValue(*bridge_state, object_id);
  napi_value prototype = LoadValue(*bridge_state, prototype_id);
  if (!object || !prototype) return napi_invalid_arg;
  return node_api_set_prototype(env, object, prototype);
}

extern "C" int snapi_bridge_get_arraybuffer_info(SnapiEnvState* env_state, uint32_t id,
                                                 uint64_t* data_out,
                                                 uint32_t* byte_length) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  void* data;
  size_t len;
  napi_status s = napi_get_arraybuffer_info(env, val, &data, &len);
  if (s != napi_ok) return s;
  if (data_out) *data_out = (uint64_t)(uintptr_t)data;
  *byte_length = (uint32_t)len;
  return napi_ok;
}

extern "C" int snapi_bridge_detach_arraybuffer(SnapiEnvState* env_state, uint32_t id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  return napi_detach_arraybuffer(env, val);
}

extern "C" int snapi_bridge_is_detached_arraybuffer(SnapiEnvState* env_state, uint32_t id, int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  bool is;
  napi_status s = napi_is_detached_arraybuffer(env, val, &is);
  if (s != napi_ok) return s;
  *result = is ? 1 : 0;
  return napi_ok;
}

// ============================================================
// TypedArray
// ============================================================

extern "C" int snapi_bridge_create_typedarray(SnapiEnvState* env_state, int type, uint32_t length,
                                              uint32_t arraybuffer_id,
                                              uint32_t byte_offset,
                                              uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value arraybuffer = LoadValue(*bridge_state, arraybuffer_id);
  if (!arraybuffer) return napi_invalid_arg;
  napi_value result;
  napi_status s = napi_create_typedarray(
      env, (napi_typedarray_type)type, (size_t)length, arraybuffer,
      (size_t)byte_offset, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_get_typedarray_info(SnapiEnvState* env_state, uint32_t id, int* type_out,
                                                uint32_t* length_out,
                                                uint64_t* data_out,
                                                uint32_t* arraybuffer_out,
                                                uint32_t* byte_offset_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  napi_typedarray_type type;
  size_t length;
  void* data = nullptr;
  napi_value arraybuffer;
  size_t byte_offset;
  napi_status s = napi_get_typedarray_info(env, val, &type, &length, &data,
                                           &arraybuffer, &byte_offset);
  if (s != napi_ok) return s;
  if (type_out) *type_out = (int)type;
  if (length_out) *length_out = (uint32_t)length;
  if (data_out) *data_out = (uint64_t)(uintptr_t)data;
  if (arraybuffer_out) *arraybuffer_out = StoreValue(*bridge_state, arraybuffer);
  if (byte_offset_out) *byte_offset_out = (uint32_t)byte_offset;
  return napi_ok;
}

// ============================================================
// DataView
// ============================================================

extern "C" int snapi_bridge_create_dataview(SnapiEnvState* env_state, uint32_t byte_length,
                                            uint32_t arraybuffer_id,
                                            uint32_t byte_offset,
                                            uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value arraybuffer = LoadValue(*bridge_state, arraybuffer_id);
  if (!arraybuffer) return napi_invalid_arg;
  napi_value result;
  napi_status s = napi_create_dataview(env, (size_t)byte_length, arraybuffer,
                                       (size_t)byte_offset, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_get_dataview_info(SnapiEnvState* env_state, uint32_t id,
                                              uint32_t* byte_length_out,
                                              uint64_t* data_out,
                                              uint32_t* arraybuffer_out,
                                              uint32_t* byte_offset_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  size_t byte_length;
  void* data = nullptr;
  napi_value arraybuffer;
  size_t byte_offset;
  napi_status s = napi_get_dataview_info(env, val, &byte_length, &data,
                                         &arraybuffer, &byte_offset);
  if (s != napi_ok) return s;
  if (byte_length_out) *byte_length_out = (uint32_t)byte_length;
  if (data_out) *data_out = (uint64_t)(uintptr_t)data;
  if (arraybuffer_out) *arraybuffer_out = StoreValue(*bridge_state, arraybuffer);
  if (byte_offset_out) *byte_offset_out = (uint32_t)byte_offset;
  return napi_ok;
}

// ============================================================
// External values
// ============================================================

extern "C" int snapi_bridge_create_external(SnapiEnvState* env_state, uint64_t data_val,
                                            uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  // Store arbitrary u64 data value as a void*. No finalizer.
  napi_value result;
  napi_status s = napi_create_external(env, (void*)(uintptr_t)data_val,
                                       nullptr, nullptr, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_get_value_external(SnapiEnvState* env_state, uint32_t id,
                                               uint64_t* data_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  void* data;
  napi_status s = napi_get_value_external(env, val, &data);
  if (s != napi_ok) return s;
  *data_out = (uint64_t)(uintptr_t)data;
  return napi_ok;
}

// ============================================================
// References
// ============================================================

extern "C" int snapi_bridge_create_reference(SnapiEnvState* env_state, uint32_t value_id,
                                             uint32_t initial_refcount,
                                             uint32_t* ref_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, value_id);
  if (!val) return napi_invalid_arg;
  napi_ref ref;
  napi_status s = napi_create_reference(env, val, initial_refcount, &ref);
  if (s != napi_ok) return s;
  *ref_out = StoreRef(*bridge_state, ref);
  return napi_ok;
}

extern "C" int snapi_bridge_delete_reference(SnapiEnvState* env_state, uint32_t ref_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_ref ref = LoadRef(*bridge_state, ref_id);
  if (!ref) return napi_invalid_arg;
  napi_status s = napi_delete_reference(env, ref);
  if (s == napi_ok) RemoveRef(*bridge_state, ref_id);
  return s;
}

extern "C" int snapi_bridge_reference_ref(SnapiEnvState* env_state, uint32_t ref_id, uint32_t* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_ref ref = LoadRef(*bridge_state, ref_id);
  if (!ref) return napi_invalid_arg;
  return napi_reference_ref(env, ref, result);
}

extern "C" int snapi_bridge_reference_unref(SnapiEnvState* env_state, uint32_t ref_id, uint32_t* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_ref ref = LoadRef(*bridge_state, ref_id);
  if (!ref) return napi_invalid_arg;
  return napi_reference_unref(env, ref, result);
}

extern "C" int snapi_bridge_get_reference_value(SnapiEnvState* env_state, uint32_t ref_id,
                                                uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_ref ref = LoadRef(*bridge_state, ref_id);
  if (!ref) return napi_invalid_arg;
  napi_value result;
  napi_status s = napi_get_reference_value(env, ref, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

// ============================================================
// Handle scopes (escapable)
// ============================================================

extern "C" int snapi_bridge_open_escapable_handle_scope(SnapiEnvState* env_state, uint32_t* scope_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_escapable_handle_scope scope;
  napi_status s = napi_open_escapable_handle_scope(env, &scope);
  if (s != napi_ok) return s;
  *scope_out = StoreEscScope(*bridge_state, scope);
  return napi_ok;
}

extern "C" int snapi_bridge_close_escapable_handle_scope(SnapiEnvState* env_state, uint32_t scope_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_escapable_handle_scope scope = LoadEscScope(*bridge_state, scope_id);
  if (!scope) return napi_invalid_arg;
  napi_status s = napi_close_escapable_handle_scope(env, scope);
  if (s == napi_ok) RemoveEscScope(*bridge_state, scope_id);
  return s;
}

extern "C" int snapi_bridge_escape_handle(SnapiEnvState* env_state, uint32_t scope_id,
                                          uint32_t escapee_id,
                                          uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_escapable_handle_scope scope = LoadEscScope(*bridge_state, scope_id);
  napi_value escapee = LoadValue(*bridge_state, escapee_id);
  if (!scope || !escapee) return napi_invalid_arg;
  napi_value result;
  napi_status s = napi_escape_handle(env, scope, escapee, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

// ============================================================
// Type tagging
// ============================================================

extern "C" int snapi_bridge_type_tag_object(SnapiEnvState* env_state, uint32_t obj_id,
                                            uint64_t tag_lower,
                                            uint64_t tag_upper) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  if (!obj) return napi_invalid_arg;
  napi_type_tag tag;
  tag.lower = tag_lower;
  tag.upper = tag_upper;
  return napi_type_tag_object(env, obj, &tag);
}

extern "C" int snapi_bridge_check_object_type_tag(SnapiEnvState* env_state, uint32_t obj_id,
                                                  uint64_t tag_lower,
                                                  uint64_t tag_upper,
                                                  int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  if (!obj) return napi_invalid_arg;
  napi_type_tag tag;
  tag.lower = tag_lower;
  tag.upper = tag_upper;
  bool matches;
  napi_status s = napi_check_object_type_tag(env, obj, &tag, &matches);
  if (s != napi_ok) return s;
  *result = matches ? 1 : 0;
  return napi_ok;
}

// ============================================================
// Function calling (call JS functions from native)
// ============================================================

extern "C" int snapi_bridge_call_function(SnapiEnvState* env_state, uint32_t recv_id, uint32_t func_id,
                                          uint32_t argc,
                                          const uint32_t* argv_ids,
                                          uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value recv = LoadValue(*bridge_state, recv_id);
  napi_value func = LoadValue(*bridge_state, func_id);
  if (!recv || !func) return napi_invalid_arg;
  std::vector<napi_value> argv(argc);
  for (uint32_t i = 0; i < argc; i++) {
    argv[i] = LoadValue(*bridge_state, argv_ids[i]);
    if (!argv[i]) return napi_invalid_arg;
  }
  napi_value result;
  napi_status s =
      napi_call_function(env, recv, func, argc, argv.data(), &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

// ============================================================
// Script execution
// ============================================================

extern "C" int snapi_bridge_run_script(SnapiEnvState* env_state, uint32_t script_id,
                                       uint32_t* out_value_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value script_val = LoadValue(*bridge_state, script_id);
  if (!script_val) return napi_invalid_arg;
  napi_value result;
  napi_status s = napi_run_script(env, script_val, &result);
  if (s != napi_ok) return s;
  *out_value_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

// ============================================================
// UTF-16 strings
// ============================================================

extern "C" int snapi_bridge_create_string_utf16(SnapiEnvState* env_state, const uint16_t* str,
                                                uint32_t wasm_length,
                                                uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  size_t length =
      (wasm_length == 0xFFFFFFFFu) ? NAPI_AUTO_LENGTH : (size_t)wasm_length;
  napi_value result;
  napi_status s = napi_create_string_utf16(env, (const char16_t*)str, length,
                                           &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_get_value_string_utf16(SnapiEnvState* env_state, uint32_t id,
                                                   uint16_t* buf,
                                                   size_t bufsize,
                                                   size_t* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  return napi_get_value_string_utf16(env, val, (char16_t*)buf, bufsize,
                                     result);
}

// ============================================================
// BigInt words (arbitrary precision)
// ============================================================

extern "C" int snapi_bridge_create_bigint_words(SnapiEnvState* env_state, int sign_bit,
                                                uint32_t word_count,
                                                const uint64_t* words,
                                                uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value result;
  napi_status s = napi_create_bigint_words(env, sign_bit, (size_t)word_count,
                                           words, &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_get_value_bigint_words(SnapiEnvState* env_state, uint32_t id,
                                                   int* sign_bit,
                                                   size_t* word_count,
                                                   uint64_t* words) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  return napi_get_value_bigint_words(env, val, sign_bit, word_count, words);
}

// ============================================================
// Instance data
// ============================================================

extern "C" int snapi_bridge_set_instance_data(SnapiEnvState* env_state, uint64_t data_val) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  return napi_set_instance_data(env, (void*)(uintptr_t)data_val,
                                nullptr, nullptr);
}

extern "C" int snapi_bridge_get_instance_data(SnapiEnvState* env_state, uint64_t* data_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  void* data = nullptr;
  napi_status s = napi_get_instance_data(env, &data);
  if (s != napi_ok) return s;
  *data_out = (uint64_t)(uintptr_t)data;
  return napi_ok;
}

extern "C" int snapi_bridge_adjust_external_memory(SnapiEnvState* env_state, int64_t change,
                                                   int64_t* adjusted) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  return napi_adjust_external_memory(env, change, adjusted);
}

// ============================================================
// Node Buffers
// ============================================================

extern "C" int snapi_bridge_create_buffer(SnapiEnvState* env_state, uint32_t length,
                                          uint64_t* data_out,
                                          uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value buffer;
  void* data = nullptr;
  napi_status s = napi_create_buffer(env, (size_t)length, &data, &buffer);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, buffer);
  if (data_out) *data_out = (uint64_t)(uintptr_t)data;
  return napi_ok;
}

extern "C" int snapi_bridge_create_buffer_copy(SnapiEnvState* env_state, uint32_t length,
                                               const void* src_data,
                                               uint64_t* result_data_out,
                                               uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value buffer;
  void* result_data = nullptr;
  napi_status s = napi_create_buffer_copy(env, (size_t)length, src_data,
                                          &result_data, &buffer);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, buffer);
  if (result_data_out) *result_data_out = (uint64_t)(uintptr_t)result_data;
  return napi_ok;
}

extern "C" int snapi_bridge_is_buffer(SnapiEnvState* env_state, uint32_t id, int* result) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  bool is_buffer = false;
  napi_status s = napi_is_buffer(env, val, &is_buffer);
  if (s != napi_ok) return s;
  *result = is_buffer ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_get_buffer_info(SnapiEnvState* env_state, uint32_t id,
                                            uint64_t* data_out,
                                            uint32_t* length_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value val = LoadValue(*bridge_state, id);
  if (!val) return napi_invalid_arg;
  void* data = nullptr;
  size_t length = 0;
  napi_status s = napi_get_buffer_info(env, val, &data, &length);
  if (s != napi_ok) return s;
  if (length_out) *length_out = (uint32_t)length;
  if (data_out) *data_out = (uint64_t)(uintptr_t)data;
  return napi_ok;
}

// ============================================================
// Node version (stub — we're not running in Node, return fake version)
// ============================================================

extern "C" int snapi_bridge_get_node_version(SnapiEnvState* env_state, uint32_t* major,
                                             uint32_t* minor,
                                             uint32_t* patch) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  // Return a reasonable fake version since we're running on pure V8
  if (major) *major = 22;
  if (minor) *minor = 0;
  if (patch) *patch = 0;
  return napi_ok;
}

// ============================================================
// Object wrapping
// ============================================================

extern "C" int snapi_bridge_wrap(SnapiEnvState* env_state, uint32_t obj_id, uint64_t native_data,
                                 uint32_t* ref_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  if (!obj) return napi_invalid_arg;
  napi_ref ref = nullptr;
  napi_status s = napi_wrap(env, obj, (void*)(uintptr_t)native_data,
                            nullptr, nullptr, ref_out ? &ref : nullptr);
  if (s != napi_ok) return s;
  if (ref_out) *ref_out = StoreRef(*bridge_state, ref);
  return napi_ok;
}

extern "C" int snapi_bridge_unwrap(SnapiEnvState* env_state, uint32_t obj_id, uint64_t* data_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  if (!obj) return napi_invalid_arg;
  void* data = nullptr;
  napi_status s = napi_unwrap(env, obj, &data);
  if (s != napi_ok) return s;
  if (data_out) *data_out = (uint64_t)(uintptr_t)data;
  return napi_ok;
}

extern "C" int snapi_bridge_remove_wrap(SnapiEnvState* env_state, uint32_t obj_id, uint64_t* data_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  if (!obj) return napi_invalid_arg;
  void* data = nullptr;
  napi_status s = napi_remove_wrap(env, obj, &data);
  if (s != napi_ok) return s;
  if (data_out) *data_out = (uint64_t)(uintptr_t)data;
  return napi_ok;
}

extern "C" int snapi_bridge_add_finalizer(SnapiEnvState* env_state, uint32_t obj_id, uint64_t data_val,
                                          uint32_t* ref_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  if (!obj) return napi_invalid_arg;
  // No actual WASM callback for finalizer; just register with nullptr callback
  napi_ref ref = nullptr;
  napi_status s = napi_add_finalizer(env, obj, (void*)(uintptr_t)data_val,
                                     nullptr, nullptr, ref_out ? &ref : nullptr);
  if (s != napi_ok) return s;
  if (ref_out) *ref_out = StoreRef(*bridge_state, ref);
  return napi_ok;
}

// ============================================================
// napi_new_instance
// ============================================================

extern "C" int snapi_bridge_new_instance(SnapiEnvState* env_state, uint32_t ctor_id, uint32_t argc,
                                         const uint32_t* argv_ids,
                                         uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value ctor = LoadValue(*bridge_state, ctor_id);
  if (!ctor) return napi_invalid_arg;
  std::vector<napi_value> argv(argc);
  for (uint32_t i = 0; i < argc; i++) {
    argv[i] = LoadValue(*bridge_state, argv_ids[i]);
    if (!argv[i]) return napi_invalid_arg;
  }
  napi_value result;
  napi_status s = napi_new_instance(env, ctor, argc, argv.data(), &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

// ============================================================
// napi_define_properties
// ============================================================

// Forward declaration (defined below in callback system section)
static napi_value generic_wasm_callback(napi_env env, napi_callback_info info);

extern "C" int snapi_bridge_define_properties(SnapiEnvState* env_state, uint32_t obj_id,
                                              uint32_t prop_count,
                                              const char** utf8names,
                                              const uint32_t* name_ids,
                                              const uint32_t* prop_types,
                                              const uint32_t* value_ids,
                                              const uint32_t* method_reg_ids,
                                              const uint32_t* getter_reg_ids,
                                              const uint32_t* setter_reg_ids,
                                              const int32_t* attributes) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  napi_value obj = LoadValue(*bridge_state, obj_id);
  if (!obj) return napi_invalid_arg;
  std::vector<napi_property_descriptor> descs(prop_count);
  for (uint32_t i = 0; i < prop_count; i++) {
    memset(&descs[i], 0, sizeof(napi_property_descriptor));
    descs[i].utf8name = utf8names != nullptr ? utf8names[i] : nullptr;
    descs[i].name = (name_ids != nullptr && name_ids[i] != 0) ? LoadValue(*bridge_state, name_ids[i]) : nullptr;
    descs[i].attributes = (napi_property_attributes)attributes[i];

    switch (prop_types[i]) {
      case 0:
        descs[i].value = LoadValue(*bridge_state, value_ids[i]);
        break;
      case 1:
        descs[i].method = generic_wasm_callback;
        descs[i].data = RegisterCallbackBinding(bridge_state, method_reg_ids[i]);
        break;
      case 2:
        descs[i].getter = generic_wasm_callback;
        descs[i].data = RegisterCallbackBinding(bridge_state, getter_reg_ids[i]);
        break;
      case 3:
        descs[i].setter = generic_wasm_callback;
        descs[i].data = RegisterCallbackBinding(bridge_state, setter_reg_ids[i]);
        break;
      case 4:
        descs[i].getter = generic_wasm_callback;
        descs[i].setter = generic_wasm_callback;
        descs[i].data = RegisterCallbackBinding(bridge_state, getter_reg_ids[i]);
        break;
    }
  }
  return napi_define_properties(env, obj, prop_count, descs.data());
}

// ============================================================
// napi_define_class
// ============================================================

// Property descriptor layout passed from Rust:
// For each property (i), we pass:
//   utf8names[i]   - property name (C string)
//   types[i]       - 0=value, 1=method, 2=getter, 3=setter, 4=getter+setter
//   value_ids[i]   - if type==0, the value handle ID
//   method_reg_ids[i]  - if type==1, the callback reg_id for the method
//   getter_reg_ids[i]  - if type==2 or 4, the callback reg_id for getter
//   setter_reg_ids[i]  - if type==3 or 4, the callback reg_id for setter
//   attributes[i]  - napi_property_attributes

extern "C" int snapi_bridge_define_class(SnapiEnvState* env_state, 
    const char* utf8name, uint32_t name_len,
    uint32_t ctor_reg_id,
    uint32_t prop_count,
    const char** prop_names,
    const uint32_t* prop_name_ids,
    const uint32_t* prop_types,
    const uint32_t* prop_value_ids,
    const uint32_t* prop_method_reg_ids,
    const uint32_t* prop_getter_reg_ids,
    const uint32_t* prop_setter_reg_ids,
    const int32_t* prop_attributes,
    uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;

  // Build property descriptors
  std::vector<napi_property_descriptor> descs(prop_count);
  for (uint32_t i = 0; i < prop_count; i++) {
    memset(&descs[i], 0, sizeof(napi_property_descriptor));
    descs[i].utf8name = prop_names != nullptr ? prop_names[i] : nullptr;
    descs[i].name = (prop_name_ids != nullptr && prop_name_ids[i] != 0) ? LoadValue(*bridge_state, prop_name_ids[i]) : nullptr;
    descs[i].attributes = (napi_property_attributes)prop_attributes[i];

    switch (prop_types[i]) {
      case 0: // value
        descs[i].value = LoadValue(*bridge_state, prop_value_ids[i]);
        break;
      case 1: // method
        descs[i].method = generic_wasm_callback;
        descs[i].data = RegisterCallbackBinding(bridge_state, prop_method_reg_ids[i]);
        break;
      case 2: // getter only
        descs[i].getter = generic_wasm_callback;
        descs[i].data = RegisterCallbackBinding(bridge_state, prop_getter_reg_ids[i]);
        break;
      case 3: // setter only
        descs[i].setter = generic_wasm_callback;
        descs[i].data = RegisterCallbackBinding(bridge_state, prop_setter_reg_ids[i]);
        break;
      case 4: // getter + setter
        descs[i].getter = generic_wasm_callback;
        descs[i].setter = generic_wasm_callback;
        descs[i].data = RegisterCallbackBinding(bridge_state, prop_getter_reg_ids[i]);
        // Note: N-API uses the same data pointer for both getter and setter.
        // The setter_reg_id is stored in the getter_reg_id for now.
        break;
    }
  }

  napi_value result;
  napi_status s = napi_define_class(
      env, utf8name,
      name_len == 0xFFFFFFFFu ? NAPI_AUTO_LENGTH : (size_t)name_len,
      generic_wasm_callback,
      RegisterCallbackBinding(bridge_state, ctor_reg_id),
      prop_count, descs.data(),
      &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

// ============================================================
// Callback system (napi_create_function + napi_get_cb_info)
// ============================================================

extern "C" void snapi_bridge_set_cb_context(uint32_t this_id, uint32_t argc,
                                            const uint32_t* argv_ids,
                                            uint64_t data_val) {
  CbContext ctx;
  ctx.this_id = this_id;
  ctx.argc = argc;
  for (uint32_t i = 0; i < argc && i < 64; i++) ctx.argv_ids[i] = argv_ids[i];
  ctx.data_val = data_val;
  ctx.original_info = nullptr;
}

extern "C" void snapi_bridge_clear_cb_context() {
}

extern "C" int snapi_bridge_get_cb_info(SnapiEnvState* env_state, uint32_t* argc_ptr, uint32_t* argv_out,
                                        uint32_t max_argv,
                                        uint32_t* this_out, uint64_t* data_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  if (bridge_state->cb_stack.empty()) return napi_generic_failure;
  const CbContext& ctx = bridge_state->cb_stack.back();
  uint32_t actual = ctx.argc;
  uint32_t wanted = *argc_ptr;
  *argc_ptr = actual;
  if (this_out) *this_out = ctx.this_id;
  if (data_out) *data_out = ctx.data_val;
  uint32_t to_copy = (wanted < actual) ? wanted : actual;
  if (argv_out) {
    for (uint32_t i = 0; i < to_copy; i++) argv_out[i] = ctx.argv_ids[i];
  }
  return napi_ok;
}

// napi_get_new_target — only valid inside a constructor callback
extern "C" int snapi_bridge_get_new_target(SnapiEnvState* env_state, uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  if (bridge_state->cb_stack.empty()) return napi_generic_failure;
  const CbContext& ctx = bridge_state->cb_stack.back();
  if (!ctx.original_info) {
    // Not inside a V8-triggered callback (shouldn't happen with trampoline)
    *out_id = 0;
    return napi_ok;
  }
  napi_value result;
  napi_status s = napi_get_new_target(env, ctx.original_info, &result);
  if (s != napi_ok) return s;
  *out_id = result ? StoreValue(*bridge_state, result) : 0;
  return napi_ok;
}

// Forward-declare the Rust trampoline (defined in lib.rs via #[no_mangle] extern "C")
extern "C" uint32_t snapi_host_invoke_wasm_callback(SnapiEnvState* env_state,
                                                    uint32_t wasm_fn_ptr,
                                                    uint64_t data_val);

struct WasmInterruptRequest {
  SnapiEnvState* state;
  uint32_t wasm_fn_ptr;
  uint32_t data_val;
};

void WasmInterruptCallback(napi_env /*env*/, void* raw) {
  auto* request = static_cast<WasmInterruptRequest*>(raw);
  if (request == nullptr) return;
  snapi_host_invoke_wasm_callback(request->state, request->wasm_fn_ptr, request->data_val);
  delete request;
}

// Generic C++ callback invoked by V8 for all napi_create_function functions.
// Stores the V8 call args in the context stack, then calls the Rust trampoline
// which dispatches to the WASM callback.
static napi_value generic_wasm_callback(napi_env env, napi_callback_info info) {
  void* raw_data;
  size_t argc = 64;
  napi_value argv[64];
  napi_value this_arg;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, &raw_data);

  auto* binding = static_cast<CallbackBinding*>(raw_data);
  if (binding == nullptr) {
    napi_value undef;
    napi_get_undefined(env, &undef);
    return undef;
  }
  auto* bridge_state = LookupEnvState(binding->state);
  if (bridge_state == nullptr) {
    napi_value undef;
    napi_get_undefined(env, &undef);
    return undef;
  }
  auto it = bridge_state->cb_registry.find(binding->reg_id);
  if (it == bridge_state->cb_registry.end()) {
    napi_value undef;
    napi_get_undefined(env, &undef);
    return undef;
  }

  // Push context onto stack
  CbContext ctx;
  ctx.this_id = StoreValue(*bridge_state, this_arg);
  ctx.argc = (uint32_t)argc;
  for (uint32_t i = 0; i < argc && i < 64; i++) {
    ctx.argv_ids[i] = StoreValue(*bridge_state, argv[i]);
  }
  ctx.data_val = it->second.data_val;
  ctx.original_info = info;  // Store for napi_get_new_target
  bridge_state->cb_stack.push_back(ctx);

  // Call Rust trampoline → WASM callback
  const uint32_t wasm_fn_ptr =
      (it->second.wasm_setter_fn_ptr != 0 && argc > 0)
          ? it->second.wasm_setter_fn_ptr
          : it->second.wasm_fn_ptr;

  uint32_t result_id =
      snapi_host_invoke_wasm_callback(bridge_state, wasm_fn_ptr, it->second.data_val);

  bridge_state->cb_stack.pop_back();

  napi_value result = LoadValue(*bridge_state, result_id);
  if (!result) {
    napi_get_undefined(env, &result);
  }
  return result;
}

// Allocate a registration ID for a new callback
extern "C" uint32_t snapi_bridge_alloc_cb_reg_id(SnapiEnvState* env_state) {
  if (env_state == nullptr) return 0;
  return env_state->next_cb_reg_id++;
}

// Register callback data for a registration ID
extern "C" void snapi_bridge_register_callback(SnapiEnvState* env_state,
                                               uint32_t reg_id,
                                               uint32_t wasm_fn_ptr,
                                               uint64_t data_val) {
  if (env_state == nullptr || reg_id == 0) return;
  env_state->cb_registry[reg_id] = { wasm_fn_ptr, 0, data_val };
}

extern "C" void snapi_bridge_register_callback_pair(SnapiEnvState* env_state,
                                                    uint32_t reg_id,
                                                    uint32_t wasm_getter_fn_ptr,
                                                    uint32_t wasm_setter_fn_ptr,
                                                    uint64_t data_val) {
  if (env_state == nullptr || reg_id == 0) return;
  env_state->cb_registry[reg_id] = { wasm_getter_fn_ptr, wasm_setter_fn_ptr, data_val };
}

// Create a JS function with generic_wasm_callback as its native callback.
// The reg_id is passed as the data pointer so the callback can look up
// which WASM function to invoke.
extern "C" int snapi_bridge_create_function(SnapiEnvState* env_state, const char* utf8name, uint32_t name_len,
                                            uint32_t reg_id,
                                            uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  CallbackBinding* binding = RegisterCallbackBinding(bridge_state, reg_id);
  napi_value result;
  napi_status s = napi_create_function(env, utf8name,
                                       name_len == 0xFFFFFFFFu ? NAPI_AUTO_LENGTH : (size_t)name_len,
                                       generic_wasm_callback,
                                       binding,
                                       &result);
  if (s != napi_ok) return s;
  *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_create_env(int32_t module_api_version,
                                                  SnapiEnvState** env_out) {
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_env env = nullptr;
  void* scope = nullptr;
  napi_status s = unofficial_napi_create_env(module_api_version, &env, &scope);
  if (s != napi_ok) return s;

  auto* state = new (std::nothrow) SnapiEnvState();
  if (state == nullptr) {
    (void)unofficial_napi_release_env(scope);
    return napi_generic_failure;
  }
  state->env = env;
  state->scope = scope;
  g_envs.insert(state);

  if (env_out != nullptr) *env_out = state;
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_create_env_with_options(
    int32_t module_api_version,
    uint32_t max_young_generation_size_in_bytes,
    uint32_t max_old_generation_size_in_bytes,
    uint32_t code_range_size_in_bytes,
    uint32_t /*stack_limit*/,
    SnapiEnvState** env_out) {
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  unofficial_napi_env_create_options options{};
  const bool has_constraints =
      max_young_generation_size_in_bytes > 0 ||
      max_old_generation_size_in_bytes > 0 ||
      code_range_size_in_bytes > 0;
  options.max_young_generation_size_in_bytes =
      max_young_generation_size_in_bytes;
  options.max_old_generation_size_in_bytes =
      max_old_generation_size_in_bytes;
  options.code_range_size_in_bytes = code_range_size_in_bytes;
  // The guest-provided stack limit is a Wasm linear-memory address, not a
  // native stack address for the host thread running V8.
  options.stack_limit = nullptr;

  napi_env env = nullptr;
  void* scope = nullptr;
  napi_status s = unofficial_napi_create_env_with_options(
      module_api_version, has_constraints ? &options : nullptr, &env, &scope);
  if (s != napi_ok) return s;

  auto* state = new (std::nothrow) SnapiEnvState();
  if (state == nullptr) {
    (void)unofficial_napi_release_env(scope);
    return napi_generic_failure;
  }
  state->env = env;
  state->scope = scope;
  g_envs.insert(state);

  if (env_out != nullptr) *env_out = state;
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_release_env(SnapiEnvState* env_state) {
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  return DisposeBridgeStateLocked(env_state);
}

extern "C" int snapi_bridge_unofficial_set_flags_from_string(const char* flags,
                                                             uint32_t length) {
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  return unofficial_napi_set_flags_from_string(flags, static_cast<size_t>(length));
}

extern "C" int snapi_bridge_unofficial_process_microtasks(SnapiEnvState* env_state) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  return unofficial_napi_process_microtasks(env);
}

extern "C" int snapi_bridge_unofficial_request_gc_for_testing(SnapiEnvState* env_state) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  return unofficial_napi_request_gc_for_testing(env);
}

extern "C" int snapi_bridge_unofficial_set_prepare_stack_trace_callback(
    SnapiEnvState* env_state,
    uint32_t callback_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value callback = callback_id == 0 ? nullptr : LoadValue(*bridge_state, callback_id);
  if (callback_id != 0 && callback == nullptr) return napi_invalid_arg;
  return unofficial_napi_set_prepare_stack_trace_callback(env, callback);
}

extern "C" int snapi_bridge_unofficial_get_promise_details(SnapiEnvState* env_state,
                                                           uint32_t promise_id,
                                                           int32_t* state_out,
                                                           uint32_t* result_out,
                                                           int* has_result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value promise = LoadValue(*bridge_state, promise_id);
  if (promise == nullptr) return napi_invalid_arg;
  napi_value result = nullptr;
  bool has_result = false;
  napi_status s =
      unofficial_napi_get_promise_details(env, promise, state_out, &result, &has_result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = StoreValue(*bridge_state, result);
  if (has_result_out != nullptr) *has_result_out = has_result ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_get_proxy_details(SnapiEnvState* env_state,
                                                         uint32_t proxy_id,
                                                         uint32_t* target_out,
                                                         uint32_t* handler_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value proxy = LoadValue(*bridge_state, proxy_id);
  if (proxy == nullptr) return napi_invalid_arg;
  napi_value target = nullptr;
  napi_value handler = nullptr;
  napi_status s = unofficial_napi_get_proxy_details(env, proxy, &target, &handler);
  if (s != napi_ok) return s;
  if (target_out != nullptr) *target_out = StoreValue(*bridge_state, target);
  if (handler_out != nullptr) *handler_out = StoreValue(*bridge_state, handler);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_preview_entries(SnapiEnvState* env_state,
                                                       uint32_t value_id,
                                                       uint32_t* entries_out,
                                                       int* is_key_value_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value value = LoadValue(*bridge_state, value_id);
  if (value == nullptr) return napi_invalid_arg;
  napi_value entries = nullptr;
  bool is_key_value = false;
  napi_status s = unofficial_napi_preview_entries(env, value, &entries, &is_key_value);
  if (s != napi_ok) return s;
  if (entries_out != nullptr) *entries_out = StoreValue(*bridge_state, entries);
  if (is_key_value_out != nullptr) *is_key_value_out = is_key_value ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_get_call_sites(SnapiEnvState* env_state,
                                                      uint32_t frames,
                                                      uint32_t* callsites_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value callsites = nullptr;
  napi_status s = unofficial_napi_get_call_sites(env, frames, &callsites);
  if (s != napi_ok) return s;
  if (callsites_out != nullptr) *callsites_out = StoreValue(*bridge_state, callsites);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_get_current_stack_trace(
    SnapiEnvState* env_state,
    uint32_t frames,
    uint32_t* callsites_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value callsites = nullptr;
  napi_status s = unofficial_napi_get_current_stack_trace(env, frames, &callsites);
  if (s != napi_ok) return s;
  if (callsites_out != nullptr) *callsites_out = StoreValue(*bridge_state, callsites);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_get_caller_location(SnapiEnvState* env_state,
                                                           uint32_t* location_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value location = nullptr;
  napi_status s = unofficial_napi_get_caller_location(env, &location);
  if (s != napi_ok) return s;
  if (location_out != nullptr) *location_out = StoreValue(*bridge_state, location);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_arraybuffer_view_has_buffer(SnapiEnvState* env_state,
                                                                   uint32_t value_id,
                                                                   int* result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value value = LoadValue(*bridge_state, value_id);
  if (value == nullptr) return napi_invalid_arg;
  bool result = false;
  napi_status s = unofficial_napi_arraybuffer_view_has_buffer(env, value, &result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = result ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_get_constructor_name(SnapiEnvState* env_state,
                                                            uint32_t value_id,
                                                            uint32_t* name_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value value = LoadValue(*bridge_state, value_id);
  if (value == nullptr) return napi_invalid_arg;
  napi_value name = nullptr;
  napi_status s = unofficial_napi_get_constructor_name(env, value, &name);
  if (s != napi_ok) return s;
  if (name_out != nullptr) *name_out = StoreValue(*bridge_state, name);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_create_private_symbol(SnapiEnvState* env_state,
                                                             const char* utf8description,
                                                             uint32_t wasm_length,
                                                             uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  size_t length =
      (wasm_length == 0xFFFFFFFFu) ? NAPI_AUTO_LENGTH : static_cast<size_t>(wasm_length);
  napi_value result = nullptr;
  napi_status s =
      unofficial_napi_create_private_symbol(env, utf8description, length, &result);
  if (s != napi_ok) return s;
  if (out_id != nullptr) *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_get_continuation_preserved_embedder_data(
    SnapiEnvState* env_state,
    uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value result = nullptr;
  napi_status s =
      unofficial_napi_get_continuation_preserved_embedder_data(env, &result);
  if (s != napi_ok) return s;
  if (out_id != nullptr) *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_set_continuation_preserved_embedder_data(
    SnapiEnvState* env_state,
    uint32_t value_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value value = value_id == 0 ? nullptr : LoadValue(*bridge_state, value_id);
  if (value_id != 0 && value == nullptr) return napi_invalid_arg;
  return unofficial_napi_set_continuation_preserved_embedder_data(env, value);
}

extern "C" int snapi_bridge_unofficial_set_enqueue_foreground_task_callback(
    SnapiEnvState* env_state) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  // Keep parity with the previous bridge behavior (no custom foreground task hook).
  // The runtime still drives microtasks via unofficial_napi_process_microtasks.
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_set_fatal_error_callbacks(
    SnapiEnvState* env_state,
    uint32_t /*fatal_callback_id*/,
    uint32_t /*oom_callback_id*/) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  return unofficial_napi_set_fatal_error_callbacks(env, nullptr, nullptr);
}

extern "C" int snapi_bridge_unofficial_terminate_execution(SnapiEnvState* env_state) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  return unofficial_napi_terminate_execution(env);
}

extern "C" int snapi_bridge_unofficial_cancel_terminate_execution(
    SnapiEnvState* env_state) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  return unofficial_napi_cancel_terminate_execution(env);
}

extern "C" int snapi_bridge_unofficial_request_interrupt(SnapiEnvState* env_state,
                                                         uint32_t callback_id,
                                                         uint32_t data) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  if (callback_id == 0) return napi_invalid_arg;
  auto* request = new (std::nothrow) WasmInterruptRequest{bridge_state, callback_id, data};
  if (request == nullptr) return napi_generic_failure;
  napi_status s =
      unofficial_napi_request_interrupt(env, WasmInterruptCallback, request);
  if (s != napi_ok) delete request;
  return s;
}

extern "C" int snapi_bridge_unofficial_enqueue_microtask(SnapiEnvState* env_state,
                                                         uint32_t callback_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value callback = LoadValue(*bridge_state, callback_id);
  if (callback == nullptr) return napi_invalid_arg;
  return unofficial_napi_enqueue_microtask(env, callback);
}

extern "C" int snapi_bridge_unofficial_set_promise_reject_callback(SnapiEnvState* env_state,
                                                                   uint32_t callback_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value callback = callback_id == 0 ? nullptr : LoadValue(*bridge_state, callback_id);
  if (callback_id != 0 && callback == nullptr) return napi_invalid_arg;
  return unofficial_napi_set_promise_reject_callback(env, callback);
}

extern "C" int snapi_bridge_unofficial_set_promise_hooks(
    SnapiEnvState* env_state,
    uint32_t init_callback_id,
    uint32_t before_callback_id,
    uint32_t after_callback_id,
    uint32_t resolve_callback_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value init = init_callback_id == 0 ? nullptr : LoadValue(*bridge_state, init_callback_id);
  napi_value before =
      before_callback_id == 0 ? nullptr : LoadValue(*bridge_state, before_callback_id);
  napi_value after = after_callback_id == 0 ? nullptr : LoadValue(*bridge_state, after_callback_id);
  napi_value resolve =
      resolve_callback_id == 0 ? nullptr : LoadValue(*bridge_state, resolve_callback_id);
  if ((init_callback_id != 0 && init == nullptr) ||
      (before_callback_id != 0 && before == nullptr) ||
      (after_callback_id != 0 && after == nullptr) ||
      (resolve_callback_id != 0 && resolve == nullptr)) {
    return napi_invalid_arg;
  }
  return unofficial_napi_set_promise_hooks(env, init, before, after, resolve);
}

extern "C" int snapi_bridge_unofficial_get_own_non_index_properties(
    SnapiEnvState* env_state,
    uint32_t value_id,
    uint32_t filter_bits,
    uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value value = LoadValue(*bridge_state, value_id);
  if (value == nullptr) return napi_invalid_arg;
  napi_value result = nullptr;
  napi_status s =
      unofficial_napi_get_own_non_index_properties(env, value, filter_bits, &result);
  if (s != napi_ok) return s;
  if (out_id != nullptr) *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_get_process_memory_info(
    SnapiEnvState* env_state,
    double* heap_total_out,
    double* heap_used_out,
    double* external_out,
    double* array_buffers_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  return unofficial_napi_get_process_memory_info(
      env, heap_total_out, heap_used_out, external_out, array_buffers_out);
}

extern "C" int snapi_bridge_unofficial_get_hash_seed(SnapiEnvState* env_state,
                                                      uint64_t* hash_seed_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  if (
      hash_seed_out == nullptr) {
    return napi_invalid_arg;
  }
  return unofficial_napi_get_hash_seed(env, hash_seed_out);
}

extern "C" int snapi_bridge_unofficial_get_error_source_positions(
    SnapiEnvState* env_state,
    uint32_t error_id,
    uint32_t* source_line_out,
    uint32_t* script_resource_name_out,
    int32_t* line_number_out,
    int32_t* start_column_out,
    int32_t* end_column_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value error = LoadValue(*bridge_state, error_id);
  if (error == nullptr) return napi_invalid_arg;
  unofficial_napi_error_source_positions positions{};
  napi_status s =
      unofficial_napi_get_error_source_positions(env, error, &positions);
  if (s != napi_ok) return s;
  if (source_line_out != nullptr) {
    *source_line_out = StoreValue(*bridge_state, positions.source_line);
  }
  if (script_resource_name_out != nullptr) {
    *script_resource_name_out =
        StoreValue(*bridge_state, positions.script_resource_name);
  }
  if (line_number_out != nullptr) *line_number_out = positions.line_number;
  if (start_column_out != nullptr) *start_column_out = positions.start_column;
  if (end_column_out != nullptr) *end_column_out = positions.end_column;
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_preserve_error_source_message(
    SnapiEnvState* env_state,
    uint32_t error_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value error = LoadValue(*bridge_state, error_id);
  if (error == nullptr) return napi_invalid_arg;
  return unofficial_napi_preserve_error_source_message(env, error);
}

extern "C" int snapi_bridge_unofficial_mark_promise_as_handled(
    SnapiEnvState* env_state,
    uint32_t promise_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value promise = LoadValue(*bridge_state, promise_id);
  if (promise == nullptr) return napi_invalid_arg;
  return unofficial_napi_mark_promise_as_handled(env, promise);
}

extern "C" int snapi_bridge_unofficial_get_heap_statistics(
    SnapiEnvState* env_state,
    unofficial_napi_heap_statistics* stats_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  if (
      stats_out == nullptr) {
    return napi_invalid_arg;
  }
  return unofficial_napi_get_heap_statistics(env, stats_out);
}

extern "C" int snapi_bridge_unofficial_get_heap_space_count(SnapiEnvState* env_state,
                                                            uint32_t* count_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  if (
      count_out == nullptr) {
    return napi_invalid_arg;
  }
  return unofficial_napi_get_heap_space_count(env, count_out);
}

extern "C" int snapi_bridge_unofficial_get_heap_space_statistics(
    SnapiEnvState* env_state,
    uint32_t space_index,
    unofficial_napi_heap_space_statistics* stats_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  if (
      stats_out == nullptr) {
    return napi_invalid_arg;
  }
  return unofficial_napi_get_heap_space_statistics(env, space_index, stats_out);
}

extern "C" int snapi_bridge_unofficial_get_heap_code_statistics(
    SnapiEnvState* env_state,
    unofficial_napi_heap_code_statistics* stats_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  if (
      stats_out == nullptr) {
    return napi_invalid_arg;
  }
  return unofficial_napi_get_heap_code_statistics(env, stats_out);
}

extern "C" int snapi_bridge_unofficial_start_cpu_profile(SnapiEnvState* env_state,
                                                         int32_t* result_out,
                                                         uint32_t* profile_id_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  if (
      result_out == nullptr || profile_id_out == nullptr) {
    return napi_invalid_arg;
  }
  unofficial_napi_cpu_profile_start_result result =
      unofficial_napi_cpu_profile_start_ok;
  napi_status s =
      unofficial_napi_start_cpu_profile(env, &result, profile_id_out);
  if (s != napi_ok) return s;
  *result_out = static_cast<int32_t>(result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_stop_cpu_profile(SnapiEnvState* env_state,
                                                        uint32_t profile_id,
                                                        int* found_out,
                                                        uint64_t* json_out,
                                                        uint32_t* json_len_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  if (
      found_out == nullptr || json_out == nullptr || json_len_out == nullptr) {
    return napi_invalid_arg;
  }
  bool found = false;
  char* json = nullptr;
  size_t json_len = 0;
  napi_status s = unofficial_napi_stop_cpu_profile(env, profile_id, &found, &json,
                                                   &json_len);
  if (s != napi_ok) return s;
  *found_out = found ? 1 : 0;
  *json_out = static_cast<uint64_t>(reinterpret_cast<uintptr_t>(json));
  *json_len_out = static_cast<uint32_t>(json_len);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_start_heap_profile(SnapiEnvState* env_state,
                                                          int* started_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  if (
      started_out == nullptr) {
    return napi_invalid_arg;
  }
  bool started = false;
  napi_status s = unofficial_napi_start_heap_profile(env, &started);
  if (s != napi_ok) return s;
  *started_out = started ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_stop_heap_profile(SnapiEnvState* env_state,
                                                         int* found_out,
                                                         uint64_t* json_out,
                                                         uint32_t* json_len_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  if (
      found_out == nullptr || json_out == nullptr || json_len_out == nullptr) {
    return napi_invalid_arg;
  }
  bool found = false;
  char* json = nullptr;
  size_t json_len = 0;
  napi_status s =
      unofficial_napi_stop_heap_profile(env, &found, &json, &json_len);
  if (s != napi_ok) return s;
  *found_out = found ? 1 : 0;
  *json_out = static_cast<uint64_t>(reinterpret_cast<uintptr_t>(json));
  *json_len_out = static_cast<uint32_t>(json_len);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_take_heap_snapshot(
    SnapiEnvState* env_state,
    int expose_internals,
    int expose_numeric_values,
    uint64_t* json_out,
    uint32_t* json_len_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  if (
      json_out == nullptr || json_len_out == nullptr) {
    return napi_invalid_arg;
  }
  unofficial_napi_heap_snapshot_options options{};
  options.expose_internals = expose_internals != 0;
  options.expose_numeric_values = expose_numeric_values != 0;
  char* json = nullptr;
  size_t json_len = 0;
  napi_status s =
      unofficial_napi_take_heap_snapshot(env, &options, &json, &json_len);
  if (s != napi_ok) return s;
  *json_out = static_cast<uint64_t>(reinterpret_cast<uintptr_t>(json));
  *json_len_out = static_cast<uint32_t>(json_len);
  return napi_ok;
}

extern "C" void snapi_bridge_unofficial_free_buffer(void* data) {
  if (data != nullptr) {
    unofficial_napi_free_buffer(data);
  }
}

extern "C" int snapi_bridge_unofficial_structured_clone(SnapiEnvState* env_state,
                                                        uint32_t value_id,
                                                        uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value value = LoadValue(*bridge_state, value_id);
  if (value == nullptr) return napi_invalid_arg;
  napi_value result = nullptr;
  napi_status s = unofficial_napi_structured_clone(env, value, &result);
  if (s != napi_ok) return s;
  if (out_id != nullptr) *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_notify_datetime_configuration_change(
    SnapiEnvState* env_state) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  return unofficial_napi_notify_datetime_configuration_change(env);
}

extern "C" int snapi_bridge_unofficial_create_serdes_binding(SnapiEnvState* env_state,
                                                             uint32_t* out_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value result = nullptr;
  napi_status s = unofficial_napi_create_serdes_binding(env, &result);
  if (s != napi_ok) return s;
  if (out_id != nullptr) *out_id = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_contextify_contains_module_syntax(
    SnapiEnvState* env_state,
    uint32_t code_id,
    uint32_t filename_id,
    uint32_t resource_name_id,
    int cjs_var_in_scope,
    int* result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value code = LoadValue(*bridge_state, code_id);
  napi_value filename = LoadValue(*bridge_state, filename_id);
  napi_value resource_name = resource_name_id == 0 ? nullptr : LoadValue(*bridge_state, resource_name_id);
  if (code == nullptr || filename == nullptr) return napi_invalid_arg;
  if (resource_name_id != 0 && resource_name == nullptr) return napi_invalid_arg;
  bool result = false;
  napi_status s = unofficial_napi_contextify_contains_module_syntax(
      env, code, filename, resource_name, cjs_var_in_scope != 0, &result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = result ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_contextify_make_context(
    SnapiEnvState* env_state,
    uint32_t sandbox_or_symbol_id,
    uint32_t name_id,
    uint32_t origin_id,
    int allow_code_gen_strings,
    int allow_code_gen_wasm,
    int own_microtask_queue,
    uint32_t host_defined_option_id,
    uint32_t* result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value sandbox_or_symbol = LoadValue(*bridge_state, sandbox_or_symbol_id);
  napi_value name = LoadValue(*bridge_state, name_id);
  napi_value origin = origin_id == 0 ? nullptr : LoadValue(*bridge_state, origin_id);
  napi_value host_defined_option =
      host_defined_option_id == 0 ? nullptr : LoadValue(*bridge_state, host_defined_option_id);
  if (sandbox_or_symbol == nullptr || name == nullptr) return napi_invalid_arg;
  if (origin_id != 0 && origin == nullptr) return napi_invalid_arg;
  if (host_defined_option_id != 0 && host_defined_option == nullptr) return napi_invalid_arg;
  napi_value result = nullptr;
  napi_status s = unofficial_napi_contextify_make_context(
      env,
      sandbox_or_symbol,
      name,
      origin,
      allow_code_gen_strings != 0,
      allow_code_gen_wasm != 0,
      own_microtask_queue != 0,
      host_defined_option,
      &result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_contextify_run_script(
    SnapiEnvState* env_state,
    uint32_t sandbox_or_null_id,
    uint32_t source_id,
    uint32_t filename_id,
    int32_t line_offset,
    int32_t column_offset,
    int64_t timeout,
    int display_errors,
    int break_on_sigint,
    int break_on_first_line,
    uint32_t host_defined_option_id,
    uint32_t* result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value sandbox_or_null = sandbox_or_null_id == 0 ? nullptr : LoadValue(*bridge_state, sandbox_or_null_id);
  napi_value source = LoadValue(*bridge_state, source_id);
  napi_value filename = LoadValue(*bridge_state, filename_id);
  napi_value host_defined_option =
      host_defined_option_id == 0 ? nullptr : LoadValue(*bridge_state, host_defined_option_id);
  if (sandbox_or_null_id != 0 && sandbox_or_null == nullptr) return napi_invalid_arg;
  if (source == nullptr || filename == nullptr) return napi_invalid_arg;
  if (host_defined_option_id != 0 && host_defined_option == nullptr) return napi_invalid_arg;
  napi_value result = nullptr;
  napi_status s = unofficial_napi_contextify_run_script(
      env,
      sandbox_or_null,
      source,
      filename,
      line_offset,
      column_offset,
      timeout,
      display_errors != 0,
      break_on_sigint != 0,
      break_on_first_line != 0,
      host_defined_option,
      &result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_contextify_dispose_context(
    SnapiEnvState* env_state,
    uint32_t sandbox_or_context_global_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value sandbox_or_context_global = LoadValue(*bridge_state, sandbox_or_context_global_id);
  if (sandbox_or_context_global == nullptr) return napi_invalid_arg;
  return unofficial_napi_contextify_dispose_context(env, sandbox_or_context_global);
}

extern "C" int snapi_bridge_unofficial_contextify_compile_function(
    SnapiEnvState* env_state,
    uint32_t code_id,
    uint32_t filename_id,
    int32_t line_offset,
    int32_t column_offset,
    uint32_t cached_data_id,
    int produce_cached_data,
    uint32_t parsing_context_id,
    uint32_t context_extensions_id,
    uint32_t params_id,
    uint32_t host_defined_option_id,
    uint32_t* result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value code = LoadValue(*bridge_state, code_id);
  napi_value filename = LoadValue(*bridge_state, filename_id);
  napi_value cached_data = cached_data_id == 0 ? nullptr : LoadValue(*bridge_state, cached_data_id);
  napi_value parsing_context = parsing_context_id == 0 ? nullptr : LoadValue(*bridge_state, parsing_context_id);
  napi_value context_extensions =
      context_extensions_id == 0 ? nullptr : LoadValue(*bridge_state, context_extensions_id);
  napi_value params = params_id == 0 ? nullptr : LoadValue(*bridge_state, params_id);
  napi_value host_defined_option =
      host_defined_option_id == 0 ? nullptr : LoadValue(*bridge_state, host_defined_option_id);
  if (code == nullptr || filename == nullptr) return napi_invalid_arg;
  if (cached_data_id != 0 && cached_data == nullptr) return napi_invalid_arg;
  if (parsing_context_id != 0 && parsing_context == nullptr) return napi_invalid_arg;
  if (context_extensions_id != 0 && context_extensions == nullptr) return napi_invalid_arg;
  if (params_id != 0 && params == nullptr) return napi_invalid_arg;
  if (host_defined_option_id != 0 && host_defined_option == nullptr) return napi_invalid_arg;
  napi_value result = nullptr;
  napi_status s = unofficial_napi_contextify_compile_function(
      env,
      code,
      filename,
      line_offset,
      column_offset,
      cached_data,
      produce_cached_data != 0,
      parsing_context,
      context_extensions,
      params,
      host_defined_option,
      &result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_contextify_compile_function_for_cjs_loader(
    SnapiEnvState* env_state,
    uint32_t code_id,
    uint32_t filename_id,
    int is_sea_main,
    int should_detect_module,
    uint32_t* result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value code = LoadValue(*bridge_state, code_id);
  napi_value filename = LoadValue(*bridge_state, filename_id);
  if (code == nullptr || filename == nullptr) return napi_invalid_arg;
  napi_value result = nullptr;
  napi_status s = unofficial_napi_contextify_compile_function_for_cjs_loader(
      env, code, filename, is_sea_main != 0, should_detect_module != 0, &result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_contextify_create_cached_data(
    SnapiEnvState* env_state,
    uint32_t code_id,
    uint32_t filename_id,
    int32_t line_offset,
    int32_t column_offset,
    uint32_t host_defined_option_id,
    uint32_t* result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value code = LoadValue(*bridge_state, code_id);
  napi_value filename = LoadValue(*bridge_state, filename_id);
  napi_value host_defined_option =
      host_defined_option_id == 0 ? nullptr : LoadValue(*bridge_state, host_defined_option_id);
  if (code == nullptr || filename == nullptr) return napi_invalid_arg;
  if (host_defined_option_id != 0 && host_defined_option == nullptr) return napi_invalid_arg;
  napi_value result = nullptr;
  napi_status s = unofficial_napi_contextify_create_cached_data(
      env,
      code,
      filename,
      line_offset,
      column_offset,
      host_defined_option,
      &result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_module_wrap_create_source_text(
    SnapiEnvState* env_state,
    uint32_t wrapper_id,
    uint32_t url_id,
    uint32_t context_id,
    uint32_t source_id,
    int32_t line_offset,
    int32_t column_offset,
    uint32_t cached_data_or_id,
    uint32_t* handle_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value wrapper = LoadValue(*bridge_state, wrapper_id);
  napi_value url = LoadValue(*bridge_state, url_id);
  napi_value context = context_id == 0 ? nullptr : LoadValue(*bridge_state, context_id);
  napi_value source = LoadValue(*bridge_state, source_id);
  napi_value cached_data = cached_data_or_id == 0 ? nullptr : LoadValue(*bridge_state, cached_data_or_id);
  if (wrapper == nullptr || url == nullptr || source == nullptr) return napi_invalid_arg;
  if (context_id != 0 && context == nullptr) return napi_invalid_arg;
  if (cached_data_or_id != 0 && cached_data == nullptr) return napi_invalid_arg;
  void* handle = nullptr;
  napi_status s = unofficial_napi_module_wrap_create_source_text(
      env, wrapper, url, context, source, line_offset, column_offset, cached_data, &handle);
  if (s != napi_ok) return s;
  if (handle_out != nullptr) *handle_out = StoreModuleWrapHandle(*bridge_state, handle);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_module_wrap_create_synthetic(
    SnapiEnvState* env_state,
    uint32_t wrapper_id,
    uint32_t url_id,
    uint32_t context_id,
    uint32_t export_names_id,
    uint32_t synthetic_eval_steps_id,
    uint32_t* handle_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value wrapper = LoadValue(*bridge_state, wrapper_id);
  napi_value url = LoadValue(*bridge_state, url_id);
  napi_value context = context_id == 0 ? nullptr : LoadValue(*bridge_state, context_id);
  napi_value export_names = LoadValue(*bridge_state, export_names_id);
  napi_value synthetic_eval_steps = LoadValue(*bridge_state, synthetic_eval_steps_id);
  if (wrapper == nullptr || url == nullptr || export_names == nullptr ||
      synthetic_eval_steps == nullptr) {
    return napi_invalid_arg;
  }
  if (context_id != 0 && context == nullptr) return napi_invalid_arg;
  void* handle = nullptr;
  napi_status s = unofficial_napi_module_wrap_create_synthetic(
      env, wrapper, url, context, export_names, synthetic_eval_steps, &handle);
  if (s != napi_ok) return s;
  if (handle_out != nullptr) *handle_out = StoreModuleWrapHandle(*bridge_state, handle);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_module_wrap_destroy(SnapiEnvState* env_state,
                                                           uint32_t handle_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  void* handle = LoadModuleWrapHandle(*bridge_state, handle_id);
  if (handle == nullptr) return napi_invalid_arg;
  napi_status s = unofficial_napi_module_wrap_destroy(env, handle);
  if (s == napi_ok) RemoveModuleWrapHandle(*bridge_state, handle_id);
  return s;
}

extern "C" int snapi_bridge_unofficial_module_wrap_get_module_requests(
    SnapiEnvState* env_state,
    uint32_t handle_id,
    uint32_t* result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  void* handle = LoadModuleWrapHandle(*bridge_state, handle_id);
  if (handle == nullptr) return napi_invalid_arg;
  napi_value result = nullptr;
  napi_status s = unofficial_napi_module_wrap_get_module_requests(env, handle, &result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_module_wrap_link(SnapiEnvState* env_state,
                                                        uint32_t handle_id,
                                                        uint32_t count,
                                                        const uint32_t* linked_handle_ids) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  void* handle = LoadModuleWrapHandle(*bridge_state, handle_id);
  if (handle == nullptr) return napi_invalid_arg;
  std::vector<void*> linked_handles(count, nullptr);
  for (uint32_t i = 0; i < count; ++i) {
    void* linked = linked_handle_ids != nullptr ? LoadModuleWrapHandle(*bridge_state, linked_handle_ids[i]) : nullptr;
    if (linked == nullptr) return napi_invalid_arg;
    linked_handles[i] = linked;
  }
  return unofficial_napi_module_wrap_link(
      env, handle, count, count == 0 ? nullptr : linked_handles.data());
}

extern "C" int snapi_bridge_unofficial_module_wrap_instantiate(SnapiEnvState* env_state,
                                                               uint32_t handle_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  void* handle = LoadModuleWrapHandle(*bridge_state, handle_id);
  if (handle == nullptr) return napi_invalid_arg;
  return unofficial_napi_module_wrap_instantiate(env, handle);
}

extern "C" int snapi_bridge_unofficial_module_wrap_evaluate(SnapiEnvState* env_state,
                                                            uint32_t handle_id,
                                                            int64_t timeout,
                                                            int break_on_sigint,
                                                            uint32_t* result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  void* handle = LoadModuleWrapHandle(*bridge_state, handle_id);
  if (handle == nullptr) return napi_invalid_arg;
  napi_value result = nullptr;
  napi_status s = unofficial_napi_module_wrap_evaluate(
      env, handle, timeout, break_on_sigint != 0, &result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_module_wrap_evaluate_sync(SnapiEnvState* env_state,
                                                                 uint32_t handle_id,
                                                                 uint32_t filename_id,
                                                                 uint32_t parent_filename_id,
                                                                 uint32_t* result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  void* handle = LoadModuleWrapHandle(*bridge_state, handle_id);
  if (handle == nullptr) return napi_invalid_arg;
  napi_value filename = filename_id == 0 ? nullptr : LoadValue(*bridge_state, filename_id);
  napi_value parent_filename =
      parent_filename_id == 0 ? nullptr : LoadValue(*bridge_state, parent_filename_id);
  if ((filename_id != 0 && filename == nullptr) ||
      (parent_filename_id != 0 && parent_filename == nullptr)) {
    return napi_invalid_arg;
  }
  napi_value result = nullptr;
  napi_status s = unofficial_napi_module_wrap_evaluate_sync(
      env, handle, filename, parent_filename, &result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_module_wrap_get_namespace(SnapiEnvState* env_state,
                                                                 uint32_t handle_id,
                                                                 uint32_t* result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  void* handle = LoadModuleWrapHandle(*bridge_state, handle_id);
  if (handle == nullptr) return napi_invalid_arg;
  napi_value result = nullptr;
  napi_status s = unofficial_napi_module_wrap_get_namespace(env, handle, &result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_module_wrap_get_status(SnapiEnvState* env_state,
                                                              uint32_t handle_id,
                                                              int32_t* status_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  void* handle = LoadModuleWrapHandle(*bridge_state, handle_id);
  if (handle == nullptr) return napi_invalid_arg;
  return unofficial_napi_module_wrap_get_status(env, handle, status_out);
}

extern "C" int snapi_bridge_unofficial_module_wrap_get_error(SnapiEnvState* env_state,
                                                             uint32_t handle_id,
                                                             uint32_t* result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  void* handle = LoadModuleWrapHandle(*bridge_state, handle_id);
  if (handle == nullptr) return napi_invalid_arg;
  napi_value result = nullptr;
  napi_status s = unofficial_napi_module_wrap_get_error(env, handle, &result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_module_wrap_has_top_level_await(
    SnapiEnvState* env_state,
    uint32_t handle_id,
    int* result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  void* handle = LoadModuleWrapHandle(*bridge_state, handle_id);
  if (handle == nullptr) return napi_invalid_arg;
  bool result = false;
  napi_status s = unofficial_napi_module_wrap_has_top_level_await(env, handle, &result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = result ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_module_wrap_has_async_graph(SnapiEnvState* env_state,
                                                                   uint32_t handle_id,
                                                                   int* result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  void* handle = LoadModuleWrapHandle(*bridge_state, handle_id);
  if (handle == nullptr) return napi_invalid_arg;
  bool result = false;
  napi_status s = unofficial_napi_module_wrap_has_async_graph(env, handle, &result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = result ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_module_wrap_check_unsettled_top_level_await(
    SnapiEnvState* env_state,
    uint32_t module_wrap_id,
    int warnings,
    int* settled_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  if (
      settled_out == nullptr) {
    return napi_invalid_arg;
  }
  napi_value module_wrap =
      module_wrap_id == 0 ? nullptr : LoadValue(*bridge_state, module_wrap_id);
  if (module_wrap_id != 0 && module_wrap == nullptr) return napi_invalid_arg;
  bool settled = true;
  napi_status s = unofficial_napi_module_wrap_check_unsettled_top_level_await(
      env, module_wrap, warnings != 0, &settled);
  if (s != napi_ok) return s;
  *settled_out = settled ? 1 : 0;
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_module_wrap_set_export(SnapiEnvState* env_state,
                                                              uint32_t handle_id,
                                                              uint32_t export_name_id,
                                                              uint32_t export_value_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  void* handle = LoadModuleWrapHandle(*bridge_state, handle_id);
  napi_value export_name = LoadValue(*bridge_state, export_name_id);
  napi_value export_value = export_value_id == 0 ? nullptr : LoadValue(*bridge_state, export_value_id);
  if (handle == nullptr || export_name == nullptr) return napi_invalid_arg;
  if (export_value_id != 0 && export_value == nullptr) return napi_invalid_arg;
  return unofficial_napi_module_wrap_set_export(env, handle, export_name, export_value);
}

extern "C" int snapi_bridge_unofficial_module_wrap_set_module_source_object(
    SnapiEnvState* env_state,
    uint32_t handle_id,
    uint32_t source_object_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  void* handle = LoadModuleWrapHandle(*bridge_state, handle_id);
  napi_value source_object = source_object_id == 0 ? nullptr : LoadValue(*bridge_state, source_object_id);
  if (handle == nullptr) return napi_invalid_arg;
  if (source_object_id != 0 && source_object == nullptr) return napi_invalid_arg;
  return unofficial_napi_module_wrap_set_module_source_object(env, handle, source_object);
}

extern "C" int snapi_bridge_unofficial_module_wrap_get_module_source_object(
    SnapiEnvState* env_state,
    uint32_t handle_id,
    uint32_t* result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  void* handle = LoadModuleWrapHandle(*bridge_state, handle_id);
  if (handle == nullptr) return napi_invalid_arg;
  napi_value result = nullptr;
  napi_status s = unofficial_napi_module_wrap_get_module_source_object(env, handle, &result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_module_wrap_create_cached_data(
    SnapiEnvState* env_state,
    uint32_t handle_id,
    uint32_t* result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  void* handle = LoadModuleWrapHandle(*bridge_state, handle_id);
  if (handle == nullptr) return napi_invalid_arg;
  napi_value result = nullptr;
  napi_status s = unofficial_napi_module_wrap_create_cached_data(env, handle, &result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_module_wrap_set_import_module_dynamically_callback(
    SnapiEnvState* env_state,
    uint32_t callback_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value callback = callback_id == 0 ? nullptr : LoadValue(*bridge_state, callback_id);
  if (callback_id != 0 && callback == nullptr) return napi_invalid_arg;
  return unofficial_napi_module_wrap_set_import_module_dynamically_callback(env, callback);
}

extern "C" int
snapi_bridge_unofficial_module_wrap_set_initialize_import_meta_object_callback(
    SnapiEnvState* env_state,
    uint32_t callback_id) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  napi_value callback = callback_id == 0 ? nullptr : LoadValue(*bridge_state, callback_id);
  if (callback_id != 0 && callback == nullptr) return napi_invalid_arg;
  return unofficial_napi_module_wrap_set_initialize_import_meta_object_callback(env, callback);
}

extern "C" int snapi_bridge_unofficial_module_wrap_import_module_dynamically(
    SnapiEnvState* env_state,
    uint32_t argc,
    const uint32_t* argv_ids,
    uint32_t* result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  std::vector<napi_value> argv(argc, nullptr);
  for (uint32_t i = 0; i < argc; ++i) {
    napi_value value = LoadValue(*bridge_state, argv_ids[i]);
    if (value == nullptr) return napi_invalid_arg;
    argv[i] = value;
  }
  napi_value result = nullptr;
  napi_status s = unofficial_napi_module_wrap_import_module_dynamically(
      env, argc, argc == 0 ? nullptr : argv.data(), &result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = StoreValue(*bridge_state, result);
  return napi_ok;
}

extern "C" int snapi_bridge_unofficial_module_wrap_create_required_module_facade(
    SnapiEnvState* env_state,
    uint32_t handle_id,
    uint32_t* result_out) {
  auto* bridge_state = RequireEnvState(env_state);
  if (bridge_state == nullptr) return napi_invalid_arg;
  napi_env env = bridge_state->env;
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  void* handle = LoadModuleWrapHandle(*bridge_state, handle_id);
  if (handle == nullptr) return napi_invalid_arg;
  napi_value result = nullptr;
  napi_status s = unofficial_napi_module_wrap_create_required_module_facade(env, handle, &result);
  if (s != napi_ok) return s;
  if (result_out != nullptr) *result_out = StoreValue(*bridge_state, result);
  return napi_ok;
}

// ============================================================
// Cleanup
// ============================================================

extern "C" void snapi_bridge_dispose() {
  std::lock_guard<std::recursive_mutex> lock(g_mu);
  std::vector<SnapiEnvState*> env_states;
  env_states.reserve(g_envs.size());
  for (auto* env_state : g_envs) {
    env_states.push_back(env_state);
  }
  for (auto* env_state : env_states) {
    (void)DisposeBridgeStateLocked(env_state);
  }
}
