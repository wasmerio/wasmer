use wasmer_memory::r#trait::MemoryUsage;
use wasmer_memory_derive::MemoryUsage;

#[derive(MemoryUsage)]
pub struct Point {
    x: i32,
    y: i32,
}

#[derive(MemoryUsage)]
pub struct AnonymousPoint(i32, i32);

#[derive(MemoryUsage)]
pub struct GenericPoint<T>
where
    T: MemoryUsage,
{
    x: T,
    y: T,
}

#[derive(MemoryUsage)]
pub struct Empty();
