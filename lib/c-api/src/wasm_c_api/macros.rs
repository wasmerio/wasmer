macro_rules! wasm_declare_vec_inner {
    (
        name: $name:ident,
        ty: $elem_ty:ty,
        c_ty: $c_ty:expr,
        c_val: $c_val:expr,
        new: $new:ident,
        empty: $empty:ident,
        uninit: $uninit:ident,
        copy: $copy:ident,
        delete: $delete:ident,
    ) => {
        #[doc = concat!("Represents a vector of `", $c_ty, "`.

Read the documentation of [`", $c_ty, "`] to see more concrete examples.

# Example

```rust
# use wasmer_inline_c::assert_c;
# fn main() {
#    (assert_c! {
# #include \"tests/wasmer.h\"
#
void example(", $c_ty, " x, ", $c_ty, " y) {
    // Create a vector of 2 `", $c_ty, "`.
    ", $c_ty, " items[2] = {x, y};

    ", stringify!($name), " vector;
    ", stringify!($new), "(&vector, 2, items);

    // Check that it contains 2 items.
    assert(vector.size == 2);

    // Free it.
    ", stringify!($delete), "(&vector);
}
#
# int main() { example(", $c_val, ", ", $c_val, "); return 0; }
#    })
#    .success();
# }
```")]
        #[repr(C)]
        pub struct $name {
            pub size: usize,
            pub data: *mut $elem_ty,
        }

        impl $name {
            // Note that this does not free any existing buffer.
            pub fn set_buffer(&mut self, buffer: Vec<$elem_ty>) {
                let mut vec = buffer.into_boxed_slice();
                self.size = vec.len();
                self.data = vec.as_mut_ptr();
                std::mem::forget(vec);
            }

            pub fn as_slice(&self) -> &[$elem_ty] {
                // Note that we're careful to not create a slice with a null
                // pointer as the data pointer, since that isn't defined
                // behavior in Rust.
                if self.size == 0 {
                    &[]
                } else {
                    assert!(!self.data.is_null());
                    unsafe { std::slice::from_raw_parts(self.data, self.size) }
                }
            }

            pub fn as_uninit_slice(&mut self) -> &mut [std::mem::MaybeUninit<$elem_ty>] {
                // Note that we're careful to not create a slice with a null
                // pointer as the data pointer, since that isn't defined
                // behavior in Rust.
                if self.size == 0 {
                    &mut []
                } else {
                    assert!(!self.data.is_null());
                    unsafe { std::slice::from_raw_parts_mut(self.data as _, self.size) }
                }
            }

            pub fn take(&mut self) -> Vec<$elem_ty> {
                if self.data.is_null() {
                    return Vec::new();
                }
                let vec = unsafe { Vec::from_raw_parts(self.data, self.size, self.size) };
                self.data = std::ptr::null_mut();
                self.size = 0;
                return vec;
            }
        }

        impl From<Vec<$elem_ty>> for $name {
            fn from(vec: Vec<$elem_ty>) -> Self {
                let mut vec = vec.into_boxed_slice();
                let result = $name {
                    size: vec.len(),
                    data: vec.as_mut_ptr(),
                };
                std::mem::forget(vec);
                result
            }
        }

        impl Clone for $name {
            fn clone(&self) -> Self {
                self.as_slice().to_vec().into()
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                drop(self.take());
            }
        }

        #[doc = concat!("Creates an empty vector of [`", $c_ty, "`].

# Example

```rust
# use wasmer_inline_c::assert_c;
# fn main() {
#    (assert_c! {
# #include \"tests/wasmer.h\"
#
int main() {
    // Creates an empty vector of `", $c_ty, "`.
    ", stringify!($name), " vector;
    ", stringify!($empty), "(&vector);

    // Check that it is empty.
    assert(vector.size == 0);

    // Free it.
    ", stringify!($delete), "(&vector);

    return 0;
}
#    })
#    .success();
# }
```")]
        #[no_mangle]
        pub extern "C" fn $empty(out: &mut $name) {
            out.size = 0;
            out.data = std::ptr::null_mut();
        }

        #[doc = concat!("Creates a new uninitialized vector of [`", $c_ty, "`].

# Example

```rust
# use wasmer_inline_c::assert_c;
# fn main() {
#    (assert_c! {
# #include \"tests/wasmer.h\"
#
int main() {
    // Creates an empty vector of `", $c_ty, "`.
    ", stringify!($name), " vector;
    ", stringify!($uninit), "(&vector, 3);

    // Check that it contains 3 items.
    assert(vector.size == 3);

    // Free it.
    ", stringify!($delete), "(&vector);

    return 0;
}
#    })
#    .success();
# }
```")]
        #[no_mangle]
        pub extern "C" fn $uninit(out: &mut $name, size: usize) {
            out.set_buffer(vec![Default::default(); size]);
        }

        #[doc = concat!("Creates a new vector of [`", $c_ty, "`].

# Example

See the [`", stringify!($name), "`] type to get an example.")]
        #[no_mangle]
        pub unsafe extern "C" fn $new(out: &mut $name, size: usize, ptr: *const $elem_ty) {
            let vec = (0..size).map(|i| ptr.add(i).read()).collect();
            out.set_buffer(vec);
        }

        #[doc = concat!("Performs a deep copy of a vector of [`", $c_ty, "`].")]
        #[no_mangle]
        pub extern "C" fn $copy(out: &mut $name, src: &$name) {
            out.set_buffer(src.as_slice().to_vec());
        }

        #[doc = concat!("Deletes a vector of [`", $c_ty, "`].

# Example

See the [`", stringify!($name), "`] type to get an example.")]
        #[no_mangle]
        pub extern "C" fn $delete(out: &mut $name) {
            out.take();
        }
    };
}

macro_rules! wasm_declare_vec {
    ($name:ident) => {
        wasm_declare_vec!($name, wasm);
    };

    ($name:ident, $prefix:ident) => {
        paste::paste! {
            wasm_declare_vec_inner!(
                name: [<$prefix _ $name _vec_t>],
                ty: [<$prefix _ $name _t>],
                c_ty: stringify!([<$prefix _ $name _t>]),
                c_val: concat!("({ ",
                    stringify!([<$prefix _ $name _t>]), " foo;\n",
                    "memset(&foo, 0, sizeof(foo));\n",
                    "foo;\n",
                "})"),
                new: [<$prefix _ $name _vec_new>],
                empty: [<$prefix _ $name _vec_new_empty>],
                uninit: [<$prefix _ $name _vec_new_uninitialized>],
                copy: [<$prefix _ $name _vec_copy>],
                delete: [<$prefix _ $name _vec_delete>],
            );
        }
    };
}

macro_rules! wasm_declare_boxed_vec {
    ($name:ident) => {
        wasm_declare_boxed_vec!($name, wasm);
    };

    ($name:ident, $prefix:ident) => {
        paste::paste! {
            wasm_declare_vec_inner!(
                name: [<$prefix _ $name _vec_t>],
                ty: Option<Box<[<$prefix _ $name _t>]>>,
                c_ty: stringify!([<$prefix _ $name _t>] *),
                c_val: "NULL",
                new: [<$prefix _ $name _vec_new>],
                empty: [<$prefix _ $name _vec_new_empty>],
                uninit: [<$prefix _ $name _vec_new_uninitialized>],
                copy: [<$prefix _ $name _vec_copy>],
                delete: [<$prefix _ $name _vec_delete>],
            );
        }
    };
}

macro_rules! wasm_impl_copy {
    ($name:ident) => {
        wasm_impl_copy!($name, wasm);
    };

    ($name:ident, $prefix:ident) => {
        paste::paste! {
            #[no_mangle]
            pub extern "C" fn [<$prefix _ $name _copy>](src: Option<&[<$prefix _ $name _t>]>) -> Option<Box<[<$prefix _ $name _t>]>> {
                Some(Box::new(src?.clone()))
            }
        }
    };
}

macro_rules! wasm_impl_delete {
    ($name:ident) => {
        wasm_impl_delete!($name, wasm);
    };

    ($name:ident, $prefix:ident) => {
        paste::paste! {
            #[no_mangle]
            pub extern "C" fn [<$prefix _ $name _delete>](_: Option<Box<[<$prefix _ $name _t>]>>) {}
        }
    };
}

macro_rules! wasm_impl_copy_delete {
    ($name:ident) => {
        wasm_impl_copy_delete!($name, wasm);
    };

    ($name:ident, $prefix:ident) => {
        wasm_impl_copy!($name, $prefix);
        wasm_impl_delete!($name, $prefix);
    };
}

macro_rules! c_try {
    ($expr:expr; otherwise ()) => {{
        let res: Result<_, _> = $expr;
        match res {
            Ok(val) => val,
            Err(err) => {
                crate::error::update_last_error(err);
                return;
            }
        }
    }};
    ($expr:expr; otherwise $return:expr) => {{
        let res: Result<_, _> = $expr;
        match res {
            Ok(val) => val,
            Err(err) => {
                crate::error::update_last_error(err);
                return $return;
            }
        }
    }};
    ($expr:expr) => {{
        c_try!($expr; otherwise None)
    }};
    ($expr:expr, $e:expr) => {{
        let opt: Option<_> = $expr;
        c_try!(opt.ok_or_else(|| $e))
    }};
}
