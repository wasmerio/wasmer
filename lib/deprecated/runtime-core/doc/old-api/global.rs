struct Global {}

impl Global {
    fn new(value: Value) -> Self;
    fn new_mutable(value: Value) -> Self;
    fn descriptor(&self) -> GlobalDescriptor;
    fn set(&self, value: Value);
    fn get(&self) -> Value;
}
