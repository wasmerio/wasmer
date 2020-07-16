struct Module {}

impl Module {
    fn instantiate(&self, import_object: &ImportObject) -> Result<Instance>;
    fn cache(&self) -> Result<Artifact, CacheError>;
    fn info(&self) -> &ModuleInfo;
    fn imports(&self) -> Vec<ImportDescriptor>;
    fn exports(&self) -> Vec<ExportDescriptor>;
    fn custom_sections(&self, key: impl AsRef<str>) -> Option<&[Vec<u8>]>
}
