use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::rc::Rc;
use wasmer_runtime_abi::vfs::device_file;
use wasmer_runtime_abi::vfs::file_like::{FileLike, Metadata};
use wasmer_runtime_abi::vfs::vfs::Vfs;

pub type Fd = i32;

pub enum FileHandle {
    Socket(Fd),
    Vf(Rc<RefCell<dyn FileLike>>),
}

pub struct EmscriptenVfs {
    pub fd_map: BTreeMap<i32, FileHandle>,
    pub vfs: Vfs,
}

impl EmscriptenVfs {
    pub fn new(mut vfs: Vfs) -> Self {
        let mut fd_map = BTreeMap::new();

        let stdin = Rc::new(RefCell::new(device_file::Stdin));
        vfs.create_device_file("/dev/stdin", stdin.clone());
        fd_map.insert(0, FileHandle::Vf(stdin));

        let stdout = Rc::new(RefCell::new(device_file::Stdout));
        vfs.create_device_file("/dev/stdout", stdout.clone());
        fd_map.insert(1, FileHandle::Vf(stdout));

        let stderr = Rc::new(RefCell::new(device_file::Stderr));
        vfs.create_device_file("/dev/stderr", stderr.clone());
        fd_map.insert(2, FileHandle::Vf(stderr));

        EmscriptenVfs { fd_map, vfs }
    }

    pub fn make_dir<P: AsRef<Path>>(&mut self, path: P) -> () {
        self.vfs.make_dir(path);
    }

    pub fn path_metadata<P: AsRef<Path>>(&mut self, path: P) -> Option<Metadata> {
        if let Some(file) = self.vfs.open_file(path) {
            RefCell::borrow(&file).metadata().ok()
        } else {
            None
        }
    }

    pub fn close_file_descriptor(&mut self, fd: i32) -> i32 {
        match self.fd_map.remove(&fd) {
            Some(FileHandle::Vf(_)) => 0,
            Some(FileHandle::Socket(fd)) => unsafe { libc::close(fd) },
            _ => -1,
        }
    }

    pub fn next_lowest_fd_raw(&self) -> i32 {
        let fd_map = &self.fd_map;
        let mut next_lowest_fd = 0;
        for (vfd, _) in fd_map.iter() {
            let host_fd = *vfd;
            if host_fd == next_lowest_fd {
                next_lowest_fd += 1;
            } else if host_fd < next_lowest_fd {
                panic!("Should not be here.");
            } else {
                break;
            }
        }
        next_lowest_fd
    }

    pub fn get_host_socket_fd(&self, fd: i32) -> Option<Fd> {
        match self.fd_map.get(&fd) {
            Some(FileHandle::Socket(fd)) => Some(*fd),
            _ => None,
        }
    }

    pub fn open_file<P: AsRef<std::path::Path>>(&mut self, path: P) -> i32 {
        match self.vfs.open_file(path) {
            Some(file) => {
                let fd = self.next_lowest_fd_raw();
                let handle = FileHandle::Vf(file);
                self.fd_map.insert(fd, handle);
                fd
            }
            None => -1,
        }
    }

    pub fn write_file(
        &mut self,
        fd: i32,
        buf_slice: &mut [u8],
        count: usize,
    ) -> Result<usize, EmscriptenVfsError> {
        match self.fd_map.get(&fd) {
            Some(FileHandle::Vf(file)) => {
                let mut mut_ref = RefCell::borrow_mut(file);
                mut_ref.write_file(buf_slice, 0).into()
            }
            Some(FileHandle::Socket(host_fd)) => unsafe {
                let result = libc::write(*host_fd, buf_slice.as_ptr() as _, count as _);
                if result == -1 {
                    Err(EmscriptenVfsError::Errno(errno::errno()))
                } else {
                    Ok(result as usize)
                }
            },
            _ => Err(EmscriptenVfsError::FileSystemError),
        }
    }

    pub fn read_file(&self, fd: i32, buf_slice: &mut [u8]) -> usize {
        match self.fd_map.get(&fd) {
            Some(FileHandle::Vf(file)) => {
                let count = {
                    let mut result = RefCell::borrow_mut(&file);
                    let result = result.read(buf_slice);
                    result.unwrap()
                };
                count as _
            }
            Some(FileHandle::Socket(host_fd)) => unsafe {
                let buf_addr = buf_slice.as_ptr() as _;
                libc::write(*host_fd, buf_addr, buf_slice.len()) as usize
            },
            _ => 0,
        }
    }

    pub fn new_socket_fd(&mut self, host_fd: Fd) -> Fd {
        let fd = self.next_lowest_fd_raw();
        self.fd_map.insert(fd, FileHandle::Socket(host_fd));
        fd
    }
}

#[derive(Debug)]
pub enum EmscriptenVfsError {
    Io(io::Error),
    Errno(errno::Errno),
    FileSystemError,
}

impl From<io::Error> for EmscriptenVfsError {
    fn from(io_error: io::Error) -> Self {
        EmscriptenVfsError::Io(io_error)
    }
}
