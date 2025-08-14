/// Automatically generate "backend" types.
#[macro_use]
macro_rules! gen_rt_ty {
    {
        $(#[$meta:meta])*
        $vis:vis
        $id:ident$(<
            $($lt:lifetime),*
            $(,)?
            $($param:ident),*
        >)?(
            $path:path
        ) $(;)?
    } => {
        paste::paste! {
            $(#[$meta])*
            $vis enum $id $(<$($lt,)* $($param,)*>)? {
                #[cfg(feature = "sys")]
                /// The implementation from the `sys` backend.
                Sys(crate::backend::sys::entities::$path),

                #[cfg(feature = "v8")]
                /// The implementation from the `v8` backend.
                V8(crate::backend::v8::entities::$path),

                #[cfg(feature = "wamr")]
                /// The implementation from the `wamr` backend.
                Wamr(crate::backend::wamr::entities::$path),

                #[cfg(feature = "wasmi")]
                /// The implementation from the `wasmi` backend.
                Wasmi(crate::backend::wasmi::entities::$path),

                #[cfg(feature = "js")]
                /// The implementation from the `js` backend.
                Js(crate::backend::js::entities::$path),

                #[cfg(feature = "jsc")]
                /// The implementation from the `jsc` backend.
                Jsc(crate::backend::jsc::entities::$path),
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
