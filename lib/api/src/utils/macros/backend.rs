/// Automatically generate "backend" types.
#[macro_use]
macro_rules! gen_rt_ty {
    // In this case we automatically try to create the struct following the canonical path to the
    // same entity in each backend.
    ($id:ident) => {
        paste::paste! {
            pub(crate) enum [<Backend $id>] {
                #[cfg(feature = "sys")]
                /// The implementation from the `sys` backend.
                Sys(crate::backend::sys::entities::[<$id:lower>]::$id),

                #[cfg(feature = "v8")]
                /// The implementation from the `v8` backend.
                V8(crate::backend::v8::entities::[<$id:lower>]::$id),

                #[cfg(feature = "wamr")]
                /// The implementation from the `wamr` backend.
                Wamr(crate::backend::wamr::entities::[<$id:lower>]::$id),

                #[cfg(feature = "wasmi")]
                /// The implementation from the `wasmi` backend.
                Wasmi(crate::backend::wasmi::entities::[<$id:lower>]::$id),

                #[cfg(feature = "js")]
                /// The implementation from the `js` backend.
                Js(crate::backend::js::entities::[<$id:lower>]::$id),

                #[cfg(feature = "jsc")]
                /// The implementation from the `jsc` backend.
                Jsc(crate::backend::jsc::entities::[<$id:lower>]::$id),
            }
        }
    };

    ($id:ident$(<$lt:lifetime>)? $(@cfg $($if:meta => $then: meta),*)? @derives $($derive:path),*) => {
        paste::paste! {
            $($(#[cfg_attr($if, $then)])*)?
            #[derive($($derive,)*)]
            pub(crate) enum [<Backend $id>]$(<$lt>)? {
                #[cfg(feature = "sys")]
                /// The implementation from the `sys` backend.
                Sys(crate::backend::sys::entities::[<$id:lower>]::$id$(<$lt>)?),

                #[cfg(feature = "v8")]
                /// The implementation from the `v8` backend.
                V8(crate::backend::v8::entities::[<$id:lower>]::$id$(<$lt>)?),

                #[cfg(feature = "wamr")]
                /// The implementation from the `wamr` backend.
                Wamr(crate::backend::wamr::entities::[<$id:lower>]::$id$(<$lt>)?),

                #[cfg(feature = "wasmi")]
                /// The implementation from the `wasmi` backend.
                Wasmi(crate::backend::wasmi::entities::[<$id:lower>]::$id$(<$lt>)?),

                #[cfg(feature = "js")]
                /// The implementation from the `js` backend.
                Js(crate::backend::js::entities::[<$id:lower>]::$id$(<$lt>)?),


                #[cfg(feature = "jsc")]
                /// The implementation from the `jsc` backend.
                Jsc(crate::backend::jsc::entities::[<$id:lower>]::$id$(<$lt>)?),
            }
        }
    };

    ($id:ident$(<$lt:lifetime>)? $(@cfg $($if:meta => $then: meta),*)? @derives $($derive:path),* ; @path $path:path ) => {
        paste::paste! {
            $($(#[cfg_attr($if, $then)])*)?
            #[derive($($derive,)*)]
            pub(crate) enum [<Backend $id>]$(<$lt>)? {
                #[cfg(feature = "sys")]
                /// The implementation from the `sys` backend.
                Sys(crate::backend::sys::entities::$path::$id$(<$lt>)?),

                #[cfg(feature = "v8")]
                /// The implementation from the `v8` backend.
                V8(crate::backend::v8::entities::$path::$id$(<$lt>)?),

                #[cfg(feature = "wamr")]
                /// The implementation from the `wamr` backend.
                Wamr(crate::backend::wamr::entities::$path::$id$(<$lt>)?),

                #[cfg(feature = "wasmi")]
                /// The implementation from the `wasmi` backend.
                Wasmi(crate::backend::wasmi::entities::$path::$id$(<$lt>)?),

                #[cfg(feature = "js")]
                /// The implementation from the `js` backend.
                Js(crate::backend::js::entities::$path::$id$(<$lt>)?),


                #[cfg(feature = "jsc")]
                /// The implementation from the `jsc` backend.
                Jsc(crate::backend::jsc::entities::$path::$id$(<$lt>)?),
            }
        }
    };


    ($id:ident @derives $($derive:path),* ; @path $path:path) => {
        paste::paste! {
            #[derive($($derive,)*)]
            pub(crate) enum [<Backend $id>] {
                #[cfg(feature = "sys")]
                /// The implementation from the `sys` backend.
                Sys(crate::backend::sys::entities::$path::$id),

                #[cfg(feature = "v8")]
                /// The implementation from the `v8` backend.
                V8(crate::backend::v8::entities::$path::$id),

                #[cfg(feature = "wamr")]
                /// The implementation from the `wamr` backend.
                Wamr(crate::backend::wamr::entities::$path::$id),

                #[cfg(feature = "wasmi")]
                /// The implementation from the `wasmi` backend.
                Wasmi(crate::backend::wasmi::entities::$path::$id),

                #[cfg(feature = "js")]
                /// The implementation from the `js` backend.
                Js(crate::backend::js::entities::$path::$id),


                #[cfg(feature = "jsc")]
                /// The implementation from the `jsc` backend.
                Jsc(crate::backend::jsc::entities::$path::$id),
            }
        }
    };
}

/// Automatically create a match statement, repeating the expression for each backend.
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
