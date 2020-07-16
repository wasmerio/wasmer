trait HostFunction<Kind, Args, Rets> {
    fn to_raw(self) -> (NonNull<Func>, Option<NonNull<FuncEnv>>);
}
