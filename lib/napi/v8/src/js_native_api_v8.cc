#include "internal/napi_v8_env.h"
#include "node_api_types.h"
#include "unofficial_napi_error_utils.h"

#include <climits>
#include <atomic>
#include <cstdio>
#include <cstring>
#include <cstdlib>
#include <limits>
#include <memory>
#include <new>
#include <string>
#include <vector>

struct napi_async_work__ {
  napi_env env = nullptr;
  napi_async_execute_callback execute = nullptr;
  napi_async_complete_callback complete = nullptr;
  void* data = nullptr;
};

struct napi_threadsafe_function__ {
  napi_env env = nullptr;
  napi_threadsafe_function_call_js call_js_cb = nullptr;
  napi_finalize finalize_cb = nullptr;
  void* finalize_data = nullptr;
  void* context = nullptr;
  std::atomic<uint32_t> refcount{0};
  std::atomic<bool> finalized{false};
};

struct napi_env_cleanup_hook__ {
  napi_cleanup_hook hook = nullptr;
  void* arg = nullptr;
};

struct napi_deferred__ {
  napi_env env = nullptr;
  v8::Global<v8::Promise::Resolver> resolver;
};

struct napi_handle_scope__ {
  napi_env env = nullptr;
};

struct napi_escapable_handle_scope__ {
  napi_env env = nullptr;
  bool escaped = false;
};

struct napi_buffer_record__ {
  napi_env env = nullptr;
  v8::Global<v8::Object> holder;
  std::shared_ptr<v8::BackingStore> backing_store;
  void* external_data = nullptr;
  node_api_basic_finalize finalize_cb = nullptr;
  void* finalize_hint = nullptr;
  bool finalized = false;
};
using napi_buffer_record = napi_buffer_record__;

struct napi_external_backing_store_hint__ {
  napi_env env = nullptr;
  void* external_data = nullptr;
  node_api_basic_finalize finalize_cb = nullptr;
  void* finalize_hint = nullptr;
};
using napi_external_backing_store_hint = napi_external_backing_store_hint__;

struct WrapFinalizerRecord {
  napi_env env = nullptr;
  void* native_object = nullptr;
  node_api_basic_finalize finalize_cb = nullptr;
  void* finalize_hint = nullptr;
  bool cancelled = false;
  v8::Global<v8::Object> handle;
};

namespace {

v8::MaybeLocal<v8::Promise> NapiHostImportModuleDynamically(
    v8::Local<v8::Context> context, v8::Local<v8::Data> /*host_defined_options*/,
    v8::Local<v8::Value> resource_name, v8::Local<v8::String> specifier,
    v8::Local<v8::FixedArray> /*import_attributes*/) {
  v8::Isolate* isolate = context->GetIsolate();
  v8::EscapableHandleScope handle_scope(isolate);
  v8::TryCatch try_catch(isolate);

  v8::Local<v8::Promise::Resolver> resolver;
  if (!v8::Promise::Resolver::New(context).ToLocal(&resolver)) {
    return v8::MaybeLocal<v8::Promise>();
  }
  v8::Local<v8::Promise> promise = resolver->GetPromise();

  v8::Local<v8::Object> global = context->Global();
  v8::Local<v8::String> helper_name =
      v8::String::NewFromUtf8Literal(isolate, "__napi_dynamic_import");
  v8::Local<v8::Value> helper_value;
  v8::Local<v8::String> process_name =
      v8::String::NewFromUtf8Literal(isolate, "process");
  v8::Local<v8::Value> process_value;
  if (global->Get(context, process_name).ToLocal(&process_value) && process_value->IsObject()) {
    auto process_obj = process_value.As<v8::Object>();
    if (!process_obj->Get(context, helper_name).ToLocal(&helper_value)) {
      helper_value = v8::Undefined(isolate);
    }
  }
  if ((!helper_value->IsFunction()) &&
      (!global->Get(context, helper_name).ToLocal(&helper_value) || !helper_value->IsFunction())) {
    v8::Local<v8::String> message = v8::String::NewFromUtf8Literal(isolate, "Not supported");
    resolver->Reject(context, v8::Exception::Error(message)).FromMaybe(false);
    return handle_scope.Escape(promise);
  }

  v8::Local<v8::Function> helper = helper_value.As<v8::Function>();
  v8::Local<v8::Value> argv[2] = {specifier, resource_name};
  v8::Local<v8::Value> result;
  if (!helper->Call(context, global, 2, argv).ToLocal(&result)) {
    if (try_catch.HasCaught()) {
      resolver->Reject(context, try_catch.Exception()).FromMaybe(false);
      try_catch.Reset();
    } else {
      v8::Local<v8::String> message = v8::String::NewFromUtf8Literal(isolate, "Not supported");
      resolver->Reject(context, v8::Exception::Error(message)).FromMaybe(false);
    }
    return handle_scope.Escape(promise);
  }

  if (result->IsPromise()) {
    return handle_scope.Escape(result.As<v8::Promise>());
  }

  resolver->Resolve(context, result).FromMaybe(false);
  return handle_scope.Escape(promise);
}

inline bool CanBeHeldWeakly(v8::Local<v8::Value> value) {
  return value->IsObject() || value->IsSymbol();
}

void ReferenceWeakCallback(const v8::WeakCallbackInfo<napi_ref__>& info) {
  napi_ref__* ref = info.GetParameter();
  if (ref == nullptr) return;
  ref->value.Reset();
}

void RemoveWrapFinalizerRecord(napi_env env, WrapFinalizerRecord* record) {
  if (env == nullptr || record == nullptr) return;
  auto& records = env->wrap_finalizers;
  for (auto it = records.begin(); it != records.end(); ++it) {
    if (*it == record) {
      records.erase(it);
      return;
    }
  }
}

void InvokeWrapFinalizer(WrapFinalizerRecord* record) {
  if (record == nullptr || record->cancelled || record->finalize_cb == nullptr) return;
  record->finalize_cb(record->env, record->native_object, record->finalize_hint);
}

void InvokeWrapFinalizerMicrotask(void* data) {
  auto* record = static_cast<WrapFinalizerRecord*>(data);
  if (record == nullptr) return;
  InvokeWrapFinalizer(record);
  delete record;
}

void WrapWeakCallback(const v8::WeakCallbackInfo<WrapFinalizerRecord>& info) {
  WrapFinalizerRecord* record = info.GetParameter();
  if (record == nullptr) return;
  napi_env env = record->env;
  if (env != nullptr) {
    RemoveWrapFinalizerRecord(env, record);
  }
  record->handle.Reset();
  if (env != nullptr && env->isolate != nullptr) {
    env->isolate->EnqueueMicrotask(InvokeWrapFinalizerMicrotask, record);
  } else {
    InvokeWrapFinalizer(record);
    delete record;
  }
}

struct CallbackPayload {
  napi_env env;
  napi_callback cb;
  void* data;
};

struct AccessorPayload {
  napi_env env;
  napi_callback getter_cb;
  napi_callback setter_cb;
  void* data;
};

inline bool CallbackInfoOwnsValue(const napi_callback_info__* cbinfo, napi_value value) {
  if (cbinfo == nullptr || value == nullptr) return false;
  if (cbinfo->this_arg == value || cbinfo->new_target == value) return true;
  for (auto* arg : cbinfo->args) {
    if (arg == value) return true;
  }
  return false;
}


inline bool CheckEnv(napi_env env) {
  return env != nullptr && env->isolate != nullptr;
}

void RunEnvCleanupHooks(napi_env env) {
  if (!CheckEnv(env)) return;
  for (auto it = env->env_cleanup_hooks.rbegin(); it != env->env_cleanup_hooks.rend(); ++it) {
    auto* entry = static_cast<napi_env_cleanup_hook__*>(*it);
    if (entry != nullptr && entry->hook != nullptr) {
      entry->hook(entry->arg);
    }
  }
  for (void* raw : env->env_cleanup_hooks) {
    delete static_cast<napi_env_cleanup_hook__*>(raw);
  }
  env->env_cleanup_hooks.clear();
}

void RemoveBufferRecord(napi_env env, napi_buffer_record* record) {
  if (!CheckEnv(env) || record == nullptr) return;
  auto& records = env->buffer_records;
  for (auto it = records.begin(); it != records.end(); ++it) {
    if (*it == record) {
      records.erase(it);
      return;
    }
  }
}

void FinalizeBufferRecord(napi_buffer_record* record) {
  if (record == nullptr || record->finalized) return;
  record->finalized = true;
  if (record->finalize_cb != nullptr) {
    record->finalize_cb(record->env, record->external_data, record->finalize_hint);
  }
}

void FinalizeBufferRecordMicrotask(void* data) {
  auto* record = static_cast<napi_buffer_record*>(data);
  if (record == nullptr) return;
  FinalizeBufferRecord(record);
  delete record;
}

void BufferWeakCallback(const v8::WeakCallbackInfo<napi_buffer_record>& info) {
  napi_buffer_record* record = info.GetParameter();
  if (record == nullptr) return;
  napi_env env = record->env;
  if (CheckEnv(env)) {
    v8::HandleScope hs(env->isolate);
    v8::Local<v8::Context> context = env->context();
    v8::Context::Scope cs(context);
    RemoveBufferRecord(env, record);
    env->isolate->EnqueueMicrotask(FinalizeBufferRecordMicrotask, record);
  } else {
    FinalizeBufferRecord(record);
    delete record;
  }
  record->holder.Reset();
}

void ExternalBackingStoreDeleter(void* data,
                                 size_t /*length*/,
                                 void* deleter_data) {
  auto* hint = static_cast<napi_external_backing_store_hint*>(deleter_data);
  if (hint == nullptr) return;
  if (hint->finalize_cb != nullptr) {
    hint->finalize_cb(hint->env, hint->external_data != nullptr ? hint->external_data : data,
                      hint->finalize_hint);
  }
  delete hint;
}

bool GetArrayBufferViewInfo(v8::Local<v8::Value> value, void** data, size_t* length) {
  if (!value->IsArrayBufferView()) return false;

  v8::Local<v8::ArrayBufferView> view = value.As<v8::ArrayBufferView>();
  std::shared_ptr<v8::BackingStore> backing_store = view->Buffer()->GetBackingStore();
  size_t byte_length = view->ByteLength();

  if (length != nullptr) *length = byte_length;
  if (data != nullptr) {
    uint8_t* base = static_cast<uint8_t*>(backing_store ? backing_store->Data() : nullptr);
    *data = (base == nullptr) ? nullptr : static_cast<void*>(base + view->ByteOffset());
  }
  return true;
}

v8::Local<v8::Object> CreateBufferObject(napi_env env,
                                         std::shared_ptr<v8::BackingStore> backing_store,
                                         size_t offset,
                                         size_t length) {
  v8::Local<v8::ArrayBuffer> ab = v8::ArrayBuffer::New(env->isolate, backing_store);
  v8::Local<v8::Context> context = env->context();
  v8::Local<v8::Object> global = context->Global();

  // Node's napi_create_buffer* APIs produce Buffer instances, not plain
  // Uint8Array views. Prefer global Buffer.from(arrayBuffer, offset, length)
  // when available so native bindings observe Node-compatible semantics.
  v8::Local<v8::String> buffer_name = v8::String::NewFromUtf8Literal(env->isolate, "Buffer");
  v8::Local<v8::Value> buffer_ctor_value;
  if (global->Get(context, buffer_name).ToLocal(&buffer_ctor_value) && buffer_ctor_value->IsObject()) {
    v8::Local<v8::Object> buffer_ctor = buffer_ctor_value.As<v8::Object>();
    v8::Local<v8::String> from_name = v8::String::NewFromUtf8Literal(env->isolate, "from");
    v8::Local<v8::Value> from_value;
    if (buffer_ctor->Get(context, from_name).ToLocal(&from_value) && from_value->IsFunction()) {
      v8::Local<v8::Function> from_fn = from_value.As<v8::Function>();
      v8::Local<v8::Value> argv[3] = {
          ab,
          v8::Number::New(env->isolate, static_cast<double>(offset)),
          v8::Number::New(env->isolate, static_cast<double>(length)),
      };
      v8::Local<v8::Value> maybe_buffer;
      if (from_fn->Call(context, buffer_ctor, 3, argv).ToLocal(&maybe_buffer) &&
          maybe_buffer->IsObject()) {
        return maybe_buffer.As<v8::Object>();
      }
    }
  }

  // Fallback used during very early bootstrap before Buffer is available.
  return v8::Uint8Array::New(ab, offset, length);
}

inline bool CheckValue(napi_env env, napi_value value) {
  return CheckEnv(env) && value != nullptr;
}

void ClearLastException(napi_env env) {
  if (env == nullptr) return;
  env->last_exception.Reset();
}

void SetLastException(napi_env env,
                      v8::Local<v8::Value> exception,
                      v8::Local<v8::Message> message = v8::Local<v8::Message>()) {
  if (env == nullptr) return;
  env->last_exception.Reset();
  if (exception.IsEmpty()) return;

  env->last_exception.Reset(env->isolate, exception);
  if (!message.IsEmpty()) {
    unofficial_napi_internal::SetArrowMessage(
        env->isolate, env->context(), exception, message);
  }
}

inline napi_status ReturnPendingIfCaught(napi_env env, v8::TryCatch& tc, const char* message) {
  if (tc.HasCaught()) {
    SetLastException(env, tc.Exception(), tc.Message());
    return napi_v8_set_last_error(env, napi_pending_exception, message);
  }
  return napi_v8_set_last_error(env, napi_generic_failure, message);
}

inline napi_status InvalidArg(napi_env env) {
  if (CheckEnv(env)) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  return napi_invalid_arg;
}

inline napi_valuetype TypeOf(v8::Local<v8::Value> value) {
  if (value->IsUndefined()) return napi_undefined;
  if (value->IsNull()) return napi_null;
  if (value->IsBoolean()) return napi_boolean;
  if (value->IsNumber()) return napi_number;
  if (value->IsString()) return napi_string;
  if (value->IsSymbol()) return napi_symbol;
  if (value->IsFunction()) return napi_function;
  if (value->IsBigInt()) return napi_bigint;
  if (value->IsExternal()) return napi_external;
  return napi_object;
}

inline v8::PropertyAttribute ToV8PropertyAttributes(napi_property_attributes attrs,
                                                    bool include_writable) {
  int v8_attrs = v8::None;
  if ((attrs & napi_enumerable) == 0) v8_attrs |= v8::DontEnum;
  if ((attrs & napi_configurable) == 0) v8_attrs |= v8::DontDelete;
  if (include_writable && (attrs & napi_writable) == 0) v8_attrs |= v8::ReadOnly;
  return static_cast<v8::PropertyAttribute>(v8_attrs);
}

inline const char* TypedArrayConstructorName(napi_typedarray_type type) {
  switch (type) {
    case napi_int8_array: return "Int8Array";
    case napi_uint8_array: return "Uint8Array";
    case napi_uint8_clamped_array: return "Uint8ClampedArray";
    case napi_int16_array: return "Int16Array";
    case napi_uint16_array: return "Uint16Array";
    case napi_int32_array: return "Int32Array";
    case napi_uint32_array: return "Uint32Array";
    case napi_float16_array: return "Float16Array";
    case napi_float32_array: return "Float32Array";
    case napi_float64_array: return "Float64Array";
    case napi_bigint64_array: return "BigInt64Array";
    case napi_biguint64_array: return "BigUint64Array";
    default: return nullptr;
  }
}

inline bool GetTypedArrayType(v8::Local<v8::Value> value, napi_typedarray_type* out_type) {
  if (value->IsInt8Array()) {
    *out_type = napi_int8_array;
  } else if (value->IsUint8Array()) {
    *out_type = napi_uint8_array;
  } else if (value->IsUint8ClampedArray()) {
    *out_type = napi_uint8_clamped_array;
  } else if (value->IsInt16Array()) {
    *out_type = napi_int16_array;
  } else if (value->IsUint16Array()) {
    *out_type = napi_uint16_array;
  } else if (value->IsInt32Array()) {
    *out_type = napi_int32_array;
  } else if (value->IsUint32Array()) {
    *out_type = napi_uint32_array;
  } else if (value->IsFloat16Array()) {
    *out_type = napi_float16_array;
  } else if (value->IsFloat32Array()) {
    *out_type = napi_float32_array;
  } else if (value->IsFloat64Array()) {
    *out_type = napi_float64_array;
  } else if (value->IsBigInt64Array()) {
    *out_type = napi_bigint64_array;
  } else if (value->IsBigUint64Array()) {
    *out_type = napi_biguint64_array;
  } else {
    return false;
  }
  return true;
}

void FunctionTrampoline(const v8::FunctionCallbackInfo<v8::Value>& info) {
  auto* payload =
      static_cast<CallbackPayload*>(info.Data().As<v8::External>()->Value());
  if (payload == nullptr || payload->env == nullptr || payload->cb == nullptr) {
    return;
  }

  napi_env env = payload->env;
  auto* cbinfo = new napi_callback_info__();
  cbinfo->env = env;
  cbinfo->data = payload->data;
  cbinfo->this_arg = napi_v8_wrap_value(env, info.This());
  if (!info.NewTarget().IsEmpty() && !info.NewTarget()->IsUndefined()) {
    cbinfo->new_target = napi_v8_wrap_value(env, info.NewTarget());
  }
  cbinfo->args.reserve(info.Length());
  for (int i = 0; i < info.Length(); ++i) {
    cbinfo->args.push_back(napi_v8_wrap_value(env, info[i]));
  }

  napi_value ret = payload->cb(env, cbinfo);
  bool pending_exception = !env->last_exception.IsEmpty();
  if (pending_exception) {
    info.GetIsolate()->ThrowException(env->last_exception.Get(env->isolate));
    ClearLastException(env);
  } else if (ret != nullptr) {
    info.GetReturnValue().Set(napi_v8_unwrap_value(ret));
  }

  if (!pending_exception && ret != nullptr && !CallbackInfoOwnsValue(cbinfo, ret)) {
    delete ret;
  }
  delete cbinfo;
}

void GetterTrampoline(v8::Local<v8::Name> property,
                      const v8::PropertyCallbackInfo<v8::Value>& info) {
  (void)property;
  auto* payload =
      static_cast<AccessorPayload*>(info.Data().As<v8::External>()->Value());
  if (payload == nullptr || payload->env == nullptr || payload->getter_cb == nullptr) {
    return;
  }
  napi_env env = payload->env;
  auto* cbinfo = new napi_callback_info__();
  cbinfo->env = env;
  cbinfo->data = payload->data;
  cbinfo->this_arg = napi_v8_wrap_value(env, info.This());
  napi_value ret = payload->getter_cb(env, cbinfo);
  bool pending_exception = !env->last_exception.IsEmpty();
  if (!env->last_exception.IsEmpty()) {
    info.GetIsolate()->ThrowException(env->last_exception.Get(env->isolate));
    ClearLastException(env);
  } else if (ret != nullptr) {
    info.GetReturnValue().Set(napi_v8_unwrap_value(ret));
  }
  if (!pending_exception && ret != nullptr && !CallbackInfoOwnsValue(cbinfo, ret)) {
    delete ret;
  }
  delete cbinfo;
}

void SetterTrampoline(v8::Local<v8::Name> property,
                      v8::Local<v8::Value> value,
                      const v8::PropertyCallbackInfo<void>& info) {
  (void)property;
  auto* payload =
      static_cast<AccessorPayload*>(info.Data().As<v8::External>()->Value());
  if (payload == nullptr || payload->env == nullptr || payload->setter_cb == nullptr) {
    return;
  }
  napi_env env = payload->env;
  auto* cbinfo = new napi_callback_info__();
  cbinfo->env = env;
  cbinfo->data = payload->data;
  cbinfo->this_arg = napi_v8_wrap_value(env, info.This());
  cbinfo->args.push_back(napi_v8_wrap_value(env, value));
  payload->setter_cb(env, cbinfo);
  if (!env->last_exception.IsEmpty()) {
    info.GetIsolate()->ThrowException(env->last_exception.Get(env->isolate));
    ClearLastException(env);
  }
  delete cbinfo;
}

}  // namespace

napi_value__::napi_value__(napi_env env, v8::Local<v8::Value> local)
    : env(env), value(env->isolate, local) {}

napi_value__::~napi_value__() = default;

napi_callback_info__::~napi_callback_info__() {
  for (auto* arg : args) {
    delete arg;
  }
  args.clear();
  delete this_arg;
  delete new_target;
}

v8::Local<v8::Value> napi_value__::local() const {
  return value.Get(env->isolate);
}

napi_ref__::napi_ref__(napi_env env,
                       v8::Local<v8::Value> value,
                       uint32_t initial_refcount)
    : env(env), value(env->isolate, value), refcount(initial_refcount), can_be_weak(CanBeHeldWeakly(value)) {
  if (refcount == 0 && can_be_weak) {
    this->value.SetWeak(this, ReferenceWeakCallback, v8::WeakCallbackType::kParameter);
  }
}

napi_ref__::~napi_ref__() {
  value.Reset();
}

napi_env__::napi_env__(v8::Local<v8::Context> context, int32_t module_api_version)
    : isolate(v8::Isolate::GetCurrent()),
      context_ref(isolate, context),
      module_api_version(module_api_version) {
  isolate->SetHostImportModuleDynamicallyCallback(NapiHostImportModuleDynamically);
  v8::Local<v8::Private> wrapKey = v8::Private::ForApi(
      isolate, v8::String::NewFromUtf8Literal(isolate, "__napi_wrap"));
  wrap_private_key.Reset(isolate, wrapKey);
  v8::Local<v8::Private> wrapRefKey = v8::Private::ForApi(
      isolate, v8::String::NewFromUtf8Literal(isolate, "__napi_wrap_ref"));
  wrap_ref_private_key.Reset(isolate, wrapRefKey);
  v8::Local<v8::Private> wrapFinalizeKey = v8::Private::ForApi(
      isolate, v8::String::NewFromUtf8Literal(isolate, "__napi_wrap_finalize"));
  wrap_finalizer_private_key.Reset(isolate, wrapFinalizeKey);
  v8::Local<v8::Private> bufferKey = v8::Private::ForApi(
      isolate, v8::String::NewFromUtf8Literal(isolate, "__napi_buffer_record"));
  buffer_private_key.Reset(isolate, bufferKey);
  napi_v8_clear_last_error(this);
}

napi_env__::~napi_env__() {
  RunEnvCleanupHooks(this);
  napi_v8_finalize_buffer_records(this);

  for (auto* raw_record : wrap_finalizers) {
    auto* record = static_cast<WrapFinalizerRecord*>(raw_record);
    if (record != nullptr) {
      InvokeWrapFinalizer(record);
      record->handle.Reset();
      delete record;
    }
  }
  wrap_finalizers.clear();

  for (auto* raw_tsfn : threadsafe_functions) {
    auto* tsfn = static_cast<napi_threadsafe_function__*>(raw_tsfn);
    if (tsfn != nullptr && !tsfn->finalized.exchange(true) && tsfn->finalize_cb != nullptr) {
      tsfn->finalize_cb(this, tsfn->finalize_data, nullptr);
    }
    delete tsfn;
  }
  threadsafe_functions.clear();

  if (instance_data_finalize_cb != nullptr) {
    instance_data_finalize_cb(this, instance_data, instance_data_finalize_hint);
  }
  if (env_destroy_callback != nullptr) {
    env_destroy_callback(this, env_destroy_callback_data);
  }
  edge_environment = nullptr;
}

v8::Local<v8::Context> napi_env__::context() const {
  return context_ref.Get(isolate);
}

napi_status napi_v8_set_last_error(napi_env env,
                                   napi_status status,
                                   const char* message) {
  if (env == nullptr) return status;
  env->last_error.error_code = status;
  env->last_error.engine_error_code = 0;
  env->last_error.engine_reserved = nullptr;
  env->last_error_message = (message == nullptr) ? "" : message;
  env->last_error.error_message =
      env->last_error_message.empty() ? nullptr : env->last_error_message.c_str();
  return status;
}

napi_status napi_v8_clear_last_error(napi_env env) {
  return napi_v8_set_last_error(env, napi_ok, nullptr);
}

napi_value napi_v8_wrap_value(napi_env env, v8::Local<v8::Value> value) {
  if (!CheckEnv(env)) return nullptr;
  return new (std::nothrow) napi_value__(env, value);
}

v8::Local<v8::Value> napi_v8_unwrap_value(napi_value value) {
  return value->local();
}

void napi_v8_finalize_buffer_records(napi_env env) {
  if (!CheckEnv(env)) return;
  for (void* raw : env->buffer_records) {
    auto* record = static_cast<napi_buffer_record*>(raw);
    if (record != nullptr) {
      FinalizeBufferRecord(record);
      record->holder.Reset();
      delete record;
    }
  }
  env->buffer_records.clear();
}

extern "C" {

void NAPI_CDECL napi_fatal_error(const char* location,
                                 size_t location_len,
                                 const char* message,
                                 size_t message_len) {
  const char* loc = (location == nullptr) ? "" : location;
  const char* msg = (message == nullptr) ? "" : message;
  size_t loc_len = (location_len == NAPI_AUTO_LENGTH) ? std::strlen(loc) : location_len;
  size_t msg_len = (message_len == NAPI_AUTO_LENGTH) ? std::strlen(msg) : message_len;
  std::fprintf(stderr, "FATAL ERROR: %.*s %.*s\n",
               static_cast<int>(loc_len), loc,
               static_cast<int>(msg_len), msg);
  std::fflush(stderr);
  std::abort();
}

napi_status NAPI_CDECL napi_get_last_error_info(
    node_api_basic_env env, const napi_extended_error_info** result) {
  if (result == nullptr) return napi_invalid_arg;
  auto* napiEnv = const_cast<napi_env>(env);
  if (!CheckEnv(napiEnv)) return napi_invalid_arg;
  *result = &napiEnv->last_error;
  return napi_ok;
}

napi_status NAPI_CDECL napi_get_undefined(napi_env env, napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  *result = napi_v8_wrap_value(env, v8::Undefined(env->isolate));
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_get_null(napi_env env, napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  *result = napi_v8_wrap_value(env, v8::Null(env->isolate));
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_get_global(napi_env env, napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  auto context = env->context();
  *result = napi_v8_wrap_value(env, context->Global());
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_get_boolean(napi_env env,
                                        bool value,
                                        napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  *result = napi_v8_wrap_value(env, v8::Boolean::New(env->isolate, value));
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_create_double(napi_env env,
                                          double value,
                                          napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  *result = napi_v8_wrap_value(env, v8::Number::New(env->isolate, value));
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_create_int32(napi_env env,
                                         int32_t value,
                                         napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  *result = napi_v8_wrap_value(env, v8::Int32::New(env->isolate, value));
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_create_int64(napi_env env,
                                         int64_t value,
                                         napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  *result = napi_v8_wrap_value(env, v8::Number::New(env->isolate, static_cast<double>(value)));
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_create_uint32(napi_env env,
                                          uint32_t value,
                                          napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  *result = napi_v8_wrap_value(env, v8::Integer::NewFromUnsigned(env->isolate, value));
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_create_bigint_int64(napi_env env,
                                                int64_t value,
                                                napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  *result = napi_v8_wrap_value(env, v8::BigInt::New(env->isolate, value));
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_create_bigint_uint64(napi_env env,
                                                 uint64_t value,
                                                 napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  *result = napi_v8_wrap_value(env, v8::BigInt::NewFromUnsigned(env->isolate, value));
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_create_bigint_words(napi_env env,
                                                int sign_bit,
                                                size_t word_count,
                                                const uint64_t* words,
                                                napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  if ((sign_bit != 0 && sign_bit != 1) || word_count > static_cast<size_t>(INT_MAX)) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  if (word_count > 0 && words == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::TryCatch tc(env->isolate);
  v8::Local<v8::BigInt> out;
  if (!v8::BigInt::NewFromWords(
           env->context(), sign_bit, static_cast<int>(word_count), words)
           .ToLocal(&out)) {
    if (tc.HasCaught()) {
      SetLastException(env, tc.Exception(), tc.Message());
      return napi_v8_set_last_error(env, napi_pending_exception, "BigInt creation threw");
    }
    return napi_generic_failure;
  }
  *result = napi_v8_wrap_value(env, out);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_create_date(napi_env env, double time, napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> out;
  if (!v8::Date::New(env->context(), time).ToLocal(&out)) {
    return napi_generic_failure;
  }
  *result = napi_v8_wrap_value(env, out);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_create_object(napi_env env, napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  *result = napi_v8_wrap_value(env, v8::Object::New(env->isolate));
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_create_array(napi_env env, napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  *result = napi_v8_wrap_value(env, v8::Array::New(env->isolate));
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_create_external(napi_env env,
                                            void* data,
                                            node_api_basic_finalize finalize_cb,
                                            void* finalize_hint,
                                            napi_value* result) {
  (void)finalize_cb;
  (void)finalize_hint;
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  *result = napi_v8_wrap_value(env, v8::External::New(env->isolate, data));
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_create_arraybuffer(napi_env env,
                                               size_t byte_length,
                                               void** data,
                                               napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  v8::Local<v8::ArrayBuffer> ab = v8::ArrayBuffer::New(env->isolate, byte_length);
  if (data != nullptr) *data = ab->Data();
  *result = napi_v8_wrap_value(env, ab);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_create_external_arraybuffer(
    napi_env env,
    void* external_data,
    size_t byte_length,
    node_api_basic_finalize finalize_cb,
    void* finalize_hint,
    napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;

  v8::Local<v8::ArrayBuffer> out;
  if (external_data == nullptr && byte_length == 0) {
    out = v8::ArrayBuffer::New(env->isolate, 0);
    out->Detach(v8::Local<v8::Value>()).FromMaybe(false);
  } else {
    if (external_data == nullptr) return napi_invalid_arg;
    auto* hint = new (std::nothrow) napi_external_backing_store_hint__();
    if (hint == nullptr) return napi_generic_failure;
    hint->env = env;
    hint->external_data = external_data;
    hint->finalize_cb = finalize_cb;
    hint->finalize_hint = finalize_hint;
    std::unique_ptr<v8::BackingStore> backing = v8::ArrayBuffer::NewBackingStore(
        external_data, byte_length, ExternalBackingStoreDeleter, hint);
    if (!backing) {
      delete hint;
      return napi_generic_failure;
    }
    out = v8::ArrayBuffer::New(env->isolate, std::move(backing));
  }

  *result = napi_v8_wrap_value(env, out);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_is_typedarray(napi_env env, napi_value value, bool* result) {
  if (!CheckEnv(env) || value == nullptr || result == nullptr) return napi_invalid_arg;
  *result = napi_v8_unwrap_value(value)->IsTypedArray();
  return napi_ok;
}

napi_status NAPI_CDECL napi_create_typedarray(napi_env env,
                                              napi_typedarray_type type,
                                              size_t length,
                                              napi_value arraybuffer,
                                              size_t byte_offset,
                                              napi_value* result) {
  if (!CheckEnv(env) || arraybuffer == nullptr || result == nullptr) return InvalidArg(env);
  v8::Local<v8::Value> local = napi_v8_unwrap_value(arraybuffer);
  if (!local->IsArrayBuffer()) return napi_arraybuffer_expected;

  const char* ctor_name = TypedArrayConstructorName(type);
  if (ctor_name == nullptr) return InvalidArg(env);

  v8::TryCatch tc(env->isolate);
  v8::Local<v8::String> key;
  if (!v8::String::NewFromUtf8(env->isolate, ctor_name, v8::NewStringType::kNormal).ToLocal(&key)) {
    return napi_generic_failure;
  }
  v8::Local<v8::Value> ctor_value;
  if (!env->context()->Global()->Get(env->context(), key).ToLocal(&ctor_value) ||
      !ctor_value->IsFunction()) {
    return napi_generic_failure;
  }
  v8::Local<v8::Value> args[3] = {
      local,
      v8::Integer::NewFromUnsigned(env->isolate, static_cast<uint32_t>(byte_offset)),
      v8::Integer::NewFromUnsigned(env->isolate, static_cast<uint32_t>(length)),
  };
  v8::Local<v8::Object> view;
  if (!ctor_value.As<v8::Function>()->NewInstance(env->context(), 3, args).ToLocal(&view)) {
    return ReturnPendingIfCaught(env, tc, "Failed to create TypedArray");
  }
  *result = napi_v8_wrap_value(env, view);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_get_typedarray_info(napi_env env,
                                                napi_value typedarray,
                                                napi_typedarray_type* type,
                                                size_t* length,
                                                void** data,
                                                napi_value* arraybuffer,
                                                size_t* byte_offset) {
  if (!CheckEnv(env) || typedarray == nullptr) return InvalidArg(env);
  v8::Local<v8::Value> local = napi_v8_unwrap_value(typedarray);
  if (!local->IsTypedArray()) return napi_invalid_arg;
  v8::Local<v8::TypedArray> ta = local.As<v8::TypedArray>();

  if (type != nullptr && !GetTypedArrayType(local, type)) return napi_generic_failure;
  if (length != nullptr) *length = ta->Length();
  if (byte_offset != nullptr) *byte_offset = ta->ByteOffset();
  if (data != nullptr) {
    size_t offset = ta->ByteOffset();
    void* buffer_data = ta->Buffer()->Data();
    *data = (buffer_data == nullptr) ? nullptr : static_cast<void*>(static_cast<uint8_t*>(buffer_data) + offset);
  }
  if (arraybuffer != nullptr) {
    *arraybuffer = napi_v8_wrap_value(env, ta->Buffer());
    if (*arraybuffer == nullptr) return napi_generic_failure;
  }
  return napi_ok;
}

napi_status NAPI_CDECL napi_detach_arraybuffer(napi_env env, napi_value arraybuffer) {
  if (!CheckEnv(env) || arraybuffer == nullptr) return InvalidArg(env);
  v8::Local<v8::Value> value = napi_v8_unwrap_value(arraybuffer);
  if (!value->IsArrayBuffer()) return napi_arraybuffer_expected;
  if (value.As<v8::ArrayBuffer>()->Detach(v8::Local<v8::Value>()).FromMaybe(false)) {
    return napi_ok;
  }
  return napi_generic_failure;
}

napi_status NAPI_CDECL napi_is_detached_arraybuffer(napi_env env,
                                                    napi_value value,
                                                    bool* result) {
  if (!CheckEnv(env) || value == nullptr || result == nullptr) return InvalidArg(env);
  v8::Local<v8::Value> local = napi_v8_unwrap_value(value);
  if (!local->IsArrayBuffer()) return napi_arraybuffer_expected;
  *result = local.As<v8::ArrayBuffer>()->WasDetached();
  return napi_ok;
}

napi_status NAPI_CDECL napi_create_array_with_length(napi_env env,
                                                     size_t length,
                                                     napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  auto context = env->context();
  uint32_t length32 = static_cast<uint32_t>(
      length > static_cast<size_t>(std::numeric_limits<uint32_t>::max())
          ? std::numeric_limits<uint32_t>::max()
          : length);
  v8::Local<v8::Array> arr = v8::Array::New(env->isolate);
  if (length32 > 0) {
    if (!arr
             ->Set(context,
                   v8::String::NewFromUtf8Literal(env->isolate, "length"),
                   v8::Integer::NewFromUnsigned(env->isolate, length32))
             .FromMaybe(false)) {
      return napi_generic_failure;
    }
  }
  *result = napi_v8_wrap_value(env, arr);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_create_string_utf8(napi_env env,
                                               const char* str,
                                               size_t length,
                                               napi_value* result) {
  if (!CheckEnv(env)) return napi_invalid_arg;
  if (result == nullptr) return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  if (str == nullptr) {
    if (length != 0) return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
    str = "";
  }
  if (length == NAPI_AUTO_LENGTH) {
    length = std::strlen(str);
  }
  if (length > static_cast<size_t>(INT_MAX)) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  const int v8Length = static_cast<int>(length);
  v8::MaybeLocal<v8::String> maybe =
      v8::String::NewFromUtf8(env->isolate, str, v8::NewStringType::kNormal, v8Length);
  v8::Local<v8::String> out;
  if (!maybe.ToLocal(&out)) return napi_v8_set_last_error(env, napi_generic_failure, "Cannot create string");
  *result = napi_v8_wrap_value(env, out);
  return (*result == nullptr) ? napi_generic_failure : napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_create_string_latin1(napi_env env,
                                                 const char* str,
                                                 size_t length,
                                                 napi_value* result) {
  if (!CheckEnv(env)) return napi_invalid_arg;
  if (result == nullptr) return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  if (str == nullptr) {
    if (length != 0) return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
    str = "";
  }
  if (length == NAPI_AUTO_LENGTH) {
    length = std::strlen(str);
  }
  if (length > static_cast<size_t>(INT_MAX)) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::MaybeLocal<v8::String> maybe = v8::String::NewFromOneByte(
      env->isolate,
      reinterpret_cast<const uint8_t*>(str),
      v8::NewStringType::kNormal,
      static_cast<int>(length));
  v8::Local<v8::String> out;
  if (!maybe.ToLocal(&out)) return napi_v8_set_last_error(env, napi_generic_failure, "Cannot create string");
  *result = napi_v8_wrap_value(env, out);
  return (*result == nullptr) ? napi_generic_failure : napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_create_string_utf16(napi_env env,
                                                const char16_t* str,
                                                size_t length,
                                                napi_value* result) {
  if (!CheckEnv(env)) return napi_invalid_arg;
  if (result == nullptr) return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  if (str == nullptr) {
    if (length != 0) return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
    static const char16_t empty[] = {0};
    str = empty;
  }
  if (length == NAPI_AUTO_LENGTH) {
    const char16_t* p = str;
    while (*p != 0) ++p;
    length = static_cast<size_t>(p - str);
  }
  if (length > static_cast<size_t>(INT_MAX)) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::MaybeLocal<v8::String> maybe = v8::String::NewFromTwoByte(
      env->isolate,
      reinterpret_cast<const uint16_t*>(str),
      v8::NewStringType::kNormal,
      static_cast<int>(length));
  v8::Local<v8::String> out;
  if (!maybe.ToLocal(&out)) return napi_v8_set_last_error(env, napi_generic_failure, "Cannot create string");
  *result = napi_v8_wrap_value(env, out);
  return (*result == nullptr) ? napi_generic_failure : napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL node_api_create_external_string_latin1(
    napi_env env,
    char* str,
    size_t length,
    node_api_basic_finalize finalize_callback,
    void* finalize_hint,
    napi_value* result,
    bool* copied) {
  (void)finalize_callback;
  (void)finalize_hint;
  if (copied != nullptr) *copied = false;
  return napi_create_string_latin1(env, str, length, result);
}

napi_status NAPI_CDECL node_api_create_external_string_utf16(
    napi_env env,
    char16_t* str,
    size_t length,
    node_api_basic_finalize finalize_callback,
    void* finalize_hint,
    napi_value* result,
    bool* copied) {
  (void)finalize_callback;
  (void)finalize_hint;
  if (copied != nullptr) *copied = false;
  return napi_create_string_utf16(env, str, length, result);
}

napi_status NAPI_CDECL node_api_create_property_key_latin1(
    napi_env env, const char* str, size_t length, napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  if (str == nullptr) {
    if (length != 0) return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
    str = "";
  }
  if (length == NAPI_AUTO_LENGTH) {
    length = std::strlen(str);
  }
  if (length > static_cast<size_t>(INT_MAX)) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::String> out;
  if (!v8::String::NewFromOneByte(
           env->isolate,
           reinterpret_cast<const uint8_t*>(str),
           v8::NewStringType::kInternalized,
           static_cast<int>(length))
           .ToLocal(&out)) {
    return napi_generic_failure;
  }
  *result = napi_v8_wrap_value(env, out);
  return (*result == nullptr) ? napi_generic_failure : napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL node_api_create_property_key_utf8(
    napi_env env, const char* str, size_t length, napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  if (str == nullptr) {
    if (length != 0) return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
    str = "";
  }
  if (length == NAPI_AUTO_LENGTH) {
    length = std::strlen(str);
  }
  if (length > static_cast<size_t>(INT_MAX)) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::String> out;
  if (!v8::String::NewFromUtf8(
           env->isolate,
           str,
           v8::NewStringType::kInternalized,
           static_cast<int>(length))
           .ToLocal(&out)) {
    return napi_generic_failure;
  }
  *result = napi_v8_wrap_value(env, out);
  return (*result == nullptr) ? napi_generic_failure : napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL node_api_create_property_key_utf16(
    napi_env env, const char16_t* str, size_t length, napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  if (str == nullptr) {
    if (length != 0) return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
    static const char16_t empty[] = {0};
    str = empty;
  }
  if (length == NAPI_AUTO_LENGTH) {
    const char16_t* p = str;
    while (*p != 0) ++p;
    length = static_cast<size_t>(p - str);
  }
  if (length > static_cast<size_t>(INT_MAX)) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::String> out;
  if (!v8::String::NewFromTwoByte(
           env->isolate,
           reinterpret_cast<const uint16_t*>(str),
           v8::NewStringType::kInternalized,
           static_cast<int>(length))
           .ToLocal(&out)) {
    return napi_generic_failure;
  }
  *result = napi_v8_wrap_value(env, out);
  return (*result == nullptr) ? napi_generic_failure : napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_create_symbol(napi_env env,
                                          napi_value description,
                                          napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> desc_value = v8::Undefined(env->isolate);
  if (description != nullptr) {
    if (!CheckValue(env, description)) return napi_invalid_arg;
    desc_value = napi_v8_unwrap_value(description);
    if (!desc_value->IsString()) return napi_string_expected;
  }
  v8::Local<v8::Symbol> sym = v8::Symbol::New(
      env->isolate, desc_value->IsString() ? desc_value.As<v8::String>() : v8::Local<v8::String>());
  *result = napi_v8_wrap_value(env, sym);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL node_api_symbol_for(napi_env env,
                                           const char* utf8description,
                                           size_t length,
                                           napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  if (utf8description == nullptr && length > 0) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  const char* desc = (utf8description == nullptr) ? "" : utf8description;
  const int v8_length = (length == NAPI_AUTO_LENGTH) ? -1 : static_cast<int>(length);
  v8::Local<v8::String> key;
  if (!v8::String::NewFromUtf8(env->isolate, desc, v8::NewStringType::kNormal, v8_length)
           .ToLocal(&key)) {
    return napi_generic_failure;
  }
  *result = napi_v8_wrap_value(env, v8::Symbol::For(env->isolate, key));
  return (*result == nullptr) ? napi_generic_failure : napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_typeof(napi_env env,
                                   napi_value value,
                                   napi_valuetype* result) {
  if (!CheckValue(env, value) || result == nullptr) return napi_invalid_arg;
  *result = TypeOf(napi_v8_unwrap_value(value));
  return napi_ok;
}

napi_status NAPI_CDECL napi_get_value_double(napi_env env,
                                             napi_value value,
                                             double* result) {
  if (!CheckEnv(env) || value == nullptr || result == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::Value> local = napi_v8_unwrap_value(value);
  if (!local->IsNumber()) {
    return napi_v8_set_last_error(env, napi_number_expected, "A number was expected");
  }
  *result = local.As<v8::Number>()->Value();
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_get_value_uint32(napi_env env,
                                             napi_value value,
                                             uint32_t* result) {
  if (!CheckEnv(env) || value == nullptr || result == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::Value> local = napi_v8_unwrap_value(value);
  if (!local->IsNumber()) {
    return napi_v8_set_last_error(env, napi_number_expected, "A number was expected");
  }
  *result = local->Uint32Value(env->context()).FromMaybe(0);
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_get_value_int32(napi_env env,
                                            napi_value value,
                                            int32_t* result) {
  if (!CheckEnv(env) || value == nullptr || result == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::Value> local = napi_v8_unwrap_value(value);
  if (!local->IsNumber()) {
    return napi_v8_set_last_error(env, napi_number_expected, "A number was expected");
  }
  *result = local->Int32Value(env->context()).FromMaybe(0);
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_get_value_int64(napi_env env,
                                            napi_value value,
                                            int64_t* result) {
  if (!CheckEnv(env) || value == nullptr || result == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::Value> local = napi_v8_unwrap_value(value);
  if (local->IsInt32()) {
    *result = local.As<v8::Int32>()->Value();
    return napi_v8_clear_last_error(env);
  }
  if (!local->IsNumber()) {
    return napi_v8_set_last_error(env, napi_number_expected, "A number was expected");
  }
  // Match Node's behavior: non-finite converts to 0, and finite values
  // use V8 IntegerValue conversion (including out-of-range sentinel values).
  double double_value = local.As<v8::Number>()->Value();
  if (std::isfinite(double_value)) {
    v8::Local<v8::Context> empty_context;
    *result = local->IntegerValue(empty_context).FromJust();
  } else {
    *result = 0;
  }
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_get_value_bigint_int64(napi_env env,
                                                   napi_value value,
                                                   int64_t* result,
                                                   bool* lossless) {
  if (!CheckEnv(env) || value == nullptr || result == nullptr || lossless == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::Value> local = napi_v8_unwrap_value(value);
  if (!local->IsBigInt()) {
    return napi_v8_set_last_error(env, napi_bigint_expected, "A bigint was expected");
  }
  *result = local.As<v8::BigInt>()->Int64Value(lossless);
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_get_value_bigint_uint64(napi_env env,
                                                    napi_value value,
                                                    uint64_t* result,
                                                    bool* lossless) {
  if (!CheckEnv(env) || value == nullptr || result == nullptr || lossless == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::Value> local = napi_v8_unwrap_value(value);
  if (!local->IsBigInt()) {
    return napi_v8_set_last_error(env, napi_bigint_expected, "A bigint was expected");
  }
  *result = local.As<v8::BigInt>()->Uint64Value(lossless);
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_get_value_bigint_words(napi_env env,
                                                   napi_value value,
                                                   int* sign_bit,
                                                   size_t* word_count,
                                                   uint64_t* words) {
  if (!CheckEnv(env) || value == nullptr || word_count == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::Value> local = napi_v8_unwrap_value(value);
  if (!local->IsBigInt()) {
    return napi_v8_set_last_error(env, napi_bigint_expected, "A bigint was expected");
  }
  v8::Local<v8::BigInt> bigint = local.As<v8::BigInt>();
  int sign = 0;
  int wc = static_cast<int>(bigint->WordCount());
  if (words == nullptr) {
    if (sign_bit != nullptr) {
      int tmp_count = wc;
      uint64_t dummy_word = 0;
      uint64_t* tmp_words = (tmp_count > 0) ? &dummy_word : nullptr;
      bigint->ToWordsArray(&sign, &tmp_count, tmp_words);
      *sign_bit = sign;
    }
    *word_count = static_cast<size_t>(wc);
    return napi_v8_clear_last_error(env);
  }
  int requested = (*word_count > static_cast<size_t>(INT_MAX))
                      ? INT_MAX
                      : static_cast<int>(*word_count);
  bigint->ToWordsArray(&sign, &requested, words);
  if (sign_bit != nullptr) *sign_bit = sign;
  *word_count = static_cast<size_t>(requested);
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_is_date(napi_env env, napi_value value, bool* is_date) {
  if (!CheckEnv(env) || value == nullptr || is_date == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  *is_date = napi_v8_unwrap_value(value)->IsDate();
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_get_date_value(napi_env env, napi_value value, double* result) {
  if (!CheckEnv(env) || value == nullptr || result == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::Value> local = napi_v8_unwrap_value(value);
  if (!local->IsDate()) {
    return napi_v8_set_last_error(env, napi_date_expected, "A date was expected");
  }
  *result = local.As<v8::Date>()->ValueOf();
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_is_arraybuffer(napi_env env, napi_value value, bool* result) {
  if (!CheckEnv(env) || value == nullptr || result == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  *result = napi_v8_unwrap_value(value)->IsArrayBuffer();
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_get_arraybuffer_info(napi_env env,
                                                 napi_value arraybuffer,
                                                 void** data,
                                                 size_t* byte_length) {
  if (!CheckEnv(env) || arraybuffer == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::Value> value = napi_v8_unwrap_value(arraybuffer);
  if (value->IsArrayBuffer()) {
    v8::Local<v8::ArrayBuffer> ab = value.As<v8::ArrayBuffer>();
    if (data != nullptr) *data = ab->Data();
    if (byte_length != nullptr) *byte_length = ab->ByteLength();
    return napi_v8_clear_last_error(env);
  }
  if (value->IsSharedArrayBuffer()) {
    v8::Local<v8::SharedArrayBuffer> sab = value.As<v8::SharedArrayBuffer>();
    if (data != nullptr) *data = sab->Data();
    if (byte_length != nullptr) *byte_length = sab->ByteLength();
    return napi_v8_clear_last_error(env);
  }
  return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
}

napi_status NAPI_CDECL node_api_is_sharedarraybuffer(node_api_basic_env env,
                                                     napi_value value,
                                                     bool* result) {
  auto* napiEnv = const_cast<napi_env>(env);
  if (!CheckEnv(napiEnv) || value == nullptr || result == nullptr) {
    return napi_v8_set_last_error(napiEnv, napi_invalid_arg, "Invalid argument");
  }
  *result = napi_v8_unwrap_value(value)->IsSharedArrayBuffer();
  return napi_v8_clear_last_error(napiEnv);
}

napi_status NAPI_CDECL node_api_create_sharedarraybuffer(napi_env env,
                                                         size_t byte_length,
                                                         void** data,
                                                         napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::SharedArrayBuffer> sab;
  if (!v8::SharedArrayBuffer::MaybeNew(env->isolate, byte_length).ToLocal(&sab)) {
    return napi_v8_set_last_error(env, napi_generic_failure, "Failed to create SharedArrayBuffer");
  }
  if (data != nullptr) *data = sab->Data();
  *result = napi_v8_wrap_value(env, sab);
  if (*result == nullptr) {
    return napi_v8_set_last_error(env, napi_generic_failure, "Failed to create SharedArrayBuffer");
  }
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_create_dataview(napi_env env,
                                            size_t length,
                                            napi_value arraybuffer,
                                            size_t byte_offset,
                                            napi_value* result) {
  if (!CheckEnv(env) || arraybuffer == nullptr || result == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::Value> ab = napi_v8_unwrap_value(arraybuffer);
  if (!ab->IsArrayBuffer() && !ab->IsSharedArrayBuffer()) {
    return napi_v8_set_last_error(env, napi_arraybuffer_expected, "ArrayBuffer expected");
  }

  v8::TryCatch tc(env->isolate);
  auto context = env->context();
  v8::Local<v8::String> ctor_name = v8::String::NewFromUtf8Literal(env->isolate, "DataView");
  v8::Local<v8::Value> ctor_val;
  if (!context->Global()->Get(context, ctor_name).ToLocal(&ctor_val) || !ctor_val->IsFunction()) {
    return napi_generic_failure;
  }
  v8::Local<v8::Function> ctor = ctor_val.As<v8::Function>();
  v8::Local<v8::Value> args[3] = {
      ab,
      v8::Integer::NewFromUnsigned(env->isolate, static_cast<uint32_t>(byte_offset)),
      v8::Integer::NewFromUnsigned(env->isolate, static_cast<uint32_t>(length)),
  };
  v8::Local<v8::Object> out;
  if (!ctor->NewInstance(context, 3, args).ToLocal(&out)) {
    if (tc.HasCaught()) {
      SetLastException(env, tc.Exception(), tc.Message());
      return napi_v8_set_last_error(env, napi_pending_exception, "DataView construction threw");
    }
    return napi_generic_failure;
  }
  *result = napi_v8_wrap_value(env, out);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_is_dataview(napi_env env, napi_value value, bool* result) {
  if (!CheckEnv(env) || value == nullptr || result == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  *result = napi_v8_unwrap_value(value)->IsDataView();
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_get_dataview_info(napi_env env,
                                              napi_value dataview,
                                              size_t* byte_length,
                                              void** data,
                                              napi_value* arraybuffer,
                                              size_t* byte_offset) {
  if (!CheckEnv(env) || dataview == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::Value> view_val = napi_v8_unwrap_value(dataview);
  if (!view_val->IsDataView()) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::DataView> view = view_val.As<v8::DataView>();
  if (byte_length != nullptr) *byte_length = view->ByteLength();
  if (byte_offset != nullptr) *byte_offset = view->ByteOffset();
  if (data != nullptr) {
    const size_t offset = view->ByteOffset();
    void* buffer_data = view->Buffer()->Data();
    *data = (buffer_data == nullptr) ? nullptr
                                     : static_cast<void*>(static_cast<uint8_t*>(buffer_data) + offset);
  }
  if (arraybuffer != nullptr) {
    *arraybuffer = napi_v8_wrap_value(env, view->Buffer());
    if (*arraybuffer == nullptr) return napi_generic_failure;
  }
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_is_array(napi_env env, napi_value value, bool* result) {
  if (!CheckValue(env, value) || result == nullptr) return napi_invalid_arg;
  *result = napi_v8_unwrap_value(value)->IsArray();
  return napi_ok;
}

napi_status NAPI_CDECL napi_get_array_length(napi_env env,
                                             napi_value value,
                                             uint32_t* result) {
  if (!CheckValue(env, value) || result == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> local = napi_v8_unwrap_value(value);
  if (!local->IsArray()) return napi_array_expected;
  *result = local.As<v8::Array>()->Length();
  return napi_ok;
}

napi_status NAPI_CDECL napi_get_element(napi_env env,
                                        napi_value object,
                                        uint32_t index,
                                        napi_value* result) {
  if (!CheckValue(env, object) || result == nullptr) return InvalidArg(env);
  v8::Local<v8::Value> local = napi_v8_unwrap_value(object);
  if (!local->IsObject()) return napi_object_expected;
  v8::TryCatch tc(env->isolate);
  v8::Local<v8::Value> out;
  if (!local.As<v8::Object>()->Get(env->context(), index).ToLocal(&out)) {
    return ReturnPendingIfCaught(env, tc, "Exception while getting element");
  }
  *result = napi_v8_wrap_value(env, out);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_set_element(napi_env env,
                                        napi_value object,
                                        uint32_t index,
                                        napi_value value) {
  if (!CheckValue(env, object) || value == nullptr) return InvalidArg(env);
  v8::Local<v8::Value> local = napi_v8_unwrap_value(object);
  if (!local->IsObject()) return napi_object_expected;
  v8::TryCatch tc(env->isolate);
  if (!local.As<v8::Object>()
           ->Set(env->context(), index, napi_v8_unwrap_value(value))
           .FromMaybe(false)) {
    return ReturnPendingIfCaught(env, tc, "Exception while setting element");
  }
  return napi_ok;
}

napi_status NAPI_CDECL napi_instanceof(napi_env env,
                                       napi_value object,
                                       napi_value constructor,
                                       bool* result) {
  if (!CheckValue(env, object) || !CheckValue(env, constructor) || result == nullptr) {
    return napi_invalid_arg;
  }
  v8::Local<v8::Value> ctor = napi_v8_unwrap_value(constructor);
  if (!ctor->IsFunction()) return napi_function_expected;
  *result = napi_v8_unwrap_value(object)
                ->InstanceOf(env->context(), ctor.As<v8::Object>())
                .FromMaybe(false);
  return napi_ok;
}

napi_status NAPI_CDECL napi_has_element(napi_env env,
                                        napi_value object,
                                        uint32_t index,
                                        bool* result) {
  if (!CheckValue(env, object) || result == nullptr) return InvalidArg(env);
  v8::Local<v8::Value> local = napi_v8_unwrap_value(object);
  if (!local->IsObject()) return napi_object_expected;
  v8::TryCatch tc(env->isolate);
  auto has = local.As<v8::Object>()->Has(env->context(), index);
  if (has.IsNothing()) {
    return ReturnPendingIfCaught(env, tc, "Exception while checking element");
  }
  *result = has.FromJust();
  return napi_ok;
}

napi_status NAPI_CDECL napi_delete_element(napi_env env,
                                           napi_value object,
                                           uint32_t index,
                                           bool* result) {
  if (!CheckValue(env, object)) return InvalidArg(env);
  v8::Local<v8::Value> local = napi_v8_unwrap_value(object);
  if (!local->IsObject()) return napi_object_expected;
  v8::TryCatch tc(env->isolate);
  auto deleted = local.As<v8::Object>()->Delete(env->context(), index);
  if (deleted.IsNothing()) {
    return ReturnPendingIfCaught(env, tc, "Exception while deleting element");
  }
  if (result != nullptr) {
    *result = deleted.FromJust();
  }
  return napi_ok;
}

napi_status NAPI_CDECL napi_get_cb_info(napi_env env,
                                        napi_callback_info cbinfo,
                                        size_t* argc,
                                        napi_value* argv,
                                        napi_value* this_arg,
                                        void** data) {
  if (!CheckEnv(env) || cbinfo == nullptr) return napi_invalid_arg;
  size_t provided = (argc == nullptr) ? 0 : *argc;
  if (argc != nullptr) {
    *argc = cbinfo->args.size();
  }
  if (argv != nullptr && provided > 0) {
    const size_t n = (provided < cbinfo->args.size()) ? provided : cbinfo->args.size();
    for (size_t i = 0; i < n; ++i) argv[i] = cbinfo->args[i];
    for (size_t i = n; i < provided; ++i) {
      argv[i] = napi_v8_wrap_value(env, v8::Undefined(env->isolate));
    }
  }
  if (this_arg != nullptr) *this_arg = cbinfo->this_arg;
  if (data != nullptr) *data = cbinfo->data;
  return napi_ok;
}

napi_status NAPI_CDECL napi_get_new_target(
    napi_env env, napi_callback_info cbinfo, napi_value* result) {
  if (!CheckEnv(env) || cbinfo == nullptr || result == nullptr) {
    return napi_invalid_arg;
  }
  *result = cbinfo->new_target;
  return napi_ok;
}

napi_status NAPI_CDECL napi_open_handle_scope(napi_env env, napi_handle_scope* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  auto* scope = new (std::nothrow) napi_handle_scope__();
  if (scope == nullptr) return napi_generic_failure;
  scope->env = env;
  *result = scope;
  return napi_ok;
}

napi_status NAPI_CDECL napi_close_handle_scope(napi_env env, napi_handle_scope scope) {
  if (!CheckEnv(env) || scope == nullptr) return napi_invalid_arg;
  delete scope;
  return napi_ok;
}

napi_status NAPI_CDECL napi_open_escapable_handle_scope(
    napi_env env, napi_escapable_handle_scope* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  auto* scope = new (std::nothrow) napi_escapable_handle_scope__();
  if (scope == nullptr) return napi_generic_failure;
  scope->env = env;
  *result = scope;
  return napi_ok;
}

napi_status NAPI_CDECL napi_close_escapable_handle_scope(
    napi_env env, napi_escapable_handle_scope scope) {
  if (!CheckEnv(env) || scope == nullptr) return napi_invalid_arg;
  delete scope;
  return napi_ok;
}

napi_status NAPI_CDECL napi_escape_handle(napi_env env,
                                          napi_escapable_handle_scope scope,
                                          napi_value escapee,
                                          napi_value* result) {
  if (!CheckEnv(env) || scope == nullptr || escapee == nullptr || result == nullptr) {
    return napi_invalid_arg;
  }
  if (scope->escaped) return napi_escape_called_twice;
  scope->escaped = true;
  *result = escapee;
  return napi_ok;
}

napi_status NAPI_CDECL napi_create_function(napi_env env,
                                            const char* utf8name,
                                            size_t length,
                                            napi_callback cb,
                                            void* data,
                                            napi_value* result) {
  if (!CheckEnv(env)) return napi_invalid_arg;
  if (cb == nullptr || result == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  auto* payload = new (std::nothrow) CallbackPayload{env, cb, data};
  if (payload == nullptr) return napi_generic_failure;

  v8::Local<v8::External> payloadValue = v8::External::New(env->isolate, payload);
  v8::Local<v8::Context> context = env->context();

  v8::Local<v8::String> name;
  if (utf8name != nullptr) {
    const int v8Length =
        (length == NAPI_AUTO_LENGTH) ? -1 : static_cast<int>(length);
    v8::MaybeLocal<v8::String> maybeName =
        v8::String::NewFromUtf8(env->isolate, utf8name, v8::NewStringType::kNormal, v8Length);
    if (!maybeName.ToLocal(&name)) return napi_generic_failure;
  }

  v8::MaybeLocal<v8::Function> maybeFn = v8::Function::New(
      context, FunctionTrampoline, payloadValue);
  v8::Local<v8::Function> fn;
  if (!maybeFn.ToLocal(&fn)) return napi_generic_failure;
  if (!name.IsEmpty()) fn->SetName(name);

  *result = napi_v8_wrap_value(env, fn);
  if (*result == nullptr) return napi_generic_failure;
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_define_class(napi_env env,
                                         const char* utf8name,
                                         size_t length,
                                         napi_callback constructor,
                                         void* data,
                                         size_t property_count,
                                         const napi_property_descriptor* properties,
                                         napi_value* result) {
  if (!CheckEnv(env)) {
    return napi_invalid_arg;
  }
  if (utf8name == nullptr || constructor == nullptr || result == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  if (property_count > 0 && properties == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }

  v8::Local<v8::Context> context = env->context();
  auto* payload = new (std::nothrow) CallbackPayload{env, constructor, data};
  if (payload == nullptr) return napi_generic_failure;

  const int v8_length = (length == NAPI_AUTO_LENGTH) ? -1 : static_cast<int>(length);
  v8::Local<v8::String> name;
  if (!v8::String::NewFromUtf8(env->isolate,
                               utf8name,
                               v8::NewStringType::kNormal,
                               v8_length)
           .ToLocal(&name)) {
    return napi_generic_failure;
  }

  // Use a FunctionTemplate so instances created through napi_define_class()
  // are V8 API objects, matching Node's host-object behavior for wrapped
  // internal classes such as JSStream.
  v8::Local<v8::FunctionTemplate> ctor_template =
      v8::FunctionTemplate::New(env->isolate,
                                FunctionTrampoline,
                                v8::External::New(env->isolate, payload));
  ctor_template->SetClassName(name);
  ctor_template->InstanceTemplate()->SetInternalFieldCount(1);

  v8::Local<v8::Function> ctor;
  if (!ctor_template->GetFunction(context).ToLocal(&ctor)) {
    return napi_generic_failure;
  }

  napi_value ctorValue = napi_v8_wrap_value(env, ctor);
  if (ctorValue == nullptr) return napi_generic_failure;
  v8::Local<v8::Object> proto = ctor->Get(context, v8::String::NewFromUtf8Literal(env->isolate, "prototype"))
                                     .ToLocalChecked()
                                     .As<v8::Object>();

  for (size_t i = 0; i < property_count; ++i) {
    const napi_property_descriptor& desc = properties[i];
    napi_status status = napi_ok;
    v8::Local<v8::Name> key;
    if (desc.utf8name != nullptr) {
      v8::Local<v8::String> key_str;
      if (!v8::String::NewFromUtf8(env->isolate, desc.utf8name, v8::NewStringType::kNormal)
               .ToLocal(&key_str)) {
        return napi_generic_failure;
      }
      key = key_str;
    } else if (desc.name != nullptr) {
      v8::Local<v8::Value> name_value = napi_v8_unwrap_value(desc.name);
      if (!name_value->IsName()) return napi_name_expected;
      key = name_value.As<v8::Name>();
    } else {
      return napi_name_expected;
    }
    v8::Local<v8::Object> target =
        (desc.attributes & napi_static) ? ctor.As<v8::Object>() : proto;

    if (desc.method != nullptr) {
      napi_value fnValue = nullptr;
      status = napi_create_function(
          env, desc.utf8name, NAPI_AUTO_LENGTH, desc.method, desc.data, &fnValue);
      if (status != napi_ok) return status;
      if (!target->DefineOwnProperty(
               context,
               key,
               napi_v8_unwrap_value(fnValue),
               ToV8PropertyAttributes(desc.attributes, true))
               .FromMaybe(false)) {
        return napi_generic_failure;
      }
      continue;
    }

    if (desc.getter != nullptr || desc.setter != nullptr) {
      v8::Local<v8::Function> getter_fn;
      v8::Local<v8::Function> setter_fn;
      if (desc.getter != nullptr) {
        napi_value getter_value = nullptr;
        status = napi_create_function(
            env, desc.utf8name, NAPI_AUTO_LENGTH, desc.getter, desc.data, &getter_value);
        if (status != napi_ok) return status;
        getter_fn = napi_v8_unwrap_value(getter_value).As<v8::Function>();
      }
      if (desc.setter != nullptr) {
        napi_value setter_value = nullptr;
        status = napi_create_function(
            env, desc.utf8name, NAPI_AUTO_LENGTH, desc.setter, desc.data, &setter_value);
        if (status != napi_ok) return status;
        setter_fn = napi_v8_unwrap_value(setter_value).As<v8::Function>();
      }
      target->SetAccessorProperty(
          key,
          getter_fn,
          setter_fn,
          ToV8PropertyAttributes(desc.attributes, false));
      continue;
    }

    if (desc.value != nullptr) {
      if (!target->DefineOwnProperty(
               context,
               key,
               napi_v8_unwrap_value(desc.value),
               ToV8PropertyAttributes(desc.attributes, true))
               .FromMaybe(false)) {
        return napi_generic_failure;
      }
      continue;
    }
  }

  *result = ctorValue;
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_new_instance(napi_env env,
                                         napi_value constructor,
                                         size_t argc,
                                         const napi_value* argv,
                                         napi_value* result) {
  if (!CheckValue(env, constructor) || result == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> ctorValue = napi_v8_unwrap_value(constructor);
  if (!ctorValue->IsFunction()) return napi_function_expected;
  v8::Local<v8::Function> ctor = ctorValue.As<v8::Function>();
  std::vector<v8::Local<v8::Value>> args;
  args.reserve(argc);
  for (size_t i = 0; i < argc; ++i) args.push_back(napi_v8_unwrap_value(argv[i]));
  v8::Local<v8::Value> out;
  v8::TryCatch tryCatch(env->isolate);
  if (!ctor->NewInstance(env->context(), static_cast<int>(argc), args.data())
           .ToLocal(&out)) {
    if (tryCatch.HasCaught()) {
      SetLastException(env, tryCatch.Exception(), tryCatch.Message());
      return napi_v8_set_last_error(env, napi_pending_exception, "Constructor threw");
    }
    return napi_generic_failure;
  }
  *result = napi_v8_wrap_value(env, out);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_call_function(napi_env env,
                                          napi_value recv,
                                          napi_value func,
                                          size_t argc,
                                          const napi_value* argv,
                                          napi_value* result) {
  if (!CheckValue(env, recv) || !CheckValue(env, func)) return napi_invalid_arg;
  auto context = env->context();
  v8::Local<v8::Function> fn;
  if (!napi_v8_unwrap_value(func)->IsFunction()) return napi_function_expected;
  fn = napi_v8_unwrap_value(func).As<v8::Function>();

  std::vector<v8::Local<v8::Value>> args;
  args.reserve(argc);
  for (size_t i = 0; i < argc; ++i) {
    args.push_back(napi_v8_unwrap_value(argv[i]));
  }

  v8::TryCatch tryCatch(env->isolate);
  v8::MaybeLocal<v8::Value> maybe = fn->Call(
      context, napi_v8_unwrap_value(recv), argc, args.data());
  if (tryCatch.HasCaught()) {
    SetLastException(env, tryCatch.Exception(), tryCatch.Message());
    return napi_v8_set_last_error(env, napi_pending_exception, "Function call threw");
  }
  if (result != nullptr) {
    v8::Local<v8::Value> out;
    if (!maybe.ToLocal(&out)) return napi_generic_failure;
    *result = napi_v8_wrap_value(env, out);
  }
  return napi_ok;
}

napi_status NAPI_CDECL napi_define_properties(
    napi_env env,
    napi_value object,
    size_t property_count,
    const napi_property_descriptor* properties) {
  if (!CheckValue(env, object) || properties == nullptr) return InvalidArg(env);
  auto context = env->context();
  v8::Local<v8::Value> targetValue = napi_v8_unwrap_value(object);
  if (!targetValue->IsObject()) return napi_object_expected;
  v8::Local<v8::Object> target = targetValue.As<v8::Object>();

  v8::TryCatch tc(env->isolate);
  for (size_t i = 0; i < property_count; ++i) {
    const napi_property_descriptor& desc = properties[i];
    v8::Local<v8::Name> key;
    if (desc.utf8name != nullptr) {
      v8::Local<v8::String> key_str;
      if (!v8::String::NewFromUtf8(
               env->isolate, desc.utf8name, v8::NewStringType::kNormal)
               .ToLocal(&key_str)) {
        return napi_generic_failure;
      }
      key = key_str;
    } else if (desc.name != nullptr) {
      v8::Local<v8::Value> name_value = napi_v8_unwrap_value(desc.name);
      if (!name_value->IsName()) return napi_name_expected;
      key = name_value.As<v8::Name>();
    } else {
      return napi_name_expected;
    }

    if (desc.method != nullptr) {
      napi_value fnValue = nullptr;
      napi_status status = napi_create_function(
          env, desc.utf8name, NAPI_AUTO_LENGTH, desc.method, desc.data, &fnValue);
      if (status != napi_ok) return status;
      if (!target->DefineOwnProperty(
               context,
               key,
               napi_v8_unwrap_value(fnValue),
               ToV8PropertyAttributes(desc.attributes, true))
               .FromMaybe(false)) {
        return ReturnPendingIfCaught(env, tc, "Exception while defining property");
      }
      continue;
    }

    if (desc.getter != nullptr || desc.setter != nullptr) {
      napi_status status = napi_ok;
      v8::Local<v8::Function> getter_fn;
      v8::Local<v8::Function> setter_fn;
      if (desc.getter != nullptr) {
        napi_value getter_value = nullptr;
        status = napi_create_function(
            env, desc.utf8name, NAPI_AUTO_LENGTH, desc.getter, desc.data, &getter_value);
        if (status != napi_ok) return status;
        getter_fn = napi_v8_unwrap_value(getter_value).As<v8::Function>();
      }
      if (desc.setter != nullptr) {
        napi_value setter_value = nullptr;
        status = napi_create_function(
            env, desc.utf8name, NAPI_AUTO_LENGTH, desc.setter, desc.data, &setter_value);
        if (status != napi_ok) return status;
        setter_fn = napi_v8_unwrap_value(setter_value).As<v8::Function>();
      }
      target->SetAccessorProperty(
          key,
          getter_fn,
          setter_fn,
          ToV8PropertyAttributes(desc.attributes, false));
      continue;
    }

    if (desc.value != nullptr) {
      if (!target->DefineOwnProperty(
               context,
               key,
               napi_v8_unwrap_value(desc.value),
               ToV8PropertyAttributes(desc.attributes, true))
               .FromMaybe(false)) {
        return ReturnPendingIfCaught(env, tc, "Exception while defining property");
      }
    }
  }

  return napi_ok;
}

napi_status NAPI_CDECL napi_create_promise(napi_env env,
                                           napi_deferred* deferred,
                                           napi_value* promise) {
  if (!CheckEnv(env) || deferred == nullptr || promise == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::TryCatch tc(env->isolate);
  v8::Local<v8::Promise::Resolver> resolver;
  if (!v8::Promise::Resolver::New(env->context()).ToLocal(&resolver)) {
    return ReturnPendingIfCaught(env, tc, "Failed to create Promise resolver");
  }
  auto* d = new (std::nothrow) napi_deferred__();
  if (d == nullptr) return napi_generic_failure;
  d->env = env;
  d->resolver.Reset(env->isolate, resolver);
  *deferred = d;
  *promise = napi_v8_wrap_value(env, resolver->GetPromise());
  if (*promise == nullptr) {
    delete d;
    *deferred = nullptr;
    return napi_generic_failure;
  }
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_resolve_deferred(napi_env env,
                                             napi_deferred deferred,
                                             napi_value resolution) {
  if (!CheckEnv(env) || deferred == nullptr || resolution == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::TryCatch tc(env->isolate);
  v8::Local<v8::Promise::Resolver> resolver = deferred->resolver.Get(env->isolate);
  if (!resolver->Resolve(env->context(), napi_v8_unwrap_value(resolution)).FromMaybe(false)) {
    return ReturnPendingIfCaught(env, tc, "Failed to resolve promise");
  }
  delete deferred;
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_reject_deferred(napi_env env,
                                            napi_deferred deferred,
                                            napi_value rejection) {
  if (!CheckEnv(env) || deferred == nullptr || rejection == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::TryCatch tc(env->isolate);
  v8::Local<v8::Promise::Resolver> resolver = deferred->resolver.Get(env->isolate);
  if (!resolver->Reject(env->context(), napi_v8_unwrap_value(rejection)).FromMaybe(false)) {
    return ReturnPendingIfCaught(env, tc, "Failed to reject promise");
  }
  delete deferred;
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_is_promise(napi_env env, napi_value value, bool* is_promise) {
  if (!CheckEnv(env) || value == nullptr || is_promise == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  *is_promise = napi_v8_unwrap_value(value)->IsPromise();
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_has_named_property(napi_env env,
                                               napi_value object,
                                               const char* utf8name,
                                               bool* result) {
  if (!CheckValue(env, object) || utf8name == nullptr || result == nullptr) {
    return InvalidArg(env);
  }
  auto context = env->context();
  v8::Local<v8::Value> targetValue = napi_v8_unwrap_value(object);
  if (!targetValue->IsObject()) return napi_object_expected;
  v8::Local<v8::String> key;
  if (!v8::String::NewFromUtf8(
           env->isolate, utf8name, v8::NewStringType::kNormal)
           .ToLocal(&key)) {
    return napi_generic_failure;
  }
  v8::TryCatch tc(env->isolate);
  auto has = targetValue.As<v8::Object>()->Has(context, key);
  if (has.IsNothing()) {
    return ReturnPendingIfCaught(env, tc, "Exception while checking named property");
  }
  *result = has.FromJust();
  return napi_ok;
}

napi_status NAPI_CDECL napi_set_property(napi_env env,
                                         napi_value object,
                                         napi_value key,
                                         napi_value value) {
  if (!CheckValue(env, object) || !CheckValue(env, key) || value == nullptr) {
    return InvalidArg(env);
  }
  v8::Local<v8::Value> target = napi_v8_unwrap_value(object);
  if (!target->IsObject()) return napi_object_expected;
  v8::TryCatch tc(env->isolate);
  if (!target.As<v8::Object>()
           ->Set(env->context(), napi_v8_unwrap_value(key), napi_v8_unwrap_value(value))
           .FromMaybe(false)) {
    return ReturnPendingIfCaught(env, tc, "Exception while setting property");
  }
  return napi_ok;
}

napi_status NAPI_CDECL napi_get_property(napi_env env,
                                         napi_value object,
                                         napi_value key,
                                         napi_value* result) {
  if (!CheckValue(env, object) || !CheckValue(env, key) || result == nullptr) {
    return InvalidArg(env);
  }
  v8::Local<v8::Value> target = napi_v8_unwrap_value(object);
  if (!target->IsObject()) return napi_object_expected;
  v8::TryCatch tc(env->isolate);
  v8::Local<v8::Value> out;
  if (!target.As<v8::Object>()->Get(env->context(), napi_v8_unwrap_value(key)).ToLocal(&out)) {
    return ReturnPendingIfCaught(env, tc, "Exception while getting property");
  }
  *result = napi_v8_wrap_value(env, out);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_has_property(napi_env env,
                                         napi_value object,
                                         napi_value key,
                                         bool* result) {
  if (!CheckValue(env, object) || !CheckValue(env, key) || result == nullptr) {
    return InvalidArg(env);
  }
  v8::Local<v8::Value> target = napi_v8_unwrap_value(object);
  if (!target->IsObject()) return napi_object_expected;
  v8::TryCatch tc(env->isolate);
  auto has = target.As<v8::Object>()->Has(env->context(), napi_v8_unwrap_value(key));
  if (has.IsNothing()) {
    return ReturnPendingIfCaught(env, tc, "Exception while checking property");
  }
  *result = has.FromJust();
  return napi_ok;
}

napi_status NAPI_CDECL napi_delete_property(napi_env env,
                                            napi_value object,
                                            napi_value key,
                                            bool* result) {
  if (!CheckValue(env, object) || !CheckValue(env, key)) {
    return InvalidArg(env);
  }
  v8::Local<v8::Value> target = napi_v8_unwrap_value(object);
  if (!target->IsObject()) return napi_object_expected;
  v8::TryCatch tc(env->isolate);
  auto deleted = target.As<v8::Object>()->Delete(env->context(), napi_v8_unwrap_value(key));
  if (deleted.IsNothing()) {
    return ReturnPendingIfCaught(env, tc, "Exception while deleting property");
  }
  if (result != nullptr) {
    *result = deleted.FromJust();
  }
  return napi_ok;
}

napi_status NAPI_CDECL napi_has_own_property(napi_env env,
                                             napi_value object,
                                             napi_value key,
                                             bool* result) {
  if (!CheckValue(env, object) || !CheckValue(env, key) || result == nullptr) {
    return InvalidArg(env);
  }
  v8::Local<v8::Value> key_value = napi_v8_unwrap_value(key);
  if (!key_value->IsName()) {
    return napi_v8_set_last_error(env, napi_name_expected, "A string or symbol was expected");
  }
  v8::Local<v8::Value> target = napi_v8_unwrap_value(object);
  if (!target->IsObject()) return napi_object_expected;
  v8::TryCatch tc(env->isolate);
  auto has = target.As<v8::Object>()->HasOwnProperty(env->context(), key_value.As<v8::Name>());
  if (has.IsNothing()) {
    return ReturnPendingIfCaught(env, tc, "Exception while checking own property");
  }
  *result = has.FromJust();
  return napi_ok;
}

napi_status NAPI_CDECL napi_get_property_names(napi_env env,
                                               napi_value object,
                                               napi_value* result) {
  if (!CheckValue(env, object) || result == nullptr) return InvalidArg(env);
  v8::Local<v8::Value> target = napi_v8_unwrap_value(object);
  if (!target->IsObject()) return napi_object_expected;
  v8::TryCatch tc(env->isolate);
  v8::Local<v8::Array> names;
  if (!target.As<v8::Object>()
           ->GetPropertyNames(env->context(),
                              v8::KeyCollectionMode::kIncludePrototypes,
                              static_cast<v8::PropertyFilter>(v8::ONLY_ENUMERABLE | v8::SKIP_SYMBOLS),
                              v8::IndexFilter::kIncludeIndices,
                              v8::KeyConversionMode::kConvertToString)
           .ToLocal(&names)) {
    return ReturnPendingIfCaught(env, tc, "Exception while getting property names");
  }
  *result = napi_v8_wrap_value(env, names);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_get_all_property_names(napi_env env,
                                                   napi_value object,
                                                   napi_key_collection_mode key_mode,
                                                   napi_key_filter key_filter,
                                                   napi_key_conversion key_conversion,
                                                   napi_value* result) {
  if (!CheckValue(env, object) || result == nullptr) return InvalidArg(env);
  v8::Local<v8::Value> target = napi_v8_unwrap_value(object);
  if (!target->IsObject()) return napi_object_expected;

  v8::KeyCollectionMode collection_mode =
      (key_mode == napi_key_own_only) ? v8::KeyCollectionMode::kOwnOnly
                                      : v8::KeyCollectionMode::kIncludePrototypes;
  int property_filter = v8::ALL_PROPERTIES;
  if ((key_filter & napi_key_writable) != 0) property_filter |= v8::ONLY_WRITABLE;
  if ((key_filter & napi_key_enumerable) != 0) property_filter |= v8::ONLY_ENUMERABLE;
  if ((key_filter & napi_key_configurable) != 0) property_filter |= v8::ONLY_CONFIGURABLE;
  if ((key_filter & napi_key_skip_strings) != 0) property_filter |= v8::SKIP_STRINGS;
  if ((key_filter & napi_key_skip_symbols) != 0) property_filter |= v8::SKIP_SYMBOLS;

  v8::KeyConversionMode conversion_mode =
      (key_conversion == napi_key_keep_numbers) ? v8::KeyConversionMode::kKeepNumbers
                                                : v8::KeyConversionMode::kConvertToString;

  v8::TryCatch tc(env->isolate);
  v8::Local<v8::Array> names;
  if (!target.As<v8::Object>()
           ->GetPropertyNames(env->context(),
                              collection_mode,
                              static_cast<v8::PropertyFilter>(property_filter),
                              v8::IndexFilter::kIncludeIndices,
                              conversion_mode)
           .ToLocal(&names)) {
    return ReturnPendingIfCaught(env, tc, "Exception while getting all property names");
  }
  *result = napi_v8_wrap_value(env, names);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_set_named_property(napi_env env,
                                               napi_value object,
                                               const char* utf8name,
                                               napi_value value) {
  if (!CheckValue(env, object) || utf8name == nullptr || value == nullptr) {
    return InvalidArg(env);
  }
  auto context = env->context();
  v8::Local<v8::Value> targetValue = napi_v8_unwrap_value(object);
  if (!targetValue->IsObject()) return napi_object_expected;
  v8::Local<v8::String> key;
  if (!v8::String::NewFromUtf8(
           env->isolate, utf8name, v8::NewStringType::kNormal)
           .ToLocal(&key)) {
    return napi_generic_failure;
  }
  v8::TryCatch tc(env->isolate);
  if (!targetValue.As<v8::Object>()
           ->Set(context, key, napi_v8_unwrap_value(value))
           .FromMaybe(false)) {
    return ReturnPendingIfCaught(env, tc, "Exception while setting named property");
  }
  return napi_ok;
}

napi_status NAPI_CDECL napi_get_named_property(napi_env env,
                                               napi_value object,
                                               const char* utf8name,
                                               napi_value* result) {
  if (!CheckValue(env, object) || utf8name == nullptr || result == nullptr) {
    return InvalidArg(env);
  }
  auto context = env->context();
  v8::Local<v8::Value> targetValue = napi_v8_unwrap_value(object);
  if (!targetValue->IsObject()) return napi_object_expected;
  v8::Local<v8::String> key;
  if (!v8::String::NewFromUtf8(
           env->isolate, utf8name, v8::NewStringType::kNormal)
           .ToLocal(&key)) {
    return napi_generic_failure;
  }
  v8::TryCatch tc(env->isolate);
  v8::Local<v8::Value> prop;
  if (!targetValue.As<v8::Object>()->Get(context, key).ToLocal(&prop)) {
    return ReturnPendingIfCaught(env, tc, "Exception while getting named property");
  }
  *result = napi_v8_wrap_value(env, prop);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_get_prototype(napi_env env,
                                          napi_value object,
                                          napi_value* result) {
  if (!CheckValue(env, object) || result == nullptr) return InvalidArg(env);
  v8::Local<v8::Value> target = napi_v8_unwrap_value(object);
  if (!target->IsObject()) return napi_object_expected;
  v8::Local<v8::Value> proto = target.As<v8::Object>()->GetPrototypeV2();
  *result = napi_v8_wrap_value(env, proto);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL node_api_set_prototype(napi_env env,
                                              napi_value object,
                                              napi_value value) {
  if (!CheckValue(env, object) || !CheckValue(env, value)) return napi_invalid_arg;
  v8::Local<v8::Value> target = napi_v8_unwrap_value(object);
  if (!target->IsObject()) return napi_object_expected;
  if (!target.As<v8::Object>()
           ->SetPrototypeV2(env->context(), napi_v8_unwrap_value(value))
           .FromMaybe(false)) {
    return napi_generic_failure;
  }
  return napi_ok;
}

napi_status NAPI_CDECL napi_get_value_bool(napi_env env,
                                           napi_value value,
                                           bool* result) {
  if (!CheckEnv(env) || value == nullptr || result == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::Value> local = napi_v8_unwrap_value(value);
  if (!local->IsBoolean()) {
    return napi_v8_set_last_error(env, napi_boolean_expected, "A boolean was expected");
  }
  *result = local.As<v8::Boolean>()->Value();
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_get_value_string_utf8(
    napi_env env, napi_value value, char* buf, size_t bufsize, size_t* result) {
  if (!CheckEnv(env) || value == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::Value> local = napi_v8_unwrap_value(value);
  if (!local->IsString()) {
    return napi_v8_set_last_error(env, napi_string_expected, "A string was expected");
  }
  v8::Local<v8::String> str = local.As<v8::String>();
  if (buf == nullptr) {
    if (result == nullptr) {
      return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
    }
    *result = str->Utf8LengthV2(env->isolate);
  } else if (bufsize != 0) {
    size_t copied = str->WriteUtf8V2(env->isolate,
                                     buf,
                                     bufsize - 1,
                                     v8::String::WriteFlags::kReplaceInvalidUtf8);
    buf[copied] = '\0';
    if (result != nullptr) *result = copied;
  } else if (result != nullptr) {
    *result = 0;
  }
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_get_value_string_latin1(
    napi_env env, napi_value value, char* buf, size_t bufsize, size_t* result) {
  if (!CheckEnv(env) || value == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::Value> local = napi_v8_unwrap_value(value);
  if (!local->IsString()) {
    return napi_v8_set_last_error(env, napi_string_expected, "A string was expected");
  }
  v8::Local<v8::String> str = local.As<v8::String>();
  if (buf == nullptr) {
    if (result == nullptr) {
      return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
    }
    *result = str->Length();
  } else if (bufsize != 0) {
    uint32_t length = static_cast<uint32_t>(
        std::min(bufsize - 1, static_cast<size_t>(str->Length())));
    str->WriteOneByteV2(env->isolate,
                        0,
                        length,
                        reinterpret_cast<uint8_t*>(buf),
                        v8::String::WriteFlags::kNullTerminate);
    if (result != nullptr) *result = length;
  } else if (result != nullptr) {
    *result = 0;
  }
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_get_value_string_utf16(napi_env env,
                                                   napi_value value,
                                                   char16_t* buf,
                                                   size_t bufsize,
                                                   size_t* result) {
  if (!CheckEnv(env) || value == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::Local<v8::Value> local = napi_v8_unwrap_value(value);
  if (!local->IsString()) {
    return napi_v8_set_last_error(env, napi_string_expected, "A string was expected");
  }
  v8::Local<v8::String> str = local.As<v8::String>();
  if (buf == nullptr) {
    if (result == nullptr) {
      return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
    }
    *result = str->Length();
  } else if (bufsize != 0) {
    uint32_t length = static_cast<uint32_t>(
        std::min(bufsize - 1, static_cast<size_t>(str->Length())));
    str->WriteV2(env->isolate,
                 0,
                 length,
                 reinterpret_cast<uint16_t*>(buf),
                 v8::String::WriteFlags::kNullTerminate);
    if (result != nullptr) *result = length;
  } else if (result != nullptr) {
    *result = 0;
  }
  return napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_coerce_to_bool(napi_env env,
                                           napi_value value,
                                           napi_value* result) {
  if (!CheckEnv(env) || value == nullptr || result == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  *result = napi_v8_wrap_value(
      env, v8::Boolean::New(env->isolate, napi_v8_unwrap_value(value)->BooleanValue(env->isolate)));
  return (*result == nullptr) ? napi_generic_failure : napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_coerce_to_number(napi_env env,
                                             napi_value value,
                                             napi_value* result) {
  if (!CheckEnv(env) || value == nullptr || result == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::TryCatch try_catch(env->isolate);
  v8::Local<v8::Number> out;
  if (!napi_v8_unwrap_value(value)->ToNumber(env->context()).ToLocal(&out)) {
    if (try_catch.HasCaught()) {
      SetLastException(env, try_catch.Exception(), try_catch.Message());
    }
    return napi_v8_set_last_error(env, napi_pending_exception, "Exception during number coercion");
  }
  *result = napi_v8_wrap_value(env, out);
  return (*result == nullptr) ? napi_generic_failure : napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_coerce_to_object(napi_env env,
                                             napi_value value,
                                             napi_value* result) {
  if (!CheckEnv(env) || value == nullptr || result == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::TryCatch try_catch(env->isolate);
  v8::Local<v8::Object> out;
  if (!napi_v8_unwrap_value(value)->ToObject(env->context()).ToLocal(&out)) {
    if (try_catch.HasCaught()) {
      SetLastException(env, try_catch.Exception(), try_catch.Message());
    }
    return napi_v8_set_last_error(env, napi_pending_exception, "Exception during object coercion");
  }
  *result = napi_v8_wrap_value(env, out);
  return (*result == nullptr) ? napi_generic_failure : napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_coerce_to_string(napi_env env,
                                             napi_value value,
                                             napi_value* result) {
  if (!CheckEnv(env) || value == nullptr || result == nullptr) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  v8::TryCatch try_catch(env->isolate);
  v8::Local<v8::String> out;
  if (!napi_v8_unwrap_value(value)->ToString(env->context()).ToLocal(&out)) {
    if (try_catch.HasCaught()) {
      SetLastException(env, try_catch.Exception(), try_catch.Message());
    }
    return napi_v8_set_last_error(env, napi_pending_exception, "Exception during string coercion");
  }
  *result = napi_v8_wrap_value(env, out);
  return (*result == nullptr) ? napi_generic_failure : napi_v8_clear_last_error(env);
}

napi_status NAPI_CDECL napi_get_value_external(napi_env env,
                                               napi_value value,
                                               void** result) {
  if (!CheckValue(env, value) || result == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> local = napi_v8_unwrap_value(value);
  if (!local->IsExternal()) return napi_invalid_arg;
  *result = local.As<v8::External>()->Value();
  return napi_ok;
}

napi_status NAPI_CDECL napi_strict_equals(napi_env env,
                                          napi_value lhs,
                                          napi_value rhs,
                                          bool* result) {
  if (!CheckValue(env, lhs) || !CheckValue(env, rhs) || result == nullptr) {
    return napi_invalid_arg;
  }
  *result = napi_v8_unwrap_value(lhs)->StrictEquals(napi_v8_unwrap_value(rhs));
  return napi_ok;
}

napi_status NAPI_CDECL napi_create_reference(napi_env env,
                                             napi_value value,
                                             uint32_t initial_refcount,
                                             napi_ref* result) {
  if (!CheckValue(env, value) || result == nullptr) return napi_invalid_arg;
  *result = new (std::nothrow)
      napi_ref__(env, napi_v8_unwrap_value(value), initial_refcount);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_delete_reference(node_api_basic_env env, napi_ref ref) {
  (void)env;
  if (ref == nullptr) return napi_invalid_arg;
  // If this weak reference is being deleted while a GC pass is active, V8 may
  // still have a queued weak callback for it. Clearing/resetting the handle and
  // keeping the bookkeeping object alive avoids a use-after-free.
  if (ref->can_be_weak && ref->refcount == 0) {
    if (!ref->value.IsEmpty()) {
      ref->value.ClearWeak();
      ref->value.Reset();
    }
    return napi_ok;
  }
  delete ref;
  return napi_ok;
}

napi_status NAPI_CDECL napi_reference_ref(napi_env env,
                                          napi_ref ref,
                                          uint32_t* result) {
  if (!CheckEnv(env) || ref == nullptr) return napi_invalid_arg;
  if (ref->value.IsEmpty()) {
    if (result != nullptr) *result = 0;
    return napi_ok;
  }
  ref->refcount++;
  if (ref->refcount == 1 && ref->can_be_weak) {
    ref->value.ClearWeak();
  }
  if (result != nullptr) *result = ref->refcount;
  return napi_ok;
}

napi_status NAPI_CDECL napi_reference_unref(napi_env env,
                                            napi_ref ref,
                                            uint32_t* result) {
  if (!CheckEnv(env) || ref == nullptr) return napi_invalid_arg;
  if (!ref->value.IsEmpty() && ref->refcount > 0) {
    ref->refcount--;
    if (ref->refcount == 0 && ref->can_be_weak) {
      ref->value.SetWeak(ref, ReferenceWeakCallback, v8::WeakCallbackType::kParameter);
    }
  }
  if (result != nullptr) *result = ref->refcount;
  return napi_ok;
}

napi_status NAPI_CDECL napi_get_reference_value(napi_env env,
                                                napi_ref ref,
                                                napi_value* result) {
  if (!CheckEnv(env) || ref == nullptr || result == nullptr) return napi_invalid_arg;
  if (ref->value.IsEmpty()) {
    *result = nullptr;
    return napi_ok;
  }
  *result = napi_v8_wrap_value(env, ref->value.Get(env->isolate));
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_wrap(napi_env env,
                                 napi_value js_object,
                                 void* native_object,
                                 node_api_basic_finalize finalize_cb,
                                 void* finalize_hint,
                                 napi_ref* result) {
  if (!CheckValue(env, js_object)) return napi_invalid_arg;
  v8::Local<v8::Value> value = napi_v8_unwrap_value(js_object);
  if (!value->IsObject()) return napi_object_expected;
  v8::Local<v8::Object> object = value.As<v8::Object>();
  v8::Local<v8::Private> wrapKey = env->wrap_private_key.Get(env->isolate);
  v8::Local<v8::Value> existing;
  if (object->GetPrivate(env->context(), wrapKey).ToLocal(&existing) &&
      existing->IsExternal()) {
    return napi_v8_set_last_error(env, napi_invalid_arg, "Invalid argument");
  }
  if (!object->SetPrivate(env->context(), wrapKey, v8::External::New(env->isolate, native_object))
           .FromMaybe(false)) {
    return napi_generic_failure;
  }
  v8::Local<v8::Private> wrapFinalizeKey = env->wrap_finalizer_private_key.Get(env->isolate);
  if (finalize_cb != nullptr) {
    auto* record = new (std::nothrow) WrapFinalizerRecord();
    if (record == nullptr) return napi_generic_failure;
    record->env = env;
    record->native_object = native_object;
    record->finalize_cb = finalize_cb;
    record->finalize_hint = finalize_hint;
    record->handle.Reset(env->isolate, object);
    record->handle.SetWeak(record, WrapWeakCallback, v8::WeakCallbackType::kParameter);
    env->wrap_finalizers.push_back(record);
    object
        ->SetPrivate(env->context(), wrapFinalizeKey, v8::External::New(env->isolate, record))
        .FromMaybe(false);
  } else {
    object->DeletePrivate(env->context(), wrapFinalizeKey).FromMaybe(false);
  }
  if (result != nullptr) {
    napi_status s = napi_create_reference(env, js_object, 0, result);
    if (s != napi_ok) return s;
    v8::Local<v8::Private> wrapRefKey = env->wrap_ref_private_key.Get(env->isolate);
    object
        ->SetPrivate(env->context(), wrapRefKey, v8::External::New(env->isolate, *result))
        .FromMaybe(false);
  }
  return napi_ok;
}

napi_status NAPI_CDECL napi_unwrap(napi_env env, napi_value js_object, void** result) {
  if (!CheckValue(env, js_object) || result == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> value = napi_v8_unwrap_value(js_object);
  if (!value->IsObject()) return napi_object_expected;
  v8::Local<v8::Object> object = value.As<v8::Object>();
  v8::Local<v8::Private> wrapKey = env->wrap_private_key.Get(env->isolate);
  v8::Local<v8::Value> wrapped;
  if (!object->GetPrivate(env->context(), wrapKey).ToLocal(&wrapped) ||
      !wrapped->IsExternal()) {
    return napi_invalid_arg;
  }
  *result = wrapped.As<v8::External>()->Value();
  return napi_ok;
}

napi_status NAPI_CDECL napi_remove_wrap(napi_env env, napi_value js_object, void** result) {
  if (!CheckValue(env, js_object)) return napi_invalid_arg;
  void* out = nullptr;
  napi_status status = napi_unwrap(env, js_object, &out);
  if (status != napi_ok) return status;
  v8::Local<v8::Object> object = napi_v8_unwrap_value(js_object).As<v8::Object>();
  v8::Local<v8::Private> wrapKey = env->wrap_private_key.Get(env->isolate);
  object->DeletePrivate(env->context(), wrapKey).FromMaybe(false);
  v8::Local<v8::Private> wrapRefKey = env->wrap_ref_private_key.Get(env->isolate);
  object->DeletePrivate(env->context(), wrapRefKey).FromMaybe(false);
  v8::Local<v8::Private> wrapFinalizeKey = env->wrap_finalizer_private_key.Get(env->isolate);
  v8::Local<v8::Value> finalizeValue;
  if (object->GetPrivate(env->context(), wrapFinalizeKey).ToLocal(&finalizeValue) &&
      finalizeValue->IsExternal()) {
    auto* record = static_cast<WrapFinalizerRecord*>(finalizeValue.As<v8::External>()->Value());
    if (record != nullptr) {
      record->cancelled = true;
      record->handle.Reset();
      RemoveWrapFinalizerRecord(env, record);
      delete record;
    }
  }
  object->DeletePrivate(env->context(), wrapFinalizeKey).FromMaybe(false);
  if (result != nullptr) *result = out;
  return napi_ok;
}

napi_status NAPI_CDECL napi_throw_error(napi_env env,
                                        const char* code,
                                        const char* msg) {
  if (!CheckEnv(env)) return napi_invalid_arg;
  v8::Local<v8::String> message;
  if (!v8::String::NewFromUtf8(
           env->isolate, (msg == nullptr) ? "N-API error" : msg, v8::NewStringType::kNormal)
           .ToLocal(&message)) {
    return napi_generic_failure;
  }
  v8::Local<v8::Object> err_obj = v8::Exception::Error(message).As<v8::Object>();
  if (code != nullptr) {
    v8::Local<v8::String> code_key = v8::String::NewFromUtf8Literal(env->isolate, "code");
    v8::Local<v8::String> code_val;
    if (v8::String::NewFromUtf8(env->isolate, code, v8::NewStringType::kNormal).ToLocal(&code_val)) {
      err_obj->Set(env->context(), code_key, code_val).FromMaybe(false);
    }
  }
  env->isolate->ThrowException(err_obj);
  SetLastException(env, err_obj);
  return napi_pending_exception;
}

napi_status NAPI_CDECL napi_throw(napi_env env, napi_value error) {
  if (!CheckValue(env, error)) return napi_invalid_arg;
  v8::Local<v8::Value> ex = napi_v8_unwrap_value(error);
  env->isolate->ThrowException(ex);
  SetLastException(env, ex);
  return napi_pending_exception;
}

napi_status NAPI_CDECL napi_is_error(napi_env env, napi_value value, bool* result) {
  if (!CheckValue(env, value) || result == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> v = napi_v8_unwrap_value(value);
  *result = v->IsNativeError();
  return napi_ok;
}

static napi_status CreateErrorCommon(napi_env env,
                                     v8::Local<v8::Value> (*factory)(v8::Local<v8::String>),
                                     napi_value code,
                                     napi_value msg,
                                     napi_value* result) {
  if (!CheckEnv(env) || msg == nullptr || result == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> msg_val = napi_v8_unwrap_value(msg);
  if (!msg_val->IsString()) return napi_string_expected;
  v8::Local<v8::String> message = msg_val.As<v8::String>();
  v8::Local<v8::Value> created = factory(message);
  if (!created->IsObject()) return napi_generic_failure;
  v8::Local<v8::Object> err_obj = created.As<v8::Object>();
  if (code != nullptr) {
    v8::Local<v8::String> code_key = v8::String::NewFromUtf8Literal(env->isolate, "code");
    err_obj->Set(env->context(), code_key, napi_v8_unwrap_value(code)).FromMaybe(false);
  }
  *result = napi_v8_wrap_value(env, err_obj);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_create_error(napi_env env,
                                         napi_value code,
                                         napi_value msg,
                                         napi_value* result) {
  return CreateErrorCommon(
      env,
      [](v8::Local<v8::String> message) { return v8::Exception::Error(message); },
      code,
      msg,
      result);
}

napi_status NAPI_CDECL napi_create_type_error(napi_env env,
                                              napi_value code,
                                              napi_value msg,
                                              napi_value* result) {
  return CreateErrorCommon(
      env,
      [](v8::Local<v8::String> message) { return v8::Exception::TypeError(message); },
      code,
      msg,
      result);
}

napi_status NAPI_CDECL napi_create_range_error(napi_env env,
                                               napi_value code,
                                               napi_value msg,
                                               napi_value* result) {
  return CreateErrorCommon(
      env,
      [](v8::Local<v8::String> message) { return v8::Exception::RangeError(message); },
      code,
      msg,
      result);
}

napi_status NAPI_CDECL node_api_create_syntax_error(napi_env env,
                                                    napi_value code,
                                                    napi_value msg,
                                                    napi_value* result) {
  return CreateErrorCommon(
      env,
      [](v8::Local<v8::String> message) { return v8::Exception::SyntaxError(message); },
      code,
      msg,
      result);
}

napi_status NAPI_CDECL napi_throw_type_error(napi_env env,
                                             const char* code,
                                             const char* msg) {
  if (!CheckEnv(env)) return napi_invalid_arg;
  v8::Local<v8::String> message;
  if (!v8::String::NewFromUtf8(env->isolate,
                               (msg == nullptr) ? "Type error" : msg,
                               v8::NewStringType::kNormal)
           .ToLocal(&message)) {
    return napi_generic_failure;
  }
  v8::Local<v8::Object> err = v8::Exception::TypeError(message).As<v8::Object>();
  if (code != nullptr) {
    v8::Local<v8::String> code_key = v8::String::NewFromUtf8Literal(env->isolate, "code");
    v8::Local<v8::String> code_val;
    if (v8::String::NewFromUtf8(env->isolate, code, v8::NewStringType::kNormal).ToLocal(&code_val)) {
      err->Set(env->context(), code_key, code_val).FromMaybe(false);
    }
  }
  env->isolate->ThrowException(err);
  SetLastException(env, err);
  return napi_pending_exception;
}

napi_status NAPI_CDECL napi_throw_range_error(napi_env env,
                                              const char* code,
                                              const char* msg) {
  if (!CheckEnv(env)) return napi_invalid_arg;
  v8::Local<v8::String> message;
  if (!v8::String::NewFromUtf8(env->isolate,
                               (msg == nullptr) ? "Range error" : msg,
                               v8::NewStringType::kNormal)
           .ToLocal(&message)) {
    return napi_generic_failure;
  }
  v8::Local<v8::Object> err = v8::Exception::RangeError(message).As<v8::Object>();
  if (code != nullptr) {
    v8::Local<v8::String> code_key = v8::String::NewFromUtf8Literal(env->isolate, "code");
    v8::Local<v8::String> code_val;
    if (v8::String::NewFromUtf8(env->isolate, code, v8::NewStringType::kNormal).ToLocal(&code_val)) {
      err->Set(env->context(), code_key, code_val).FromMaybe(false);
    }
  }
  env->isolate->ThrowException(err);
  SetLastException(env, err);
  return napi_pending_exception;
}

napi_status NAPI_CDECL node_api_throw_syntax_error(napi_env env,
                                                   const char* code,
                                                   const char* msg) {
  if (!CheckEnv(env)) return napi_invalid_arg;
  v8::Local<v8::String> message;
  if (!v8::String::NewFromUtf8(env->isolate,
                               (msg == nullptr) ? "Syntax error" : msg,
                               v8::NewStringType::kNormal)
           .ToLocal(&message)) {
    return napi_generic_failure;
  }
  v8::Local<v8::Object> err = v8::Exception::SyntaxError(message).As<v8::Object>();
  if (code != nullptr) {
    v8::Local<v8::String> code_key = v8::String::NewFromUtf8Literal(env->isolate, "code");
    v8::Local<v8::String> code_val;
    if (v8::String::NewFromUtf8(env->isolate, code, v8::NewStringType::kNormal).ToLocal(&code_val)) {
      err->Set(env->context(), code_key, code_val).FromMaybe(false);
    }
  }
  env->isolate->ThrowException(err);
  SetLastException(env, err);
  return napi_pending_exception;
}

napi_status NAPI_CDECL napi_is_exception_pending(napi_env env, bool* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  *result = !env->last_exception.IsEmpty();
  return napi_ok;
}

napi_status NAPI_CDECL napi_get_and_clear_last_exception(napi_env env,
                                                         napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  if (env->last_exception.IsEmpty()) return napi_generic_failure;
  v8::Local<v8::Value> ex = env->last_exception.Get(env->isolate);
  ClearLastException(env);
  *result = napi_v8_wrap_value(env, ex);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_set_instance_data(node_api_basic_env basic_env,
                                              void* data,
                                              napi_finalize finalize_cb,
                                              void* finalize_hint) {
  napi_env env = const_cast<napi_env>(basic_env);
  if (!CheckEnv(env)) return napi_invalid_arg;
  env->instance_data = data;
  env->instance_data_finalize_cb = finalize_cb;
  env->instance_data_finalize_hint = finalize_hint;
  return napi_ok;
}

napi_status NAPI_CDECL napi_get_instance_data(node_api_basic_env basic_env,
                                              void** data) {
  napi_env env = const_cast<napi_env>(basic_env);
  if (!CheckEnv(env) || data == nullptr) return napi_invalid_arg;
  *data = env->instance_data;
  return napi_ok;
}

napi_status NAPI_CDECL napi_run_script(napi_env env,
                                       napi_value script,
                                       napi_value* result) {
  if (!CheckValue(env, script) || result == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> source = napi_v8_unwrap_value(script);
  if (!source->IsString()) return napi_string_expected;
  v8::TryCatch tc(env->isolate);
  v8::Local<v8::Script> compiled;
  if (!v8::Script::Compile(env->context(), source.As<v8::String>()).ToLocal(&compiled)) {
    if (tc.HasCaught()) {
      SetLastException(env, tc.Exception(), tc.Message());
      return napi_pending_exception;
    }
    return napi_generic_failure;
  }
  v8::Local<v8::Value> out;
  if (!compiled->Run(env->context()).ToLocal(&out)) {
    if (tc.HasCaught()) {
      SetLastException(env, tc.Exception(), tc.Message());
      return napi_pending_exception;
    }
    return napi_generic_failure;
  }
  *result = napi_v8_wrap_value(env, out);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_fatal_exception(napi_env env, napi_value err) {
  if (!CheckEnv(env) || err == nullptr) return napi_invalid_arg;
  SetLastException(env, napi_v8_unwrap_value(err));
  env->isolate->ThrowException(napi_v8_unwrap_value(err));
  return napi_ok;
}

napi_status NAPI_CDECL napi_add_env_cleanup_hook(node_api_basic_env env,
                                                 napi_cleanup_hook fun,
                                                 void* arg) {
  auto* napiEnv = const_cast<napi_env>(env);
  if (!CheckEnv(napiEnv) || fun == nullptr) return napi_invalid_arg;
  auto* entry = new (std::nothrow) napi_env_cleanup_hook__();
  if (entry == nullptr) return napi_generic_failure;
  entry->hook = fun;
  entry->arg = arg;
  napiEnv->env_cleanup_hooks.push_back(entry);
  return napi_ok;
}

napi_status NAPI_CDECL napi_remove_env_cleanup_hook(node_api_basic_env env,
                                                    napi_cleanup_hook fun,
                                                    void* arg) {
  auto* napiEnv = const_cast<napi_env>(env);
  if (!CheckEnv(napiEnv) || fun == nullptr) return napi_invalid_arg;
  auto& hooks = napiEnv->env_cleanup_hooks;
  for (auto it = hooks.begin(); it != hooks.end(); ++it) {
    auto* entry = static_cast<napi_env_cleanup_hook__*>(*it);
    if (entry != nullptr && entry->hook == fun && entry->arg == arg) {
      delete entry;
      hooks.erase(it);
      return napi_ok;
    }
  }
  return napi_invalid_arg;
}

napi_status NAPI_CDECL napi_create_buffer(napi_env env,
                                          size_t length,
                                          void** data,
                                          napi_value* result) {
  if (!CheckEnv(env) || data == nullptr || result == nullptr) return napi_invalid_arg;
  v8::Local<v8::Context> context = env->context();
  v8::Context::Scope context_scope(context);

  auto backing = v8::ArrayBuffer::NewBackingStore(env->isolate, length);
  if (!backing) return napi_generic_failure;
  *data = backing->Data();

  auto* record = new (std::nothrow) napi_buffer_record__();
  if (record == nullptr) return napi_generic_failure;
  record->env = env;
  record->backing_store = std::move(backing);

  v8::Local<v8::Object> buffer_obj = CreateBufferObject(env, record->backing_store, 0, length);
  record->holder.Reset(env->isolate, buffer_obj);
  record->holder.SetWeak(record, BufferWeakCallback, v8::WeakCallbackType::kParameter);
  v8::Local<v8::Private> key = env->buffer_private_key.Get(env->isolate);
  buffer_obj
      ->SetPrivate(env->context(), key, v8::External::New(env->isolate, record))
      .FromJust();
  env->buffer_records.push_back(record);

  *result = napi_v8_wrap_value(env, buffer_obj);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_create_buffer_copy(napi_env env,
                                               size_t length,
                                               const void* data,
                                               void** result_data,
                                               napi_value* result) {
  void* out = nullptr;
  napi_status status = napi_create_buffer(env, length, &out, result);
  if (status != napi_ok) return status;
  if (length > 0 && data != nullptr) {
    std::memcpy(out, data, length);
  }
  if (result_data != nullptr) *result_data = out;
  return napi_ok;
}

napi_status NAPI_CDECL napi_create_external_buffer(napi_env env,
                                                   size_t length,
                                                   void* data,
                                                   node_api_basic_finalize finalize_cb,
                                                   void* finalize_hint,
                                                   napi_value* result) {
  if (!CheckEnv(env) || data == nullptr || result == nullptr) return napi_invalid_arg;
  v8::Local<v8::Context> context = env->context();
  v8::Context::Scope context_scope(context);

  auto* hint = new (std::nothrow) napi_external_backing_store_hint__();
  if (hint == nullptr) return napi_generic_failure;
  hint->env = env;
  hint->external_data = data;
  hint->finalize_cb = finalize_cb;
  hint->finalize_hint = finalize_hint;

  std::unique_ptr<v8::BackingStore> backing =
      v8::ArrayBuffer::NewBackingStore(data, length, ExternalBackingStoreDeleter, hint);
  if (!backing) {
    delete hint;
    return napi_generic_failure;
  }

  auto* record = new (std::nothrow) napi_buffer_record__();
  if (record == nullptr) return napi_generic_failure;
  record->env = env;
  record->backing_store = std::move(backing);

  v8::Local<v8::Object> buffer_obj = CreateBufferObject(env, record->backing_store, 0, length);
  record->holder.Reset(env->isolate, buffer_obj);
  record->holder.SetWeak(record, BufferWeakCallback, v8::WeakCallbackType::kParameter);
  v8::Local<v8::Private> key = env->buffer_private_key.Get(env->isolate);
  buffer_obj
      ->SetPrivate(env->context(), key, v8::External::New(env->isolate, record))
      .FromJust();
  env->buffer_records.push_back(record);

  *result = napi_v8_wrap_value(env, buffer_obj);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_is_buffer(napi_env env, napi_value value, bool* result) {
  if (!CheckEnv(env) || value == nullptr || result == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> raw = napi_v8_unwrap_value(value);
  *result = raw->IsArrayBufferView();
  return napi_ok;
}

napi_status NAPI_CDECL napi_get_buffer_info(napi_env env,
                                            napi_value value,
                                            void** data,
                                            size_t* length) {
  if (!CheckEnv(env) || value == nullptr) return napi_invalid_arg;
  if (!GetArrayBufferViewInfo(napi_v8_unwrap_value(value), data, length)) {
    return napi_invalid_arg;
  }
  return napi_ok;
}

napi_status NAPI_CDECL node_api_create_buffer_from_arraybuffer(
    napi_env env,
    napi_value arraybuffer,
    size_t byte_offset,
    size_t byte_length,
    napi_value* result) {
  if (!CheckEnv(env) || arraybuffer == nullptr || result == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> raw = napi_v8_unwrap_value(arraybuffer);
  if (!raw->IsArrayBuffer()) return napi_invalid_arg;
  v8::Local<v8::ArrayBuffer> ab = raw.As<v8::ArrayBuffer>();
  size_t ab_length = ab->ByteLength();
  if (byte_offset > ab_length || byte_length > (ab_length - byte_offset)) {
    return napi_invalid_arg;
  }

  auto* record = new (std::nothrow) napi_buffer_record__();
  if (record == nullptr) return napi_generic_failure;
  record->env = env;
  record->backing_store = ab->GetBackingStore();

  v8::Local<v8::Object> buffer_obj =
      CreateBufferObject(env, record->backing_store, byte_offset, byte_length);
  record->holder.Reset(env->isolate, buffer_obj);
  record->holder.SetWeak(record, BufferWeakCallback, v8::WeakCallbackType::kParameter);
  v8::Local<v8::Private> key = env->buffer_private_key.Get(env->isolate);
  buffer_obj
      ->SetPrivate(env->context(), key, v8::External::New(env->isolate, record))
      .FromJust();
  env->buffer_records.push_back(record);

  *result = napi_v8_wrap_value(env, buffer_obj);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL napi_adjust_external_memory(
    node_api_basic_env basic_env, int64_t change_in_bytes, int64_t* adjusted_value) {
  napi_env env = const_cast<napi_env>(basic_env);
  if (!CheckEnv(env) || adjusted_value == nullptr) return napi_invalid_arg;
  *adjusted_value = env->isolate->AdjustAmountOfExternalAllocatedMemory(change_in_bytes);
  return napi_ok;
}

napi_status NAPI_CDECL napi_add_finalizer(napi_env env,
                                          napi_value js_object,
                                          void* finalize_data,
                                          node_api_basic_finalize finalize_cb,
                                          void* finalize_hint,
                                          napi_ref* result) {
  if (!CheckValue(env, js_object) || finalize_cb == nullptr) return napi_invalid_arg;

  v8::Local<v8::Value> value = napi_v8_unwrap_value(js_object);
  if (!value->IsObject()) return napi_object_expected;

  auto* record = new (std::nothrow) WrapFinalizerRecord();
  if (record == nullptr) return napi_generic_failure;

  record->env = env;
  record->native_object = finalize_data;
  record->finalize_cb = finalize_cb;
  record->finalize_hint = finalize_hint;
  record->handle.Reset(env->isolate, value.As<v8::Object>());
  record->handle.SetWeak(record, WrapWeakCallback, v8::WeakCallbackType::kParameter);
  env->wrap_finalizers.push_back(record);

  if (result != nullptr) {
    napi_status status = napi_create_reference(env, js_object, 0, result);
    if (status != napi_ok) {
      RemoveWrapFinalizerRecord(env, record);
      record->handle.Reset();
      delete record;
      return status;
    }
  }

  return napi_ok;
}

napi_status NAPI_CDECL napi_get_version(node_api_basic_env env, uint32_t* result) {
  if (result == nullptr) return napi_invalid_arg;
  auto* napiEnv = const_cast<napi_env>(env);
  if (!CheckEnv(napiEnv)) return napi_invalid_arg;
  *result = 10;
  return napi_ok;
}

napi_status NAPI_CDECL napi_object_freeze(napi_env env, napi_value object) {
  if (!CheckValue(env, object)) return napi_invalid_arg;
  v8::Local<v8::Value> target = napi_v8_unwrap_value(object);
  if (!target->IsObject()) return napi_object_expected;
  if (!target.As<v8::Object>()
           ->SetIntegrityLevel(env->context(), v8::IntegrityLevel::kFrozen)
           .FromMaybe(false)) {
    return napi_generic_failure;
  }
  return napi_ok;
}

napi_status NAPI_CDECL napi_object_seal(napi_env env, napi_value object) {
  if (!CheckValue(env, object)) return napi_invalid_arg;
  v8::Local<v8::Value> target = napi_v8_unwrap_value(object);
  if (!target->IsObject()) return napi_object_expected;
  if (!target.As<v8::Object>()
           ->SetIntegrityLevel(env->context(), v8::IntegrityLevel::kSealed)
           .FromMaybe(false)) {
    return napi_generic_failure;
  }
  return napi_ok;
}

napi_status NAPI_CDECL napi_type_tag_object(
    napi_env env, napi_value value, const napi_type_tag* type_tag) {
  if (!CheckValue(env, value) || type_tag == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> target = napi_v8_unwrap_value(value);
  if (!target->IsObject() && !target->IsExternal()) return napi_invalid_arg;

  for (auto& entry : env->type_tag_entries) {
    if (!entry.value.IsEmpty() && entry.value.Get(env->isolate)->StrictEquals(target)) {
      entry.tag = *type_tag;
      return napi_ok;
    }
  }

  napi_env__::TypeTagEntry entry;
  entry.value.Reset(env->isolate, target);
  entry.tag = *type_tag;
  env->type_tag_entries.push_back(std::move(entry));
  return napi_ok;
}

napi_status NAPI_CDECL napi_check_object_type_tag(napi_env env,
                                                  napi_value value,
                                                  const napi_type_tag* type_tag,
                                                  bool* result) {
  if (!CheckValue(env, value) || type_tag == nullptr || result == nullptr) {
    return napi_invalid_arg;
  }
  v8::Local<v8::Value> target = napi_v8_unwrap_value(value);
  if (!target->IsObject() && !target->IsExternal()) {
    *result = false;
    return napi_ok;
  }

  for (auto& entry : env->type_tag_entries) {
    if (entry.value.IsEmpty()) continue;
    if (entry.value.Get(env->isolate)->StrictEquals(target)) {
      *result = (entry.tag.lower == type_tag->lower && entry.tag.upper == type_tag->upper);
      return napi_ok;
    }
  }
  *result = false;
  return napi_ok;
}

napi_status NAPI_CDECL
node_api_create_object_with_properties(napi_env env,
                                       napi_value prototype_or_null,
                                       napi_value* property_names,
                                       napi_value* property_values,
                                       size_t property_count,
                                       napi_value* result) {
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  if ((property_count > 0) && (property_names == nullptr || property_values == nullptr)) {
    return napi_invalid_arg;
  }
  v8::Local<v8::Object> obj = v8::Object::New(env->isolate);
  if (prototype_or_null != nullptr) {
    v8::Local<v8::Value> proto = napi_v8_unwrap_value(prototype_or_null);
    if (!proto->IsNull() && !proto->IsObject()) return napi_object_expected;
    if (!obj->SetPrototypeV2(env->context(), proto).FromMaybe(false)) {
      return napi_generic_failure;
    }
  }
  for (size_t i = 0; i < property_count; ++i) {
    if (property_names[i] == nullptr || property_values[i] == nullptr) return napi_invalid_arg;
    if (!obj
             ->Set(env->context(),
                   napi_v8_unwrap_value(property_names[i]),
                   napi_v8_unwrap_value(property_values[i]))
             .FromMaybe(false)) {
      return napi_generic_failure;
    }
  }
  *result = napi_v8_wrap_value(env, obj);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

}  // extern "C"
