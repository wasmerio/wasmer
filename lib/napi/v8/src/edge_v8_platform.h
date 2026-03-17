#ifndef NAPI_V8_EDGE_V8_PLATFORM_H_
#define NAPI_V8_EDGE_V8_PLATFORM_H_

#include <memory>
#include <mutex>
#include <unordered_map>
#include <vector>

#include <v8-platform.h>

#include "unofficial_napi.h"

class EdgeV8Platform final : public v8::Platform {
 public:
  struct FinishedCallback;
  struct IsolateState;

  static std::unique_ptr<EdgeV8Platform> Create();

  ~EdgeV8Platform() override;

  bool RegisterIsolate(v8::Isolate* isolate);
  void AddIsolateFinishedCallback(v8::Isolate* isolate,
                                  void (*callback)(void*),
                                  void* data);
  void NotifyIsolateShutdown(v8::Isolate* isolate);
  void DisposeIsolate(v8::Isolate* isolate);
  void UnregisterIsolate(v8::Isolate* isolate);
  bool BindForegroundTaskTarget(v8::Isolate* isolate,
                                napi_env env,
                                unofficial_napi_enqueue_foreground_task_callback callback,
                                void* target);
  void ClearForegroundTaskTarget(v8::Isolate* isolate, napi_env env);
  void AddPendingForegroundTask(const std::shared_ptr<IsolateState>& state);
  void CompletePendingForegroundTask(const std::shared_ptr<IsolateState>& state);

  int NumberOfWorkerThreads() override;
  std::shared_ptr<v8::TaskRunner> GetForegroundTaskRunner(
      v8::Isolate* isolate, v8::TaskPriority priority) override;
  bool IdleTasksEnabled(v8::Isolate* isolate) override;
  double MonotonicallyIncreasingTime() override;
  double CurrentClockTimeMillis() override;
  v8::TracingController* GetTracingController() override;
  v8::PageAllocator* GetPageAllocator() override;
  v8::ThreadIsolatedAllocator* GetThreadIsolatedAllocator() override;
  void OnCriticalMemoryPressure() override;
  void DumpWithoutCrashing() override;
  v8::HighAllocationThroughputObserver* GetHighAllocationThroughputObserver() override;
  StackTracePrinter GetStackTracePrinter() override;
  std::unique_ptr<v8::ScopedBlockingCall> CreateBlockingScope(
      v8::BlockingType blocking_type) override;

 protected:
  std::unique_ptr<v8::JobHandle> CreateJobImpl(
      v8::TaskPriority priority,
      std::unique_ptr<v8::JobTask> job_task,
      const v8::SourceLocation& location) override;
  void PostTaskOnWorkerThreadImpl(v8::TaskPriority priority,
                                  std::unique_ptr<v8::Task> task,
                                  const v8::SourceLocation& location) override;
  void PostDelayedTaskOnWorkerThreadImpl(
      v8::TaskPriority priority,
      std::unique_ptr<v8::Task> task,
      double delay_in_seconds,
      const v8::SourceLocation& location) override;

 private:
  class ForegroundTaskRunner;

  explicit EdgeV8Platform(std::unique_ptr<v8::Platform> fallback);

  std::shared_ptr<IsolateState> EnsureState(v8::Isolate* isolate);
  std::shared_ptr<ForegroundTaskRunner> EnsureRunner(v8::Isolate* isolate);
  std::shared_ptr<IsolateState> GetState(v8::Isolate* isolate);
  void BeginShutdown(const std::shared_ptr<IsolateState>& state);
  void MaybeFinishIsolate(const std::shared_ptr<IsolateState>& state,
                          bool begin_shutdown);

  std::unique_ptr<v8::Platform> fallback_;
  std::mutex mutex_;
  std::unordered_map<v8::Isolate*, std::shared_ptr<IsolateState>> isolates_;
};

#endif  // NAPI_V8_EDGE_V8_PLATFORM_H_
