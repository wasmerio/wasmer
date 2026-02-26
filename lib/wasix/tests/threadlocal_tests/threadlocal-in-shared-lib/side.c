_Thread_local int my_tls_int = 42; // Set a thread-local variable

int get_value() {
    return my_tls_int;
}