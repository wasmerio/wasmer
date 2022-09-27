use wasi;

fn main() {
    unsafe {
        // in wasmer, the first dir passed to the engine has fd = 4
        let dir_fd = 4;
        let data = &[1u8; 10];
        let result = wasi::fd_fdstat_get(dir_fd)
        .expect("cannot access / (fd 4)");

        let file_fd = wasi::path_open(
            dir_fd,
            0,
            "file",
            wasi::OFLAGS_CREAT,
            0,
            0,
            0,
        )
        .expect("creating a file");

        let written = wasi::fd_write(
            file_fd,
            &[wasi::Ciovec {
                buf: data.as_ptr(),
                buf_len: data.len(),
            }],
        )
        .unwrap();

        println!("written : {}", written);

        wasi::fd_seek(file_fd, 0, wasi::WHENCE_SET).expect("seeking file");

        let file_content = &mut [0u8; 10];

        let iovec = wasi::Iovec {
            buf: file_content.as_mut_ptr(),
            buf_len: file_content.len(),
        };

        let read = wasi::fd_read(file_fd, &[iovec]).unwrap(); // should panic
        
        println!("read : {}", read);

        println!("file_content after : {:?}", file_content);

        wasi::path_unlink_file(dir_fd, "file").unwrap();
    }
}