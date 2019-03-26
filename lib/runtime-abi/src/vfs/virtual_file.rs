use failure::Error;

use crate::vfs::file_like::{FileLike, Metadata};
use std::io;
use std::io::{Seek, SeekFrom, Write};

impl FileLike for zbox::File {
    fn metadata(&self) -> Result<Metadata, Error> {
        self.metadata()
            .map(|m| Metadata {
                len: m.content_len(),
                is_file: m.is_file(),
            })
            .map_err(|e: zbox::Error| e.into())
    }

    fn write_file(&mut self, buf: &[u8], offset: usize) -> Result<usize, io::Error> {
        self.seek(SeekFrom::Start(offset as _))?;
        let result = self.write(buf);
        self.finish().unwrap();
        result
    }
}
