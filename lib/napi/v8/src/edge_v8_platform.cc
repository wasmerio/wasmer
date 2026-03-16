#include "edge_v8_platform.h"

#include <atomic>
#include <condition_variable>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <memory>
#include <mutex>
#include <utility>
#include <vector>

#include <libplatform/libplatform.h>
#include <v8.h>

namespace {

struct WorkerWarmupState {
  std::mutex mutex;
  std::condition_variable cv;
  size_t pending = 0;
};

class WorkerWarmupTask final : public v8::Task {
 public:
  explicit WorkerWarmupTask(WorkerWarmupState* state) : state_(state) {}

  void Run() override {
    if (state_ == nullptr) return;
    std::lock_guard<std::mutex> lock(state_->mutex);
    if (state_->pending > 0) {
      state_->pending -= 1;
    }
    if (state_->pending == 0) {
      state_->cv.notify_all();
    }
  }

 private:
  WorkerWarmupState* state_ = nullptr;
};

void WarmUpFallbackWorkerThreads(v8::Platform* fallback) {
  if (fallback == nullptr) return;

  const int worker_threads = fallback->NumberOfWorkerThreads();
  if (worker_threads <= 0) return;

  WorkerWarmupState state;
  {
    std::lock_guard<std::mutex> lock(state.mutex);
    state.pending = static_cast<size_t>(worker_threads) + 1;
  }

  for (int i = 0; i < worker_threads; ++i) {
    fallback->PostTaskOnWorkerThread(
        v8::TaskPriority::kUserVisible,
        std::make_unique<WorkerWarmupTask>(&state));
  }
  fallback->PostDelayedTaskOnWorkerThread(
      v8::TaskPriority::kUserVisible,
      std::make_unique<WorkerWarmupTask>(&state),
      0);

  std::unique_lock<std::mutex> lock(state.mutex);
  while (state.pending != 0) {
    state.cv.wait(lock);
  }
}

struct ForegroundTaskRecord {
  std::shared_ptr<EdgeV8Platform::IsolateState> isolate_state;
  std::unique_ptr<v8::Task> task;
};

void RunForegroundTaskRecord(napi_env /*env*/, void* data);
void CleanupForegroundTaskRecord(napi_env /*env*/, void* data);

}  // namespace

struct EdgeV8Platform::FinishedCallback {
  void (*callback)(void*) = nullptr;
  void* data = nullptr;
};

struct EdgeV8Platform::IsolateState {
  IsolateState(EdgeV8Platform* platform_in,
               v8::Isolate* isolate_in,
               std::shared_ptr<ForegroundTaskRunner> runner_in)
      : platform(platform_in),
        isolate(isolate_in),
        runner(std::move(runner_in)) {}

  EdgeV8Platform* platform = nullptr;
  v8::Isolate* isolate = nullptr;
  std::shared_ptr<ForegroundTaskRunner> runner;
  std::mutex mutex;
  size_t pending_foreground_tasks = 0;
  bool shutdown_started = false;
  bool finished = false;
  std::vector<FinishedCallback> finished_callbacks;
};

namespace {

class CountedForegroundTask final : public v8::Task {
 public:
  CountedForegroundTask(std::shared_ptr<EdgeV8Platform::IsolateState> isolate_state,
                        std::unique_ptr<v8::Task> task)
      : isolate_state_(std::move(isolate_state)),
        task_(std::move(task)) {}

  ~CountedForegroundTask() override { Finish(); }

  void Run() override {
    if (task_) {
      task_->Run();
      task_.reset();
    }
    Finish();
  }

 private:
  void Finish() {
    if (finished_.exchange(true, std::memory_order_acq_rel)) return;
    std::shared_ptr<EdgeV8Platform::IsolateState> isolate_state =
        std::move(isolate_state_);
    task_.reset();
    if (isolate_state && isolate_state->platform != nullptr) {
      isolate_state->platform->CompletePendingForegroundTask(isolate_state);
    }
  }

  std::shared_ptr<EdgeV8Platform::IsolateState> isolate_state_;
  std::unique_ptr<v8::Task> task_;
  std::atomic<bool> finished_ {false};
};

void RunForegroundTaskRecord(napi_env /*env*/, void* data) {
  auto* record = static_cast<ForegroundTaskRecord*>(data);
  if (record != nullptr && record->task) {
    record->task->Run();
  }
}

void CleanupForegroundTaskRecord(napi_env /*env*/, void* data) {
  auto* record = static_cast<ForegroundTaskRecord*>(data);
  if (record == nullptr) {
    delete record;
    return;
  }
  record->task.reset();
  std::shared_ptr<EdgeV8Platform::IsolateState> isolate_state =
      std::move(record->isolate_state);
  if (isolate_state && isolate_state->platform != nullptr) {
    isolate_state->platform->CompletePendingForegroundTask(isolate_state);
  }
  delete record;
}

}  // namespace

class EdgeV8Platform::ForegroundTaskRunner final : public v8::TaskRunner {
 public:
  ForegroundTaskRunner(std::shared_ptr<IsolateState> isolate_state,
                       v8::Isolate* isolate,
                       v8::Platform* fallback)
      : isolate_state_(std::move(isolate_state)),
        isolate_(isolate),
        fallback_(fallback) {}

  bool IdleTasksEnabled() override { return false; }
  bool NonNestableTasksEnabled() const override { return true; }
  bool NonNestableDelayedTasksEnabled() const override { return true; }

 protected:
  void PostTaskImpl(std::unique_ptr<v8::Task> task,
                    const v8::SourceLocation& location) override {
    PostTaskCommon(std::move(task), 0, location);
  }

  void PostNonNestableTaskImpl(std::unique_ptr<v8::Task> task,
                               const v8::SourceLocation& location) override {
    PostTaskCommon(std::move(task), 0, location);
  }

  void PostDelayedTaskImpl(std::unique_ptr<v8::Task> task,
                           double delay_in_seconds,
                           const v8::SourceLocation& location) override {
    uint64_t delay_ms = 0;
    if (delay_in_seconds > 0) {
      delay_ms = static_cast<uint64_t>(std::llround(delay_in_seconds * 1000.0));
    }
    PostTaskCommon(std::move(task), delay_ms, location);
  }

  void PostNonNestableDelayedTaskImpl(
      std::unique_ptr<v8::Task> task,
      double delay_in_seconds,
      const v8::SourceLocation& location) override {
    PostDelayedTaskImpl(std::move(task), delay_in_seconds, location);
  }

  void PostIdleTaskImpl(std::unique_ptr<v8::IdleTask> /*task*/,
                        const v8::SourceLocation& /*location*/) override {}

 private:
  void PostTaskCommon(std::unique_ptr<v8::Task> task,
                      uint64_t delay_ms,
                      const v8::SourceLocation& location) {
    if (!task) return;
    if (shutting_down_.load(std::memory_order_acquire)) {
      return;
    }
    std::shared_ptr<IsolateState> isolate_state = isolate_state_.lock();

    unofficial_napi_enqueue_foreground_task_callback enqueue =
        target_enqueue_.load(std::memory_order_acquire);
    void* target = target_data_.load(std::memory_order_acquire);
    if (enqueue != nullptr && target != nullptr) {
      ForegroundTaskRecord* record = new (std::nothrow) ForegroundTaskRecord();
      if (record != nullptr) {
        record->isolate_state.reset();
        record->task = std::move(task);
        if (enqueue(target,
                    RunForegroundTaskRecord,
                    record,
                    CleanupForegroundTaskRecord,
                    delay_ms) == napi_ok) {
          if (isolate_state != nullptr &&
              isolate_state->platform != nullptr) {
            isolate_state->platform->AddPendingForegroundTask(isolate_state);
            record->isolate_state = isolate_state;
          }
          return;
        }
        task = std::move(record->task);
        record->isolate_state.reset();
        delete record;
      }
    }

    auto runner = fallback_ != nullptr ? fallback_->GetForegroundTaskRunner(isolate_) : nullptr;
    if (runner) {
      used_fallback_runner_.store(true, std::memory_order_release);
      std::unique_ptr<v8::Task> counted_task = std::move(task);
      if (isolate_state != nullptr && isolate_state->platform != nullptr) {
        isolate_state->platform->AddPendingForegroundTask(isolate_state);
        counted_task =
            std::make_unique<CountedForegroundTask>(isolate_state, std::move(counted_task));
      }
      if (delay_ms == 0) {
        runner->PostTask(std::move(counted_task), location);
      } else {
        runner->PostDelayedTask(std::move(counted_task), delay_ms / 1000.0, location);
      }
    }
  }

  std::weak_ptr<IsolateState> isolate_state_;
  v8::Isolate* isolate_ = nullptr;
  v8::Platform* fallback_ = nullptr;
  std::atomic<napi_env> target_env_ {nullptr};
  std::atomic<unofficial_napi_enqueue_foreground_task_callback> target_enqueue_ {nullptr};
  std::atomic<void*> target_data_ {nullptr};
  std::atomic<bool> shutting_down_ {false};
  std::atomic<bool> used_fallback_runner_ {false};

 public:
  ~ForegroundTaskRunner() override = default;

  void BindTarget(napi_env env,
                  unofficial_napi_enqueue_foreground_task_callback callback,
                  void* target) {
    target_data_.store(target, std::memory_order_release);
    target_enqueue_.store(callback, std::memory_order_release);
    target_env_.store(env, std::memory_order_release);
  }

  void ClearTarget(napi_env env) {
    if (target_env_.load(std::memory_order_acquire) != env) return;
    target_env_.store(nullptr, std::memory_order_release);
    target_enqueue_.store(nullptr, std::memory_order_release);
    target_data_.store(nullptr, std::memory_order_release);
  }

  void NotifyIsolateShutdown() {
    shutting_down_.store(true, std::memory_order_release);
    target_env_.store(nullptr, std::memory_order_release);
    target_enqueue_.store(nullptr, std::memory_order_release);
    target_data_.store(nullptr, std::memory_order_release);
  }

  bool used_fallback_runner() const {
    return used_fallback_runner_.load(std::memory_order_acquire);
  }
};

std::unique_ptr<EdgeV8Platform> EdgeV8Platform::Create() {
  std::unique_ptr<v8::Platform> fallback = v8::platform::NewDefaultPlatform();
  if (!fallback) return nullptr;
  WarmUpFallbackWorkerThreads(fallback.get());
  return std::unique_ptr<EdgeV8Platform>(new EdgeV8Platform(std::move(fallback)));
}

EdgeV8Platform::EdgeV8Platform(std::unique_ptr<v8::Platform> fallback)
    : fallback_(std::move(fallback)) {}

EdgeV8Platform::~EdgeV8Platform() = default;

std::shared_ptr<EdgeV8Platform::IsolateState> EdgeV8Platform::GetState(v8::Isolate* isolate) {
  if (isolate == nullptr) return nullptr;
  std::lock_guard<std::mutex> lock(mutex_);
  auto it = isolates_.find(isolate);
  return it != isolates_.end() ? it->second : nullptr;
}

std::shared_ptr<EdgeV8Platform::IsolateState> EdgeV8Platform::EnsureState(v8::Isolate* isolate) {
  if (isolate == nullptr) return nullptr;
  std::lock_guard<std::mutex> lock(mutex_);
  auto it = isolates_.find(isolate);
  if (it != isolates_.end()) return it->second;
  if (fallback_ != nullptr) {
    // libplatform's NotifyIsolateShutdown() assumes a foreground runner entry
    // exists for every isolate it tears down.
    (void)fallback_->GetForegroundTaskRunner(isolate);
  }
  auto state = std::make_shared<IsolateState>(this, isolate, nullptr);
  state->runner = std::make_shared<ForegroundTaskRunner>(state, isolate, fallback_.get());
  isolates_.emplace(isolate, state);
  return state;
}

std::shared_ptr<EdgeV8Platform::ForegroundTaskRunner> EdgeV8Platform::EnsureRunner(v8::Isolate* isolate) {
  std::shared_ptr<IsolateState> state = EnsureState(isolate);
  return state != nullptr ? state->runner : nullptr;
}

bool EdgeV8Platform::RegisterIsolate(v8::Isolate* isolate) { return EnsureRunner(isolate) != nullptr; }

void EdgeV8Platform::AddPendingForegroundTask(const std::shared_ptr<IsolateState>& state) {
  if (!state) return;
  std::lock_guard<std::mutex> lock(state->mutex);
  if (state->finished) return;
  state->pending_foreground_tasks += 1;
}

void EdgeV8Platform::CompletePendingForegroundTask(const std::shared_ptr<IsolateState>& state) {
  if (!state) return;
  {
    std::lock_guard<std::mutex> lock(state->mutex);
    if (state->pending_foreground_tasks > 0) {
      state->pending_foreground_tasks -= 1;
    }
  }
  MaybeFinishIsolate(state, false);
}

void EdgeV8Platform::BeginShutdown(const std::shared_ptr<IsolateState>& state) {
  if (!state) return;
  std::lock_guard<std::mutex> lock(state->mutex);
  state->shutdown_started = true;
}

void EdgeV8Platform::MaybeFinishIsolate(const std::shared_ptr<IsolateState>& state,
                                        bool begin_shutdown) {
  if (!state) return;

  std::vector<FinishedCallback> callbacks;
  {
    std::lock_guard<std::mutex> lock(state->mutex);
    if (begin_shutdown) {
      state->shutdown_started = true;
    }
    if (state->finished ||
        !state->shutdown_started ||
        state->pending_foreground_tasks != 0) {
      return;
    }
    state->finished = true;
    callbacks.swap(state->finished_callbacks);
  }

  {
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = isolates_.find(state->isolate);
    if (it != isolates_.end() && it->second.get() == state.get()) {
      isolates_.erase(it);
    }
  }

  for (const FinishedCallback& callback : callbacks) {
    if (callback.callback != nullptr) {
      callback.callback(callback.data);
    }
  }
}

void EdgeV8Platform::AddIsolateFinishedCallback(v8::Isolate* isolate,
                                                void (*callback)(void*),
                                                void* data) {
  if (callback == nullptr) return;
  std::shared_ptr<IsolateState> state;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = isolates_.find(isolate);
    if (it != isolates_.end()) {
      state = it->second;
    }
  }
  if (!state) {
    callback(data);
    return;
  }

  bool run_now = false;
  {
    std::lock_guard<std::mutex> lock(state->mutex);
    if (state->finished) {
      run_now = true;
    } else {
      state->finished_callbacks.push_back(FinishedCallback{callback, data});
      run_now = state->shutdown_started && state->pending_foreground_tasks == 0;
    }
  }

  if (run_now) {
    MaybeFinishIsolate(state, false);
  }
}

void EdgeV8Platform::NotifyIsolateShutdown(v8::Isolate* isolate) {
  if (isolate == nullptr) return;

  std::shared_ptr<IsolateState> state = GetState(isolate);
  std::shared_ptr<ForegroundTaskRunner> runner = state != nullptr ? state->runner : nullptr;
  if (runner) {
    runner->NotifyIsolateShutdown();
  }
  // The fallback platform owns all background V8 work for these isolates,
  // even when foreground tasks stayed on the Edge-specific runner.
  if (fallback_ != nullptr) {
    v8::platform::NotifyIsolateShutdown(fallback_.get(), isolate);
  }
  BeginShutdown(state);
  MaybeFinishIsolate(state, false);
}

void EdgeV8Platform::DisposeIsolate(v8::Isolate* isolate) {
  if (isolate == nullptr) return;

  std::shared_ptr<IsolateState> state = GetState(isolate);
  NotifyIsolateShutdown(isolate);

  // Keep the isolate registered while it is being disposed because V8 may
  // still post tasks during teardown, then drop the map entry before the
  // address can be reused for a new isolate.
  isolate->Dispose();
  UnregisterIsolate(isolate);
  MaybeFinishIsolate(state, false);
}

void EdgeV8Platform::UnregisterIsolate(v8::Isolate* isolate) {
  if (isolate == nullptr) return;
  std::lock_guard<std::mutex> lock(mutex_);
  isolates_.erase(isolate);
}

bool EdgeV8Platform::BindForegroundTaskTarget(
    v8::Isolate* isolate,
    napi_env env,
    unofficial_napi_enqueue_foreground_task_callback callback,
    void* target) {
  std::shared_ptr<ForegroundTaskRunner> runner = EnsureRunner(isolate);
  if (!runner) return false;
  runner->BindTarget(env, callback, target);
  return true;
}

void EdgeV8Platform::ClearForegroundTaskTarget(v8::Isolate* isolate, napi_env env) {
  std::shared_ptr<IsolateState> state;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = isolates_.find(isolate);
    if (it == isolates_.end()) return;
    state = it->second;
  }
  if (!state || !state->runner) return;
  state->runner->ClearTarget(env);
}

int EdgeV8Platform::NumberOfWorkerThreads() {
  return fallback_ != nullptr ? fallback_->NumberOfWorkerThreads() : 0;
}

std::shared_ptr<v8::TaskRunner> EdgeV8Platform::GetForegroundTaskRunner(
    v8::Isolate* isolate,
    v8::TaskPriority /*priority*/) {
  return EnsureRunner(isolate);
}

bool EdgeV8Platform::IdleTasksEnabled(v8::Isolate* isolate) {
  (void)isolate;
  return false;
}

double EdgeV8Platform::MonotonicallyIncreasingTime() {
  if (fallback_ != nullptr) return fallback_->MonotonicallyIncreasingTime();
  using clock = std::chrono::steady_clock;
  const auto now = clock::now().time_since_epoch();
  return std::chrono::duration<double>(now).count();
}

double EdgeV8Platform::CurrentClockTimeMillis() {
  return fallback_ != nullptr ? fallback_->CurrentClockTimeMillis()
                              : v8::Platform::SystemClockTimeMillis();
}

v8::TracingController* EdgeV8Platform::GetTracingController() {
  return fallback_ != nullptr ? fallback_->GetTracingController() : nullptr;
}

v8::PageAllocator* EdgeV8Platform::GetPageAllocator() {
  return fallback_ != nullptr ? fallback_->GetPageAllocator() : nullptr;
}

v8::ThreadIsolatedAllocator* EdgeV8Platform::GetThreadIsolatedAllocator() {
  return fallback_ != nullptr ? fallback_->GetThreadIsolatedAllocator() : nullptr;
}

void EdgeV8Platform::OnCriticalMemoryPressure() {
  if (fallback_ != nullptr) fallback_->OnCriticalMemoryPressure();
}

void EdgeV8Platform::DumpWithoutCrashing() {
  if (fallback_ != nullptr) fallback_->DumpWithoutCrashing();
}

v8::HighAllocationThroughputObserver* EdgeV8Platform::GetHighAllocationThroughputObserver() {
  if (fallback_ != nullptr) return fallback_->GetHighAllocationThroughputObserver();
  static v8::HighAllocationThroughputObserver observer;
  return &observer;
}

v8::Platform::StackTracePrinter EdgeV8Platform::GetStackTracePrinter() {
  return fallback_ != nullptr ? fallback_->GetStackTracePrinter() : nullptr;
}

std::unique_ptr<v8::ScopedBlockingCall> EdgeV8Platform::CreateBlockingScope(
    v8::BlockingType blocking_type) {
  return fallback_ != nullptr ? fallback_->CreateBlockingScope(blocking_type) : nullptr;
}

std::unique_ptr<v8::JobHandle> EdgeV8Platform::CreateJobImpl(
    v8::TaskPriority priority,
    std::unique_ptr<v8::JobTask> job_task,
    const v8::SourceLocation& location) {
  (void)location;
  return v8::platform::NewDefaultJobHandle(this, priority, std::move(job_task),
                                           static_cast<size_t>(std::max(1, NumberOfWorkerThreads())));
}

void EdgeV8Platform::PostTaskOnWorkerThreadImpl(v8::TaskPriority priority,
                                               std::unique_ptr<v8::Task> task,
                                               const v8::SourceLocation& location) {
  if (fallback_ != nullptr) {
    fallback_->PostTaskOnWorkerThread(priority, std::move(task), location);
  }
}

void EdgeV8Platform::PostDelayedTaskOnWorkerThreadImpl(
    v8::TaskPriority priority,
    std::unique_ptr<v8::Task> task,
    double delay_in_seconds,
    const v8::SourceLocation& location) {
  if (fallback_ != nullptr) {
    fallback_->PostDelayedTaskOnWorkerThread(priority, std::move(task), delay_in_seconds, location);
  }
}
