struct Namespace {}

impl Namespace {
    fn new() -> Self;
    fn insert<S, E>(&mut self, name: S, export: E) -> Option<Box<dyn IsExport + Send>>;
    fn contains_key<S>(&mut self, key: S) -> bool;
}
