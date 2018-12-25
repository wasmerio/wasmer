use std::marker::PhantomData;
use std::{slice, iter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    /// The `i32` type.
    I32,
    /// The `i64` type.
    I64,
    /// The `f32` type.
    F32,
    /// The `f64` type.
    F64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Val {
    /// The `i32` type.
    I32(i32),
    /// The `i64` type.
    I64(i64),
    /// The `f32` type.
    F32(u32),
    /// The `f64` type.
    F64(u64),
}

impl Val {
    pub fn ty(&self) -> Type {
        match self {
            Val::I32(_) => Type::I32,
            Val::I64(_) => Type::I64,
            Val::F32(_) => Type::F32,
            Val::F64(_) => Type::F64,
        }
    }
}

impl From<i32> for Val {
    fn from(n: i32) -> Self {
        Self::I32(n)
    }
}

impl From<i64> for Val {
    fn from(n: i64) -> Self {
        Self::I64(n)
    }
}

impl From<f32> for Val {
    fn from(n: f32) -> Self {
        Self::F32(n.to_bits())
    }
}

impl From<f64> for Val {
    fn from(n: f64) -> Self {
        Self::I64(n.to_bits())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementType {
    /// Any wasm function.
    Anyfunc,
}

#[derive(Debug, Clone, Copy)]
pub struct Table {
    /// Type of data stored in this table.
    pub ty: ElementType,
    /// The minimum number of elements that must be stored in this table.
    pub min: u32,
    /// The maximum number of elements in this table.
    pub max: Option<u32>,
}

/// A global value initializer.
/// Overtime, this will be able to represent more and more
/// complex expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalInit {
    Val(Val),
    GetGlobal(GlobalIndex),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalDesc {
    pub mutable: bool,
    pub ty: Type,
}

/// A wasm global.
#[derive(Debug, Clone, Copy)]
pub struct Global {
    pub desc: GlobalDesc,
    pub init: GlobalInit,
}

/// A wasm memory.
#[derive(Debug, Clone, Copy)]
pub struct Memory {
    /// The minimum number of allowed pages.
    pub min: u32,
    /// The maximum number of allowed pages.
    pub max: Option<u32>,
    /// This memory can be shared between wasm threads.
    pub shared: bool,
}

impl Memory {
    pub fn is_static_heap(&self) -> bool {
        self.max.is_some()
    }
}

/// A wasm func.
#[derive(Debug, PartialEq, Eq)]
pub struct FuncSig {
    pub params: Vec<Type>,
    pub returns: Vec<Type>,
}

pub trait MapIndex {
    fn new(index: usize) -> Self;
    fn index(&self) -> usize;
}

/// Dense item map
#[derive(Debug, Clone)]
pub struct Map<T, I>
where
    I: MapIndex,
{
    elems: Vec<T>,
    _marker: PhantomData<I>,
}

impl Map {
    pub fn new() -> Self {
        Self {
            elems: Vec::new(),
            _marker: PhantomData,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            elems: Vec::with_capacity(capacity),
            _marker: PhantomData,
        }
    }

    pub fn get(&self, index: I) -> Option<&T> {
        self.elems.get(index.index())
    }

    pub fn len(&self) -> usize {
        self.elems.len()
    }

    pub fn push(&mut self, value: T) -> I {
        let len = self.len();
        self.elems.push(value);
        I::new(len)
    }
}

impl<T, I> Index<I> for Map<T, I>
where
    I: MapIndex,
{
    type Output = T;
    fn index(&self, index: I) -> &T {
        &self.elems[index.index()]
    }
}

impl<T, I> IndexMut<I> for Map<T, I>
where
    I: MapIndex,
{
    fn index_mut(&mut self, index: I) -> &mut T {
        &mut self.elems[index.index()]
    }
}

impl<'a, T, I> IntoIterator for &'a Map<T, I>
where
    I: MapIndex
{
    type Item = (I, &'a T);
    type IntoIter = Iter<'a, T, I>;

    fn into_iter(self) -> Self::IntoIter {
        Iter::new(self.elems.iter())
    }
}

impl<'a, T, I> IntoIterator for &'a mut Map<T, I>
where
    I: MapIndex
{
    type Item = (I, &'a mut T);
    type IntoIter = IterMut<'a, T, I>;

    fn into_iter(self) -> Self::IntoIter {
        Iter::new(self.elems.iter_mut())
    }
}

pub struct Iter<'a, T: 'a, I: MapIndex> {
    enumerated: iter::Enumerate<slice::Iter<'A, T>>,
    _marker: PhantomData,
}

impl<'a, T: 'a, I: MapIndex> Iter<'a, T, I> {
    fn new(iter: slice::Iter<'a, T>) -> Self {
        Self {
            enumerated: iter.enumerate(),
            _marker: PhantomData,
        }
    }
}

impl<'a, T: 'a, I: MapIndex> Iterator for Iter<'a, T, I> {
    type Item = (I, &'a T);

    fn next(&mut self) -> Self::Item {
        self.enumerated.next().map(|i, v| (I::new(i), v))
    }
}

pub struct IterMut<'a, T: 'a, I: MapIndex> {
    enumerated: iter::Enumerate<slice::Iter<'A, T>>,
    _marker: PhantomData,
}

impl<'a, T: 'a, I: MapIndex> IterMut<'a, T, I> {
    fn new(iter: slice::Iter<'a, T>) -> Self {
        Self {
            enumerated: iter.enumerate(),
            _marker: PhantomData,
        }
    }
}

impl<'a, T: 'a, I: MapIndex> Iterator for IterMut<'a, T, I> {
    type Item = (I, &'a mut T);

    fn next(&mut self) -> Self::Item {
        self.enumerated.next().map(|i, v| (I::new(i), v))
    }
}

macro_rules! define_map_index {
    ($ty:ident) => {
        #[derive(Debug, Copy, Clone, PartialEq, Eq)]
        pub struct $ty (u32);
        impl MapIndex for $ty {
            fn new(index: usize) -> Self {
                $ty (index as _)
            }

            fn index(&self) -> usize {
                self.0 as usize
            }
        }
    };
    ($($ty:ident),*) => {
        $(
            define_map_index!($ty);
        )*
    };
}

define_map_index![
    FuncIndex, MemoryIndex, GlobalIndex, TableIndex,
    SignatureIndex,
];