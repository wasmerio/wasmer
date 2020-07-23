struct Artifact {}

impl Artifact {
    unsafe fn deserialize(bytes: &[u8]) -> Result<Self, Error>;
    fn info(&self) -> &ModuleInfo;
    fn serialize(&self) -> Result<Vec<u8>, Error>;
    fn module(self) -> Module;
}
