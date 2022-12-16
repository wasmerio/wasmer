use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::interface::{Export, Import};

/// A struct containing data for more efficient matching.
///
/// An ideal use case for this is to parse [`Interface`]s at compile time,
/// create [`InterfaceMatcher`]s, and store them as bytes so that they
/// can be efficiently loaded at runtime for matching.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct InterfaceMatcher {
    pub namespaces: HashSet<String>,
    pub namespace_imports: HashMap<String, HashSet<Import>>,
    pub exports: HashSet<Export>,
}

#[cfg(feature = "binary_encode")]
impl InterfaceMatcher {
    /// Store the matcher as bytes to avoid reparsing
    fn into_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("Could not serialize InterfaceMatcher")
    }

    /// Load the matcher from bytes to avoid reparsing
    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        bincode::deserialize(bytes).ok()
    }
}
