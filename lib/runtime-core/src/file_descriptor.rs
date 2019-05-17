use std::io;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Read;

pub struct FileDescriptor(libc::c_int);

impl FileDescriptor {
    pub fn new(file_descriptor_number: libc::c_int) -> FileDescriptor {
        FileDescriptor(file_descriptor_number)
    }
}

impl Read for FileDescriptor {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let file_descriptor: libc::c_int = self.0;
        let count =
            unsafe { libc::read(file_descriptor, buf.as_mut_ptr() as *mut libc::c_void, 1) };
        if count < 0 {
            Err(Error::new(ErrorKind::Other, "read error"))
        } else {
            Ok(count as usize)
        }
    }
}
