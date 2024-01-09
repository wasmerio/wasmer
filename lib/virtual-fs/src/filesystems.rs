use crate::FileSystem;

/// A chain of one or more [`FileSystem`]s.
pub trait FileSystems<'a>: 'a {
    // FIXME(Michael-F-Bryan): Rewrite this to use GATs when we bump the MSRV to
    // 1.65 or higher. That'll get rid of all the lifetimes and HRTBs.
    type Iter: IntoIterator<Item = &'a (dyn FileSystem + Send)> + Send + 'a;

    /// Get something that can be used to iterate over the underlying
    /// filesystems.
    fn filesystems(&'a self) -> Self::Iter;
}

impl<'a, 'b, S> FileSystems<'a> for &'b S
where
    S: FileSystems<'a> + 'b,
    'b: 'a,
{
    type Iter = S::Iter;

    fn filesystems(&'a self) -> Self::Iter {
        (**self).filesystems()
    }
}

impl<'a, T> FileSystems<'a> for Vec<T>
where
    T: FileSystem + Send,
{
    type Iter = <[T] as FileSystems<'a>>::Iter;

    fn filesystems(&'a self) -> Self::Iter {
        self[..].filesystems()
    }
}

impl<'a, T, const N: usize> FileSystems<'a> for [T; N]
where
    T: FileSystem + Send,
{
    type Iter = [&'a (dyn FileSystem + Send); N];

    fn filesystems(&'a self) -> Self::Iter {
        // TODO: rewrite this when array::each_ref() is stable
        let mut i = 0;
        [(); N].map(|_| {
            let f = &self[i] as &(dyn FileSystem + Send);
            i += 1;
            f
        })
    }
}

impl<'a, T> FileSystems<'a> for [T]
where
    T: FileSystem + Send,
{
    type Iter = std::iter::Map<std::slice::Iter<'a, T>, fn(&T) -> &(dyn FileSystem + Send)>;

    fn filesystems(&'a self) -> Self::Iter {
        self.iter().map(|fs| fs as &(dyn FileSystem + Send))
    }
}

impl<'a> FileSystems<'a> for () {
    type Iter = std::iter::Empty<&'a (dyn FileSystem + Send)>;

    fn filesystems(&'a self) -> Self::Iter {
        std::iter::empty()
    }
}

macro_rules! count {
    ($first:tt $($rest:tt)*) => {
        1 + count!($($rest)*)
    };
    () => { 0 };
}

macro_rules! tuple_filesystems {
    ($first:ident $(, $rest:ident)* $(,)?) => {
        impl<'a, $first, $( $rest ),*> FileSystems<'a> for ($first, $($rest),*)
        where
            $first: FileSystem,
            $($rest: FileSystem),*
        {
            type Iter = [&'a (dyn FileSystem + Send); count!($first $($rest)*)];

            fn filesystems(&'a self) -> Self::Iter {
                #[allow(non_snake_case)]
                let ($first, $($rest),*) = self;

                [
                    $first as &(dyn FileSystem + Send),
                    $($rest),*
                ]
            }

        }

        tuple_filesystems!($($rest),*);
    };
    () => {};
}

tuple_filesystems!(F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, F13, F14, F15, F16,);
