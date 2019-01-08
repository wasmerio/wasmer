use std::marker::PhantomData;
use std::{
    iter,
    ops::{Index, IndexMut},
    slice,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
pub enum Value {
    /// The `i32` type.
    I32(i32),
    /// The `i64` type.
    I64(i64),
    /// The `f32` type.
    F32(u32),
    /// The `f64` type.
    F64(u64),
}

impl Value {
    pub fn ty(&self) -> Type {
        match self {
            Value::I32(_) => Type::I32,
            Value::I64(_) => Type::I64,
            Value::F32(_) => Type::F32,
            Value::F64(_) => Type::F64,
        }
    }
}

impl From<i32> for Value {
    fn from(i: i32) -> Self {
        Value::I32(i)
    }
}

impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Value::I64(i)
    }
}

impl From<f32> for Value {
    fn from(f: f32) -> Self {
        Value::F32(f as _)
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Self {
        Value::F64(f as _)
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
pub enum Initializer {
    /// Corresponds to a `const.*` instruction.
    Const(Value),
    /// Corresponds to a `get_global` instruction.
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
    pub init: Initializer,
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FuncSig {
    pub params: Vec<Type>,
    pub returns: Vec<Type>,
}

impl FuncSig {
    pub fn check_sig(&self, params: &[Value]) -> bool {
        self.params.len() == params.len()
            && self
                .params
                .iter()
                .zip(params.iter().map(|val| val.ty()))
                .all(|(t0, ref t1)| t0 == t1)
    }
}

pub trait MapIndex {
    fn new(index: usize) -> Self;
    fn index(&self) -> usize;
}

/// Dense item map
#[derive(Debug, Clone)]
pub struct Map<I, T>
where
    I: MapIndex,
{
    elems: Vec<T>,
    _marker: PhantomData<I>,
}

impl<I, T> Map<I, T>
where
    I: MapIndex,
{
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

    pub fn as_ptr(&self) -> *const T {
        self.elems.as_ptr()
    }

    pub fn reserve_exact(&mut self, size: usize) {
        self.elems.reserve_exact(size);
    }

    pub fn iter(&self) -> Iter<T, I> {
        Iter::new(self.elems.iter())
    }
}

impl<I, T> Index<I> for Map<I, T>
where
    I: MapIndex,
{
    type Output = T;
    fn index(&self, index: I) -> &T {
        &self.elems[index.index()]
    }
}

impl<I, T> IndexMut<I> for Map<I, T>
where
    I: MapIndex,
{
    fn index_mut(&mut self, index: I) -> &mut T {
        &mut self.elems[index.index()]
    }
}

impl<'a, I, T> IntoIterator for &'a Map<I, T>
where
    I: MapIndex,
{
    type Item = (I, &'a T);
    type IntoIter = Iter<'a, T, I>;

    fn into_iter(self) -> Self::IntoIter {
        Iter::new(self.elems.iter())
    }
}

impl<'a, I, T> IntoIterator for &'a mut Map<I, T>
where
    I: MapIndex,
{
    type Item = (I, &'a mut T);
    type IntoIter = IterMut<'a, T, I>;

    fn into_iter(self) -> Self::IntoIter {
        IterMut::new(self.elems.iter_mut())
    }
}

pub struct Iter<'a, T: 'a, I: MapIndex> {
    enumerated: iter::Enumerate<slice::Iter<'a, T>>,
    _marker: PhantomData<I>,
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

    fn next(&mut self) -> Option<Self::Item> {
        self.enumerated.next().map(|(i, v)| (I::new(i), v))
    }
}

pub struct IterMut<'a, T: 'a, I: MapIndex> {
    enumerated: iter::Enumerate<slice::IterMut<'a, T>>,
    _marker: PhantomData<I>,
}

impl<'a, T: 'a, I: MapIndex> IterMut<'a, T, I> {
    fn new(iter: slice::IterMut<'a, T>) -> Self {
        Self {
            enumerated: iter.enumerate(),
            _marker: PhantomData,
        }
    }
}

impl<'a, T: 'a, I: MapIndex> Iterator for IterMut<'a, T, I> {
    type Item = (I, &'a mut T);

    fn next(&mut self) -> Option<Self::Item> {
        self.enumerated.next().map(|(i, v)| (I::new(i), v))
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
    ($($ty:ident,)*) => {
        $(
            define_map_index!($ty);
        )*
    };
}

define_map_index![GlobalIndex, FuncIndex, MemoryIndex, TableIndex, SigIndex,];

