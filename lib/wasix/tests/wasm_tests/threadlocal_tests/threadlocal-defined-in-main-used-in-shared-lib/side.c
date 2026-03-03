extern _Thread_local int my_tls_int;

int get_value() {
    return my_tls_int;
}