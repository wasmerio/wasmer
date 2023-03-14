use crate::FileSystem;
use std::ops::ControlFlow;

/// A chain of one or more [`FileSystem`]s.
pub trait FileSystems {
    // FIXME(Michael-F-Bryan): Rewrite this to use GATs and an external iterator
    // when we bump the MSRV to 1.65 or higher.
    fn for_each_filesystems<F, Ret>(&self, func: F) -> Option<Ret>
    where
        F: FnMut(&dyn FileSystem) -> ControlFlow<Ret>;
}

impl<'b, S> FileSystems for &'b S
where
    S: FileSystems + 'b,
{
    fn for_each_filesystems<F, Ret>(&self, func: F) -> Option<Ret>
    where
        F: FnMut(&dyn FileSystem) -> ControlFlow<Ret>,
    {
        (**self).for_each_filesystems(func)
    }
}

impl<T> FileSystems for Vec<T>
where
    T: FileSystem,
{
    fn for_each_filesystems<F, Ret>(&self, func: F) -> Option<Ret>
    where
        F: FnMut(&dyn FileSystem) -> ControlFlow<Ret>,
    {
        self[..].for_each_filesystems(func)
    }
}

impl<T, const N: usize> FileSystems for [T; N]
where
    T: FileSystem,
{
    fn for_each_filesystems<F, Ret>(&self, func: F) -> Option<Ret>
    where
        F: FnMut(&dyn FileSystem) -> ControlFlow<Ret>,
    {
        self[..].for_each_filesystems(func)
    }
}

impl<T> FileSystems for [T]
where
    T: FileSystem,
{
    fn for_each_filesystems<F, Ret>(&self, mut func: F) -> Option<Ret>
    where
        F: FnMut(&dyn FileSystem) -> ControlFlow<Ret>,
    {
        for fs in self.iter() {
            match func(fs) {
                ControlFlow::Continue(_) => continue,
                ControlFlow::Break(result) => return Some(result),
            }
        }

        None
    }
}

impl FileSystems for () {
    fn for_each_filesystems<F, Ret>(&self, _func: F) -> Option<Ret>
    where
        F: FnMut(&dyn FileSystem) -> ControlFlow<Ret>,
    {
        None
    }
}

macro_rules! tuple_filesystems {
    ($first:ident $(, $rest:ident)* $(,)?) => {
        impl<$first, $( $rest ),*> FileSystems for ($first, $($rest),*)
        where
            $first: FileSystem,
            $($rest: FileSystem),*
        {
            fn for_each_filesystems<F, Ret>(&self, mut func: F) -> Option<Ret>
            where
                F: FnMut(&dyn FileSystem) -> ControlFlow<Ret>,
            {
                #[allow(non_snake_case)]
                let ($first, $($rest),*) = &self;

                if let ControlFlow::Break(result) = func($first) {
                    return Some(result);
                }

                $(
                    if let ControlFlow::Break(result) = func($rest) {
                        return Some(result);
                    }
                )*

                None
            }
        }

        tuple_filesystems!($($rest),*);
    };
    () => {};
}

tuple_filesystems!(F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, F13, F14, F15, F16,);
