(module
	(type $t1 (func (param i32 i32) (result i32)))
	(type $t2 (func (param i32)))
	(type $t3 (func ))
	(func $printf (import "env" "printf") (type $t1))
	(func $abort (import "env" "abort") (type $t2))
	(func $_abort (import "env" "_abort") (type $t3))
	(func $abort_on_cannot_grow_memory (import "env" "abortOnCannotGrowMemory") (type $t3))
 	(memory 1)
 	(data (i32.const 0) ">>> First\00")
 	(data (i32.const 24) ">>> Second\00")
 	(data (i32.const 48) "Aborting abruptly!\00")
	(func $main (export "main")
		;; print ">>> First"
		(call $printf (i32.const 0) (i32.const 0))
		(drop)

		;; aborts
		(call $_abort) ;; ()

		;; aborts
		(call $abort_on_cannot_grow_memory) ;; ()

		;; aborts
		(call $abort (i32.const 48)) ;; (message: u32)

		;; print ">>> Second"
		(call $printf (i32.const 24) (i32.const 0))
		(drop)
	)
)
