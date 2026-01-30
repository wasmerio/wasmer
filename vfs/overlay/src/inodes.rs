use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

use vfs_core::BackendInodeId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackingKey {
    Upper { inode: BackendInodeId },
    Lower { layer: u16, inode: BackendInodeId },
}

impl Hash for BackingKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            BackingKey::Upper { inode } => {
                0u8.hash(state);
                inode.get().hash(state);
            }
            BackingKey::Lower { layer, inode } => {
                1u8.hash(state);
                layer.hash(state);
                inode.get().hash(state);
            }
        }
    }
}

#[derive(Debug)]
pub struct BackingState {
    pub primary: BackingKey,
}

#[derive(Debug)]
pub struct OverlayInodeTable {
    next: AtomicU64,
    key_to_overlay: RwLock<HashMap<BackingKey, BackendInodeId>>,
    overlay_to_backing: RwLock<HashMap<BackendInodeId, BackingState>>,
}

impl OverlayInodeTable {
    pub fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
            key_to_overlay: RwLock::new(HashMap::new()),
            overlay_to_backing: RwLock::new(HashMap::new()),
        }
    }

    pub fn overlay_id_for(&self, key: BackingKey) -> BackendInodeId {
        if let Ok(map) = self.key_to_overlay.read() {
            if let Some(id) = map.get(&key) {
                return *id;
            }
        }
        let mut map = self
            .key_to_overlay
            .write()
            .expect("overlay inode table poisoned");
        if let Some(id) = map.get(&key) {
            return *id;
        }
        let id = BackendInodeId::new(self.next.fetch_add(1, Ordering::Relaxed))
            .expect("overlay inode must be non-zero");
        map.insert(key, id);
        let mut overlay = self
            .overlay_to_backing
            .write()
            .expect("overlay inode table poisoned");
        overlay.insert(id, BackingState { primary: key });
        id
    }

    pub fn promote(&self, overlay_id: BackendInodeId, upper_key: BackingKey) {
        let mut map = self
            .key_to_overlay
            .write()
            .expect("overlay inode table poisoned");
        map.insert(upper_key, overlay_id);
        if let Ok(mut overlay) = self.overlay_to_backing.write() {
            if let Some(state) = overlay.get_mut(&overlay_id) {
                state.primary = upper_key;
            }
        }
    }
}
