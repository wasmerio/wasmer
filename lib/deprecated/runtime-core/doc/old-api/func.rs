struct Func<Args, Rets> {}

impl<Args, Rets> Func<Args, Rets> {
    fn new<F>(func: F) -> Self;
    fn params(&self) -> &'static [Type];
    fn returns(&self) -> &'static [Type];
    fn call(...) -> Results<Rets, RuntimeError>;
    fn get_vm_func(&self) -> NonNull<Func>;
}
