struct Table {}

impl Table {
    fn new(desc: TableDescriptor, initial_value: Value) -> Result<Self, RuntimeError>;
    fn descriptor(&self) -> TableDescriptor;
    fn set(&self, index: u32, element: Value) -> Result<(), RuntimeError>;
    fn get(&self, index: u32) -> Option<Value>;
    fn size(&self) -> u32;
    fn grow(&self, delta: u32, initial_value: Value) -> Result<u32, RuntimeError>;
}
