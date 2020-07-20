struct Namespace {}

impl Namespace {
    fn new() -> Self;
    fn insert<S, E>(&mut self, name: S, export: E);
    fn contains_key<S>(&mut self, key: S) -> bool;
}
