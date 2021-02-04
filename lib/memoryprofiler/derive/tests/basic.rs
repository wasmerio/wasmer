use wasmer_memoryprofiler::r#trait::{MemoryUsage, MemoryUsageVisited};
use wasmer_memoryprofiler_derive::MemoryUsage;

use std::collections::BTreeSet;

#[derive(MemoryUsage)]
pub struct Point {
    x: i32,
    y: i32,
}
#[test]
fn test_struct_point() {
    let p = Point { x: 1, y: 2 };
    assert_eq!(8, MemoryUsage::size_of_val(&p, &mut BTreeSet::new()));
}

#[derive(MemoryUsage)]
pub struct AnonymousPoint(i32, i32);
#[test]
fn test_struct_anonymous_point() {
    let p = AnonymousPoint(1, 2);
    assert_eq!(8, MemoryUsage::size_of_val(&p, &mut BTreeSet::new()));
}

#[derive(MemoryUsage)]
pub struct GenericPoint<T>
where
    T: MemoryUsage,
{
    x: T,
    y: T,
}
#[test]
fn test_struct_generic_point() {
    let g = GenericPoint { x: 1i64, y: 2i64 };
    assert_eq!(16, MemoryUsage::size_of_val(&g, &mut BTreeSet::new()));
}

#[derive(MemoryUsage)]
pub struct Empty();
#[test]
fn test_struct_empty() {
    let e = Empty();
    assert_eq!(0, MemoryUsage::size_of_val(&e, &mut BTreeSet::new()));
}

// This struct is packed in order <x, z, y> because 'y: i32' requires 32-bit
// alignment but x and z do not. It starts with bytes 'x...yyyy' then adds 'z' in
// the first place it fits producing 'xz..yyyy' and not 12 bytes 'x...yyyyz...'.
#[derive(MemoryUsage)]
pub struct Padding {
    x: i8,
    y: i32,
    z: i8,
}
#[test]
fn test_struct_padding() {
    let p = Padding { x: 1, y: 2, z: 3 };
    assert_eq!(8, MemoryUsage::size_of_val(&p, &mut BTreeSet::new()));
}

#[derive(MemoryUsage)]
pub enum Things {
    A,
    B(),
    C(i32),
    D { x: i32 },
    E(i32, i32),
    F { x: i32, y: i32 },
}
