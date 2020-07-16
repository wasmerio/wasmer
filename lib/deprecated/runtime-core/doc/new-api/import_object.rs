struct ImportObject {}

impl ImportObject {
    fn new() -> Self;
    fn new_with_data<F>(state_creator: F) -> Self;
    fn register<S, N>(&mut self, name: S, namespace: N) -> Option<Box<dyn LikeNamespace>>;
    fn contains_namespace(&self, name: &str) -> bool;
    fn call_state_creator(&self) -> Option<(*mut c_void, fn(*mut c_void))>;
    fn get_export(&self, module: &str, name: &str) -> Option<Export>;
    fn clone_ref(&self) -> Self;
}
