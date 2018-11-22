(module
	(type $t1 (func (param i32)))
	(func $putchar (import "env" "putchar") (type $t1))
	(func $sys_exit (import "env" "___syscall1") (type $t1))
 	(memory 1)
	(func $main (export "main")
		;; print "Hello"
		(call $putchar (i32.const 72))
		(call $putchar (i32.const 101))
		(call $putchar (i32.const 108))
		(call $putchar (i32.const 108))
		(call $putchar (i32.const 111))
		(call $putchar (i32.const 32))

		;; exit abruptly
		(call $sys_exit (i32.const 255)) ;; (status: c_int) -> c_int

		;; print " World!"
		(call $putchar (i32.const 87))
		(call $putchar (i32.const 111))
		(call $putchar (i32.const 114))
		(call $putchar (i32.const 108))
		(call $putchar (i32.const 100))
		(call $putchar (i32.const 33))
		(call $putchar (i32.const 10))
	)
)
