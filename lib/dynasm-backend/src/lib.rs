#![feature(proc_macro_hygiene)]

#[macro_use]
extern crate dynasmrt;

#[macro_use]
extern crate dynasm;

mod codegen;
mod codegen_x64;
mod parse;
