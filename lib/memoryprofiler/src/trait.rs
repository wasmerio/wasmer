pub trait MemoryUsage {
    /// Returns the size of the referenced value in bytes.
    ///
    /// Recursively visits the value and any children returning the sum of their
    /// sizes. The size always includes any tail padding if applicable.
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
        std::mem::size_of_val(self)
            + self
                .iter()
                .map(|v| MemoryUsage::size_of_val(v) - std::mem::size_of_val(v))
                .sum::<usize>()
    }
}

// arrays
impl<T: MemoryUsage, const N: usize> MemoryUsage for [T; N] {
    fn size_of_val(&self) -> usize {
        std::mem::size_of_val(self)
            + self
                .iter()
                .map(|v| MemoryUsage::size_of_val(v) - std::mem::size_of_val(v))
                .sum::<usize>()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Copy, Clone)]
    struct TestMemoryUsage {
        // Must be greater than or equal to std::mem::size_of::<TestMemoryUsage>() or else MemoryUsage may overflow.
        pub size_to_report: usize,
    }
    impl MemoryUsage for TestMemoryUsage {
        fn size_of_val(&self) -> usize {
            // Try to prevent buggy tests before they're hard to debug.
            assert!(self.size_to_report >= std::mem::size_of::<TestMemoryUsage>());
            self.size_to_report
        }
    }

    #[test]
    fn test_ints() {
        assert_eq!(MemoryUsage::size_of_val(&32), 4);
        assert_eq!(32.size_of_val(), 4);
    }

    #[test]
    fn test_arrays() {
        let x: [[u8; 7]; 13] = [[0; 7]; 13];
        assert_eq!(7 * 13, std::mem::size_of_val(&x));
        assert_eq!(7 * 13, MemoryUsage::size_of_val(&x));
    }

    #[test]
    fn test_slice_no_static_size() {
        {
            let x: [u8; 13] = [0; 13];
            let y: &[u8] = &x;
            assert_eq!(13, std::mem::size_of_val(y));
            assert_eq!(13, MemoryUsage::size_of_val(y));
        }

        {
            let mut x: [TestMemoryUsage; 13] = [TestMemoryUsage {
                size_to_report: std::mem::size_of::<TestMemoryUsage>(),
            }; 13];
            x[0].size_to_report += 7;
            let y: &[TestMemoryUsage] = &x;
            assert_eq!(
                13 * std::mem::size_of::<TestMemoryUsage>(),
                std::mem::size_of_val(y)
            );
            assert_eq!(
                13 * std::mem::size_of::<TestMemoryUsage>() + 7,
                MemoryUsage::size_of_val(y)
            );
        }
    }

    #[test]
    fn test_vecs() {
        let mut x = vec![];
        let empty_vec_size = std::mem::size_of_val(&x);
        let tmu_size = std::mem::size_of::<TestMemoryUsage>();
        x.push(TestMemoryUsage {
            size_to_report: tmu_size + 3,
        });
        x.push(TestMemoryUsage {
            size_to_report: tmu_size + 7,
        });
        assert_eq!(empty_vec_size, std::mem::size_of_val(&x));
        assert_eq!(
            empty_vec_size + 2 * tmu_size + 3 + 7,
            MemoryUsage::size_of_val(&x)
        );
    }
}
