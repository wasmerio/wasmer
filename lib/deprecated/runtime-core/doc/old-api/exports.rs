struct Exports {}

impl Exports {
    fn get<'a, T: Exportable<'a>>(&'a self, name: &str) -> Result<T, ResolveError>;
    fn into_iter(&self) -> ExportIter;
}
