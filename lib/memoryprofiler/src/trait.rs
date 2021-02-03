pub trait MemoryUsage {
    fn size_of_val(&self) -> usize;
}

/// Primitive types
impl MemoryUsage for bool {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self)
    }
}
impl MemoryUsage for char {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self)
    }
}
impl MemoryUsage for f32 {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self)
    }
}
impl MemoryUsage for f64 {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self)
    }
}
impl MemoryUsage for i8 {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self)
    }
}
impl MemoryUsage for i16 {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self)
    }
}
impl MemoryUsage for i32 {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self)
    }
}
impl MemoryUsage for i64 {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self)
    }
}
impl MemoryUsage for isize {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self)
    }
}
impl MemoryUsage for u8 {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self)
    }
}
impl MemoryUsage for u16 {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self)
    }
}
impl MemoryUsage for u32 {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self)
    }
}
impl MemoryUsage for u64 {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self)
    }
}
impl MemoryUsage for usize {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self)
    }
}

// pointer
// Pointers aren't necessarily safe to dereference, even if they're nonnull.

// references
impl<T: MemoryUsage> MemoryUsage for &T {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self)
    }
}
impl<T: MemoryUsage> MemoryUsage for &mut T {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self)
    }
}

// slices
impl<T: MemoryUsage> MemoryUsage for [T] {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self) + self.iter().map(MemoryUsage::size_of_val).sum::<usize>()
    }
}

// arrays
impl<T: MemoryUsage, const N: usize> MemoryUsage for [T; N] {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self) + self.iter().map(MemoryUsage::size_of_val).sum::<usize>()
    }
}

// strs
/*
impl MemoryUsage for str {
    fn size_of_val(&self) -> usize {
        self.as_bytes().size_of()
    }
}
*/

// TODO: tuples

/// Standard library types

// TODO: Arc

//impl<T: MemoryUsage> MemoryUsage for Box<T> {
//}

// Cell

// Is a Pin always dereferenceable?
//impl<T: MemoryUsage> MemoryUsage for Pin<T> {
//}

// TODO: Mutex

// TODO: NonNull might be possible when '*const T' is MemoryUsage.

impl<T: MemoryUsage> MemoryUsage for Option<T> {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self) + self.iter().map(MemoryUsage::size_of_val).sum::<usize>()
    }
}

// TODO: Rc

// TODO: Ref, RefCell, RefMut

//impl<T: MemoryUsage, E: MemoryUsage> MemoryUsage for Result<T, E> {
//}

// TODO: RwLock

// string?

// TODO: UnsafeCell

impl<T: MemoryUsage> MemoryUsage for Vec<T> {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self) + self.iter().map(MemoryUsage::size_of_val).sum::<usize>()
    }
}

impl<T> MemoryUsage for std::marker::PhantomData<T> {
    fn size_of_val(&self) -> usize {
        0
    }
}

// TODO: PhantomPinned?

#[test]
fn test_ints() {
    assert_eq!(MemoryUsage::size_of_val(&32), 4);
    assert_eq!(32.size_of_val(), 4);
}
