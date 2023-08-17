/// Assert that a type does **not** implement a set of traits.
macro_rules! assert_not_implemented {
    ($t:ty : !$first:ident $(+ !$rest:ident)*) => {
        const _: fn() = || {
            // Generic trait with a blanket impl over `()` for all types.
            trait AmbiguousIfImpl<A> {
                // Required for actually being able to reference the trait.
                fn some_item() {}
            }

            impl<T: ?Sized> AmbiguousIfImpl<()> for T {}

            // Creates multiple scoped `Invalid` types for each trait,
            // over which a specialized `AmbiguousIfImpl<Invalid>` is
            // implemented for every type that implements the trait.
            #[allow(dead_code)]
            struct InvalidFirst;
            impl<T: ?Sized + $first> AmbiguousIfImpl<InvalidFirst> for T {}

            $({
                #[allow(dead_code)]
                struct Invalid;
            impl<T: ?Sized + $rest> AmbiguousIfImpl<Invalid> for T {}
            })*

            // If there is only one specialized trait impl, type inference
            // with `_` can be resolved and this can compile. Fails to
            // compile if `$t` implements any `AmbiguousIfImpl<Invalid>`.
            let _ = <$t as AmbiguousIfImpl<_>>::some_item;
        };
    }
}
