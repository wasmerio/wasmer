;; Test `return_call` operator

(module
  ;; Auxiliary definitions
  (func $const-i32 (result i32) (i32.const 0x132))
  (func $const-i64 (result i64) (i64.const 0x164))
  (func $const-f32 (result f32) (f32.const 0xf32))
  (func $const-f64 (result f64) (f64.const 0xf64))

  (func $id-i32 (param i32) (result i32) (local.get 0))
  (func $id-i64 (param i64) (result i64) (local.get 0))
  (func $id-f32 (param f32) (result f32) (local.get 0))
  (func $id-f64 (param f64) (result f64) (local.get 0))

  (func $f32-i32 (param f32 i32) (result i32) (local.get 1))
  (func $i32-i64 (param i32 i64) (result i64) (local.get 1))
  (func $f64-f32 (param f64 f32) (result f32) (local.get 1))
  (func $i64-f64 (param i64 f64) (result f64) (local.get 1))

  ;; Typing

  (func (export "type-i32") (result i32) (return_call $const-i32))
  (func (export "type-i64") (result i64) (return_call $const-i64))
  (func (export "type-f32") (result f32) (return_call $const-f32))
  (func (export "type-f64") (result f64) (return_call $const-f64))

  (func (export "type-first-i32") (result i32) (return_call $id-i32 (i32.const 32)))
  (func (export "type-first-i64") (result i64) (return_call $id-i64 (i64.const 64)))
  (func (export "type-first-f32") (result f32) (return_call $id-f32 (f32.const 1.32)))
  (func (export "type-first-f64") (result f64) (return_call $id-f64 (f64.const 1.64)))

  (func (export "type-second-i32") (result i32)
    (return_call $f32-i32 (f32.const 32.1) (i32.const 32))
  )
  (func (export "type-second-i64") (result i64)
    (return_call $i32-i64 (i32.const 32) (i64.const 64))
  )
  (func (export "type-second-f32") (result f32)
    (return_call $f64-f32 (f64.const 64) (f32.const 32))
  )
  (func (export "type-second-f64") (result f64)
    (return_call $i64-f64 (i64.const 64) (f64.const 64.1))
  )

  ;; Recursion

  (func $fac-acc (export "fac-acc") (param i64 i64) (result i64)
    (if (result i64) (i64.eqz (local.get 0))
      (then (local.get 1))
      (else
        (return_call $fac-acc
          (i64.sub (local.get 0) (i64.const 1))
          (i64.mul (local.get 0) (local.get 1))
        )
      )
    )
  )

  (func $count (export "count") (param i64) (result i64)
    (if (result i64) (i64.eqz (local.get 0))
      (then (local.get 0))
      (else (return_call $count (i64.sub (local.get 0) (i64.const 1))))
    )
  )

  (func $even (export "even") (param i64) (result i32)
    (if (result i32) (i64.eqz (local.get 0))
      (then (i32.const 44))
      (else (return_call $odd (i64.sub (local.get 0) (i64.const 1))))
    )
  )
  (func $odd (export "odd") (param i64) (result i32)
    (if (result i32) (i64.eqz (local.get 0))
      (then (i32.const 99))
      (else (return_call $even (i64.sub (local.get 0) (i64.const 1))))
    )
  )
)

(assert_return (invoke "type-i32") (i32.const 0x132))
(assert_return (invoke "type-i64") (i64.const 0x164))
(assert_return (invoke "type-f32") (f32.const 0xf32))
(assert_return (invoke "type-f64") (f64.const 0xf64))

(assert_return (invoke "type-first-i32") (i32.const 32))
(assert_return (invoke "type-first-i64") (i64.const 64))
(assert_return (invoke "type-first-f32") (f32.const 1.32))
(assert_return (invoke "type-first-f64") (f64.const 1.64))

(assert_return (invoke "type-second-i32") (i32.const 32))
(assert_return (invoke "type-second-i64") (i64.const 64))
(assert_return (invoke "type-second-f32") (f32.const 32))
(assert_return (invoke "type-second-f64") (f64.const 64.1))

(assert_return (invoke "fac-acc" (i64.const 0) (i64.const 1)) (i64.const 1))
(assert_return (invoke "fac-acc" (i64.const 1) (i64.const 1)) (i64.const 1))
(assert_return (invoke "fac-acc" (i64.const 5) (i64.const 1)) (i64.const 120))
(assert_return
  (invoke "fac-acc" (i64.const 25) (i64.const 1))
  (i64.const 7034535277573963776)
)

(assert_return (invoke "count" (i64.const 0)) (i64.const 0))
(assert_return (invoke "count" (i64.const 1000)) (i64.const 0))
(assert_return (invoke "count" (i64.const 1_000_000)) (i64.const 0))

(assert_return (invoke "even" (i64.const 0)) (i32.const 44))
(assert_return (invoke "even" (i64.const 1)) (i32.const 99))
(assert_return (invoke "even" (i64.const 100)) (i32.const 44))
(assert_return (invoke "even" (i64.const 77)) (i32.const 99))
(assert_return (invoke "even" (i64.const 1_000_000)) (i32.const 44))
(assert_return (invoke "even" (i64.const 1_000_001)) (i32.const 99))
(assert_return (invoke "odd" (i64.const 0)) (i32.const 99))
(assert_return (invoke "odd" (i64.const 1)) (i32.const 44))
(assert_return (invoke "odd" (i64.const 200)) (i32.const 99))
(assert_return (invoke "odd" (i64.const 77)) (i32.const 44))
(assert_return (invoke "odd" (i64.const 1_000_000)) (i32.const 99))
(assert_return (invoke "odd" (i64.const 999_999)) (i32.const 44))


;; Invalid typing

(assert_invalid
  (module
    (func $type-void-vs-num (result i32) (return_call 1) (i32.const 0))
    (func)
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-num-vs-num (result i32) (return_call 1) (i32.const 0))
    (func (result i64) (i64.const 1))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (func $arity-0-vs-1 (return_call 1))
    (func (param i32))
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $arity-0-vs-2 (return_call 1))
    (func (param f64 i32))
  )
  "type mismatch"
)

(module
  (func $arity-1-vs-0 (i32.const 1) (return_call 1))
  (func)
)

(module
  (func $arity-2-vs-0 (f64.const 2) (i32.const 1) (return_call 1))
  (func)
)

(assert_invalid
  (module
    (func $type-first-void-vs-num (return_call 1 (nop) (i32.const 1)))
    (func (param i32 i32))
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-second-void-vs-num (return_call 1 (i32.const 1) (nop)))
    (func (param i32 i32))
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-first-num-vs-num (return_call 1 (f64.const 1) (i32.const 1)))
    (func (param i32 f64))
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-second-num-vs-num (return_call 1 (i32.const 1) (f64.const 1)))
    (func (param f64 i32))
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $f (result i32 i32) unreachable)
    (func (result i32)
      return_call $f
    )
  )
  "type mismatch"
)

;; Unbound function

(assert_invalid
  (module (func $unbound-func (return_call 1)))
  "unknown function"
)
(assert_invalid
  (module (func $large-func (return_call 1012321300)))
  "unknown function"
)
