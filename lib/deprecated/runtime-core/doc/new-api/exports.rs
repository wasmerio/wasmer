struct Exports {}

impl Exports {
    fn new() -> Self;
    fn get<'a, T>(&'a self, name: &str) -> Result<T, ExportError>;
}
