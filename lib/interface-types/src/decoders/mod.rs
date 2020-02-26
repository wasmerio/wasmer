//! Reads the AST from a particular data representation; for instance,
//! [`decoders::binary`](binary) reads the [AST](crate::ast)
//! from a binary.

pub mod binary;
pub mod wat;
