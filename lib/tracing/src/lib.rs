use std::os::raw::c_int;

extern "C" {
    fn wasmer_tracing_probe_instance_start();
    fn wasmer_tracing_probe_instance_end();
    fn wasmer_tracing_probe_function_start();
    fn wasmer_tracing_probe_function_invoke2(arg0: c_int, arg1: c_int);
    fn wasmer_tracing_probe_function_end();
}

pub fn instance_start() {
    unsafe {
        wasmer_tracing_probe_instance_start();
    }
}

pub fn instance_end() {
    unsafe {
        wasmer_tracing_probe_instance_end();
    }
}

pub fn function_start() {
    unsafe {
        wasmer_tracing_probe_function_start();
    }
}

pub fn function_invoke2(arg0: c_int, arg1: c_int) {
    unsafe {
        wasmer_tracing_probe_function_invoke2(arg0, arg1);
    }
}

pub fn function_end() {
    unsafe {
        wasmer_tracing_probe_function_end();
    }
}
