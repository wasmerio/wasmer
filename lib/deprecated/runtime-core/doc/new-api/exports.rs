struct Exports {}

impl Exports {
    fn get<'a, T: Exportable<'a> + Clone + 'a>(&'a self, name: &str) -> Result<T, ExportError>;
}
