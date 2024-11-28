#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(unused)]

include!(concat!(env!("OUT_DIR"), "/wasmi_bindings.rs"));

#[allow(unused_imports)]
// This is here to force its linking.
use wasmi_c_api::*;
