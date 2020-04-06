//! Serde is not necessary to use WIT. It only provides a nicer API
//! for the end-user to send or receive its complex types to/from WIT
//! values, like `record` for instance.

pub(crate) mod de;
pub(crate) mod ser;
