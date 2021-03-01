#include "probes.h"

void wasmer_tracing_probe_instance_start() {
  WASMER_INSTANCE_START();
}

void wasmer_tracing_probe_instance_end() {
  WASMER_INSTANCE_END();
}

void wasmer_tracing_probe_function_start() {
  WASMER_FUNCTION_START();
}

void wasmer_tracing_probe_function_invoke2(int arg0, int arg1) {
  WASMER_FUNCTION_INVOKE2(arg0, arg1);
}

void wasmer_tracing_probe_function_end() {
  WASMER_FUNCTION_END();
}
