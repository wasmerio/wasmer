struct Instance {
    exports: Exports,
}

impl Instance {
    fn load<T>(&self, _loader: T) -> Result<Self, ()>;
    fn fun<Args, Rets>(&self, name: &str) -> Result<Func<Args, Rets>, ExportError>;
    fn resolve_func(&self, name: &str) -> Result<usize, ()>;
    fn dyn_fun(&self, name: &str) -> Result<DynFunc, ExportError>;
    fn call(&self, name: &str, params: &[Value]) -> Result<Vec<Value>, Box<dyn Error>>;
    fn context(&self) -> Ref<Ctx>;
    fn context_mut(&mut self) -> RefMut<Ctx>;
    fn module(&self) -> Module;
}
