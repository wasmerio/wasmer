struct Exports {}

impl Exports {
    fn new() -> Self;
    fn get<'a, T: Exportable<'a> + Clone + 'a>(&'a self, name: &str) -> Result<T, ExportError>;
}
