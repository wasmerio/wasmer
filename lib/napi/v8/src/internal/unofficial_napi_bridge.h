#ifndef NAPI_V8_UNOFFICIAL_NAPI_BRIDGE_H_
#define NAPI_V8_UNOFFICIAL_NAPI_BRIDGE_H_

#include <v8.h>

#include "unofficial_napi.h"

bool NapiV8LookupForegroundTaskTarget(v8::Isolate* isolate,
                                      napi_env* env_out,
                                      unofficial_napi_enqueue_foreground_task_callback* callback_out);
bool NapiV8IsContextifyContext(napi_env env, v8::Local<v8::Context> context);
void* NapiV8GetCurrentEdgeEnvironment(v8::Isolate* isolate);
void* NapiV8GetCurrentEdgeEnvironment(v8::Local<v8::Context> context);

#endif  // NAPI_V8_UNOFFICIAL_NAPI_BRIDGE_H_
