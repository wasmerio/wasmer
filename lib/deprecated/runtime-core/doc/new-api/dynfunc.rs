struct DynFunc {}

impl DynFunc {
    fn call(&self, params: &[Value]) -> Result<Vec<Value>, CallError>;
    fn signature(&self) -> &FuncSig;
}
