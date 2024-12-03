/// Automatically generate "runtime" types.
#[macro_use]
macro_rules! gen_rt_ty {
    // In this case we automatically try to create the struct following the canonical path to the
    // same entity in each runtime.
    ($id:ident) => {
        paste::paste! {
            pub(crate) enum [<Runtime $id>] {
                #[cfg(feature = "sys")]
                /// The implementation from the `sys` runtime.
                Sys(crate::rt::sys::entities::[<$id:lower>]::$id),

                #[cfg(feature = "v8")]
                /// The implementation from the `v8` runtime.
                V8(crate::rt::v8::entities::[<$id:lower>]::$id),

                #[cfg(feature = "wamr")]
                /// The implementation from the `wamr` runtime.
                Wamr(crate::rt::wamr::entities::[<$id:lower>]::$id),

                #[cfg(feature = "wasmi")]
                /// The implementation from the `wasmi` runtime.
                Wasmi(crate::rt::wasmi::entities::[<$id:lower>]::$id),

                #[cfg(feature = "js")]
                /// The implementation from the `js` runtime.
                Js(crate::rt::js::entities::[<$id:lower>]::$id),

                #[cfg(feature = "jsc")]
                /// The implementation from the `jsc` runtime.
                Jsc(crate::rt::jsc::entities::[<$id:lower>]::$id),
            }
        }
    };

    ($id:ident$(<$lt:lifetime>)? $(@cfg $($if:meta => $then: meta),*)? @derives $($derive:path),*) => {
        paste::paste! {
            $($(#[cfg_attr($if, $then)])*)?
            #[derive($($derive,)*)]
            pub(crate) enum [<Runtime $id>]$(<$lt>)? {
                #[cfg(feature = "sys")]
                /// The implementation from the `sys` runtime.
                Sys(crate::rt::sys::entities::[<$id:lower>]::$id$(<$lt>)?),

                #[cfg(feature = "v8")]
                /// The implementation from the `v8` runtime.
                V8(crate::rt::v8::entities::[<$id:lower>]::$id$(<$lt>)?),

                #[cfg(feature = "wamr")]
                /// The implementation from the `wamr` runtime.
                Wamr(crate::rt::wamr::entities::[<$id:lower>]::$id$(<$lt>)?),

                #[cfg(feature = "wasmi")]
                /// The implementation from the `wasmi` runtime.
                Wasmi(crate::rt::wasmi::entities::[<$id:lower>]::$id$(<$lt>)?),

                #[cfg(feature = "js")]
                /// The implementation from the `js` runtime.
                Js(crate::rt::js::entities::[<$id:lower>]::$id$(<$lt>)?),


                #[cfg(feature = "jsc")]
                /// The implementation from the `jsc` runtime.
                Jsc(crate::rt::jsc::entities::[<$id:lower>]::$id$(<$lt>)?),
            }
        }
    };

    ($id:ident$(<$lt:lifetime>)? $(@cfg $($if:meta => $then: meta),*)? @derives $($derive:path),* ; @path $path:path ) => {
        paste::paste! {
            $($(#[cfg_attr($if, $then)])*)?
            #[derive($($derive,)*)]
            pub(crate) enum [<Runtime $id>]$(<$lt>)? {
                #[cfg(feature = "sys")]
                /// The implementation from the `sys` runtime.
                Sys(crate::rt::sys::entities::$path::$id$(<$lt>)?),

                #[cfg(feature = "v8")]
                /// The implementation from the `v8` runtime.
                V8(crate::rt::v8::entities::$path::$id$(<$lt>)?),

                #[cfg(feature = "wamr")]
                /// The implementation from the `wamr` runtime.
                Wamr(crate::rt::wamr::entities::$path::$id$(<$lt>)?),

                #[cfg(feature = "wasmi")]
                /// The implementation from the `wasmi` runtime.
                Wasmi(crate::rt::wasmi::entities::$path::$id$(<$lt>)?),

                #[cfg(feature = "js")]
                /// The implementation from the `js` runtime.
                Js(crate::rt::js::entities::$path::$id$(<$lt>)?),


                #[cfg(feature = "jsc")]
                /// The implementation from the `jsc` runtime.
                Jsc(crate::rt::jsc::entities::$path::$id$(<$lt>)?),
            }
        }
    };


    ($id:ident @derives $($derive:path),* ; @path $path:path) => {
        paste::paste! {
            #[derive($($derive,)*)]
            pub(crate) enum [<Runtime $id>] {
                #[cfg(feature = "sys")]
                /// The implementation from the `sys` runtime.
                Sys(crate::rt::sys::entities::$path::$id),

                #[cfg(feature = "v8")]
                /// The implementation from the `v8` runtime.
                V8(crate::rt::v8::entities::$path::$id),

                #[cfg(feature = "wamr")]
                /// The implementation from the `wamr` runtime.
                Wamr(crate::rt::wamr::entities::$path::$id),

                #[cfg(feature = "wasmi")]
                /// The implementation from the `wasmi` runtime.
                Wasmi(crate::rt::wasmi::entities::$path::$id),

                #[cfg(feature = "js")]
                /// The implementation from the `js` runtime.
                Js(crate::rt::js::entities::$path::$id),


                #[cfg(feature = "jsc")]
                /// The implementation from the `jsc` runtime.
                Jsc(crate::rt::jsc::entities::$path::$id),
            }
        }
    };
}

/// Automatically create a match statement, repeating the expression for each runtime.
#[macro_use]
macro_rules! match_rt {
    (on $self:expr => $var:ident { $stmt:expr }) => {
        match $self {
            #[cfg(feature = "sys")]
            Self::Sys($var) => $stmt,
            #[cfg(feature = "wamr")]
            Self::Wamr($var) => $stmt,
            #[cfg(feature = "wasmi")]
            Self::Wasmi($var) => $stmt,
            #[cfg(feature = "v8")]
            Self::V8($var) => $stmt,
            #[cfg(feature = "js")]
            Self::Js($var) => $stmt,
            #[cfg(feature = "jsc")]
            Self::Jsc($var) => $stmt,
        }
    };

    (on $value:expr ; $match:expr => $var:ident { $stmt:expr }) => {
        match $self {
            #[cfg(feature = "sys")]
            Self::Sys($var) => $stmt,
            #[cfg(feature = "wamr")]
            Self::Wamr($var) => $stmt,
            #[cfg(feature = "wasmi")]
            Self::Wasmi($var) => $stmt,
            #[cfg(feature = "v8")]
            Self::V8($var) => $stmt,
            #[cfg(feature = "js")]
            Self::Js($var) => $stmt,
            #[cfg(feature = "jsc")]
            Self::Jsc($var) => $stmt,
        }
    };
}

pub(crate) use {gen_rt_ty, match_rt};
