namespace v8 {
using FatalErrorCallback = void (*)(const char* location, const char* message);
using OOMErrorCallback = void (*)(const char* location, bool is_heap_oom);

class Isolate {
 public:
  static Isolate* TryGetCurrent();
  void SetFatalErrorHandler(FatalErrorCallback callback);
  void SetOOMErrorHandler(OOMErrorCallback callback);
};
}  // namespace v8

extern "C" void snapi_v8_set_fatal_error_handler(
    v8::Isolate* isolate,
    v8::FatalErrorCallback callback) {
  if (isolate == nullptr) return;
  isolate->SetFatalErrorHandler(callback);
}

extern "C" void snapi_v8_set_oom_error_handler(
    v8::Isolate* isolate,
    v8::OOMErrorCallback callback) {
  if (isolate == nullptr) return;
  isolate->SetOOMErrorHandler(callback);
}

extern "C" v8::Isolate* snapi_v8_try_get_current_isolate() {
  return v8::Isolate::TryGetCurrent();
}
