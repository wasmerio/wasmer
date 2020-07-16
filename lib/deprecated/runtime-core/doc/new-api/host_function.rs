trait HostFunction<Args, Rets, Kind, T> {
    fn function_body_ptr(self) -> *const VMFunctionBody;
}
