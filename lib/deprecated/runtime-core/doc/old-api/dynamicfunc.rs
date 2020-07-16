struct DynamicFunc {}

impl DynamicFunc {
    fn new<F>(signature: Arc<FuncSig>, func: F) -> Self;
}
