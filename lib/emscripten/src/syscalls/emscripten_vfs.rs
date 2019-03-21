use crate::syscalls::emscripten_vfs::FileHandle::{Socket, VirtualFile};
use crate::varargs::VarArgs;
use std::cmp::{Eq, Ord, Ordering, PartialEq};
use std::collections::BTreeMap;
use std::env::home_dir;
use std::fmt::Display;
use wasmer_runtime_abi::vfs::device_file;
use wasmer_runtime_abi::vfs::vfs::Vfs;

pub type Fd = i32;

#[derive(Clone, Debug)]
pub struct VirtualFd(pub Fd);

impl Ord for VirtualFd {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl PartialOrd for VirtualFd {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.0.cmp(&other.0))
    }
}

impl PartialEq for VirtualFd {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for VirtualFd {}

impl Display for VirtualFd {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        write!(f, "Fd({})", self.0)
    }
}

pub enum FileHandle {
    VirtualFile(Fd),
    Socket(Fd),
}

pub type Map<K, V> = BTreeMap<K, V>;
pub type FdMap = Map<VirtualFd, FileHandle>;

pub struct EmscriptenVfs {
    pub fd_map: FdMap,
    pub vfs: Vfs,
}

impl EmscriptenVfs {
    pub fn new(vfs: Vfs) -> Self {
        let mut fd_map = FdMap::new();

        vfs.fd_map
            .keys()
            .map(|handle| (handle, handle))
            .for_each(|(virtual_handle, handle)| {
                let vfd = VirtualFd(*virtual_handle);
                fd_map.insert(vfd, FileHandle::VirtualFile(*handle));
            });

//        let _ = repo.create_dir(PathBuf::from("/dev/"));
//        let stdin = repo.create_file(PathBuf::from("/dev/stdin"))?;
//        let stdout = repo.create_file(PathBuf::from("/dev/stdout"))?;
//        let stderr = repo.create_file(PathBuf::from("/dev/stderr"))?;

        let stdin_fd = VirtualFd(0);
        let stdin_handle = FileHandle::VirtualFile(0);
        let stdout_fd = VirtualFd(1);
        let stdout_handle = FileHandle::VirtualFile(1);
        let stderr_fd = VirtualFd(2);
        let stderr_handle = FileHandle::VirtualFile(2);

        fd_map.insert(stdin_fd, stdin_handle);
        fd_map.insert(stdout_fd, stdout_handle);
        fd_map.insert(stderr_fd, stderr_handle);

        EmscriptenVfs { fd_map, vfs }
    }

    pub fn close(&mut self, vfd: &VirtualFd) -> () {
        match self.fd_map.get(&vfd) {
            Some(FileHandle::VirtualFile(handle)) => {
                self.vfs.close(handle);
            },
            Some(FileHandle::Socket(fd)) => unsafe {
                libc::close(*fd);
            },
            None => panic!(),
        }
        self.fd_map.remove(&vfd);
    }

    pub fn next_lowest_fd(&self) -> VirtualFd {
        next_lowest(&self.fd_map)
    }

    pub fn get_host_socket_fd(&self, vfd: &VirtualFd) -> Option<Fd> {
        match self.fd_map.get(&vfd) {
            Some(FileHandle::Socket(fd)) => Some(*fd),
            _ => None,
        }
    }

    pub fn get_virtual_file_handle(&self, vfd: VirtualFd) -> Option<Fd> {
        match self.fd_map.get(&vfd) {
            Some(FileHandle::VirtualFile(fd)) => Some(*fd),
            _ => None,
        }
    }

    pub fn open_file<P: AsRef<std::path::Path>>(&mut self, path: P) -> VirtualFd {
        let fd = self.vfs.open_file(path).unwrap();
        let vfd = VirtualFd(fd);
        let file = FileHandle::VirtualFile(fd);
        self.fd_map.insert(vfd.clone(), file);
        vfd
    }

    pub fn new_socket_fd(&mut self, host_fd: Fd) -> VirtualFd {
        let vfd = self.next_lowest_fd();
        self.fd_map.insert(vfd.clone(), FileHandle::Socket(host_fd));
        vfd
    }
}

fn next_lowest(fd_map: &FdMap) -> VirtualFd {
    let mut next_lowest_fd = 0;
    for (vfd, _) in fd_map.iter() {
        let host_fd = vfd.0;
        if host_fd == next_lowest_fd {
            next_lowest_fd += 1;
        } else if host_fd < next_lowest_fd {
            panic!("Should not be here.");
        } else {
            break;
        }
    }
    VirtualFd(next_lowest_fd)
}
