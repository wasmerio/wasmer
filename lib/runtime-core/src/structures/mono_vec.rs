
#[derive(Debug, Clone)]
enum MonoVecInner<T> {
    None,
    Inline(T),
    Heap(Vec<T>),
}

/// A type that can hold zero items,
/// one item, or many items.
#[derive(Debug, Clone)]
pub struct MonoVec<T> {
    inner: MonoVecInner<T>,
}

impl<T> MonoVec<T> {
    pub fn new() -> Self {
        Self {
            inner: MonoVecInner::None,
        }
    }

    pub fn new_inline(item: T) -> Self {
        Self {
            inner: MonoVecInner::Inline(item),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        match capacity {
            0 | 1 => Self::new(),
            _ => Self {
                inner: MonoVecInner::Heap(Vec::with_capacity(capacity)),
            },
        }
    }

    pub fn push(&mut self, item: T) {
        let uninit = unsafe { mem::uninitialized() };
        let prev = mem::replace(&mut self.inner, uninit);
        let next = match prev {
            MonoVecInner::None => MonoVecInner::Inline(item),
            MonoVecInner::Inline(previous_item) => MonoVecInner::Heap(vec![previous_item, item]),
            MonoVecInner::Heap(mut v) => {
                v.push(item);
                MonoVecInner::Heap(v)
            }
        };
        let uninit = mem::replace(&mut self.inner, next);
        mem::forget(uninit);
    }

    pub fn pop(&mut self) -> Option<T> {
        match self.inner {
            MonoVecInner::None => None,
            MonoVecInner::Inline(ref mut item) => {
                let uninit = unsafe { mem::uninitialized() };
                let item = mem::replace(item, uninit);
                let uninit = mem::replace(&mut self.inner, MonoVecInner::None);
                mem::forget(uninit);
                Some(item)
            }
            MonoVecInner::Heap(ref mut v) => v.pop(),
        }
    }

    pub fn as_slice(&self) -> &[T] {
        match self.inner {
            MonoVecInner::None => unsafe {
                slice::from_raw_parts(mem::align_of::<T>() as *const T, 0)
            },
            MonoVecInner::Inline(ref item) => slice::from_ref(item),
            MonoVecInner::Heap(ref v) => &v[..],
        }
    }

    pub fn as_slice_mut(&mut self) -> &mut [T] {
        match self.inner {
            MonoVecInner::None => unsafe {
                slice::from_raw_parts_mut(mem::align_of::<T>() as *mut T, 0)
            },
            MonoVecInner::Inline(ref mut item) => slice::from_mut(item),
            MonoVecInner::Heap(ref mut v) => &mut v[..],
        }
    }
}
