#ifndef EDGE_ENVIRONMENT_H_
#define EDGE_ENVIRONMENT_H_

#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <deque>
#include <map>
#include <mutex>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include <uv.h>

#include "internal_binding/binding_messaging.h"
#include "node_api.h"

struct napi_async_cleanup_hook_handle__ {
  napi_env env = nullptr;
  napi_async_cleanup_hook hook = nullptr;
  void* arg = nullptr;
  bool removed = false;
};

namespace edge {

namespace EnvironmentFlags {
enum Flags : uint64_t {
  kNoFlags = 0,
  kDefaultFlags = 1 << 0,
  kOwnsProcessState = 1 << 1,
  kOwnsInspector = 1 << 2,
  kNoRegisterESMLoader = 1 << 3,
  kTrackUnmanagedFds = 1 << 4,
  kHideConsoleWindows = 1 << 5,
  kNoNativeAddons = 1 << 6,
  kNoGlobalSearchPaths = 1 << 7,
  kNoBrowserGlobals = 1 << 8,
  kNoCreateInspector = 1 << 9,
  kNoStartDebugSignalHandler = 1 << 10,
  kNoWaitForInspectorFrontend = 1 << 11,
};
}  // namespace EnvironmentFlags

struct TickInfo {
  int32_t* fields = nullptr;
  napi_ref ref = nullptr;
};

struct ImmediateInfo {
  int32_t* fields = nullptr;
  napi_ref ref = nullptr;
};

struct TimeoutInfo {
  int32_t* fields = nullptr;
  napi_ref ref = nullptr;
};

struct StreamBaseState {
  int32_t* fields = nullptr;
  napi_ref ref = nullptr;
};

struct PrincipalRealmShim {
  napi_env env = nullptr;
  void* data = nullptr;
};

struct CleanupHookEntry {
  using Callback = void (*)(void* arg);
  Callback callback = nullptr;
  void* arg = nullptr;
};

struct CleanupStageEntry {
  using Callback = void (*)(napi_env env, void* arg);
  Callback callback = nullptr;
  void* arg = nullptr;
  int order = 0;
};

struct AtExitEntry {
  using Callback = void (*)(void* arg);
  Callback callback = nullptr;
  void* arg = nullptr;
};

struct ThreadsafeImmediateEntry {
  using Callback = void (*)(napi_env env, void* data);
  Callback callback = nullptr;
  void* data = nullptr;
  bool refed = true;
};

struct SlotEntry {
  void* data = nullptr;
  void (*deleter)(void* data) = nullptr;
};

using EdgeEnvironmentHandleHasRef = bool (*)(void* data);
using EdgeEnvironmentHandleGetOwner = napi_value (*)(napi_env env, void* data);
using EdgeEnvironmentHandleClose = void (*)(void* data);
using EdgeEnvironmentRequestCancel = void (*)(void* data);
using EdgeEnvironmentRequestGetOwner = napi_value (*)(napi_env env, void* data);

struct ActiveHandleEntry;
struct ActiveRequestEntry;
class Environment;

}  // namespace edge

struct EdgeEnvironmentConfig {
  bool is_main_thread = true;
  bool is_internal_thread = false;
  bool owns_process_state = true;
  bool share_env = true;
  bool tracks_unmanaged_fds = false;
  int32_t thread_id = 0;
  std::string thread_name = "main";
  std::array<double, 4> resource_limits = {-1, -1, -1, -1};
  std::map<std::string, std::string> env_vars;
  internal_binding::EdgeMessagePortDataPtr env_message_port_data;
  std::string local_process_title;
  uint32_t local_debug_port = 0;
  uint64_t flags = edge::EnvironmentFlags::kDefaultFlags;
  uv_loop_t* external_event_loop = nullptr;
};

using EdgeWorkerEnvConfig = EdgeEnvironmentConfig;

enum EdgeEnvironmentSlotId : size_t {
  kEdgeEnvironmentSlotTypesBindingState = 1,
  kEdgeEnvironmentSlotErrorsBindingState,
  kEdgeEnvironmentSlotBufferBindingState,
  kEdgeEnvironmentSlotProcessMethodsBindingState,
  kEdgeEnvironmentSlotReportBindingState,
  kEdgeEnvironmentSlotTlsBindingState,
  kEdgeEnvironmentSlotTimersHostState,
  kEdgeEnvironmentSlotDomainCallbackCache,
  kEdgeEnvironmentSlotParserReadBufferState,
  kEdgeEnvironmentSlotAsyncWrapCache,
  kEdgeEnvironmentSlotPlatformTaskState,
  kEdgeEnvironmentSlotStreamWrapBindingState,
  kEdgeEnvironmentSlotHandleSymbolCache,
  kEdgeEnvironmentSlotHandleWrapEnvState,
  kEdgeEnvironmentSlotActiveResourceState,
  kEdgeEnvironmentSlotPipeBindingState,
  kEdgeEnvironmentSlotStreamSymbolCache,
  kEdgeEnvironmentSlotStreamBaseEnvState,
  kEdgeEnvironmentSlotCaresChannelSet,
  kEdgeEnvironmentSlotTaskQueueBindingState,
  kEdgeEnvironmentSlotUdpBindingState,
  kEdgeEnvironmentSlotTcpBindingState,
  kEdgeEnvironmentSlotTtyBindingState,
  kEdgeEnvironmentSlotPerformanceBindingState,
  kEdgeEnvironmentSlotAsyncContextFrameBindingState,
  kEdgeEnvironmentSlotWorkerParentState,
  kEdgeEnvironmentSlotPermissionBindingState,
  kEdgeEnvironmentSlotHttp2BindingState,
  kEdgeEnvironmentSlotInternalModuleWrapBindingState,
  kEdgeEnvironmentSlotStreamPipeBindingState,
  kEdgeEnvironmentSlotFsDirBindingState,
  kEdgeEnvironmentSlotFsBindingState,
  kEdgeEnvironmentSlotIcuBindingState,
  kEdgeEnvironmentSlotMksnapshotBindingState,
  kEdgeEnvironmentSlotInternalAsyncWrapBindingState,
  kEdgeEnvironmentSlotV8BindingState,
  kEdgeEnvironmentSlotMessagingBindingState,
  kEdgeEnvironmentSlotBlobBindingState,
  kEdgeEnvironmentSlotWasmWebApiBindingState,
  kEdgeEnvironmentSlotCryptoBindingState,
  kEdgeEnvironmentSlotModuleLoaderState,
  kEdgeEnvironmentSlotContextifyRecords,
  kEdgeEnvironmentSlotContextifyModuleWrapBindingState,
  kEdgeEnvironmentSlotPrepareStackTraceState,
  kEdgeEnvironmentSlotProfilerState,
  kEdgeEnvironmentSlotLazyPropertyStore,
};

namespace edge {

class Environment {
 public:
  using CleanupHookCallback = CleanupHookEntry::Callback;
  using CleanupStageCallback = CleanupStageEntry::Callback;
  using AtExitCallback = AtExitEntry::Callback;
  using ThreadsafeImmediateCallback = ThreadsafeImmediateEntry::Callback;
  using InterruptCallback = void (*)(napi_env env, void* data);

  static Environment* Get(napi_env env);
  static Environment* Attach(napi_env env, const EdgeEnvironmentConfig& config);
  static void Detach(napi_env env);

  explicit Environment(napi_env env);
  ~Environment();

  napi_env env() const { return env_; }

  void Configure(const EdgeEnvironmentConfig& config);
  EdgeEnvironmentConfig config() const;

  uint64_t flags() const;
  bool is_main_thread() const;
  bool is_internal_thread() const;
  bool owns_process_state() const;
  bool shares_environment() const;
  bool tracks_unmanaged_fds() const;
  bool stop_requested() const;
  bool exiting() const;
  void set_exiting(bool exiting);
  bool has_exit_code() const;
  int exit_code(int default_code = 0) const;
  void set_exit_code(int code);
  void clear_exit_code();
  int32_t thread_id() const;
  std::string thread_name() const;
  std::array<double, 4> resource_limits() const;
  std::string process_title() const;
  void set_process_title(const std::string& title);
  uint32_t debug_port() const;
  void set_debug_port(uint32_t port);
  std::map<std::string, std::string> snapshot_env_vars() const;
  void set_local_env_var(const std::string& key, const std::string& value);
  void unset_local_env_var(const std::string& key);
  void RequestStop();

  napi_value binding() const;
  void set_binding(napi_value binding);
  napi_value env_message_port() const;
  void set_env_message_port(napi_value port);
  internal_binding::EdgeMessagePortDataPtr env_message_port_data() const;

  napi_status EnsureEventLoop(uv_loop_t** loop_out = nullptr);
  uv_loop_t* event_loop();
  uv_loop_t* GetExistingEventLoop() const;
  uv_loop_t* ReleaseEventLoop();
  static void DestroyReleasedEventLoop(uv_loop_t* loop);
  void CloseAndDestroyEventLoop();
  napi_status InitializeTimers();
  double GetNowMs();
  void ScheduleTimer(int64_t duration_ms);
  void ToggleTimerRef(bool ref);
  void EnsureImmediatePump();
  void ToggleImmediateRef(bool ref);
  int32_t active_timeout_count() const;
  uint32_t immediate_count() const;
  uint32_t immediate_ref_count() const;
  bool immediate_has_outstanding() const;

  void AddCleanupHook(CleanupHookCallback callback, void* arg);
  void RemoveCleanupHook(CleanupHookCallback callback, void* arg);
  void AddCleanupStage(CleanupStageCallback callback, void* arg, int order);
  void RemoveCleanupStage(CleanupStageCallback callback, void* arg);
  void AtExit(AtExitCallback callback, void* arg);
  void RunAtExitCallbacks();
  void RunCleanup(bool close_event_loop = true);
  bool cleanup_started() const;
  bool can_call_into_js() const;
  void set_can_call_into_js(bool can_call_into_js);
  bool is_stopping() const;
  void set_stopping(bool stopping);
  bool filehandle_close_warning() const;
  void set_filehandle_close_warning(bool on);
  void QueueFileHandleGcWarning(int fd, int close_status);

  void AddUnmanagedFd(int fd);
  void RemoveUnmanagedFd(int fd);
  void CloseTrackedUnmanagedFds();

  void* RegisterActiveHandle(napi_value keepalive_owner,
                             const char* resource_name,
                             EdgeEnvironmentHandleHasRef has_ref,
                             EdgeEnvironmentHandleGetOwner get_owner,
                             void* data,
                             EdgeEnvironmentHandleClose close_callback = nullptr);
  void UnregisterActiveHandle(void* token);
  void* RegisterActiveRequest(napi_value owner,
                              const char* resource_name,
                              void* data = nullptr,
                              EdgeEnvironmentRequestCancel cancel = nullptr,
                              EdgeEnvironmentRequestGetOwner get_owner = nullptr);
  void UnregisterActiveRequest(void* token);
  void UnregisterActiveRequestByOwner(napi_value owner);
  void CancelActiveRequests();
  void CloseActiveHandles();
  napi_value GetActiveHandlesArray();
  napi_value GetActiveRequestsArray();
  napi_value GetActiveResourcesInfoArray();

  size_t callback_scope_depth() const;
  void IncrementCallbackScopeDepth();
  void DecrementCallbackScopeDepth();
  size_t open_callback_scopes() const;
  void IncrementOpenCallbackScopes();
  void DecrementOpenCallbackScopes();

  size_t async_callback_scope_depth() const;
  void IncrementAsyncCallbackScopeDepth();
  void DecrementAsyncCallbackScopeDepth();

  std::vector<napi_async_cleanup_hook_handle> async_cleanup_hooks() const;
  void AddAsyncCleanupHook(napi_async_cleanup_hook_handle handle);
  bool RemoveAsyncCleanupHook(napi_async_cleanup_hook_handle handle);
  void RunAsyncCleanupHooks();
  bool async_cleanup_hook_registered() const;
  void set_async_cleanup_hook_registered(bool registered);

  napi_status SetImmediateThreadsafe(ThreadsafeImmediateCallback callback,
                                     void* data,
                                     bool refed);
  napi_status RequestInterrupt(InterruptCallback callback, void* data);
  size_t DrainInterrupts();
  size_t DrainThreadsafeImmediates();

  TickInfo* tick_info();
  ImmediateInfo* immediate_info();
  TimeoutInfo* timeout_info();
  StreamBaseState* stream_base_state();
  PrincipalRealmShim* principal_realm();

  void AssignToContext(void* token);
  void UnassignFromContext(void* token);
  bool HasAttachedContext(void* token) const;

  SlotEntry GetSlot(size_t slot_id) const;
  void SetSlot(size_t slot_id, void* data, void (*deleter)(void* data) = nullptr);
  void ClearSlot(size_t slot_id);

 private:
  static void OnThreadsafeImmediate(uv_async_t* handle);
  static void OnThreadsafeImmediateClosed(uv_handle_t* handle);
  static void OnTimer(uv_timer_t* handle);
  static void OnImmediateCheck(uv_check_t* handle);

  void ResetTrackedRefs();
  void DeleteRefIfPresent(napi_ref* ref);
  bool EnsureThreadsafeImmediateHandleLocked();
  bool EnsureTimerHandleLocked();
  bool EnsureImmediateCheckHandleLocked();
  bool EnsureImmediateIdleHandleLocked();
  void CloseThreadsafeImmediateHandleLocked();
  void ClosePerEnvHandlesLocked();
  void ScheduleTimerFromExpiry(double next_expiry, double now_ms);
  static void OnInterruptFromV8(napi_env env, void* data);
  void CleanupActiveRegistryEntries();
  void EmitProcessWarning(const std::string& message,
                          const char* type = nullptr,
                          const char* code = nullptr) const;
  static uint64_t DeriveFlags(const EdgeEnvironmentConfig& config);

  napi_env env_ = nullptr;
  mutable std::mutex mutex_;
  EdgeEnvironmentConfig config_;
  uint64_t flags_ = EnvironmentFlags::kNoFlags;
  bool cleanup_started_ = false;
  bool at_exit_ran_ = false;
  bool stop_requested_ = false;
  bool exiting_ = false;
  bool has_exit_code_ = false;
  int exit_code_ = 0;
  bool can_call_into_js_ = true;
  bool stopping_ = false;
  bool emit_filehandle_warning_ = true;
  uv_loop_t* loop_ = nullptr;
  uv_timer_t timer_handle_{};
  uv_check_t immediate_check_handle_{};
  uv_idle_t immediate_idle_handle_{};
  bool timer_handle_initialized_ = false;
  bool immediate_check_handle_initialized_ = false;
  bool immediate_check_handle_running_ = false;
  bool immediate_idle_handle_initialized_ = false;
  bool immediate_idle_handle_running_ = false;
  double timer_base_ms_ = -1;
  size_t callback_scope_depth_ = 0;
  size_t open_callback_scopes_ = 0;
  size_t async_callback_scope_depth_ = 0;
  std::vector<napi_async_cleanup_hook_handle> async_cleanup_hooks_;
  bool async_cleanup_hook_registered_ = false;
  std::vector<CleanupHookEntry> cleanup_hooks_;
  std::vector<CleanupStageEntry> cleanup_stages_;
  std::deque<AtExitEntry> at_exit_callbacks_;
  std::unordered_set<int> unmanaged_fds_;
  std::vector<ActiveHandleEntry*> active_handles_;
  std::vector<ActiveRequestEntry*> active_requests_;
  napi_ref binding_ref_ = nullptr;
  napi_ref env_message_port_ref_ = nullptr;
  TickInfo tick_info_;
  ImmediateInfo immediate_info_;
  TimeoutInfo timeout_info_;
  StreamBaseState stream_base_state_;
  PrincipalRealmShim principal_realm_;
  std::unordered_set<void*> attached_contexts_;
  std::unordered_map<size_t, SlotEntry> slots_;
  uv_async_t threadsafe_immediate_async_{};
  bool threadsafe_immediate_async_initialized_ = false;
  bool threadsafe_immediate_async_closed_ = true;
  std::deque<ThreadsafeImmediateEntry> interrupts_;
  std::deque<ThreadsafeImmediateEntry> threadsafe_immediates_;
};

}  // namespace edge

edge::Environment* EdgeEnvironmentGet(napi_env env);
bool EdgeEnvironmentAttach(napi_env env, const EdgeEnvironmentConfig* config = nullptr);
void EdgeEnvironmentDetach(napi_env env);
bool EdgeEnvironmentGetConfig(napi_env env, EdgeEnvironmentConfig* out);
uv_loop_t* EdgeEnvironmentReleaseEventLoop(napi_env env);
void EdgeEnvironmentDestroyReleasedEventLoop(uv_loop_t* loop);
void EdgeEnvironmentRunCleanup(napi_env env);
void EdgeEnvironmentRunCleanupPreserveLoop(napi_env env);
void EdgeEnvironmentRunAtExitCallbacks(napi_env env);
bool EdgeEnvironmentCleanupStarted(napi_env env);
edge::SlotEntry EdgeEnvironmentGetOpaqueSlot(napi_env env, size_t slot_id);
void EdgeEnvironmentSetOpaqueSlot(napi_env env,
                                  size_t slot_id,
                                  void* data,
                                  void (*deleter)(void* data) = nullptr);
void EdgeEnvironmentClearOpaqueSlot(napi_env env, size_t slot_id);
void EdgeEnvironmentRegisterCleanupStage(napi_env env,
                                         edge::Environment::CleanupStageCallback callback,
                                         void* arg,
                                         int order);
void EdgeEnvironmentUnregisterCleanupStage(napi_env env,
                                           edge::Environment::CleanupStageCallback callback,
                                           void* arg);

template <typename T>
T* EdgeEnvironmentGetSlotData(napi_env env, size_t slot_id) {
  return static_cast<T*>(EdgeEnvironmentGetOpaqueSlot(env, slot_id).data);
}

template <typename T>
T& EdgeEnvironmentGetOrCreateSlotData(napi_env env, size_t slot_id) {
  if (auto* existing = EdgeEnvironmentGetSlotData<T>(env, slot_id); existing != nullptr) {
    return *existing;
  }
  auto* created = new T(env);
  EdgeEnvironmentSetOpaqueSlot(
      env,
      slot_id,
      created,
      [](void* data) {
        delete static_cast<T*>(data);
      });
  return *created;
}

#endif  // EDGE_ENVIRONMENT_H_
