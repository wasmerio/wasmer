#define _POSIX_C_SOURCE 200809L
#include <node_api.h>
#include <stdint.h>
#include <time.h>

static napi_value read_clock(napi_env env, int resolution) {
    struct timespec t;
    int rc = resolution ? clock_getres(CLOCK_THREAD_CPUTIME_ID, &t)
                        : clock_gettime(CLOCK_THREAD_CPUTIME_ID, &t);
    if (rc != 0) {
        napi_throw_error(env, 0, "Cannot read the POSIX per-thread CPU clock");
        return 0;
    }
    napi_value out;
    if (napi_create_double(env, (double)t.tv_sec * 1e9 + t.tv_nsec, &out) != napi_ok)
        return 0;
    return out;
}

static napi_value now_ns(napi_env env, napi_callback_info info) {
    (void)info;
    return read_clock(env, 0);
}
static napi_value resolution_ns(napi_env env, napi_callback_info info) {
    (void)info;
    return read_clock(env, 1);
}
static napi_value init(napi_env env, napi_value exports) {
    napi_property_descriptor methods[] = {
        {.utf8name = "nowNs", .method = now_ns},
        {.utf8name = "resolutionNs", .method = resolution_ns},
    };
    if (napi_define_properties(env, exports, 2, methods) != napi_ok) return 0;
    return exports;
}
NAPI_MODULE(native_thread_clock, init)
