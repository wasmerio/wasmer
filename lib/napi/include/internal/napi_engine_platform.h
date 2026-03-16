#ifndef NAPI_ENGINE_PLATFORM_H_
#define NAPI_ENGINE_PLATFORM_H_

#include <stdint.h>

#include "js_native_api.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*napi_engine_worker_task_callback)(void* data);
typedef void (*napi_engine_worker_task_cleanup)(void* data);

typedef napi_status (*napi_engine_enqueue_worker_task_callback)(
    void* target,
    napi_engine_worker_task_callback callback,
    void* data,
    napi_engine_worker_task_cleanup cleanup,
    int flags,
    uint64_t delay_millis);

typedef uint32_t (*napi_engine_worker_thread_count_callback)(void* target);

// Internal engine-adapter hook used by embedder runtimes to supply worker task
// execution. This is not part of the N-API contract.
NAPI_EXTERN napi_status napi_engine_set_worker_task_callbacks(
    napi_engine_enqueue_worker_task_callback enqueue_callback,
    napi_engine_worker_thread_count_callback thread_count_callback,
    void* target);

#ifdef __cplusplus
}
#endif

#endif  // NAPI_ENGINE_PLATFORM_H_
