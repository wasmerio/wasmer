pub trait MemoryUsage {
    fn size_of(&self) -> usize;
}

/// Primitive types
impl MemoryUsage for bool {
    fn size_of(&self) -> usize {
        std::mem::size_of::<bool>()
    }
}
impl MemoryUsage for char {
    fn size_of(&self) -> usize {
        std::mem::size_of::<char>()
    }
}
impl MemoryUsage for f32 {
    fn size_of(&self) -> usize {
        std::mem::size_of::<f32>()
    }
}
impl MemoryUsage for f64 {
    fn size_of(&self) -> usize {
        std::mem::size_of::<f64>()
    }
}
impl MemoryUsage for i8 {
    fn size_of(&self) -> usize {
        std::mem::size_of::<i8>()
    }
}
impl MemoryUsage for i16 {
    fn size_of(&self) -> usize {
        std::mem::size_of::<i16>()
    }
}
impl MemoryUsage for i32 {
    fn size_of(&self) -> usize {
        std::mem::size_of::<i32>()
    }
}
impl MemoryUsage for i64 {
    fn size_of(&self) -> usize {
        std::mem::size_of::<i64>()
    }
}
impl MemoryUsage for isize {
    fn size_of(&self) -> usize {
        std::mem::size_of::<isize>()
    }
}
impl MemoryUsage for u8 {
    fn size_of(&self) -> usize {
        std::mem::size_of::<u8>()
    }
}
impl MemoryUsage for u16 {
    fn size_of(&self) -> usize {
        std::mem::size_of::<u16>()
    }
}
impl MemoryUsage for u32 {
    fn size_of(&self) -> usize {
        std::mem::size_of::<u32>()
    }
}
impl MemoryUsage for u64 {
    fn size_of(&self) -> usize {
        std::mem::size_of::<u64>()
    }
}
impl MemoryUsage for usize {
    fn size_of(&self) -> usize {
        std::mem::size_of::<usize>()
    }
}

// pointer
// Pointers aren't necessarily safe to dereference, even if they're nonnull.

// references
impl<T: MemoryUsage> MemoryUsage for &T {
    fn size_of(&self) -> usize {
        std::mem::size_of::<&T>()
    }
}
impl<T: MemoryUsage> MemoryUsage for &mut T {
    fn size_of(&self) -> usize {
        std::mem::size_of::<&mut T>()
    }
}

// slices
impl<T: MemoryUsage> MemoryUsage for [T] {
    fn size_of(&self) -> usize {
        std::mem::size_of::<[T; 0]>() + self.iter().map(MemoryUsage::size_of).sum::<usize>()
    }
}

// arrays
impl<T: MemoryUsage, const N: usize> MemoryUsage for [T; N] {
    fn size_of(&self) -> usize {
        std::mem::size_of::<[T; N]>() + self.iter().map(MemoryUsage::size_of).sum::<usize>()
    }
}

// strs
/*
impl MemoryUsage for str {
    fn size_of(&self) -> usize {
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
    fn size_of(&self) -> usize {
        std::mem::size_of::<Option<T>>() + self.iter().map(MemoryUsage::size_of).sum::<usize>()
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
    fn size_of(&self) -> usize {
        std::mem::size_of::<Vec<T>>() + self.iter().map(MemoryUsage::size_of).sum::<usize>()
    }
}

impl<T> MemoryUsage for std::marker::PhantomData<T> {
    fn size_of(&self) -> usize {
        0
    }
}

// TODO: PhantomPinned?

#[test]
fn test_ints() {
    assert_eq!(MemoryUsage::size_of(&32), 4);
    assert_eq!(32.size_of(), 4);
}
