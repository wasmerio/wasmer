struct DynamicFunc {}

impl DynamicFunc {
    fn new<F>(signature: &FuncSig, func: F) -> Self;
    fn signature(&self) -> &FuncDescriptor;
    fn params(&self) -> &[Type];
    fn returns(&self) -> &[Type];
    fn call(&self, params: &[Value] -> Result<Box<[Value]>, RuntimeError>;
}
