(module
  (tag $e0)

  (func (export "simple-catch-all") (param i32) (result i32)
    (block $h
      (try_table (catch_all $h) (if (i32.eqz (local.get 0)) (then (throw $e0)) (else))
		;; No exception thrown.
		(i32.const 0)
      	(return)
	  )
    )

	;; The exception was thrown. 
	(i32.const 1)
	(return)
  )
)

(assert_return (invoke "simple-catch-all" (i32.const 0)) (i32.const 1))
(assert_return (invoke "simple-catch-all" (i32.const 1)) (i32.const 0))

(module
  (tag $e1-i32 (param i32))

  (func (export "simple-catch-i32") (param i32) (result i32)
    (block $h (result i32)
      (try_table (result i32) 
		  (catch $e1-i32 $h)
        (throw $e1-i32 (local.get 0))

		;; Never going to be executed, of course.
        (i32.const 2)
      )
    )
    (return)
  )
)
(assert_return (invoke "simple-catch-i32" (i32.const 0)) (i32.const 0))
(assert_return (invoke "simple-catch-i32" (i32.const 1)) (i32.const 1))
(assert_return (invoke "simple-catch-i32" (i32.const 2)) (i32.const 2))
