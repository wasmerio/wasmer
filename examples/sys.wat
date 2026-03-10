(module
	(type $t1 (func (param i32)))
	(type $t2 (func (param i32 i32 i32) (result i32)))
	(type $t3 (func (param i32) (result i32)))
	(type $t4 (func (param i32 i32) (result i32)))
	(func $putchar (import "env" "putchar") (type $t1))
	(func $printf (import "env" "printf") (type $t4))
	(func $sys_open (import "env" "sys_open") (type $t2))
	(func $sys_read (import "env" "sys_read") (type $t2))
	(func $sys_close (import "env" "sys_close") (type $t3))
	(func $sys_exit (import "env" "sys_exit") (type $t1))
 	(memory 1)
 	(data $filename (i32.const 0) "/Users/xxxx/Desktop/hello.txt\00")
	(func $main (export "_main")
        ;; declare variables
		(local $string_buf_addr i32)
		(local $string_buf_len i32)
		(local $file_access_flag i32)
		(local $file_permission_flag i32)
		(local $file_descriptor i32)

        ;; set variables
		(set_local $string_buf_addr (i32.const 72)) ;; string_buf_addr at offset 72
		(set_local $string_buf_len (i32.const 10)) ;; string_buf_len is 5
		(set_local $file_access_flag (i32.const 02)) ;; file_access_flag has O_RDWR permission
		(set_local $file_permission_flag (i32.const 700)) ;; file_permission_flag has S_IRWXU permission

		;; open file
		(call $sys_open (i32.const 0) (get_local $file_access_flag) (get_local $file_permission_flag)) ;; (path: u32, flags: c_int, mode: c_int) -> c_int
		(set_local $file_descriptor) ;; set file_descriptor to the value returned by sys_open

		;; read file content
		(call $sys_read (get_local $file_descriptor) (get_local $string_buf_addr) (get_local $string_buf_len)) ;; (fd: c_int, buf: u32, count: size_t) -> ssize_t
		(drop) ;; ignoring errors

		;; close file
		(call $sys_close (get_local $file_descriptor)) ;; (fd: c_int) -> c_int
		(drop) ;; ignoring errors

		;; print file content
		(call $printf (get_local $string_buf_addr) (i32.const 0))
		(drop) ;; ignoring errors

		;; exit
		(call $exit (i32.const 0))
	)
)
