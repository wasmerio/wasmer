use crate::FileSystem;

/// A chain of one or more [`FileSystem`]s.
// FIXME(Michael-F-Bryan): We could remove this trait's HRTBs and lifetimes if
// we had access to GATs, but our MSRV is currently 1.64 and GATs require 1.65.
pub trait FileSystems<'a>: 'a {
    type Iter: IntoIterator<Item = &'a dyn FileSystem> + 'a;

    fn iter_filesystems(&'a self) -> Self::Iter;
}

impl<'a, S> FileSystems<'a> for &'a S
where
    S: FileSystems<'a> + 'a,
{
    type Iter = <S as FileSystems<'a>>::Iter;

    fn iter_filesystems(&'a self) -> Self::Iter {
        (**self).iter_filesystems()
    }
}

impl<'a, F> FileSystems<'a> for Vec<F>
where
    F: FileSystem + 'a,
{
    type Iter = std::iter::Map<std::slice::Iter<'a, F>, fn(&F) -> &dyn FileSystem>;

    fn iter_filesystems(&'a self) -> Self::Iter {
        fn downcast<T: FileSystem>(value: &T) -> &dyn FileSystem {
            value
        }
        self.iter().map(downcast)
    }
}

impl<'a, F, const N: usize> FileSystems<'a> for [F; N]
where
    F: FileSystem + 'a,
{
    type Iter = [&'a dyn FileSystem; N];

    fn iter_filesystems(&'a self) -> Self::Iter {
        // a poor man's version of the unstable array::each_ref()
        let mut i = 0;
        [(); N].map(|()| {
            let f = &self[i] as &dyn FileSystem;
            i += 1;
            f
        })
    }
}

impl<'a> FileSystems<'a> for () {
    type Iter = std::iter::Empty<&'a dyn FileSystem>;

    fn iter_filesystems(&'a self) -> Self::Iter {
        std::iter::empty()
    }
}

macro_rules! count {
    ($($t:ident),* $(,)?) => {
        0 $( + count!(@$t) )*
    };
    (@$t:ident) => { 1 };
}

macro_rules! tuple_filesystems {
    ($first:ident $(, $rest:ident)* $(,)?) => {
        impl<'a, $first, $( $rest ),*> FileSystems<'a> for ($first, $($rest),*)
        where
            $first: FileSystem + 'a,
            $($rest: FileSystem + 'a),*
        {
            type Iter = [ &'a dyn FileSystem; { count!($first, $($rest),*) }];

            fn iter_filesystems(&'a self) -> Self::Iter {
                #[allow(non_snake_case)]
                let ($first, $($rest),*) = &self;

                [
                    $first as &dyn FileSystem,
                    $(
                        $rest as &dyn FileSystem,
                    )*
                ]
            }
        }

        tuple_filesystems!($($rest),*);
    };
    () => {};
}

tuple_filesystems!(A, B, C, D, E, F, G, H, I, J, K);
