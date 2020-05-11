use wasmer::{Memory, MemoryType, Pages, Store};

#[test]
fn growing_memory_with_api() {
    let desc = MemoryType::new(Pages(10), Some(Pages(16)), false);
    let store = Store::default();

    let memory = Memory::new(&store, desc);

    assert_eq!(memory.size(), Pages(10));
    let result = memory.grow(Pages(2)).unwrap();
    assert_eq!(result, Pages(10));
    assert_eq!(memory.size(), Pages(12));
}
