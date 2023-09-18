use virtual_fs::{VirtualUnixSocket, VirtualUnixSocketConnection};

// TODO: socket options
#[derive(Debug)]
pub enum InodeUnixSocketKind {
    PreSocket {
        sock: Box<dyn VirtualUnixSocket + Sync>,
    },
    Listener {
        sock: Box<dyn VirtualUnixSocket + Sync>,
    },
    Connection {
        sock: Box<dyn VirtualUnixSocketConnection + Sync>,
    },
}

#[derive(Debug)]
pub(crate) struct InodeUnixSocketProtected {
    kind: InodeUnixSocketKind,
}

#[derive(Debug)]
//#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub(crate) struct InodeUnixSocketInner {
    pub protected: RwLock<InodeUnixSocketProtected>,
}

#[derive(Debug, Clone)]
//#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct InodeUnixSocket {
    pub(crate) inner: Arc<InodeUnixSocketInner>,
}

impl InodeUnixSocket {
    fn bind(
        &self,
        net: &dyn VirtualNetworking,
        state: &WasiState,
        inodes: &WasiInodes,
        addr: UnixSocketAddr,
    ) -> Result<Option<Self>, Errno> {
        let fs = state.fs;

        let mut addr = addr.0;
        if !addr.starts_with('/') {
            addr = fs.relative_path_to_absolute(addr);
        }

        let (parent_inode, path) =
            fs.get_parent_inode_at_path(inodes, VIRTUAL_ROOT_FD, Path::new(addr.as_str()), true)?;

        match *parent_inode.write() {
            fs::Kind::Dir {
                ref mut entries, ..
            } => {
                if entries.contains_key(&path) {
                    return Err(Errno::Addrinuse);
                }
            }
            _ => return Err(Errno::Addrnotavail),
        }

        ()
    }
}
