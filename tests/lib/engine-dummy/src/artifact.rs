//! Define `NativeArtifact` to allow compiling and instantiating to be
//! done as separate steps.

use crate::engine::DummyEngine;
use std::any::Any;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use wasm_common::entity::{BoxedSlice, EntityRef, PrimaryMap};
use wasm_common::{
    DataInitializer, FunctionIndex, LocalFunctionIndex, MemoryIndex, OwnedDataInitializer,
    SignatureIndex, TableIndex,
};
use wasmer_compiler::CompileError;
use wasmer_engine::{
    resolve_imports, Artifact, DeserializeError, Engine, InstantiationError, Resolver,
    RuntimeError, SerializeError,
};
use wasmer_runtime::{
    InstanceHandle, ModuleInfo, SignatureRegistry, VMFunctionBody, VMSharedSignatureIndex,
    VMTrampoline,
};
use wasmer_runtime::{MemoryPlan, TablePlan};

/// A dummy artifact.
pub struct DummyArtifact {
    module: Arc<ModuleInfo>,
    finished_functions: BoxedSlice<LocalFunctionIndex, *mut [VMFunctionBody]>,
    finished_dynamic_function_trampolines: BoxedSlice<FunctionIndex, *const VMFunctionBody>,
    signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
}

impl DummyArtifact {}

impl Artifact for DummyArtifact {
    fn module(&self) -> &ModuleInfo {
        &self.module
    }

    fn module_mut(&mut self) -> &mut ModuleInfo {
        Arc::get_mut(&mut self.module).unwrap()
    }
}
