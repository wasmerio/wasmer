#ifndef NAPI_V8_UNOFFICIAL_NAPI_ERROR_UTILS_H_
#define NAPI_V8_UNOFFICIAL_NAPI_ERROR_UTILS_H_

#include <string>

#include "internal/napi_v8_env.h"
#include "unofficial_napi.h"

namespace unofficial_napi_internal {

std::string BuildSyntaxArrowMessage(v8::Isolate* isolate,
                                    v8::Local<v8::Context> context,
                                    v8::Local<v8::Message> message);

void SetArrowMessage(v8::Isolate* isolate,
                     v8::Local<v8::Context> context,
                     v8::Local<v8::Value> exception,
                     v8::Local<v8::Message> message);

void AttachSyntaxArrowMessage(v8::Isolate* isolate,
                              v8::Local<v8::Context> context,
                              v8::Local<v8::Value> exception,
                              v8::Local<v8::Message> message);

napi_status GetErrorSourcePositions(napi_env env,
                                    napi_value error,
                                    unofficial_napi_error_source_positions* out);

}  // namespace unofficial_napi_internal

#endif  // NAPI_V8_UNOFFICIAL_NAPI_ERROR_UTILS_H_
