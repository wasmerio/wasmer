#include "unofficial_napi.h"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <ctime>
#include <thread>
#include <cstdio>
#include <cstdlib>
#include <cstdint>
#include <cstring>
#include <memory>
#include <mutex>
#include <new>
#include <optional>
#include <random>
#include <sstream>
#include <unordered_map>
#include <vector>

#include <libplatform/libplatform.h>
#include <v8.h>
#include <v8-profiler.h>

extern "C" uint64_t uv_get_total_memory(void);
extern "C" uint64_t uv_get_constrained_memory(void);
struct uv_loop_s;
using uv_loop_t = struct uv_loop_s;
enum uv_run_mode {
  UV_RUN_DEFAULT = 0,
  UV_RUN_ONCE,
  UV_RUN_NOWAIT
};
extern "C" int uv_run(uv_loop_t* loop, uv_run_mode mode);

#include "internal/node_v8_default_flags.h"
#include "internal/napi_v8_env.h"
#include "internal/unofficial_napi_bridge.h"
#include "unofficial_napi_error_utils.h"
#include "edge_v8_platform.h"

namespace {

struct SharedRuntime {
  std::unique_ptr<EdgeV8Platform> platform;
  uint32_t refcount = 0;
};

class TrackingArrayBufferAllocator;

struct UnofficialEnvScope {
  v8::Isolate* isolate = nullptr;
  std::shared_ptr<TrackingArrayBufferAllocator> allocator;
  std::optional<v8::Isolate::Scope> isolate_scope;
  std::optional<v8::HandleScope> handle_scope;
  std::optional<v8::Global<v8::Context>> context;
  std::optional<v8::Context::Scope> context_scope;
  napi_env env = nullptr;

  explicit UnofficialEnvScope(
      v8::Isolate* isolate_in,
      std::shared_ptr<TrackingArrayBufferAllocator> allocator_in)
      : isolate(isolate_in), allocator(std::move(allocator_in)) {
    isolate_scope.emplace(isolate_in);
    handle_scope.emplace(isolate_in);
  }

  ~UnofficialEnvScope() {
    if (context.has_value()) {
      context->Reset();
    }
    context_scope.reset();
    handle_scope.reset();
    isolate_scope.reset();
  }
};

struct PrepareStackTraceContextCallback {
  v8::Global<v8::Context> context;
  v8::Global<v8::Function> callback;
};

struct PrepareStackTraceState {
  v8::Global<v8::Function> principal_callback;
  std::vector<PrepareStackTraceContextCallback> context_callbacks;
};

std::mutex g_runtime_mu;
SharedRuntime g_runtime;
std::unordered_map<v8::Isolate*, napi_env> g_env_by_isolate;
std::unordered_map<v8::Isolate*, uint64_t> g_hash_seeds;
std::unordered_map<v8::Isolate*, v8::Global<v8::Function>> g_promise_reject_callbacks;
std::unordered_map<v8::Isolate*, std::array<v8::Global<v8::Function>, 4>> g_promise_hooks;
std::unordered_map<napi_env, PrepareStackTraceState> g_prepare_stack_trace_callbacks;
std::unordered_map<v8::ArrayBuffer::Allocator*, std::shared_ptr<TrackingArrayBufferAllocator>>
    g_tracking_allocators;

struct FatalErrorCallbacks {
  unofficial_napi_fatal_error_callback fatal = nullptr;
  unofficial_napi_oom_error_callback oom = nullptr;
};

std::unordered_map<v8::Isolate*, FatalErrorCallbacks> g_fatal_error_callbacks;

struct NearHeapLimitCallbackState {
  unofficial_napi_near_heap_limit_callback callback = nullptr;
  void* data = nullptr;
};

std::unordered_map<v8::Isolate*, NearHeapLimitCallbackState> g_near_heap_limit_callbacks;

struct InterruptRequest {
  napi_env env = nullptr;
  unofficial_napi_interrupt_callback callback = nullptr;
  void* data = nullptr;
};

struct ProfilerState {
  v8::CpuProfiler* cpu_profiler = nullptr;
  std::vector<uint32_t> active_cpu_profiles;
  bool heap_profile_started = false;
};

std::unordered_map<napi_env, ProfilerState> g_profiler_states;

class StringOutputStream final : public v8::OutputStream {
 public:
  WriteResult WriteAsciiChunk(char* data, int size) override {
    if (data != nullptr && size > 0) output_.append(data, size);
    return kContinue;
  }

  void EndOfStream() override {}

  const std::string& output() const { return output_; }

 private:
  std::string output_;
};

void DisposeProfilerState(napi_env env, ProfilerState* state) {
  if (env == nullptr || env->isolate == nullptr || state == nullptr) return;
  if (state->heap_profile_started) {
    env->isolate->GetHeapProfiler()->StopSamplingHeapProfiler();
    state->heap_profile_started = false;
  }
  if (state->cpu_profiler != nullptr) {
    for (uint32_t profile_id : state->active_cpu_profiles) {
      if (v8::CpuProfile* profile = state->cpu_profiler->Stop(profile_id)) {
        profile->Delete();
      }
    }
    state->active_cpu_profiles.clear();
    state->cpu_profiler->Dispose();
    state->cpu_profiler = nullptr;
  }
}

ProfilerState& EnsureProfilerState(napi_env env) {
  return g_profiler_states[env];
}

bool CopyStringToMallocBuffer(const std::string& input, char** data_out, size_t* len_out) {
  if (data_out == nullptr || len_out == nullptr) return false;
  *data_out = nullptr;
  *len_out = 0;
  char* buffer = static_cast<char*>(std::malloc(input.size() + 1));
  if (buffer == nullptr) return false;
  if (!input.empty()) {
    std::memcpy(buffer, input.data(), input.size());
  }
  buffer[input.size()] = '\0';
  *data_out = buffer;
  *len_out = input.size();
  return true;
}

uint64_t GenerateHashSeed() {
  try {
    std::random_device random_device;
    const uint64_t high = static_cast<uint64_t>(random_device());
    const uint64_t low = static_cast<uint64_t>(random_device());
    const auto now = std::chrono::steady_clock::now().time_since_epoch();
    const uint64_t monotonic_ticks = static_cast<uint64_t>(
        std::chrono::duration_cast<std::chrono::nanoseconds>(now).count());
    const uint64_t mixed = (high << 32) ^ low ^ monotonic_ticks;
    if (mixed != 0) return mixed;
  } catch (...) {
    // Fall back below if entropy is unavailable in this runtime.
  }
  return 1;
}

void AppendEscapedJsonString(std::string* out, std::string_view input) {
  if (out == nullptr) return;
  out->push_back('"');
  for (unsigned char ch : input) {
    switch (ch) {
      case '"':
        out->append("\\\"");
        break;
      case '\\':
        out->append("\\\\");
        break;
      case '\b':
        out->append("\\b");
        break;
      case '\f':
        out->append("\\f");
        break;
      case '\n':
        out->append("\\n");
        break;
      case '\r':
        out->append("\\r");
        break;
      case '\t':
        out->append("\\t");
        break;
      default:
        if (ch < 0x20) {
          char buffer[7];
          std::snprintf(buffer, sizeof(buffer), "\\u%04x", ch);
          out->append(buffer);
        } else {
          out->push_back(static_cast<char>(ch));
        }
    }
  }
  out->push_back('"');
}

template <typename T>
void AppendJsonNumber(std::string* out, T value) {
  if (out == nullptr) return;
  std::ostringstream stream;
  stream << value;
  out->append(stream.str());
}

void BuildHeapProfileNode(v8::Isolate* isolate,
                          const v8::AllocationProfile::Node* profile_node,
                          std::string* out) {
  if (out == nullptr) return;
  size_t self_size = 0;
  for (const auto& allocation : profile_node->allocations) {
    self_size += allocation.size * allocation.count;
  }

  out->push_back('{');
  out->append("\"selfSize\":");
  AppendJsonNumber(out, self_size);
  out->append(",\"id\":");
  AppendJsonNumber(out, profile_node->node_id);
  out->append(",\"callFrame\":{");
  out->append("\"scriptId\":");
  AppendJsonNumber(out, profile_node->script_id);
  out->append(",\"lineNumber\":");
  AppendJsonNumber(out, profile_node->line_number - 1);
  out->append(",\"columnNumber\":");
  AppendJsonNumber(out, profile_node->column_number - 1);
  v8::String::Utf8Value fn_name(isolate, profile_node->name);
  v8::String::Utf8Value script_name(isolate, profile_node->script_name);
  out->append(",\"functionName\":");
  AppendEscapedJsonString(out, *fn_name ? *fn_name : "");
  out->append(",\"url\":");
  AppendEscapedJsonString(out, *script_name ? *script_name : "");
  out->append("},\"children\":[");
  bool first = true;
  for (const auto* child : profile_node->children) {
    if (!first) out->push_back(',');
    BuildHeapProfileNode(isolate, child, out);
    first = false;
  }
  out->append("]}");
}

bool SerializeHeapProfile(v8::Isolate* isolate, std::string* out) {
  if (isolate == nullptr || out == nullptr) return false;
  v8::HeapProfiler* profiler = isolate->GetHeapProfiler();
  std::unique_ptr<v8::AllocationProfile> profile(profiler->GetAllocationProfile());
  if (!profile) return false;

  out->clear();
  out->append("{\"samples\":[");
  bool first = true;
  for (const auto& sample : profile->GetSamples()) {
    if (!first) out->push_back(',');
    out->append("{\"size\":");
    AppendJsonNumber(out, sample.size * sample.count);
    out->append(",\"nodeId\":");
    AppendJsonNumber(out, sample.node_id);
    out->append(",\"ordinal\":");
    AppendJsonNumber(out, static_cast<double>(sample.sample_id));
    out->push_back('}');
    first = false;
  }
  out->append("],\"head\":");
  BuildHeapProfileNode(isolate, profile->GetRootNode(), out);
  out->push_back('}');
  return true;
}

void ResetPrepareStackTraceState(PrepareStackTraceState* state) {
  if (state == nullptr) return;
  state->principal_callback.Reset();
  for (auto& entry : state->context_callbacks) {
    entry.context.Reset();
    entry.callback.Reset();
  }
  state->context_callbacks.clear();
}

v8::Local<v8::Function> LookupPrepareStackTraceCallback(napi_env env,
                                                        v8::Local<v8::Context> context) {
  if (env == nullptr || env->isolate == nullptr || context.IsEmpty()) {
    return v8::Local<v8::Function>();
  }

  auto state_it = g_prepare_stack_trace_callbacks.find(env);
  if (state_it == g_prepare_stack_trace_callbacks.end()) {
    return v8::Local<v8::Function>();
  }

  PrepareStackTraceState& state = state_it->second;
  v8::Local<v8::Context> principal_context = env->context();
  const bool use_principal_callback =
      !principal_context.IsEmpty() &&
      (context == principal_context || NapiV8IsContextifyContext(env, context));

  if (use_principal_callback) {
    return state.principal_callback.Get(context->GetIsolate());
  }

  for (const auto& entry : state.context_callbacks) {
    v8::Local<v8::Context> candidate = entry.context.Get(context->GetIsolate());
    if (!candidate.IsEmpty() && candidate == context) {
      return entry.callback.Get(context->GetIsolate());
    }
  }

  return state.principal_callback.Get(context->GetIsolate());
}

v8::MaybeLocal<v8::Value> NapiPrepareStackTraceCallback(v8::Local<v8::Context> context,
                                                        v8::Local<v8::Value> exception,
                                                        v8::Local<v8::Array> trace) {
  napi_env env = nullptr;
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    auto env_it = g_env_by_isolate.find(context->GetIsolate());
    if (env_it != g_env_by_isolate.end()) {
      env = env_it->second;
    }
  }

  if (env == nullptr) {
    return exception->ToString(context);
  }

  v8::Local<v8::Function> callback;
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    callback = LookupPrepareStackTraceCallback(env, context);
  }

  if (callback.IsEmpty()) {
    return exception->ToString(context);
  }

  v8::TryCatch try_catch(context->GetIsolate());
  v8::Local<v8::Value> argv[3] = {
      context->Global(),
      exception,
      trace,
  };
  v8::MaybeLocal<v8::Value> result =
      callback->Call(context, v8::Undefined(context->GetIsolate()), 3, argv);
  if (try_catch.HasCaught() && !try_catch.HasTerminated()) {
    try_catch.ReThrow();
  }
  return result;
}

bool IsEnvThreadEntered(napi_env env) {
  return env != nullptr && env->isolate != nullptr && v8::Isolate::GetCurrent() == env->isolate;
}

size_t NearHeapLimitCallback(void* raw_env,
                             size_t current_heap_limit,
                             size_t initial_heap_limit) {
  napi_env env = static_cast<napi_env>(raw_env);
  v8::Isolate* isolate = env != nullptr ? env->isolate : nullptr;
  if (isolate == nullptr) {
    return current_heap_limit;
  }
  NearHeapLimitCallbackState state;
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    auto callback_it = g_near_heap_limit_callbacks.find(isolate);
    if (callback_it != g_near_heap_limit_callbacks.end()) {
      state = callback_it->second;
    }
  }
  if (state.callback == nullptr) {
    return current_heap_limit;
  }
  return state.callback(env, state.data, current_heap_limit, initial_heap_limit);
}

class TrackingArrayBufferAllocator final : public v8::ArrayBuffer::Allocator {
 public:
  TrackingArrayBufferAllocator()
      : backing_(v8::ArrayBuffer::Allocator::NewDefaultAllocator()) {}

  ~TrackingArrayBufferAllocator() override { delete backing_; }

  void* Allocate(size_t length) override {
    void* data = backing_ != nullptr ? backing_->Allocate(length) : nullptr;
    if (data != nullptr) {
      total_mem_usage_.fetch_add(length, std::memory_order_relaxed);
    }
    return data;
  }

  void* AllocateUninitialized(size_t length) override {
    void* data = backing_ != nullptr ? backing_->AllocateUninitialized(length) : nullptr;
    if (data != nullptr) {
      total_mem_usage_.fetch_add(length, std::memory_order_relaxed);
    }
    return data;
  }

  void Free(void* data, size_t length) override {
    if (data != nullptr) {
      total_mem_usage_.fetch_sub(length, std::memory_order_relaxed);
    }
    if (backing_ != nullptr) {
      backing_->Free(data, length);
    }
  }

  size_t MaxAllocationSize() const override {
    return backing_ != nullptr ? backing_->MaxAllocationSize()
                               : v8::ArrayBuffer::Allocator::MaxAllocationSize();
  }

  v8::PageAllocator* GetPageAllocator() override {
    return backing_ != nullptr ? backing_->GetPageAllocator() : nullptr;
  }

  uint64_t total_mem_usage() const {
    return total_mem_usage_.load(std::memory_order_relaxed);
  }

 private:
  v8::ArrayBuffer::Allocator* backing_ = nullptr;
  std::atomic<uint64_t> total_mem_usage_ {0};
};

void ApplyNodeIsolateCreateParams(v8::Isolate::CreateParams* params) {
  if (params == nullptr) return;

  const uint64_t constrained_memory = uv_get_constrained_memory();
  const uint64_t total_memory =
      constrained_memory > 0
          ? std::min<uint64_t>(uv_get_total_memory(), constrained_memory)
          : uv_get_total_memory();
  if (total_memory > 0 &&
      params->constraints.max_old_generation_size_in_bytes() == 0) {
    params->constraints.ConfigureDefaults(total_memory, 0);
  }
}

v8::IsolateGroup GetOrCreateIsolateGroup() {
  if (v8::IsolateGroup::CanCreateNewGroups()) {
    return v8::IsolateGroup::Create();
  }
  return v8::IsolateGroup::GetDefault();
}

v8::Isolate* CreateIsolateForEnv(EdgeV8Platform* platform,
                                 const v8::Isolate::CreateParams& params) {
  v8::Isolate* isolate = v8::Isolate::Allocate(GetOrCreateIsolateGroup());
  if (isolate == nullptr) return nullptr;
  if (platform != nullptr && !platform->RegisterIsolate(isolate)) {
    isolate->Dispose();
    return nullptr;
  }
  v8::Isolate::Initialize(isolate, params);
  return isolate;
}

struct PlatformShutdownWaiter {
  std::mutex mutex;
  std::condition_variable cv;
  bool finished = false;
};

void OnPlatformShutdownFinished(void* data) {
  auto* waiter = static_cast<PlatformShutdownWaiter*>(data);
  if (waiter == nullptr) return;
  {
    std::lock_guard<std::mutex> lock(waiter->mutex);
    waiter->finished = true;
  }
  waiter->cv.notify_all();
}

void WaitForPlatformShutdown(PlatformShutdownWaiter* waiter,
                             uv_loop_t* loop) {
  if (waiter == nullptr) return;
  std::unique_lock<std::mutex> lock(waiter->mutex);
  while (!waiter->finished) {
    if (loop != nullptr) {
      lock.unlock();
      (void)uv_run(loop, UV_RUN_ONCE);
      lock.lock();
      continue;
    }
    waiter->cv.wait_for(lock, std::chrono::milliseconds(1));
  }
}

void DisposeIsolateAndWait(EdgeV8Platform* platform,
                           v8::Isolate* isolate,
                           uv_loop_t* loop = nullptr) {
  if (isolate == nullptr) return;

  PlatformShutdownWaiter waiter;
  if (platform != nullptr) {
    platform->AddIsolateFinishedCallback(
        isolate, OnPlatformShutdownFinished, &waiter);
    platform->DisposeIsolate(isolate);
  } else {
    waiter.finished = true;
    isolate->Dispose();
  }
  WaitForPlatformShutdown(&waiter, loop);
}

void ApplyDefaultV8Flags() {
  v8::V8::SetFlagsFromString(
      kNodeDefaultShippingV8Flags,
      static_cast<int>(sizeof(kNodeDefaultShippingV8Flags) - 1));
}

void FatalErrorCallback(const char* location, const char* message) {
  v8::Isolate* isolate = v8::Isolate::TryGetCurrent();
  if (isolate == nullptr) return;

  unofficial_napi_fatal_error_callback callback = nullptr;
  napi_env env = nullptr;
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    auto callback_it = g_fatal_error_callbacks.find(isolate);
    if (callback_it != g_fatal_error_callbacks.end()) {
      callback = callback_it->second.fatal;
    }
    auto env_it = g_env_by_isolate.find(isolate);
    if (env_it != g_env_by_isolate.end()) {
      env = env_it->second;
    }
  }
  if (callback != nullptr) {
    callback(env, location, message);
  }
}

void OOMErrorCallback(const char* location, const v8::OOMDetails& details) {
  v8::Isolate* isolate = v8::Isolate::TryGetCurrent();
  if (isolate == nullptr) return;

  unofficial_napi_oom_error_callback callback = nullptr;
  napi_env env = nullptr;
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    auto callback_it = g_fatal_error_callbacks.find(isolate);
    if (callback_it != g_fatal_error_callbacks.end()) {
      callback = callback_it->second.oom;
    }
    auto env_it = g_env_by_isolate.find(isolate);
    if (env_it != g_env_by_isolate.end()) {
      env = env_it->second;
    }
  }
  if (callback != nullptr) {
    callback(env, location, details.is_heap_oom, details.detail);
  }
}

napi_status AcquireRuntime(EdgeV8Platform** platform_out) {
  if (platform_out == nullptr) return napi_invalid_arg;
  std::lock_guard<std::mutex> lock(g_runtime_mu);

  if (g_runtime.refcount == 0 && g_runtime.platform == nullptr) {
    ApplyDefaultV8Flags();
    v8::V8::InitializeICUDefaultLocation("");
    v8::V8::InitializeExternalStartupData("");
    g_runtime.platform = EdgeV8Platform::Create();
    v8::V8::InitializePlatform(g_runtime.platform.get());
    v8::V8::Initialize();
  }

  g_runtime.refcount++;
  *platform_out = g_runtime.platform.get();
  return *platform_out != nullptr ? napi_ok : napi_generic_failure;
}

void ReleaseRuntime() {
  std::lock_guard<std::mutex> lock(g_runtime_mu);
  if (g_runtime.refcount == 0) return;
  g_runtime.refcount--;
  // Keep shared V8 runtime alive for process lifetime in tests to avoid
  // repeated Dispose/Initialize instability in embedded setups.
}

void PromiseRejectCallback(v8::PromiseRejectMessage message) {
  v8::Local<v8::Promise> promise = message.GetPromise();
  if (promise.IsEmpty()) return;

  v8::Isolate* isolate = promise->GetIsolate();
  napi_env env = nullptr;
  v8::Local<v8::Function> callback;
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    const auto env_it = g_env_by_isolate.find(isolate);
    if (env_it == g_env_by_isolate.end() || env_it->second == nullptr) return;
    env = env_it->second;
    const auto cb_it = g_promise_reject_callbacks.find(isolate);
    if (cb_it == g_promise_reject_callbacks.end() || cb_it->second.IsEmpty()) return;
    callback = cb_it->second.Get(isolate);
  }
  if (env == nullptr || callback.IsEmpty()) return;

  v8::PromiseRejectEvent event = message.GetEvent();
  v8::Local<v8::Value> value;
  switch (event) {
    case v8::kPromiseRejectWithNoHandler:
    case v8::kPromiseResolveAfterResolved:
    case v8::kPromiseRejectAfterResolved:
      value = message.GetValue();
      if (value.IsEmpty()) value = v8::Undefined(isolate);
      break;
    case v8::kPromiseHandlerAddedAfterReject:
      value = v8::Undefined(isolate);
      break;
    default:
      return;
  }

  v8::HandleScope handle_scope(isolate);
  v8::Local<v8::Context> context = env->context();
  v8::Context::Scope context_scope(context);
  v8::TryCatch tc(isolate);
  v8::Local<v8::Value> args[] = {
      v8::Integer::New(isolate, static_cast<int>(event)),
      promise,
      value};
  (void)callback->Call(context, v8::Undefined(isolate), 3, args);
  if (tc.HasCaught() && !tc.HasTerminated()) {
    // Match Node behavior: V8 expects this callback to return without a pending
    // exception. Print a best-effort diagnostic instead of scheduling it.
    std::fprintf(stderr, "Exception in PromiseRejectCallback:\n");
    v8::Local<v8::Value> caught = tc.Exception();
    v8::String::Utf8Value text(isolate, caught);
    if (*text != nullptr) {
      std::fprintf(stderr, "%s\n", *text);
    } else {
      std::fprintf(stderr, "<exception>\n");
    }
  }
}

bool IsNullishValue(v8::Local<v8::Value> value) {
  return value.IsEmpty() || value->IsUndefined() || value->IsNull();
}

bool GetPromiseHookFunction(napi_env env,
                            napi_value value,
                            v8::Local<v8::Function>* function_out) {
  if (env == nullptr || function_out == nullptr) return false;
  *function_out = v8::Local<v8::Function>();
  if (value == nullptr) return true;

  v8::Local<v8::Value> raw = napi_v8_unwrap_value(value);
  if (IsNullishValue(raw)) return true;
  if (!raw->IsFunction()) return false;

  *function_out = raw.As<v8::Function>();
  return true;
}

size_t PromiseHookIndex(v8::PromiseHookType type) {
  switch (type) {
    case v8::PromiseHookType::kInit:
      return 0;
    case v8::PromiseHookType::kBefore:
      return 1;
    case v8::PromiseHookType::kAfter:
      return 2;
    case v8::PromiseHookType::kResolve:
      return 3;
    default:
      return 4;
  }
}

bool HasPromiseHooks(const std::array<v8::Global<v8::Function>, 4>& hooks) {
  for (const auto& hook : hooks) {
    if (!hook.IsEmpty()) return true;
  }
  return false;
}

void PromiseHookCallback(v8::PromiseHookType type,
                         v8::Local<v8::Promise> promise,
                         v8::Local<v8::Value> parent) {
  v8::Isolate* isolate = promise->GetIsolate();
  napi_env env = nullptr;
  v8::Local<v8::Function> callback;
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    const auto env_it = g_env_by_isolate.find(isolate);
    if (env_it == g_env_by_isolate.end() || env_it->second == nullptr) return;
    env = env_it->second;

    const auto hooks_it = g_promise_hooks.find(isolate);
    if (hooks_it == g_promise_hooks.end()) return;

    const size_t index = PromiseHookIndex(type);
    if (index >= hooks_it->second.size() || hooks_it->second[index].IsEmpty()) return;
    callback = hooks_it->second[index].Get(isolate);
  }
  if (env == nullptr || callback.IsEmpty()) return;

  v8::HandleScope handle_scope(isolate);
  v8::Local<v8::Context> context =
      isolate->InContext() ? isolate->GetCurrentContext() : env->context();
  if (context.IsEmpty()) return;
  std::optional<v8::Context::Scope> context_scope;
  if (!isolate->InContext()) {
    context_scope.emplace(context);
  }

  v8::TryCatch tc(isolate);
  tc.SetVerbose(false);
  v8::Local<v8::Value> args[] = {
      promise,
      parent.IsEmpty() ? v8::Undefined(isolate) : parent,
  };
  const int argc = type == v8::PromiseHookType::kInit ? 2 : 1;
  (void)callback->Call(context, v8::Undefined(isolate), argc, args);
  if (tc.HasCaught() && !tc.HasTerminated()) {
    tc.ReThrow();
  }
}

v8::Local<v8::String> OneByteString(v8::Isolate* isolate, const char* value) {
  return v8::String::NewFromUtf8(isolate, value, v8::NewStringType::kInternalized)
      .ToLocalChecked();
}

void ThrowTypeErrorWithCode(v8::Local<v8::Context> context,
                            v8::Isolate* isolate,
                            const char* code,
                            const char* message) {
  v8::Local<v8::Value> exception = v8::Exception::TypeError(OneByteString(isolate, message));
  if (exception->IsObject()) {
    (void)exception.As<v8::Object>()->Set(
        context, OneByteString(isolate, "code"), OneByteString(isolate, code));
  }
  isolate->ThrowException(exception);
}

v8::Local<v8::Object> CreateBufferObject(v8::Local<v8::Context> context,
                                         v8::Local<v8::ArrayBuffer> array_buffer,
                                         size_t offset,
                                         size_t length) {
  v8::Isolate* isolate = context->GetIsolate();
  v8::Local<v8::Object> global = context->Global();

  v8::Local<v8::Value> buffer_ctor_value;
  if (global->Get(context, OneByteString(isolate, "Buffer")).ToLocal(&buffer_ctor_value) &&
      buffer_ctor_value->IsObject()) {
    v8::Local<v8::Object> buffer_ctor = buffer_ctor_value.As<v8::Object>();
    v8::Local<v8::Value> from_value;
    if (buffer_ctor->Get(context, OneByteString(isolate, "from")).ToLocal(&from_value) &&
        from_value->IsFunction()) {
      v8::Local<v8::Value> argv[3] = {
          array_buffer,
          v8::Number::New(isolate, static_cast<double>(offset)),
          v8::Number::New(isolate, static_cast<double>(length)),
      };
      v8::Local<v8::Value> maybe_buffer;
      if (from_value.As<v8::Function>()->Call(context, buffer_ctor, 3, argv).ToLocal(&maybe_buffer) &&
          maybe_buffer->IsObject()) {
        return maybe_buffer.As<v8::Object>();
      }
    }
  }

  return v8::Uint8Array::New(array_buffer, offset, length);
}

bool ReadArrayBufferViewBytes(v8::Local<v8::Value> value,
                              const uint8_t** data_out,
                              size_t* size_out) {
  if (data_out == nullptr || size_out == nullptr || value.IsEmpty() || !value->IsArrayBufferView()) {
    return false;
  }
  v8::Local<v8::ArrayBufferView> view = value.As<v8::ArrayBufferView>();
  std::shared_ptr<v8::BackingStore> store = view->Buffer()->GetBackingStore();
  if (!store || store->Data() == nullptr) {
    *data_out = nullptr;
    *size_out = 0;
    return true;
  }
  *data_out = static_cast<const uint8_t*>(store->Data()) + view->ByteOffset();
  *size_out = view->ByteLength();
  return true;
}

class SerializerContext : public v8::ValueSerializer::Delegate {
 public:
  SerializerContext(napi_env env, v8::Local<v8::Object> wrap)
      : env_(env), isolate_(env->isolate), serializer_(isolate_, this) {
    wrap_.Reset(isolate_, wrap);
    wrap_.SetWeak(this, WeakCallback, v8::WeakCallbackType::kParameter);
  }

  ~SerializerContext() { wrap_.Reset(); }

  void ThrowDataCloneError(v8::Local<v8::String> message) override {
    v8::Local<v8::Context> context = env_->context();
    v8::Local<v8::Object> wrap = wrap_.Get(isolate_);

    v8::Local<v8::Value> get_data_clone_error;
    if (!wrap->Get(context, OneByteString(isolate_, "_getDataCloneError"))
             .ToLocal(&get_data_clone_error) ||
        !get_data_clone_error->IsFunction()) {
      isolate_->ThrowException(v8::Exception::Error(message));
      return;
    }

    v8::Local<v8::Value> argv[1] = {message};
    v8::Local<v8::Value> error;
    if (get_data_clone_error.As<v8::Function>()->Call(context, wrap, 1, argv).ToLocal(&error)) {
      isolate_->ThrowException(error);
    }
  }

  v8::Maybe<uint32_t> GetSharedArrayBufferId(
      v8::Isolate* isolate,
      v8::Local<v8::SharedArrayBuffer> shared_array_buffer) override {
    v8::Local<v8::Context> context = env_->context();
    v8::Local<v8::Object> wrap = wrap_.Get(isolate);

    v8::Local<v8::Value> hook;
    if (!wrap->Get(context, OneByteString(isolate, "_getSharedArrayBufferId")).ToLocal(&hook) ||
        !hook->IsFunction()) {
      return v8::ValueSerializer::Delegate::GetSharedArrayBufferId(isolate, shared_array_buffer);
    }

    v8::Local<v8::Value> argv[1] = {shared_array_buffer};
    v8::Local<v8::Value> id;
    if (!hook.As<v8::Function>()->Call(context, wrap, 1, argv).ToLocal(&id)) {
      return v8::Nothing<uint32_t>();
    }
    return id->Uint32Value(context);
  }

  v8::Maybe<bool> WriteHostObject(v8::Isolate* isolate, v8::Local<v8::Object> input) override {
    v8::Local<v8::Context> context = env_->context();
    v8::Local<v8::Object> wrap = wrap_.Get(isolate);

    v8::Local<v8::Value> hook;
    if (!wrap->Get(context, OneByteString(isolate, "_writeHostObject")).ToLocal(&hook)) {
      return v8::Nothing<bool>();
    }
    if (!hook->IsFunction()) {
      return v8::ValueSerializer::Delegate::WriteHostObject(isolate, input);
    }

    v8::Local<v8::Value> argv[1] = {input};
    v8::Local<v8::Value> ret;
    if (!hook.As<v8::Function>()->Call(context, wrap, 1, argv).ToLocal(&ret)) {
      return v8::Nothing<bool>();
    }
    return v8::Just(true);
  }

  static void New(const v8::FunctionCallbackInfo<v8::Value>& args) {
    v8::Isolate* isolate = args.GetIsolate();
    if (!args.IsConstructCall()) {
      ThrowTypeErrorWithCode(args.GetIsolate()->GetCurrentContext(),
                             isolate,
                             "ERR_CONSTRUCT_CALL_REQUIRED",
                             "Class constructor Serializer cannot be invoked without 'new'");
      return;
    }
    if (!args.Data()->IsExternal()) {
      isolate->ThrowException(v8::Exception::Error(
          OneByteString(isolate, "Internal serializer constructor state missing")));
      return;
    }

    auto* env = static_cast<napi_env>(v8::Local<v8::External>::Cast(args.Data())->Value());
    if (env == nullptr) {
      isolate->ThrowException(v8::Exception::Error(
          OneByteString(isolate, "Internal serializer environment missing")));
      return;
    }

    auto* ctx = new SerializerContext(env, args.This());
    args.This()->SetAlignedPointerInInternalField(0, ctx);
  }

  static void WriteHeader(const v8::FunctionCallbackInfo<v8::Value>& args) {
    SerializerContext* ctx = Unwrap(args);
    if (ctx == nullptr) return;
    ctx->serializer_.WriteHeader();
  }

  static void WriteValue(const v8::FunctionCallbackInfo<v8::Value>& args) {
    SerializerContext* ctx = Unwrap(args);
    if (ctx == nullptr) return;
    v8::Local<v8::Value> value = args.Length() >= 1 ? args[0] : v8::Undefined(ctx->isolate_);
    bool ret = false;
    if (ctx->serializer_.WriteValue(ctx->env_->context(), value).To(&ret)) {
      args.GetReturnValue().Set(ret);
    }
  }

  static void SetTreatArrayBufferViewsAsHostObjects(
      const v8::FunctionCallbackInfo<v8::Value>& args) {
    SerializerContext* ctx = Unwrap(args);
    if (ctx == nullptr) return;
    const bool value = args.Length() >= 1 && args[0]->BooleanValue(ctx->isolate_);
    ctx->serializer_.SetTreatArrayBufferViewsAsHostObjects(value);
  }

  static void ReleaseBuffer(const v8::FunctionCallbackInfo<v8::Value>& args) {
    SerializerContext* ctx = Unwrap(args);
    if (ctx == nullptr) return;

    std::pair<uint8_t*, size_t> serialized = ctx->serializer_.Release();
    v8::Isolate* isolate = ctx->isolate_;
    v8::HandleScope handle_scope(isolate);
    v8::Local<v8::Context> context = ctx->env_->context();
    v8::Context::Scope context_scope(context);

    auto backing_store = v8::ArrayBuffer::NewBackingStore(
        serialized.first,
        serialized.second,
        [](void* data, size_t, void*) { std::free(data); },
        nullptr);
    if (!backing_store) {
      std::free(serialized.first);
      return;
    }
    v8::Local<v8::ArrayBuffer> ab = v8::ArrayBuffer::New(isolate, std::move(backing_store));
    args.GetReturnValue().Set(CreateBufferObject(context, ab, 0, serialized.second));
  }

  static void TransferArrayBuffer(const v8::FunctionCallbackInfo<v8::Value>& args) {
    SerializerContext* ctx = Unwrap(args);
    if (ctx == nullptr) return;
    if (args.Length() < 2) return;

    uint32_t id = 0;
    if (!args[0]->Uint32Value(ctx->env_->context()).To(&id)) return;

    if (!args[1]->IsArrayBuffer()) {
      ThrowTypeErrorWithCode(ctx->env_->context(),
                             ctx->isolate_,
                             "ERR_INVALID_ARG_TYPE",
                             "arrayBuffer must be an ArrayBuffer");
      return;
    }
    ctx->serializer_.TransferArrayBuffer(id, args[1].As<v8::ArrayBuffer>());
  }

  static void WriteUint32(const v8::FunctionCallbackInfo<v8::Value>& args) {
    SerializerContext* ctx = Unwrap(args);
    if (ctx == nullptr || args.Length() < 1) return;
    uint32_t value = 0;
    if (args[0]->Uint32Value(ctx->env_->context()).To(&value)) {
      ctx->serializer_.WriteUint32(value);
    }
  }

  static void WriteUint64(const v8::FunctionCallbackInfo<v8::Value>& args) {
    SerializerContext* ctx = Unwrap(args);
    if (ctx == nullptr || args.Length() < 2) return;
    uint32_t hi = 0;
    uint32_t lo = 0;
    if (!args[0]->Uint32Value(ctx->env_->context()).To(&hi) ||
        !args[1]->Uint32Value(ctx->env_->context()).To(&lo)) {
      return;
    }
    ctx->serializer_.WriteUint64((static_cast<uint64_t>(hi) << 32) | static_cast<uint64_t>(lo));
  }

  static void WriteDouble(const v8::FunctionCallbackInfo<v8::Value>& args) {
    SerializerContext* ctx = Unwrap(args);
    if (ctx == nullptr || args.Length() < 1) return;
    double value = 0;
    if (args[0]->NumberValue(ctx->env_->context()).To(&value)) {
      ctx->serializer_.WriteDouble(value);
    }
  }

  static void WriteRawBytes(const v8::FunctionCallbackInfo<v8::Value>& args) {
    SerializerContext* ctx = Unwrap(args);
    if (ctx == nullptr || args.Length() < 1) return;
    const uint8_t* data = nullptr;
    size_t size = 0;
    if (!ReadArrayBufferViewBytes(args[0], &data, &size)) {
      ThrowTypeErrorWithCode(ctx->env_->context(),
                             ctx->isolate_,
                             "ERR_INVALID_ARG_TYPE",
                             "source must be a TypedArray or a DataView");
      return;
    }
    ctx->serializer_.WriteRawBytes(data, size);
  }

 private:
  static SerializerContext* Unwrap(const v8::FunctionCallbackInfo<v8::Value>& args) {
    if (args.This().IsEmpty() || args.This()->InternalFieldCount() < 1) return nullptr;
    return static_cast<SerializerContext*>(args.This()->GetAlignedPointerFromInternalField(0));
  }

  static void WeakCallback(const v8::WeakCallbackInfo<SerializerContext>& info) {
    delete info.GetParameter();
  }

  napi_env env_;
  v8::Isolate* isolate_;
  v8::Global<v8::Object> wrap_;
  v8::ValueSerializer serializer_;
};

class DeserializerContext : public v8::ValueDeserializer::Delegate {
 public:
  DeserializerContext(napi_env env,
                      v8::Local<v8::Object> wrap,
                      v8::Local<v8::Value> buffer)
      : env_(env), isolate_(env->isolate) {
    const uint8_t* data = nullptr;
    size_t size = 0;
    if (!ReadArrayBufferViewBytes(buffer, &data, &size)) return;
    if (data != nullptr && size > 0) {
      data_.assign(data, data + size);
    }
    deserializer_ = std::make_unique<v8::ValueDeserializer>(
        isolate_, data_.empty() ? nullptr : data_.data(), data_.size(), this);

    wrap_.Reset(isolate_, wrap);
    wrap_.SetWeak(this, WeakCallback, v8::WeakCallbackType::kParameter);

    v8::Local<v8::Context> context = env_->context();
    v8::Context::Scope context_scope(context);
    (void)wrap->Set(context, OneByteString(isolate_, "buffer"), buffer);
  }

  ~DeserializerContext() {
    wrap_.Reset();
    deserializer_.reset();
  }

  v8::MaybeLocal<v8::Object> ReadHostObject(v8::Isolate* isolate) override {
    v8::Local<v8::Context> context = env_->context();
    v8::Local<v8::Object> wrap = wrap_.Get(isolate);

    v8::Local<v8::Value> hook;
    if (!wrap->Get(context, OneByteString(isolate, "_readHostObject")).ToLocal(&hook)) {
      return {};
    }
    if (!hook->IsFunction()) {
      return v8::ValueDeserializer::Delegate::ReadHostObject(isolate);
    }

    v8::Isolate::AllowJavascriptExecutionScope allow_js(isolate);
    v8::Local<v8::Value> ret;
    if (!hook.As<v8::Function>()->Call(context, wrap, 0, nullptr).ToLocal(&ret)) {
      return {};
    }
    if (!ret->IsObject()) {
      isolate->ThrowException(v8::Exception::TypeError(
          OneByteString(isolate, "readHostObject must return an object")));
      return {};
    }
    return ret.As<v8::Object>();
  }

  static void New(const v8::FunctionCallbackInfo<v8::Value>& args) {
    v8::Isolate* isolate = args.GetIsolate();
    if (!args.IsConstructCall()) {
      ThrowTypeErrorWithCode(args.GetIsolate()->GetCurrentContext(),
                             isolate,
                             "ERR_CONSTRUCT_CALL_REQUIRED",
                             "Class constructor Deserializer cannot be invoked without 'new'");
      return;
    }
    if (!args.Data()->IsExternal()) {
      isolate->ThrowException(v8::Exception::Error(
          OneByteString(isolate, "Internal deserializer constructor state missing")));
      return;
    }
    if (args.Length() < 1 || !args[0]->IsArrayBufferView()) {
      ThrowTypeErrorWithCode(args.GetIsolate()->GetCurrentContext(),
                             isolate,
                             "ERR_INVALID_ARG_TYPE",
                             "buffer must be a TypedArray or a DataView");
      return;
    }

    auto* env = static_cast<napi_env>(v8::Local<v8::External>::Cast(args.Data())->Value());
    if (env == nullptr) {
      isolate->ThrowException(v8::Exception::Error(
          OneByteString(isolate, "Internal deserializer environment missing")));
      return;
    }
    auto* ctx = new DeserializerContext(env, args.This(), args[0]);
    args.This()->SetAlignedPointerInInternalField(0, ctx);
  }

  static void ReadHeader(const v8::FunctionCallbackInfo<v8::Value>& args) {
    DeserializerContext* ctx = Unwrap(args);
    if (ctx == nullptr || !ctx->deserializer_) return;
    bool ok = false;
    if (ctx->deserializer_->ReadHeader(ctx->env_->context()).To(&ok)) {
      args.GetReturnValue().Set(ok);
    }
  }

  static void ReadValue(const v8::FunctionCallbackInfo<v8::Value>& args) {
    DeserializerContext* ctx = Unwrap(args);
    if (ctx == nullptr || !ctx->deserializer_) return;
    v8::Local<v8::Value> out;
    if (ctx->deserializer_->ReadValue(ctx->env_->context()).ToLocal(&out)) {
      args.GetReturnValue().Set(out);
    }
  }

  static void TransferArrayBuffer(const v8::FunctionCallbackInfo<v8::Value>& args) {
    DeserializerContext* ctx = Unwrap(args);
    if (ctx == nullptr || !ctx->deserializer_ || args.Length() < 2) return;
    uint32_t id = 0;
    if (!args[0]->Uint32Value(ctx->env_->context()).To(&id)) return;
    if (args[1]->IsArrayBuffer()) {
      ctx->deserializer_->TransferArrayBuffer(id, args[1].As<v8::ArrayBuffer>());
      return;
    }
    if (args[1]->IsSharedArrayBuffer()) {
      ctx->deserializer_->TransferSharedArrayBuffer(id, args[1].As<v8::SharedArrayBuffer>());
      return;
    }
    ThrowTypeErrorWithCode(ctx->env_->context(),
                           ctx->isolate_,
                           "ERR_INVALID_ARG_TYPE",
                           "arrayBuffer must be an ArrayBuffer or SharedArrayBuffer");
  }

  static void GetWireFormatVersion(const v8::FunctionCallbackInfo<v8::Value>& args) {
    DeserializerContext* ctx = Unwrap(args);
    if (ctx == nullptr || !ctx->deserializer_) return;
    args.GetReturnValue().Set(v8::Integer::NewFromUnsigned(
        ctx->isolate_, ctx->deserializer_->GetWireFormatVersion()));
  }

  static void ReadUint32(const v8::FunctionCallbackInfo<v8::Value>& args) {
    DeserializerContext* ctx = Unwrap(args);
    if (ctx == nullptr || !ctx->deserializer_) return;
    uint32_t value = 0;
    if (!ctx->deserializer_->ReadUint32(&value)) {
      ctx->isolate_->ThrowException(v8::Exception::Error(
          OneByteString(ctx->isolate_, "ReadUint32() failed")));
      return;
    }
    args.GetReturnValue().Set(v8::Integer::NewFromUnsigned(ctx->isolate_, value));
  }

  static void ReadUint64(const v8::FunctionCallbackInfo<v8::Value>& args) {
    DeserializerContext* ctx = Unwrap(args);
    if (ctx == nullptr || !ctx->deserializer_) return;
    uint64_t value = 0;
    if (!ctx->deserializer_->ReadUint64(&value)) {
      ctx->isolate_->ThrowException(v8::Exception::Error(
          OneByteString(ctx->isolate_, "ReadUint64() failed")));
      return;
    }
    const uint32_t hi = static_cast<uint32_t>(value >> 32);
    const uint32_t lo = static_cast<uint32_t>(value);
    v8::Local<v8::Value> vals[2] = {
        v8::Integer::NewFromUnsigned(ctx->isolate_, hi),
        v8::Integer::NewFromUnsigned(ctx->isolate_, lo),
    };
    args.GetReturnValue().Set(v8::Array::New(ctx->isolate_, vals, 2));
  }

  static void ReadDouble(const v8::FunctionCallbackInfo<v8::Value>& args) {
    DeserializerContext* ctx = Unwrap(args);
    if (ctx == nullptr || !ctx->deserializer_) return;
    double value = 0;
    if (!ctx->deserializer_->ReadDouble(&value)) {
      ctx->isolate_->ThrowException(v8::Exception::Error(
          OneByteString(ctx->isolate_, "ReadDouble() failed")));
      return;
    }
    args.GetReturnValue().Set(value);
  }

  static void ReadRawBytes(const v8::FunctionCallbackInfo<v8::Value>& args) {
    DeserializerContext* ctx = Unwrap(args);
    if (ctx == nullptr || !ctx->deserializer_ || args.Length() < 1) return;

    int64_t requested = 0;
    if (!args[0]->IntegerValue(ctx->env_->context()).To(&requested) || requested < 0) {
      return;
    }
    const size_t length = static_cast<size_t>(requested);

    const void* read_ptr = nullptr;
    if (!ctx->deserializer_->ReadRawBytes(length, &read_ptr)) {
      ctx->isolate_->ThrowException(v8::Exception::Error(
          OneByteString(ctx->isolate_, "ReadRawBytes() failed")));
      return;
    }
    const uint8_t* pos = static_cast<const uint8_t*>(read_ptr);
    const uint8_t* base = ctx->data_.empty() ? nullptr : ctx->data_.data();
    if (base == nullptr || pos < base || pos + length > base + ctx->data_.size()) {
      ctx->isolate_->ThrowException(v8::Exception::Error(
          OneByteString(ctx->isolate_, "ReadRawBytes() returned out-of-range data")));
      return;
    }
    const uint32_t offset = static_cast<uint32_t>(pos - base);
    args.GetReturnValue().Set(v8::Integer::NewFromUnsigned(ctx->isolate_, offset));
  }

 private:
  static DeserializerContext* Unwrap(const v8::FunctionCallbackInfo<v8::Value>& args) {
    if (args.This().IsEmpty() || args.This()->InternalFieldCount() < 1) return nullptr;
    return static_cast<DeserializerContext*>(args.This()->GetAlignedPointerFromInternalField(0));
  }

  static void WeakCallback(const v8::WeakCallbackInfo<DeserializerContext>& info) {
    delete info.GetParameter();
  }

  napi_env env_;
  v8::Isolate* isolate_;
  v8::Global<v8::Object> wrap_;
  std::vector<uint8_t> data_;
  std::unique_ptr<v8::ValueDeserializer> deserializer_;
};

void SetProtoMethod(v8::Isolate* isolate,
                    v8::Local<v8::FunctionTemplate> tmpl,
                    const char* name,
                    v8::FunctionCallback callback) {
  tmpl->PrototypeTemplate()->Set(
      isolate,
      name,
      v8::FunctionTemplate::New(isolate, callback));
}

bool SetConstructorFunction(v8::Local<v8::Context> context,
                            v8::Local<v8::Object> target,
                            const char* name,
                            v8::Local<v8::FunctionTemplate> tmpl) {
  v8::Local<v8::Function> ctor;
  if (!tmpl->GetFunction(context).ToLocal(&ctor)) return false;
  return target->Set(context, OneByteString(context->GetIsolate(), name), ctor).FromMaybe(false);
}

class StructuredCloneSerializerDelegate final : public v8::ValueSerializer::Delegate {
 public:
  explicit StructuredCloneSerializerDelegate(v8::Isolate* isolate)
      : isolate_(isolate) {}

  void ThrowDataCloneError(v8::Local<v8::String> message) override {
    isolate_->ThrowException(v8::Exception::Error(message));
  }

  v8::Maybe<uint32_t> GetSharedArrayBufferId(
      v8::Isolate* /*isolate*/,
      v8::Local<v8::SharedArrayBuffer> shared_array_buffer) override {
    std::shared_ptr<v8::BackingStore> backing_store =
        shared_array_buffer->GetBackingStore();
    for (uint32_t i = 0; i < shared_array_buffers_.size(); ++i) {
      if (shared_array_buffers_[i] == backing_store) {
        return v8::Just(i);
      }
    }
    shared_array_buffers_.push_back(std::move(backing_store));
    return v8::Just(static_cast<uint32_t>(shared_array_buffers_.size() - 1));
  }

  v8::Maybe<uint32_t> GetWasmModuleTransferId(
      v8::Isolate* /*isolate*/,
      v8::Local<v8::WasmModuleObject> module) override {
    wasm_modules_.push_back(module->GetCompiledModule());
    return v8::Just(static_cast<uint32_t>(wasm_modules_.size() - 1));
  }

  const std::vector<std::shared_ptr<v8::BackingStore>>& shared_array_buffers() const {
    return shared_array_buffers_;
  }

  const std::vector<v8::CompiledWasmModule>& wasm_modules() const {
    return wasm_modules_;
  }

  std::vector<v8::CompiledWasmModule> TakeWasmModules() {
    return std::move(wasm_modules_);
  }

 private:
  v8::Isolate* isolate_ = nullptr;
  std::vector<std::shared_ptr<v8::BackingStore>> shared_array_buffers_;
  std::vector<v8::CompiledWasmModule> wasm_modules_;
};

class StructuredCloneDeserializerDelegate final : public v8::ValueDeserializer::Delegate {
 public:
  StructuredCloneDeserializerDelegate(
      v8::Isolate* isolate,
      const std::vector<std::shared_ptr<v8::BackingStore>>& shared_array_buffers,
      const std::vector<v8::CompiledWasmModule>& wasm_modules)
      : isolate_(isolate),
        shared_array_buffers_(shared_array_buffers),
        wasm_modules_(wasm_modules) {}

  v8::MaybeLocal<v8::SharedArrayBuffer> GetSharedArrayBufferFromId(
      v8::Isolate* isolate,
      uint32_t clone_id) override {
    if (clone_id >= shared_array_buffers_.size()) return {};
    return v8::SharedArrayBuffer::New(isolate, shared_array_buffers_[clone_id]);
  }

  v8::MaybeLocal<v8::WasmModuleObject> GetWasmModuleFromId(
      v8::Isolate* isolate,
      uint32_t transfer_id) override {
    if (transfer_id >= wasm_modules_.size()) return {};
    return v8::WasmModuleObject::FromCompiledModule(isolate, wasm_modules_[transfer_id]);
  }

 private:
  v8::Isolate* isolate_ = nullptr;
  const std::vector<std::shared_ptr<v8::BackingStore>>& shared_array_buffers_;
  const std::vector<v8::CompiledWasmModule>& wasm_modules_;
};

struct SerializedClonePayload {
  std::vector<uint8_t> bytes;
  std::vector<std::shared_ptr<v8::BackingStore>> array_buffers;
  std::vector<std::shared_ptr<v8::BackingStore>> shared_array_buffers;
  std::vector<v8::CompiledWasmModule> wasm_modules;
};

void ThrowCloneTransferError(v8::Isolate* isolate, const char* message) {
  v8::Local<v8::String> text =
      v8::String::NewFromUtf8(isolate, message, v8::NewStringType::kNormal)
          .ToLocalChecked();
  isolate->ThrowException(
      v8::Exception::Error(text));
}

napi_status CollectTransferArrayBuffers(
    napi_env env,
    v8::Local<v8::Context> context,
    v8::ValueSerializer* serializer,
    napi_value transfer_list,
    std::vector<v8::Local<v8::ArrayBuffer>>* out) {
  if (transfer_list == nullptr) return napi_ok;
  if (env == nullptr || serializer == nullptr || out == nullptr) return napi_invalid_arg;

  v8::Isolate* isolate = env->isolate;
  v8::Local<v8::Value> transfer_value = napi_v8_unwrap_value(transfer_list);
  if (!transfer_value->IsArray()) return napi_invalid_arg;

  v8::Local<v8::Array> array = transfer_value.As<v8::Array>();
  for (uint32_t i = 0; i < array->Length(); ++i) {
    v8::Local<v8::Value> entry;
    if (!array->Get(context, i).ToLocal(&entry)) {
      return napi_pending_exception;
    }
    if (!entry->IsArrayBuffer()) {
      ThrowCloneTransferError(isolate, "Only ArrayBuffer instances can be transferred");
      return napi_pending_exception;
    }
    v8::Local<v8::ArrayBuffer> array_buffer = entry.As<v8::ArrayBuffer>();
    if (!array_buffer->IsDetachable() || array_buffer->WasDetached()) {
      ThrowCloneTransferError(isolate, "An ArrayBuffer is detached and could not be cloned.");
      return napi_pending_exception;
    }
    if (std::find(out->begin(), out->end(), array_buffer) != out->end()) {
      ThrowCloneTransferError(isolate, "Transfer list contains duplicate ArrayBuffer");
      return napi_pending_exception;
    }
    serializer->TransferArrayBuffer(static_cast<uint32_t>(out->size()), array_buffer);
    out->push_back(array_buffer);
  }

  return napi_ok;
}

napi_status DetachTransferredArrayBuffers(
    const std::vector<v8::Local<v8::ArrayBuffer>>& array_buffers,
    std::vector<std::shared_ptr<v8::BackingStore>>* out) {
  if (out == nullptr) return napi_invalid_arg;
  out->clear();
  out->reserve(array_buffers.size());
  for (v8::Local<v8::ArrayBuffer> array_buffer : array_buffers) {
    std::shared_ptr<v8::BackingStore> backing_store = array_buffer->GetBackingStore();
    if (array_buffer->Detach(v8::Local<v8::Value>()).IsNothing()) {
      return napi_pending_exception;
    }
    out->push_back(std::move(backing_store));
  }
  return napi_ok;
}

napi_status DeserializeTransferredClone(
    napi_env env,
    const std::vector<uint8_t>& bytes,
    const std::vector<std::shared_ptr<v8::BackingStore>>& array_buffers,
    const std::vector<std::shared_ptr<v8::BackingStore>>& shared_array_buffers,
    const std::vector<v8::CompiledWasmModule>& wasm_modules,
    napi_value* result_out) {
  if (env == nullptr || env->isolate == nullptr || result_out == nullptr) {
    return napi_invalid_arg;
  }

  v8::Isolate* isolate = env->isolate;
  v8::Local<v8::Context> context = env->context();
  StructuredCloneDeserializerDelegate deserializer_delegate(
      isolate, shared_array_buffers, wasm_modules);
  v8::ValueDeserializer deserializer(
      isolate,
      bytes.data(),
      bytes.size(),
      &deserializer_delegate);

  for (uint32_t i = 0; i < array_buffers.size(); ++i) {
    v8::Local<v8::ArrayBuffer> array_buffer =
        v8::ArrayBuffer::New(isolate, array_buffers[i]);
    deserializer.TransferArrayBuffer(i, array_buffer);
  }

  bool header_ok = false;
  if (!deserializer.ReadHeader(context).To(&header_ok) || !header_ok) {
    return isolate->IsExecutionTerminating() ? napi_pending_exception : napi_pending_exception;
  }

  v8::Local<v8::Value> output;
  if (!deserializer.ReadValue(context).ToLocal(&output)) {
    return isolate->IsExecutionTerminating() ? napi_pending_exception : napi_pending_exception;
  }

  *result_out = napi_v8_wrap_value(env, output);
  return *result_out == nullptr ? napi_generic_failure : napi_ok;
}

napi_status StructuredCloneImpl(
    napi_env env,
    napi_value value,
    napi_value transfer_list,
    napi_value* result_out) {
  if (env == nullptr || env->isolate == nullptr || value == nullptr || result_out == nullptr) {
    return napi_invalid_arg;
  }

  v8::Isolate* isolate = env->isolate;
  v8::HandleScope handle_scope(isolate);
  v8::Local<v8::Context> context = env->context();
  v8::Context::Scope context_scope(context);

  v8::Local<v8::Value> input = napi_v8_unwrap_value(value);
  StructuredCloneSerializerDelegate serializer_delegate(isolate);
  v8::ValueSerializer serializer(isolate, &serializer_delegate);

  std::vector<v8::Local<v8::ArrayBuffer>> array_buffers;
  napi_status transfer_status =
      CollectTransferArrayBuffers(env, context, &serializer, transfer_list, &array_buffers);
  if (transfer_status != napi_ok) {
    return transfer_status;
  }

  serializer.WriteHeader();
  if (serializer.WriteValue(context, input).IsNothing()) {
    return isolate->IsExecutionTerminating() ? napi_pending_exception : napi_pending_exception;
  }

  std::vector<std::shared_ptr<v8::BackingStore>> transferred_array_buffers;
  transfer_status = DetachTransferredArrayBuffers(array_buffers, &transferred_array_buffers);
  if (transfer_status != napi_ok) {
    return transfer_status;
  }

  std::pair<uint8_t*, size_t> released = serializer.Release();
  if (released.first == nullptr) return napi_generic_failure;
  std::unique_ptr<uint8_t, decltype(&std::free)> buffer(released.first, &std::free);

  std::vector<uint8_t> bytes(buffer.get(), buffer.get() + released.second);
  return DeserializeTransferredClone(
      env,
      bytes,
      transferred_array_buffers,
      serializer_delegate.shared_array_buffers(),
      serializer_delegate.wasm_modules(),
      result_out);
}

}  // namespace

extern "C" {

napi_status NAPI_CDECL unofficial_napi_set_enqueue_foreground_task_callback(
    napi_env env,
    unofficial_napi_enqueue_foreground_task_callback callback,
    void* target) {
  if (env == nullptr) return napi_invalid_arg;
  env->enqueue_foreground_task_callback = callback;
  env->enqueue_foreground_task_target = target;
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    if (g_runtime.platform != nullptr &&
        !g_runtime.platform->BindForegroundTaskTarget(env->isolate, env, callback, target)) {
      return napi_generic_failure;
    }
  }
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_create_env_from_context(
    v8::Local<v8::Context> context, int32_t module_api_version, napi_env* result) {
  if (result == nullptr || context.IsEmpty()) return napi_invalid_arg;
  context->GetIsolate()->SetMicrotasksPolicy(v8::MicrotasksPolicy::kExplicit);
  auto* env = new (std::nothrow) napi_env__(context, module_api_version);
  if (env == nullptr) return napi_generic_failure;
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    g_env_by_isolate[env->isolate] = env;
  }
  *result = env;
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_set_edge_environment(napi_env env, void* environment) {
  if (env == nullptr) return napi_invalid_arg;
  env->edge_environment = environment;
  return napi_ok;
}

void* unofficial_napi_get_edge_environment(napi_env env) {
  return env == nullptr ? nullptr : env->edge_environment;
}

napi_status NAPI_CDECL unofficial_napi_set_env_cleanup_callback(
    napi_env env,
    unofficial_napi_env_cleanup_callback callback,
    void* data) {
  if (env == nullptr) return napi_invalid_arg;
  env->env_cleanup_callback = callback;
  env->env_cleanup_callback_data = data;
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_set_env_destroy_callback(
    napi_env env,
    unofficial_napi_env_destroy_callback callback,
    void* data) {
  if (env == nullptr) return napi_invalid_arg;
  env->env_destroy_callback = callback;
  env->env_destroy_callback_data = data;
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_set_context_token_callbacks(
    napi_env env,
    unofficial_napi_context_token_callback assign_callback,
    unofficial_napi_context_token_callback unassign_callback,
    void* data) {
  if (env == nullptr) return napi_invalid_arg;
  env->context_token_assign_callback = assign_callback;
  env->context_token_unassign_callback = unassign_callback;
  env->context_token_callback_data = data;
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_destroy_env_instance(napi_env env) {
  if (env == nullptr) return napi_invalid_arg;
  if (env->env_cleanup_callback != nullptr) {
    env->env_cleanup_callback(env, env->env_cleanup_callback_data);
  }
  ProfilerState profiler_state;
  bool has_profiler_state = false;
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    if (g_runtime.platform != nullptr) {
      g_runtime.platform->ClearForegroundTaskTarget(env->isolate, env);
    }
    auto env_it = g_env_by_isolate.find(env->isolate);
    if (env_it != g_env_by_isolate.end() && env_it->second == env) {
      g_env_by_isolate.erase(env_it);
    }
    g_hash_seeds.erase(env->isolate);
    auto cb_it = g_promise_reject_callbacks.find(env->isolate);
    if (cb_it != g_promise_reject_callbacks.end()) {
      cb_it->second.Reset();
      g_promise_reject_callbacks.erase(cb_it);
    }
    auto hooks_it = g_promise_hooks.find(env->isolate);
    if (hooks_it != g_promise_hooks.end()) {
      for (auto& hook : hooks_it->second) {
        hook.Reset();
      }
      g_promise_hooks.erase(hooks_it);
    }
    auto prepare_it = g_prepare_stack_trace_callbacks.find(env);
    if (prepare_it != g_prepare_stack_trace_callbacks.end()) {
      ResetPrepareStackTraceState(&prepare_it->second);
      g_prepare_stack_trace_callbacks.erase(prepare_it);
    }
    g_fatal_error_callbacks.erase(env->isolate);
    g_near_heap_limit_callbacks.erase(env->isolate);
    auto profiler_it = g_profiler_states.find(env);
    if (profiler_it != g_profiler_states.end()) {
      profiler_state = std::move(profiler_it->second);
      g_profiler_states.erase(profiler_it);
      has_profiler_state = true;
    }
  }
  if (env->isolate != nullptr) {
    if (has_profiler_state) {
      DisposeProfilerState(env, &profiler_state);
    }
    env->isolate->CancelTerminateExecution();
    env->isolate->SetPromiseHook(nullptr);
    env->isolate->SetPromiseRejectCallback(nullptr);
    env->isolate->SetFatalErrorHandler(nullptr);
    env->isolate->SetOOMErrorHandler(nullptr);
  }
  delete env;
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_set_fatal_error_callbacks(
    napi_env env,
    unofficial_napi_fatal_error_callback fatal_callback,
    unofficial_napi_oom_error_callback oom_callback) {
  if (env == nullptr || env->isolate == nullptr) return napi_invalid_arg;

  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    auto& entry = g_fatal_error_callbacks[env->isolate];
    entry.fatal = fatal_callback;
    entry.oom = oom_callback;
  }

  env->isolate->SetFatalErrorHandler(fatal_callback != nullptr ? FatalErrorCallback : nullptr);
  env->isolate->SetOOMErrorHandler(oom_callback != nullptr ? OOMErrorCallback : nullptr);
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_set_near_heap_limit_callback(
    napi_env env,
    unofficial_napi_near_heap_limit_callback callback,
    void* data) {
  if (env == nullptr || env->isolate == nullptr) return napi_invalid_arg;

  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    auto& entry = g_near_heap_limit_callbacks[env->isolate];
    entry.callback = callback;
    entry.data = data;
  }
  env->isolate->AddNearHeapLimitCallback(NearHeapLimitCallback, env);
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_remove_near_heap_limit_callback(
    napi_env env,
    size_t heap_limit) {
  if (env == nullptr || env->isolate == nullptr) return napi_invalid_arg;
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    g_near_heap_limit_callbacks.erase(env->isolate);
  }
  env->isolate->RemoveNearHeapLimitCallback(NearHeapLimitCallback, heap_limit);
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_set_stack_limit(napi_env env, void* stack_limit) {
  if (env == nullptr || env->isolate == nullptr || stack_limit == nullptr) return napi_invalid_arg;
  env->isolate->SetStackLimit(reinterpret_cast<uintptr_t>(stack_limit));
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_wrap_existing_value(napi_env env,
                                                           v8::Local<v8::Value> value,
                                                           napi_value* result) {
  if (env == nullptr || value.IsEmpty() || result == nullptr) return napi_invalid_arg;
  *result = napi_v8_wrap_value(env, value);
  return (*result == nullptr) ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_create_env(int32_t module_api_version,
                                                  napi_env* env_out,
                                                  void** scope_out) {
  return unofficial_napi_create_env_with_options(
      module_api_version, nullptr, env_out, scope_out);
}

napi_status NAPI_CDECL unofficial_napi_create_env_with_options(
    int32_t module_api_version,
    const unofficial_napi_env_create_options* options,
    napi_env* env_out,
    void** scope_out) {
  if (env_out == nullptr || scope_out == nullptr) return napi_invalid_arg;

  EdgeV8Platform* platform = nullptr;
  napi_status status = AcquireRuntime(&platform);
  if (status != napi_ok || platform == nullptr) return status != napi_ok ? status : napi_generic_failure;

  auto allocator = std::make_shared<TrackingArrayBufferAllocator>();
  if (!allocator) {
    ReleaseRuntime();
    return napi_generic_failure;
  }

  v8::Isolate::CreateParams params{};
  params.array_buffer_allocator_shared = allocator;
  if (options != nullptr) {
    if (options->max_young_generation_size_in_bytes > 0) {
      params.constraints.set_max_young_generation_size_in_bytes(
          options->max_young_generation_size_in_bytes);
    }
    if (options->max_old_generation_size_in_bytes > 0) {
      params.constraints.set_max_old_generation_size_in_bytes(
          options->max_old_generation_size_in_bytes);
    }
    if (options->code_range_size_in_bytes > 0) {
      params.constraints.set_code_range_size_in_bytes(
          options->code_range_size_in_bytes);
    }
    if (options->stack_limit != nullptr) {
      params.constraints.set_stack_limit(
          static_cast<uint32_t*>(options->stack_limit));
    }
  }
  ApplyNodeIsolateCreateParams(&params);
  v8::Isolate* isolate = CreateIsolateForEnv(platform, params);
  if (isolate == nullptr) {
    ReleaseRuntime();
    return napi_generic_failure;
  }
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    g_hash_seeds[isolate] = GenerateHashSeed();
    g_tracking_allocators[allocator.get()] = allocator;
  }

  auto* scope = new (std::nothrow) UnofficialEnvScope(isolate, allocator);
  if (scope == nullptr) {
    DisposeIsolateAndWait(platform, isolate);
    {
      std::lock_guard<std::mutex> lock(g_runtime_mu);
      g_tracking_allocators.erase(allocator.get());
    }
    ReleaseRuntime();
    return napi_generic_failure;
  }

  v8::Local<v8::Context> context = v8::Context::New(isolate);
  scope->context.emplace(isolate, context);
  scope->context_scope.emplace(context);
  status = unofficial_napi_create_env_from_context(context, module_api_version, &scope->env);
  if (status != napi_ok || scope->env == nullptr) {
    delete scope;
    DisposeIsolateAndWait(platform, isolate);
    {
      std::lock_guard<std::mutex> lock(g_runtime_mu);
      g_tracking_allocators.erase(allocator.get());
    }
    ReleaseRuntime();
    return (status == napi_ok) ? napi_generic_failure : status;
  }

  *env_out = scope->env;
  *scope_out = scope;
  return napi_ok;
}

napi_status ReleaseEnvScope(void* scope_ptr, uv_loop_t* loop) {
  if (scope_ptr == nullptr) return napi_invalid_arg;
  auto* scope = static_cast<UnofficialEnvScope*>(scope_ptr);

  napi_status status = napi_ok;
  if (scope->env != nullptr) {
    status = unofficial_napi_destroy_env_instance(scope->env);
    scope->env = nullptr;
  }

  v8::Isolate* isolate = scope->isolate;
  std::shared_ptr<TrackingArrayBufferAllocator> allocator = scope->allocator;
  delete scope;

  if (isolate != nullptr) {
    EdgeV8Platform* platform = nullptr;
    {
      std::lock_guard<std::mutex> lock(g_runtime_mu);
      platform = g_runtime.platform.get();
    }
    DisposeIsolateAndWait(platform, isolate, loop);
  }
  if (allocator != nullptr) {
    {
      std::lock_guard<std::mutex> lock(g_runtime_mu);
      g_tracking_allocators.erase(allocator.get());
    }
  }
  ReleaseRuntime();
  return status;
}

napi_status NAPI_CDECL unofficial_napi_release_env(void* scope_ptr) {
  return ReleaseEnvScope(scope_ptr, nullptr);
}

napi_status NAPI_CDECL unofficial_napi_release_env_with_loop(void* scope_ptr,
                                                             uv_loop_t* loop) {
  return ReleaseEnvScope(scope_ptr, loop);
}

napi_status NAPI_CDECL unofficial_napi_low_memory_notification(napi_env env) {
  if (env == nullptr || env->isolate == nullptr) return napi_invalid_arg;
  env->isolate->LowMemoryNotification();
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_set_flags_from_string(
    const char* flags,
    size_t length) {
  if (flags == nullptr) return napi_invalid_arg;
  v8::V8::SetFlagsFromString(flags, static_cast<int>(length));
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_set_prepare_stack_trace_callback(
    napi_env env,
    napi_value callback) {
  if (env == nullptr || env->isolate == nullptr) return napi_invalid_arg;

  v8::Local<v8::Value> raw = callback != nullptr ? napi_v8_unwrap_value(callback) : v8::Local<v8::Value>();
  if (!raw.IsEmpty() && !raw->IsFunction()) return napi_invalid_arg;
  v8::Local<v8::Context> current_context = env->isolate->GetCurrentContext();
  if (current_context.IsEmpty()) {
    current_context = env->context();
  }

  bool has_prepare_stack_trace_callback = false;
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    PrepareStackTraceState& state = g_prepare_stack_trace_callbacks[env];
    v8::Local<v8::Context> principal_context = env->context();
    const bool use_principal_callback =
        current_context.IsEmpty() ||
        (!principal_context.IsEmpty() &&
         (current_context == principal_context ||
          NapiV8IsContextifyContext(env, current_context)));

    if (use_principal_callback) {
      state.principal_callback.Reset();
      if (!raw.IsEmpty()) {
        state.principal_callback.Reset(env->isolate, raw.As<v8::Function>());
      }
    } else {
      for (auto it = state.context_callbacks.begin(); it != state.context_callbacks.end(); ++it) {
        v8::Local<v8::Context> candidate = it->context.Get(env->isolate);
        if (!candidate.IsEmpty() && candidate == current_context) {
          it->context.Reset();
          it->callback.Reset();
          state.context_callbacks.erase(it);
          break;
        }
      }
      if (!raw.IsEmpty()) {
        PrepareStackTraceContextCallback entry;
        entry.context.Reset(env->isolate, current_context);
        entry.callback.Reset(env->isolate, raw.As<v8::Function>());
        state.context_callbacks.push_back(std::move(entry));
      }
    }

    if (state.principal_callback.IsEmpty() && state.context_callbacks.empty()) {
      g_prepare_stack_trace_callbacks.erase(env);
    } else {
      has_prepare_stack_trace_callback = true;
    }
  }

  env->isolate->SetPrepareStackTraceCallback(
      has_prepare_stack_trace_callback ? NapiPrepareStackTraceCallback : nullptr);
  return napi_ok;
}

void DrainMicrotasksForEnv(napi_env env) {
  if (env == nullptr || env->isolate == nullptr) return;
  v8::Local<v8::Context> context = env->context();
  if (!context.IsEmpty()) {
    v8::MicrotaskQueue* queue = context->GetMicrotaskQueue();
    if (queue != nullptr) {
      queue->PerformCheckpoint(env->isolate);
      return;
    }
  }
  env->isolate->PerformMicrotaskCheckpoint();
}

napi_status NAPI_CDECL unofficial_napi_request_gc_for_testing(napi_env env) {
  if (env == nullptr || env->isolate == nullptr) return napi_invalid_arg;
  // Match Node test expectations for global.gc(): force an actual full GC
  // cycle rather than only hinting memory pressure.
  env->isolate->RequestGarbageCollectionForTesting(
      v8::Isolate::GarbageCollectionType::kFullGarbageCollection);
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_process_microtasks(napi_env env) {
  if (env == nullptr || env->isolate == nullptr) return napi_invalid_arg;
  // Keep this helper scoped to the current context's microtask queue.
  // Foreground task pumping is owned by higher-level runtime loop policy.
  DrainMicrotasksForEnv(env);
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_terminate_execution(napi_env env) {
  if (env == nullptr || env->isolate == nullptr) return napi_invalid_arg;
  env->isolate->TerminateExecution();
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_cancel_terminate_execution(napi_env env) {
  if (env == nullptr || env->isolate == nullptr) return napi_invalid_arg;
  env->isolate->CancelTerminateExecution();
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_request_interrupt(
    napi_env env,
    unofficial_napi_interrupt_callback callback,
    void* data) {
  if (env == nullptr || env->isolate == nullptr || callback == nullptr) {
    return napi_invalid_arg;
  }

  auto* request = new (std::nothrow) InterruptRequest();
  if (request == nullptr) return napi_generic_failure;
  request->env = env;
  request->callback = callback;
  request->data = data;

  env->isolate->RequestInterrupt(
      [](v8::Isolate* isolate, void* raw) {
        std::unique_ptr<InterruptRequest> request(
            static_cast<InterruptRequest*>(raw));
        if (request == nullptr || request->env == nullptr ||
            request->callback == nullptr ||
            request->env->isolate != isolate) {
          return;
        }
        v8::HandleScope handle_scope(isolate);
        v8::Local<v8::Context> context = request->env->context();
        if (context.IsEmpty()) return;
        v8::Context::Scope context_scope(context);
        request->callback(request->env, request->data);
      },
      request);
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_structured_clone(
    napi_env env,
    napi_value value,
    napi_value* result_out) {
  return StructuredCloneImpl(env, value, nullptr, result_out);
}

napi_status NAPI_CDECL unofficial_napi_structured_clone_with_transfer(
    napi_env env,
    napi_value value,
    napi_value transfer_list,
    napi_value* result_out) {
  return StructuredCloneImpl(env, value, transfer_list, result_out);
}

napi_status NAPI_CDECL unofficial_napi_serialize_value(
    napi_env env,
    napi_value value,
    void** payload_out) {
  if (env == nullptr || env->isolate == nullptr || value == nullptr || payload_out == nullptr) {
    return napi_invalid_arg;
  }
  *payload_out = nullptr;

  v8::Isolate* isolate = env->isolate;
  v8::HandleScope handle_scope(isolate);
  v8::Local<v8::Context> context = env->context();
  v8::Context::Scope context_scope(context);

  v8::Local<v8::Value> input = napi_v8_unwrap_value(value);
  StructuredCloneSerializerDelegate serializer_delegate(isolate);
  v8::ValueSerializer serializer(isolate, &serializer_delegate);

  serializer.WriteHeader();
  if (serializer.WriteValue(context, input).IsNothing()) {
    return napi_pending_exception;
  }

  std::pair<uint8_t*, size_t> released = serializer.Release();
  if (released.first == nullptr) return napi_generic_failure;

  auto* payload = new (std::nothrow) SerializedClonePayload();
  if (payload == nullptr) {
    std::free(released.first);
    return napi_generic_failure;
  }
  payload->bytes.assign(released.first, released.first + released.second);
  std::free(released.first);
  payload->shared_array_buffers = serializer_delegate.shared_array_buffers();
  payload->wasm_modules = serializer_delegate.TakeWasmModules();
  *payload_out = payload;
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_deserialize_value(
    napi_env env,
    void* payload_ptr,
    napi_value* result_out) {
  if (env == nullptr || env->isolate == nullptr || payload_ptr == nullptr || result_out == nullptr) {
    return napi_invalid_arg;
  }
  *result_out = nullptr;

  auto* payload = static_cast<SerializedClonePayload*>(payload_ptr);
  v8::Isolate* isolate = env->isolate;
  v8::HandleScope handle_scope(isolate);
  v8::Local<v8::Context> context = env->context();
  v8::Context::Scope context_scope(context);

  StructuredCloneDeserializerDelegate deserializer_delegate(
      isolate, payload->shared_array_buffers, payload->wasm_modules);
  v8::ValueDeserializer deserializer(
      isolate,
      payload->bytes.data(),
      payload->bytes.size(),
      &deserializer_delegate);

  for (uint32_t i = 0; i < payload->array_buffers.size(); ++i) {
    v8::Local<v8::ArrayBuffer> array_buffer =
        v8::ArrayBuffer::New(isolate, payload->array_buffers[i]);
    deserializer.TransferArrayBuffer(i, array_buffer);
  }

  bool header_ok = false;
  if (!deserializer.ReadHeader(context).To(&header_ok) || !header_ok) {
    return napi_pending_exception;
  }

  v8::Local<v8::Value> output;
  if (!deserializer.ReadValue(context).ToLocal(&output)) {
    return napi_pending_exception;
  }

  *result_out = napi_v8_wrap_value(env, output);
  return *result_out == nullptr ? napi_generic_failure : napi_ok;
}

void NAPI_CDECL unofficial_napi_release_serialized_value(void* payload_ptr) {
  delete static_cast<SerializedClonePayload*>(payload_ptr);
}

napi_status NAPI_CDECL unofficial_napi_enqueue_microtask(napi_env env, napi_value callback) {
  if (env == nullptr || env->isolate == nullptr || callback == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> raw = napi_v8_unwrap_value(callback);
  if (!raw->IsFunction()) return napi_function_expected;
  env->context()->GetMicrotaskQueue()->EnqueueMicrotask(env->isolate, raw.As<v8::Function>());
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_set_promise_reject_callback(napi_env env,
                                                                   napi_value callback) {
  if (env == nullptr || env->isolate == nullptr || callback == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> raw = napi_v8_unwrap_value(callback);
  if (!raw->IsFunction()) return napi_function_expected;
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    g_env_by_isolate[env->isolate] = env;
    auto& slot = g_promise_reject_callbacks[env->isolate];
    slot.Reset();
    slot.Reset(env->isolate, raw.As<v8::Function>());
  }
  env->isolate->SetPromiseRejectCallback(PromiseRejectCallback);
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_set_promise_hooks(napi_env env,
                                                         napi_value init,
                                                         napi_value before,
                                                         napi_value after,
                                                         napi_value resolve) {
  if (env == nullptr || env->isolate == nullptr) return napi_invalid_arg;

  std::array<v8::Local<v8::Function>, 4> hooks;
  if (!GetPromiseHookFunction(env, init, &hooks[0]) ||
      !GetPromiseHookFunction(env, before, &hooks[1]) ||
      !GetPromiseHookFunction(env, after, &hooks[2]) ||
      !GetPromiseHookFunction(env, resolve, &hooks[3])) {
    return napi_function_expected;
  }

  std::array<v8::Global<v8::Function>, 4> persistent_hooks;
  for (size_t i = 0; i < hooks.size(); ++i) {
    if (!hooks[i].IsEmpty()) {
      persistent_hooks[i].Reset(env->isolate, hooks[i]);
    }
  }
  const bool has_hooks = HasPromiseHooks(persistent_hooks);

  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    g_env_by_isolate[env->isolate] = env;

    auto existing = g_promise_hooks.find(env->isolate);
    if (existing != g_promise_hooks.end()) {
      for (auto& hook : existing->second) {
        hook.Reset();
      }
    }

    if (has_hooks) {
      g_promise_hooks[env->isolate] = std::move(persistent_hooks);
    } else if (existing != g_promise_hooks.end()) {
      g_promise_hooks.erase(existing);
    }
  }

  env->isolate->SetPromiseHook(has_hooks ? PromiseHookCallback : nullptr);
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_get_promise_details(napi_env env,
                                                           napi_value promise,
                                                           int32_t* state_out,
                                                           napi_value* result_out,
                                                           bool* has_result_out) {
  if (env == nullptr || promise == nullptr || state_out == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> raw = napi_v8_unwrap_value(promise);
  if (raw.IsEmpty() || !raw->IsPromise()) return napi_invalid_arg;

  v8::Local<v8::Promise> p = raw.As<v8::Promise>();
  const v8::Promise::PromiseState state = p->State();
  *state_out = static_cast<int32_t>(state);

  const bool has_result = state != v8::Promise::PromiseState::kPending;
  if (has_result_out != nullptr) *has_result_out = has_result;

  if (result_out != nullptr) {
    *result_out = nullptr;
    if (has_result) {
      *result_out = napi_v8_wrap_value(env, p->Result());
      if (*result_out == nullptr) return napi_generic_failure;
    }
  }

  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_get_error_source_positions(
    napi_env env,
    napi_value error,
    unofficial_napi_error_source_positions* out) {
  return unofficial_napi_internal::GetErrorSourcePositions(env, error, out);
}

napi_status NAPI_CDECL unofficial_napi_preserve_error_source_message(
    napi_env env,
    napi_value error) {
  if (env == nullptr || env->isolate == nullptr || error == nullptr) {
    return napi_invalid_arg;
  }

  v8::HandleScope scope(env->isolate);
  v8::Local<v8::Context> context = env->context();
  v8::Local<v8::Value> raw = napi_v8_unwrap_value(error);
  if (raw.IsEmpty() || !raw->IsObject()) {
    return napi_invalid_arg;
  }

  v8::Local<v8::Message> message = v8::Exception::CreateMessage(env->isolate, raw);
  if (message.IsEmpty()) {
    return napi_generic_failure;
  }

  unofficial_napi_internal::SetArrowMessage(
      env->isolate, context, raw, message);
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_mark_promise_as_handled(
    napi_env env,
    napi_value promise) {
  if (env == nullptr || promise == nullptr) return napi_invalid_arg;

  v8::Local<v8::Value> raw = napi_v8_unwrap_value(promise);
  if (raw.IsEmpty() || !raw->IsPromise()) return napi_invalid_arg;

  v8::Isolate* isolate = env->isolate;
  v8::HandleScope handle_scope(isolate);
  v8::Local<v8::Context> context = env->context();
  v8::Context::Scope context_scope(context);
  v8::Local<v8::Promise> local_promise = raw.As<v8::Promise>();
  if (local_promise->State() != v8::Promise::PromiseState::kRejected) {
    return napi_ok;
  }

  v8::Local<v8::Function> callback;
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    const auto cb_it = g_promise_reject_callbacks.find(isolate);
    if (cb_it == g_promise_reject_callbacks.end() || cb_it->second.IsEmpty()) {
      return napi_ok;
    }
    callback = cb_it->second.Get(isolate);
  }
  if (callback.IsEmpty()) return napi_ok;

  v8::Local<v8::Value> args[3] = {
      v8::Integer::New(
          isolate,
          static_cast<int32_t>(v8::PromiseRejectEvent::kPromiseHandlerAddedAfterReject)),
      local_promise,
      v8::Undefined(isolate),
  };
  v8::TryCatch try_catch(isolate);
  v8::MaybeLocal<v8::Value> maybe_result =
      callback->Call(context, v8::Undefined(isolate), 3, args);
  if (maybe_result.IsEmpty()) {
    if (try_catch.HasCaught() && !try_catch.HasTerminated()) {
      try_catch.ReThrow();
      return napi_pending_exception;
    }
    return napi_generic_failure;
  }
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_get_proxy_details(napi_env env,
                                                         napi_value proxy,
                                                         napi_value* target_out,
                                                         napi_value* handler_out) {
  if (env == nullptr || proxy == nullptr || target_out == nullptr || handler_out == nullptr) {
    return napi_invalid_arg;
  }

  v8::Local<v8::Value> raw = napi_v8_unwrap_value(proxy);
  if (raw.IsEmpty() || !raw->IsProxy()) return napi_invalid_arg;

  v8::Local<v8::Proxy> p = raw.As<v8::Proxy>();
  *target_out = napi_v8_wrap_value(env, p->GetTarget());
  *handler_out = napi_v8_wrap_value(env, p->GetHandler());
  if (*target_out == nullptr || *handler_out == nullptr) return napi_generic_failure;
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_preview_entries(napi_env env,
                                                       napi_value value,
                                                       napi_value* entries_out,
                                                       bool* is_key_value_out) {
  if (env == nullptr || value == nullptr || entries_out == nullptr || is_key_value_out == nullptr) {
    return napi_invalid_arg;
  }

  v8::Local<v8::Value> raw = napi_v8_unwrap_value(value);
  if (raw.IsEmpty() || !raw->IsObject()) return napi_invalid_arg;

  bool is_key_value = false;
  v8::Local<v8::Array> entries;
  if (!raw.As<v8::Object>()->PreviewEntries(&is_key_value).ToLocal(&entries)) {
    return napi_generic_failure;
  }
  *entries_out = napi_v8_wrap_value(env, entries);
  if (*entries_out == nullptr) return napi_generic_failure;
  *is_key_value_out = is_key_value;
  return napi_ok;
}

namespace {

napi_status GetCallSitesImpl(napi_env env,
                             uint32_t frames,
                             uint32_t skip_frames,
                             napi_value* callsites_out) {
  if (env == nullptr || env->isolate == nullptr || callsites_out == nullptr) return napi_invalid_arg;
  if (frames < 1 || frames > 200) return napi_invalid_arg;

  v8::Isolate* isolate = env->isolate;
  v8::HandleScope scope(isolate);
  v8::Local<v8::Context> context = env->context();

  v8::Local<v8::StackTrace> stack = v8::StackTrace::CurrentStackTrace(isolate, frames + skip_frames);
  const int frame_count = stack->GetFrameCount();
  const int start_index = frame_count > 0 ? static_cast<int>(skip_frames) : 0;
  const int available = frame_count - start_index;
  uint32_t count = available > 0 ? static_cast<uint32_t>(available) : 0;
  if (count > frames) count = frames;
  v8::Local<v8::Array> out = v8::Array::New(isolate, count);

  auto set_named = [&](v8::Local<v8::Object> obj, const char* key, v8::Local<v8::Value> value) -> bool {
    v8::Local<v8::String> k;
    if (!v8::String::NewFromUtf8(isolate, key, v8::NewStringType::kNormal).ToLocal(&k)) return false;
    return obj->Set(context, k, value).FromMaybe(false);
  };

  for (uint32_t out_index = 0; out_index < count; ++out_index) {
    const int i = start_index + static_cast<int>(out_index);
    v8::Local<v8::StackFrame> frame = stack->GetFrame(isolate, i);
    v8::Local<v8::Object> callsite = v8::Object::New(isolate);
    if (callsite->SetPrototype(context, v8::Null(isolate)).IsNothing()) return napi_generic_failure;

    v8::Local<v8::Value> function_name = frame->GetFunctionName();
    if (function_name.IsEmpty()) function_name = v8::String::Empty(isolate);

    v8::Local<v8::Value> script_name = frame->GetScriptName();
    if (script_name.IsEmpty()) script_name = v8::String::Empty(isolate);
    const std::string script_id = std::to_string(frame->GetScriptId());
    v8::Local<v8::String> script_id_v8;
    if (!v8::String::NewFromUtf8(isolate,
                                 script_id.data(),
                                 v8::NewStringType::kNormal,
                                 static_cast<int>(script_id.size()))
             .ToLocal(&script_id_v8)) {
      return napi_generic_failure;
    }

    const uint32_t line = frame->GetLineNumber();
    const uint32_t col = frame->GetColumn();
    if (!set_named(callsite, "functionName", function_name) ||
        !set_named(callsite, "scriptId", script_id_v8) ||
        !set_named(callsite, "scriptName", script_name) ||
        !set_named(callsite, "lineNumber", v8::Integer::NewFromUnsigned(isolate, line)) ||
        !set_named(callsite, "columnNumber", v8::Integer::NewFromUnsigned(isolate, col)) ||
        !set_named(callsite, "column", v8::Integer::NewFromUnsigned(isolate, col)) ||
        out->Set(context, out_index, callsite).IsNothing()) {
      return napi_generic_failure;
    }
  }

  *callsites_out = napi_v8_wrap_value(env, out);
  if (*callsites_out == nullptr) return napi_generic_failure;
  return napi_ok;
}

}  // namespace

napi_status NAPI_CDECL unofficial_napi_get_call_sites(napi_env env,
                                                      uint32_t frames,
                                                      napi_value* callsites_out) {
  return GetCallSitesImpl(env, frames, 1, callsites_out);
}

napi_status NAPI_CDECL unofficial_napi_get_current_stack_trace(napi_env env,
                                                               uint32_t frames,
                                                               napi_value* callsites_out) {
  return GetCallSitesImpl(env, frames, 0, callsites_out);
}

napi_status NAPI_CDECL unofficial_napi_get_caller_location(napi_env env, napi_value* location_out) {
  if (env == nullptr || env->isolate == nullptr || location_out == nullptr) return napi_invalid_arg;
  *location_out = nullptr;

  v8::Isolate* isolate = env->isolate;
  v8::HandleScope scope(isolate);

  v8::Local<v8::StackTrace> trace = v8::StackTrace::CurrentStackTrace(isolate, 2);
  if (trace->GetFrameCount() != 2) {
    return napi_ok;
  }

  v8::Local<v8::StackFrame> frame = trace->GetFrame(isolate, 1);
  v8::Local<v8::Value> file = frame->GetScriptNameOrSourceURL();
  if (file.IsEmpty()) {
    return napi_ok;
  }
  v8::Local<v8::Value> values[] = {
      v8::Integer::New(isolate, frame->GetLineNumber()),
      v8::Integer::New(isolate, frame->GetColumn()),
      file,
  };
  v8::Local<v8::Array> location = v8::Array::New(isolate, values, 3);
  *location_out = napi_v8_wrap_value(env, location);
  return *location_out == nullptr ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_arraybuffer_view_has_buffer(napi_env env,
                                                                   napi_value value,
                                                                   bool* result_out) {
  if (env == nullptr || value == nullptr || result_out == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> raw = napi_v8_unwrap_value(value);
  if (raw.IsEmpty() || !raw->IsArrayBufferView()) return napi_invalid_arg;
  *result_out = raw.As<v8::ArrayBufferView>()->HasBuffer();
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_get_constructor_name(napi_env env,
                                                            napi_value value,
                                                            napi_value* name_out) {
  if (env == nullptr || value == nullptr || name_out == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> raw = napi_v8_unwrap_value(value);
  if (raw.IsEmpty() || !raw->IsObject()) return napi_invalid_arg;
  v8::Local<v8::String> name = raw.As<v8::Object>()->GetConstructorName();
  *name_out = napi_v8_wrap_value(env, name);
  return *name_out == nullptr ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_get_own_non_index_properties(
    napi_env env,
    napi_value value,
    uint32_t filter_bits,
    napi_value* result_out) {
  if (env == nullptr || value == nullptr || result_out == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> raw = napi_v8_unwrap_value(value);
  if (raw.IsEmpty() || !raw->IsObject()) return napi_invalid_arg;

  v8::Local<v8::Array> properties;
  if (!raw.As<v8::Object>()
           ->GetPropertyNames(env->context(),
                              v8::KeyCollectionMode::kOwnOnly,
                              static_cast<v8::PropertyFilter>(filter_bits),
                              v8::IndexFilter::kSkipIndices)
           .ToLocal(&properties)) {
    return napi_generic_failure;
  }

  *result_out = napi_v8_wrap_value(env, properties);
  return *result_out == nullptr ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_create_private_symbol(napi_env env,
                                                             const char* utf8description,
                                                             size_t length,
                                                             napi_value* result_out) {
  if (env == nullptr || env->isolate == nullptr || result_out == nullptr) return napi_invalid_arg;
  if (utf8description == nullptr && length > 0) return napi_invalid_arg;

  v8::Isolate* isolate = env->isolate;
  v8::HandleScope scope(isolate);
  v8::Local<v8::Context> context = env->context();

  const char* description = utf8description != nullptr ? utf8description : "";
  const int v8_length = (length == NAPI_AUTO_LENGTH) ? -1 : static_cast<int>(length);
  v8::Local<v8::String> desc;
  if (!v8::String::NewFromUtf8(isolate, description, v8::NewStringType::kInternalized, v8_length)
           .ToLocal(&desc)) {
    return napi_generic_failure;
  }

  v8::Local<v8::Private> priv = v8::Private::ForApi(isolate, desc);
  v8::Local<v8::ObjectTemplate> tmpl = v8::ObjectTemplate::New(isolate);
  tmpl->Set(v8::String::NewFromUtf8Literal(isolate, "value"), priv);

  v8::Local<v8::Object> holder;
  if (!tmpl->NewInstance(context).ToLocal(&holder)) {
    return napi_generic_failure;
  }

  v8::Local<v8::Value> symbol_value;
  if (!holder->Get(context, v8::String::NewFromUtf8Literal(isolate, "value")).ToLocal(&symbol_value)) {
    return napi_generic_failure;
  }

  *result_out = napi_v8_wrap_value(env, symbol_value);
  return *result_out == nullptr ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_get_process_memory_info(
    napi_env env,
    double* heap_total_out,
    double* heap_used_out,
    double* external_out,
    double* array_buffers_out) {
  if (env == nullptr || env->isolate == nullptr || heap_total_out == nullptr ||
      heap_used_out == nullptr || external_out == nullptr || array_buffers_out == nullptr) {
    return napi_invalid_arg;
  }

  v8::HeapStatistics stats;
  env->isolate->GetHeapStatistics(&stats);

  uint64_t array_buffers = 0;
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    auto it = g_tracking_allocators.find(env->isolate->GetArrayBufferAllocator());
    if (it != g_tracking_allocators.end() && it->second) {
      array_buffers = it->second->total_mem_usage();
    }
  }

  *heap_total_out = static_cast<double>(stats.total_heap_size());
  *heap_used_out = static_cast<double>(stats.used_heap_size());
  *external_out = static_cast<double>(stats.external_memory());
  *array_buffers_out = static_cast<double>(array_buffers);
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_get_hash_seed(napi_env env,
                                                     uint64_t* hash_seed_out) {
  if (env == nullptr || env->isolate == nullptr || hash_seed_out == nullptr) {
    return napi_invalid_arg;
  }
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    auto it = g_hash_seeds.find(env->isolate);
    if (it != g_hash_seeds.end()) {
      *hash_seed_out = it->second;
      return napi_ok;
    }
  }
  *hash_seed_out = env->isolate->GetHashSeed();
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_get_heap_statistics(
    napi_env env,
    unofficial_napi_heap_statistics* stats_out) {
  if (env == nullptr || env->isolate == nullptr || stats_out == nullptr) {
    return napi_invalid_arg;
  }

  v8::HeapStatistics stats;
  env->isolate->GetHeapStatistics(&stats);

  stats_out->total_heap_size = stats.total_heap_size();
  stats_out->total_heap_size_executable = stats.total_heap_size_executable();
  stats_out->total_physical_size = stats.total_physical_size();
  stats_out->total_available_size = stats.total_available_size();
  stats_out->used_heap_size = stats.used_heap_size();
  stats_out->heap_size_limit = stats.heap_size_limit();
  stats_out->does_zap_garbage = stats.does_zap_garbage();
  stats_out->malloced_memory = stats.malloced_memory();
  stats_out->peak_malloced_memory = stats.peak_malloced_memory();
  stats_out->number_of_native_contexts = stats.number_of_native_contexts();
  stats_out->number_of_detached_contexts = stats.number_of_detached_contexts();
  stats_out->total_global_handles_size = stats.total_global_handles_size();
  stats_out->used_global_handles_size = stats.used_global_handles_size();
  stats_out->external_memory = stats.external_memory();
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_get_heap_space_count(
    napi_env env,
    uint32_t* count_out) {
  if (env == nullptr || env->isolate == nullptr || count_out == nullptr) {
    return napi_invalid_arg;
  }

  *count_out = static_cast<uint32_t>(env->isolate->NumberOfHeapSpaces());
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_get_heap_space_statistics(
    napi_env env,
    uint32_t space_index,
    unofficial_napi_heap_space_statistics* stats_out) {
  if (env == nullptr || env->isolate == nullptr || stats_out == nullptr) {
    return napi_invalid_arg;
  }

  const uint32_t space_count =
      static_cast<uint32_t>(env->isolate->NumberOfHeapSpaces());
  if (space_index >= space_count) {
    return napi_invalid_arg;
  }

  v8::HeapSpaceStatistics stats;
  env->isolate->GetHeapSpaceStatistics(&stats, space_index);

  std::snprintf(stats_out->space_name,
                sizeof(stats_out->space_name),
                "%s",
                stats.space_name() != nullptr ? stats.space_name() : "");
  stats_out->space_size = stats.space_size();
  stats_out->space_used_size = stats.space_used_size();
  stats_out->space_available_size = stats.space_available_size();
  stats_out->physical_space_size = stats.physical_space_size();
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_get_heap_code_statistics(
    napi_env env,
    unofficial_napi_heap_code_statistics* stats_out) {
  if (env == nullptr || env->isolate == nullptr || stats_out == nullptr) {
    return napi_invalid_arg;
  }

  v8::HeapCodeStatistics stats;
  env->isolate->GetHeapCodeAndMetadataStatistics(&stats);

  stats_out->code_and_metadata_size = stats.code_and_metadata_size();
  stats_out->bytecode_and_metadata_size = stats.bytecode_and_metadata_size();
  stats_out->external_script_source_size = stats.external_script_source_size();
  stats_out->cpu_profiler_metadata_size = stats.cpu_profiler_metadata_size();
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_start_cpu_profile(
    napi_env env,
    unofficial_napi_cpu_profile_start_result* result_out,
    uint32_t* profile_id_out) {
  if (env == nullptr || env->isolate == nullptr || result_out == nullptr ||
      profile_id_out == nullptr) {
    return napi_invalid_arg;
  }
  *result_out = unofficial_napi_cpu_profile_start_ok;
  *profile_id_out = 0;
  if (!IsEnvThreadEntered(env)) return napi_cannot_run_js;

  ProfilerState* state = nullptr;
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    state = &EnsureProfilerState(env);
    if (state->cpu_profiler == nullptr) {
      state->cpu_profiler = v8::CpuProfiler::New(env->isolate);
      if (state->cpu_profiler == nullptr) return napi_generic_failure;
    }
  }

  v8::CpuProfilingResult result = state->cpu_profiler->Start(
      v8::CpuProfilingOptions{v8::CpuProfilingMode::kLeafNodeLineNumbers,
                              v8::CpuProfilingOptions::kNoSampleLimit});
  if (result.status == v8::CpuProfilingStatus::kErrorTooManyProfilers) {
    *result_out = unofficial_napi_cpu_profile_start_too_many;
    return napi_ok;
  }
  if (result.status != v8::CpuProfilingStatus::kStarted) {
    return napi_generic_failure;
  }

  *profile_id_out = static_cast<uint32_t>(result.id);
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    auto it = g_profiler_states.find(env);
    if (it != g_profiler_states.end()) {
      it->second.active_cpu_profiles.push_back(*profile_id_out);
    }
  }
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_stop_cpu_profile(
    napi_env env,
    uint32_t profile_id,
    bool* found_out,
    char** json_out,
    size_t* json_len_out) {
  if (env == nullptr || env->isolate == nullptr || found_out == nullptr ||
      json_out == nullptr || json_len_out == nullptr) {
    return napi_invalid_arg;
  }
  *found_out = false;
  *json_out = nullptr;
  *json_len_out = 0;
  if (!IsEnvThreadEntered(env)) return napi_cannot_run_js;

  v8::CpuProfiler* cpu_profiler = nullptr;
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    auto it = g_profiler_states.find(env);
    if (it == g_profiler_states.end() || it->second.cpu_profiler == nullptr) {
      return napi_ok;
    }
    auto active_it = std::find(
        it->second.active_cpu_profiles.begin(),
        it->second.active_cpu_profiles.end(),
        profile_id);
    if (active_it == it->second.active_cpu_profiles.end()) {
      return napi_ok;
    }
    it->second.active_cpu_profiles.erase(active_it);
    cpu_profiler = it->second.cpu_profiler;
  }

  v8::CpuProfile* profile = cpu_profiler->Stop(profile_id);
  if (profile == nullptr) {
    return napi_ok;
  }

  StringOutputStream stream;
  profile->Serialize(&stream, v8::CpuProfile::SerializationFormat::kJSON);
  profile->Delete();
  if (!CopyStringToMallocBuffer(stream.output(), json_out, json_len_out)) {
    return napi_generic_failure;
  }
  *found_out = true;
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_start_heap_profile(
    napi_env env,
    bool* started_out) {
  if (env == nullptr || env->isolate == nullptr || started_out == nullptr) {
    return napi_invalid_arg;
  }
  *started_out = false;
  if (!IsEnvThreadEntered(env)) return napi_cannot_run_js;

  const bool started = env->isolate->GetHeapProfiler()->StartSamplingHeapProfiler();
  if (started) {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    EnsureProfilerState(env).heap_profile_started = true;
  }
  *started_out = started;
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_stop_heap_profile(
    napi_env env,
    bool* found_out,
    char** json_out,
    size_t* json_len_out) {
  if (env == nullptr || env->isolate == nullptr || found_out == nullptr ||
      json_out == nullptr || json_len_out == nullptr) {
    return napi_invalid_arg;
  }
  *found_out = false;
  *json_out = nullptr;
  *json_len_out = 0;
  if (!IsEnvThreadEntered(env)) return napi_cannot_run_js;

  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    auto it = g_profiler_states.find(env);
    if (it == g_profiler_states.end() || !it->second.heap_profile_started) {
      return napi_ok;
    }
  }

  std::string json;
  if (!SerializeHeapProfile(env->isolate, &json)) {
    return napi_ok;
  }
  env->isolate->GetHeapProfiler()->StopSamplingHeapProfiler();
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    auto it = g_profiler_states.find(env);
    if (it != g_profiler_states.end()) {
      it->second.heap_profile_started = false;
    }
  }
  if (!CopyStringToMallocBuffer(json, json_out, json_len_out)) {
    return napi_generic_failure;
  }
  *found_out = true;
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_take_heap_snapshot(
    napi_env env,
    const unofficial_napi_heap_snapshot_options* options,
    char** json_out,
    size_t* json_len_out) {
  if (env == nullptr || env->isolate == nullptr || json_out == nullptr ||
      json_len_out == nullptr) {
    return napi_invalid_arg;
  }
  *json_out = nullptr;
  *json_len_out = 0;
  if (!IsEnvThreadEntered(env)) return napi_cannot_run_js;

  v8::HeapProfiler::HeapSnapshotOptions snapshot_options;
  snapshot_options.snapshot_mode =
      (options != nullptr && options->expose_internals)
          ? v8::HeapProfiler::HeapSnapshotMode::kExposeInternals
          : v8::HeapProfiler::HeapSnapshotMode::kRegular;
  snapshot_options.numerics_mode =
      (options != nullptr && options->expose_numeric_values)
          ? v8::HeapProfiler::NumericsMode::kExposeNumericValues
          : v8::HeapProfiler::NumericsMode::kHideNumericValues;

  const v8::HeapSnapshot* snapshot =
      env->isolate->GetHeapProfiler()->TakeHeapSnapshot(snapshot_options);
  if (snapshot == nullptr) return napi_generic_failure;

  StringOutputStream stream;
  snapshot->Serialize(&stream, v8::HeapSnapshot::kJSON);
  const_cast<v8::HeapSnapshot*>(snapshot)->Delete();
  if (!CopyStringToMallocBuffer(stream.output(), json_out, json_len_out)) {
    return napi_generic_failure;
  }
  return napi_ok;
}

void NAPI_CDECL unofficial_napi_free_buffer(void* data) {
  std::free(data);
}

napi_status NAPI_CDECL unofficial_napi_get_continuation_preserved_embedder_data(
    napi_env env,
    napi_value* result_out) {
  if (env == nullptr || env->isolate == nullptr || result_out == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> value = env->isolate->GetContinuationPreservedEmbedderData();
  if (value.IsEmpty()) value = v8::Undefined(env->isolate);
  *result_out = napi_v8_wrap_value(env, value);
  return *result_out == nullptr ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_set_continuation_preserved_embedder_data(
    napi_env env,
    napi_value value) {
  if (env == nullptr || env->isolate == nullptr) return napi_invalid_arg;
  v8::Local<v8::Value> raw =
      value != nullptr ? napi_v8_unwrap_value(value) : v8::Local<v8::Value>();
  if (raw.IsEmpty()) raw = v8::Undefined(env->isolate);
  env->isolate->SetContinuationPreservedEmbedderData(raw);
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_notify_datetime_configuration_change(napi_env env) {
  if (env == nullptr || env->isolate == nullptr) return napi_invalid_arg;
#if defined(__POSIX__)
  tzset();
#elif defined(_WIN32)
  _tzset();
#endif
  env->isolate->DateTimeConfigurationChangeNotification(
      v8::Isolate::TimeZoneDetection::kRedetect);
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_create_serdes_binding(napi_env env,
                                                             napi_value* result_out) {
  if (env == nullptr || env->isolate == nullptr || result_out == nullptr) return napi_invalid_arg;

  v8::Isolate* isolate = env->isolate;
  v8::HandleScope handle_scope(isolate);
  v8::Local<v8::Context> context = env->context();
  v8::Context::Scope context_scope(context);

  v8::Local<v8::Object> target = v8::Object::New(isolate);
  v8::Local<v8::External> env_data = v8::External::New(isolate, env);

  v8::Local<v8::FunctionTemplate> serializer_tmpl =
      v8::FunctionTemplate::New(isolate, SerializerContext::New, env_data);
  serializer_tmpl->InstanceTemplate()->SetInternalFieldCount(1);
  serializer_tmpl->SetClassName(OneByteString(isolate, "Serializer"));
  SetProtoMethod(isolate, serializer_tmpl, "writeHeader", SerializerContext::WriteHeader);
  SetProtoMethod(isolate, serializer_tmpl, "writeValue", SerializerContext::WriteValue);
  SetProtoMethod(isolate, serializer_tmpl, "releaseBuffer", SerializerContext::ReleaseBuffer);
  SetProtoMethod(isolate, serializer_tmpl, "transferArrayBuffer", SerializerContext::TransferArrayBuffer);
  SetProtoMethod(isolate, serializer_tmpl, "writeUint32", SerializerContext::WriteUint32);
  SetProtoMethod(isolate, serializer_tmpl, "writeUint64", SerializerContext::WriteUint64);
  SetProtoMethod(isolate, serializer_tmpl, "writeDouble", SerializerContext::WriteDouble);
  SetProtoMethod(isolate, serializer_tmpl, "writeRawBytes", SerializerContext::WriteRawBytes);
  SetProtoMethod(isolate,
                 serializer_tmpl,
                 "_setTreatArrayBufferViewsAsHostObjects",
                 SerializerContext::SetTreatArrayBufferViewsAsHostObjects);
  serializer_tmpl->ReadOnlyPrototype();
  if (!SetConstructorFunction(context, target, "Serializer", serializer_tmpl)) {
    return napi_generic_failure;
  }

  v8::Local<v8::FunctionTemplate> deserializer_tmpl =
      v8::FunctionTemplate::New(isolate, DeserializerContext::New, env_data);
  deserializer_tmpl->InstanceTemplate()->SetInternalFieldCount(1);
  deserializer_tmpl->SetClassName(OneByteString(isolate, "Deserializer"));
  SetProtoMethod(isolate, deserializer_tmpl, "readHeader", DeserializerContext::ReadHeader);
  SetProtoMethod(isolate, deserializer_tmpl, "readValue", DeserializerContext::ReadValue);
  SetProtoMethod(
      isolate, deserializer_tmpl, "getWireFormatVersion", DeserializerContext::GetWireFormatVersion);
  SetProtoMethod(
      isolate, deserializer_tmpl, "transferArrayBuffer", DeserializerContext::TransferArrayBuffer);
  SetProtoMethod(isolate, deserializer_tmpl, "readUint32", DeserializerContext::ReadUint32);
  SetProtoMethod(isolate, deserializer_tmpl, "readUint64", DeserializerContext::ReadUint64);
  SetProtoMethod(isolate, deserializer_tmpl, "readDouble", DeserializerContext::ReadDouble);
  SetProtoMethod(isolate, deserializer_tmpl, "_readRawBytes", DeserializerContext::ReadRawBytes);
  deserializer_tmpl->SetLength(1);
  deserializer_tmpl->ReadOnlyPrototype();
  if (!SetConstructorFunction(context, target, "Deserializer", deserializer_tmpl)) {
    return napi_generic_failure;
  }

  *result_out = napi_v8_wrap_value(env, target);
  return (*result_out == nullptr) ? napi_generic_failure : napi_ok;
}

}  // extern "C"

void* NapiV8GetCurrentEdgeEnvironment(v8::Isolate* isolate) {
  if (isolate == nullptr) return nullptr;
  std::lock_guard<std::mutex> lock(g_runtime_mu);
  auto it = g_env_by_isolate.find(isolate);
  if (it == g_env_by_isolate.end()) return nullptr;
  napi_env env = it->second;
  return env != nullptr ? env->edge_environment : nullptr;
}

void* NapiV8GetCurrentEdgeEnvironment(v8::Local<v8::Context> context) {
  if (context.IsEmpty()) return nullptr;
  napi_env env = nullptr;
  {
    std::lock_guard<std::mutex> lock(g_runtime_mu);
    auto it = g_env_by_isolate.find(context->GetIsolate());
    if (it == g_env_by_isolate.end()) return nullptr;
    env = it->second;
  }
  if (env == nullptr) return nullptr;
  v8::Local<v8::Context> principal_context = env->context();
  if (context != principal_context && !NapiV8IsContextifyContext(env, context)) {
    return nullptr;
  }
  return env->edge_environment;
}
