//! Operation-scoped descriptor identity and opaque per-slot metadata. Providers interpret
//! neither attachment policy nor guest ABIs; metadata is published with the descriptor.

use std::{
    any::Any,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{Errno, Fd};

/// Host-owned, immutable metadata for one descriptor slot. Copies share the value; an update
/// replaces the slot's attachment rather than mutating the payload or other copies.
#[derive(Clone)]
pub struct DescriptorAttachment(Arc<dyn Any + Send + Sync>);

impl DescriptorAttachment {
    pub fn new<T: Any + Send + Sync>(value: T) -> Self {
        Self(Arc::new(value))
    }

    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// A captured slot, including a captured absence. An absent entry in DescriptorSet instead
/// means the operation never acquired that descriptor and must not fall through to a lookup.
#[derive(Clone)]
pub struct Descriptor {
    pub fd: Fd,
    pub present: bool,
    pub attachment: Option<DescriptorAttachment>,
}

#[derive(Clone)]
pub struct DescriptorSet(Arc<Set>);

struct Set {
    payload: Box<dyn Any + Send + Sync>,
    descriptors: Vec<Descriptor>,
}

impl DescriptorSet {
    /// Providers keep actual files and unforgeable identity in their private payload. Retain
    /// only a weak table identity: keeping a table alive changes copy/unshare semantics.
    pub fn new<T: Any + Send + Sync>(payload: T, descriptors: Vec<Descriptor>) -> Self {
        Self(Arc::new(Set {
            payload: Box::new(payload),
            descriptors,
        }))
    }

    pub fn payload<T: Any>(&self) -> Option<&T> {
        self.0.payload.downcast_ref()
    }

    pub fn get(&self, fd: Fd) -> Option<&Descriptor> {
        self.0.descriptors.iter().find(|entry| entry.fd == fd)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DescriptorUpdate {
    Replaced,
    /// Same slot, but another update replaced the attachment. Acquire/check a fresh set.
    Conflict,
    /// The task changed tables, or the captured slot was removed/replaced.
    Stale,
}

/// Attachments and exact output identities of one descriptor-producing operation. A provider
/// claims the plan before side effects and publishes each result under its installation lock.
/// Subsequent shim helpers can use that result without looking its number up again.
#[derive(Clone)]
pub struct DescriptorInstallations(Arc<Installations>);

struct Installations {
    attachments: Vec<Option<DescriptorAttachment>>,
    results: Vec<OnceLock<DescriptorSet>>,
    claimed: AtomicBool,
}

impl DescriptorInstallations {
    pub fn new(attachments: Vec<Option<DescriptorAttachment>>) -> Self {
        let results = (0..attachments.len()).map(|_| OnceLock::new()).collect();
        Self(Arc::new(Installations {
            attachments,
            results,
            claimed: AtomicBool::new(false),
        }))
    }

    pub fn claim(&self, count: usize) -> Result<(), Errno> {
        self.validate(count)?;
        if self.0.claimed.swap(true, Ordering::AcqRel) {
            return Err(Errno::EINVAL);
        }
        Ok(())
    }

    pub fn validate(&self, count: usize) -> Result<(), Errno> {
        if self.0.attachments.len() != count || self.0.claimed.load(Ordering::Acquire) {
            return Err(Errno::EINVAL);
        }
        Ok(())
    }

    pub fn attachment(&self, index: usize) -> Option<DescriptorAttachment> {
        self.0.attachments[index].clone()
    }

    pub fn publish(&self, index: usize, descriptors: DescriptorSet) {
        assert!(
            self.0.results[index].set(descriptors).is_ok(),
            "descriptor result published once"
        );
    }

    pub fn result(&self, index: usize) -> Option<&DescriptorSet> {
        self.0.results.get(index)?.get()
    }
}
