use test_utils::get_default_store;
use wasmer::{Memory, MemoryType, Pages};

#[test]
fn growing_memory_with_api() {
    let desc = MemoryType::new(Pages(10), Some(Pages(16)), false);
    let store = get_default_store();

    let memory = Memory::new(&store, desc);

    assert_eq!(memory.size(), Pages(10));
    let result = memory.grow(Pages(2)).unwrap();
    assert_eq!(result, Pages(10));
    assert_eq!(memory.size(), Pages(12));
}
