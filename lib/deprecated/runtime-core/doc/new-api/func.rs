struct Func<Args, Rets> {}

impl<Args, Rets> Func<Args, Rets> {
    fn new<F>(func: F) -> Self;
    fn params(&self) -> &[Type];
    fn returns(&self) -> &[Type];
    fn call(...) -> Result<Rets, RuntimeError>;
    fn dyn_call(&self, params: &[Value]) -> Result<Box<[Value]>, RuntimeError>;
    fn signature(&self) -> &FuncSig;
}
