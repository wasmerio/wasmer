use test_utils::get_default_store;
use wasmer::{Memory, MemoryError, MemoryType, Pages};

#[test]
fn growing_memory_with_api() {
    let desc = MemoryType::new(Pages(10), Some(Pages(16)), false);
    let store = get_default_store();

    let memory = Memory::new(&store, desc).unwrap();

    assert_eq!(memory.size(), Pages(10));
    let result = memory.grow(Pages(2)).unwrap();
    assert_eq!(result, Pages(10));
    assert_eq!(memory.size(), Pages(12));

    let result = memory.grow(Pages(10));
    assert_eq!(
        result,
        Err(MemoryError::SizeExceeded {
            current: 12.into(),
            attempted_delta: 10.into(),
            maximum: 16.into(),
        })
    );

    let bad_desc = MemoryType::new(Pages(15), Some(Pages(10)), false);
    let bad_result = Memory::new(&store, bad_desc);

    assert!(matches!(
        bad_result,
        Err(MemoryError::InvalidMemoryPlan { .. })
    ));
}
