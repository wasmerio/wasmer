struct ImportObject {
    allow_missing_functions: bool,
}

impl ImportObject {
    fn new() -> Self;
    fn new_with_data<F>(state_creator: F) -> Self;
    fn register<S, N>(&mut self, name: S, namespace: N) -> Option<Box<dyn LikeNamespace>>;
    fn with_namespace<Func, InnerRet>(&self, namespace: &str, f: Func) -> Option<InnerRet>;
    fn maybe_with_namespace<Func, InnerRet>(&self, namespace: &str, f: Func) -> Option<InnerRet>;
    fn contains_namespace(&self, name: &str) -> bool;
}
