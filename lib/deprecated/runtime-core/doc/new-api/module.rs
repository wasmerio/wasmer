struct Module {}

impl Module {
    fn instantiate(&self, import_object: &ImportObject) -> Result<Instance, InstantiationError>;
    fn cache(&self) -> Result<Artifact, Infallible>;
    fn info(&self) -> &ModuleInfo;
    fn imports(&self) -> Vec<ImportDescriptor>;
    fn exports(&self) -> Vec<ExportDescriptor>;
    fn custom_sections(&self, name: impl Asref<str>) -> Option<Vec<Vec<u8>>>;
}
