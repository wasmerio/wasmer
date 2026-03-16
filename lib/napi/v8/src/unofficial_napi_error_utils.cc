#include "unofficial_napi_error_utils.h"

#include <algorithm>
#include <string>

#include "node_api.h"

namespace {

v8::Local<v8::String> OneByteString(v8::Isolate* isolate, const char* value) {
  return v8::String::NewFromUtf8(isolate, value, v8::NewStringType::kInternalized)
      .ToLocalChecked();
}

v8::Local<v8::Private> ApiPrivate(v8::Isolate* isolate, const char* description) {
  return v8::Private::ForApi(isolate, OneByteString(isolate, description));
}

std::string V8ValueToUtf8(v8::Isolate* isolate, v8::Local<v8::Value> value) {
  if (value.IsEmpty()) return {};
  v8::String::Utf8Value utf8(isolate, value);
  if (*utf8 == nullptr) return {};
  return std::string(*utf8, utf8.length());
}

v8::Local<v8::Message> GetMessageFromError(napi_env env, napi_value error) {
  v8::Isolate* isolate = env->isolate;
  v8::Local<v8::Value> raw = napi_v8_unwrap_value(error);
  if (raw.IsEmpty()) return v8::Local<v8::Message>();
  return v8::Exception::CreateMessage(isolate, raw);
}

}  // namespace

namespace unofficial_napi_internal {

std::string BuildSyntaxArrowMessage(v8::Isolate* isolate,
                                    v8::Local<v8::Context> context,
                                    v8::Local<v8::Message> message) {
  if (message.IsEmpty()) return {};

  std::string filename = "<anonymous_script>";
  v8::Local<v8::Value> script_resource_name = message->GetScriptResourceName();
  if (!script_resource_name.IsEmpty() && !script_resource_name->IsUndefined()) {
    const std::string utf8_name = V8ValueToUtf8(isolate, script_resource_name);
    if (!utf8_name.empty()) filename = utf8_name;
  }

  const int line_number = message->GetLineNumber(context).FromMaybe(0);
  v8::MaybeLocal<v8::String> source_line_maybe = message->GetSourceLine(context);
  v8::Local<v8::String> source_line_v8;
  if (!source_line_maybe.ToLocal(&source_line_v8)) return {};

  const std::string source_line = V8ValueToUtf8(isolate, source_line_v8);
  if (source_line.empty()) return {};

  int start = message->GetStartColumn(context).FromMaybe(0);
  int end = message->GetEndColumn(context).FromMaybe(start + 1);
  v8::ScriptOrigin origin = message->GetScriptOrigin();
  const int script_start =
      (line_number - origin.LineOffset()) == 1 ? origin.ColumnOffset() : 0;
  if (start >= script_start) {
    end -= script_start;
    start -= script_start;
  }
  if (end <= start) end = start + 1;
  if (start < 0) start = 0;
  if (end < 0) end = 0;

  std::string underline(static_cast<size_t>(start), ' ');
  underline.append(static_cast<size_t>(std::max(1, end - start)), '^');

  return filename + ":" + std::to_string(line_number) + "\n" +
         source_line + "\n" +
         underline + "\n";
}

void AttachSyntaxArrowMessage(v8::Isolate* isolate,
                              v8::Local<v8::Context> context,
                              v8::Local<v8::Value> exception,
                              v8::Local<v8::Message> message) {
  if (exception.IsEmpty() || !exception->IsObject() || message.IsEmpty()) return;

  v8::Local<v8::Object> err_obj = exception.As<v8::Object>();
  v8::Local<v8::Private> arrow_key = ApiPrivate(isolate, "node:arrowMessage");
  v8::Local<v8::Private> decorated_key = ApiPrivate(isolate, "node:decorated");

  SetArrowMessage(isolate, context, exception, message);

  v8::Local<v8::Value> decorated;
  const bool already_decorated =
      err_obj->GetPrivate(context, decorated_key).ToLocal(&decorated) && decorated->IsTrue();
  if (already_decorated) return;

  v8::Local<v8::Value> existing_arrow;
  if (!err_obj->GetPrivate(context, arrow_key).ToLocal(&existing_arrow) || !existing_arrow->IsString()) {
    return;
  }

  v8::Local<v8::Value> stack_value;
  if (!err_obj->Get(context, OneByteString(isolate, "stack")).ToLocal(&stack_value) || !stack_value->IsString()) {
    return;
  }

  v8::Local<v8::String> decorated_stack =
      v8::String::Concat(isolate, existing_arrow.As<v8::String>(), stack_value.As<v8::String>());
  if (!err_obj->Set(context, OneByteString(isolate, "stack"), decorated_stack).FromMaybe(false)) {
    return;
  }
  (void)err_obj->SetPrivate(context, decorated_key, v8::True(isolate));
}

void SetArrowMessage(v8::Isolate* isolate,
                     v8::Local<v8::Context> context,
                     v8::Local<v8::Value> exception,
                     v8::Local<v8::Message> message) {
  if (exception.IsEmpty() || !exception->IsObject() || message.IsEmpty()) return;

  v8::Local<v8::Object> err_obj = exception.As<v8::Object>();
  v8::Local<v8::Private> arrow_key = ApiPrivate(isolate, "node:arrowMessage");
  v8::Local<v8::Value> existing_arrow;
  if (err_obj->GetPrivate(context, arrow_key).ToLocal(&existing_arrow) && existing_arrow->IsString()) {
    return;
  }

  const std::string arrow = BuildSyntaxArrowMessage(isolate, context, message);
  if (arrow.empty()) return;

  v8::Local<v8::String> arrow_v8;
  if (!v8::String::NewFromUtf8(isolate, arrow.c_str(), v8::NewStringType::kNormal).ToLocal(&arrow_v8)) {
    return;
  }
  (void)err_obj->SetPrivate(context, arrow_key, arrow_v8);
}

napi_status GetErrorSourcePositions(napi_env env,
                                    napi_value error,
                                    unofficial_napi_error_source_positions* out) {
  if (env == nullptr || env->isolate == nullptr || error == nullptr || out == nullptr) {
    return napi_invalid_arg;
  }

  out->source_line = nullptr;
  out->script_resource_name = nullptr;
  out->line_number = 0;
  out->start_column = 0;
  out->end_column = 0;

  v8::Isolate* isolate = env->isolate;
  v8::HandleScope scope(isolate);
  v8::Local<v8::Context> context = env->context();
  v8::Local<v8::Value> raw = napi_v8_unwrap_value(error);
  if (raw.IsEmpty() || !raw->IsObject()) return napi_invalid_arg;

  v8::Local<v8::Message> msg = GetMessageFromError(env, error);
  if (msg.IsEmpty()) return napi_generic_failure;

  v8::Local<v8::String> source_line;
  if (!msg->GetSourceLine(context).ToLocal(&source_line)) {
    return napi_generic_failure;
  }

  int line_number = 0;
  if (!msg->GetLineNumber(context).To(&line_number)) {
    return napi_generic_failure;
  }

  out->source_line = napi_v8_wrap_value(env, source_line);
  if (out->source_line == nullptr) return napi_generic_failure;

  v8::Local<v8::Value> resource_name = msg->GetScriptOrigin().ResourceName();
  out->script_resource_name = napi_v8_wrap_value(env, resource_name);
  if (out->script_resource_name == nullptr) return napi_generic_failure;

  out->line_number = line_number;
  out->start_column = msg->GetStartColumn(context).FromMaybe(0);
  out->end_column = msg->GetEndColumn(context).FromMaybe(out->start_column + 1);
  return napi_ok;
}

}  // namespace unofficial_napi_internal
