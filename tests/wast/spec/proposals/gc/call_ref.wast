(module
  (type $ii (func (param i32) (result i32)))

  (func $apply (param $f (ref $ii)) (param $x i32) (result i32)
    (call_ref (local.get $x) (local.get $f))
  )

  (func $f (type $ii) (i32.mul (local.get 0) (local.get 0)))
  (func $g (type $ii) (i32.sub (i32.const 0) (local.get 0)))

  (elem declare func $f $g)

  (func (export "run") (param $x i32) (result i32)
    (local $rf (ref null $ii))
    (local $rg (ref null $ii))
    (local.set $rf (ref.func $f))
    (local.set $rg (ref.func $g))
    (call_ref (call_ref (local.get $x) (local.get $rf)) (local.get $rg))
  )

  (func (export "null") (result i32)
    (call_ref (i32.const 1) (ref.null $ii))
  )

  ;; Recursion

  (type $ll (func (param i64) (result i64)))
  (type $lll (func (param i64 i64) (result i64)))

  (elem declare func $fac)
  (global $fac (ref $ll) (ref.func $fac))

  (func $fac (export "fac") (type $ll)
    (if (result i64) (i64.eqz (local.get 0))
      (then (i64.const 1))
      (else
        (i64.mul
          (local.get 0)
          (call_ref (i64.sub (local.get 0) (i64.const 1)) (global.get $fac))
        )
      )
    )
  )

  (elem declare func $fac-acc)
  (global $fac-acc (ref $lll) (ref.func $fac-acc))

  (func $fac-acc (export "fac-acc") (type $lll)
    (if (result i64) (i64.eqz (local.get 0))
      (then (local.get 1))
      (else
        (call_ref
          (i64.sub (local.get 0) (i64.const 1))
          (i64.mul (local.get 0) (local.get 1))
          (global.get $fac-acc)
        )
      )
    )
  )

  (elem declare func $fib)
  (global $fib (ref $ll) (ref.func $fib))

  (func $fib (export "fib") (type $ll)
    (if (result i64) (i64.le_u (local.get 0) (i64.const 1))
      (then (i64.const 1))
      (else
        (i64.add
          (call_ref (i64.sub (local.get 0) (i64.const 2)) (global.get $fib))
          (call_ref (i64.sub (local.get 0) (i64.const 1)) (global.get $fib))
        )
      )
    )
  )

  (elem declare func $even $odd)
  (global $even (ref $ll) (ref.func $even))
  (global $odd (ref $ll) (ref.func $odd))

  (func $even (export "even") (type $ll)
    (if (result i64) (i64.eqz (local.get 0))
      (then (i64.const 44))
      (else (call_ref (i64.sub (local.get 0) (i64.const 1)) (global.get $odd)))
    )
  )
  (func $odd (export "odd") (type $ll)
    (if (result i64) (i64.eqz (local.get 0))
      (then (i64.const 99))
      (else (call_ref (i64.sub (local.get 0) (i64.const 1)) (global.get $even)))
    )
  )
)

(assert_return (invoke "run" (i32.const 0)) (i32.const 0))
(assert_return (invoke "run" (i32.const 3)) (i32.const -9))

(assert_trap (invoke "null") "null function")

(assert_return (invoke "fac" (i64.const 0)) (i64.const 1))
(assert_return (invoke "fac" (i64.const 1)) (i64.const 1))
(assert_return (invoke "fac" (i64.const 5)) (i64.const 120))
(assert_return (invoke "fac" (i64.const 25)) (i64.const 7034535277573963776))
(assert_return (invoke "fac-acc" (i64.const 0) (i64.const 1)) (i64.const 1))
(assert_return (invoke "fac-acc" (i64.const 1) (i64.const 1)) (i64.const 1))
(assert_return (invoke "fac-acc" (i64.const 5) (i64.const 1)) (i64.const 120))
(assert_return
  (invoke "fac-acc" (i64.const 25) (i64.const 1))
  (i64.const 7034535277573963776)
)

(assert_return (invoke "fib" (i64.const 0)) (i64.const 1))
(assert_return (invoke "fib" (i64.const 1)) (i64.const 1))
(assert_return (invoke "fib" (i64.const 2)) (i64.const 2))
(assert_return (invoke "fib" (i64.const 5)) (i64.const 8))
(assert_return (invoke "fib" (i64.const 20)) (i64.const 10946))

(assert_return (invoke "even" (i64.const 0)) (i64.const 44))
(assert_return (invoke "even" (i64.const 1)) (i64.const 99))
(assert_return (invoke "even" (i64.const 100)) (i64.const 44))
(assert_return (invoke "even" (i64.const 77)) (i64.const 99))
(assert_return (invoke "odd" (i64.const 0)) (i64.const 99))
(assert_return (invoke "odd" (i64.const 1)) (i64.const 44))
(assert_return (invoke "odd" (i64.const 200)) (i64.const 99))
(assert_return (invoke "odd" (i64.const 77)) (i64.const 44))


;; Unreachable typing.

(module
  (func (export "unreachable") (result i32)
    (unreachable)
    (call_ref)
  )
)
(assert_trap (invoke "unreachable") "unreachable")

(module
  (elem declare func $f)
  (func $f (param i32) (result i32) (local.get 0))

  (func (export "unreachable") (result i32)
    (unreachable)
    (ref.func $f)
    (call_ref)
  )
)
(assert_trap (invoke "unreachable") "unreachable")

(module
  (elem declare func $f)
  (func $f (param i32) (result i32) (local.get 0))

  (func (export "unreachable") (result i32)
    (unreachable)
    (i32.const 0)
    (ref.func $f)
    (call_ref)
    (drop)
    (i32.const 0)
  )
)
(assert_trap (invoke "unreachable") "unreachable")

(assert_invalid
  (module
    (elem declare func $f)
    (func $f (param i32) (result i32) (local.get 0))

    (func (export "unreachable") (result i32)
      (unreachable)
      (i64.const 0)
      (ref.func $f)
      (call_ref)
    )
  )
  "type mismatch"
)

(assert_invalid
  (module
    (elem declare func $f)
    (func $f (param i32) (result i32) (local.get 0))

    (func (export "unreachable") (result i32)
      (unreachable)
      (ref.func $f)
      (call_ref)
      (drop)
      (i64.const 0)
    )
  )
  "type mismatch"
)

(assert_invalid
  (module
    (func $f (param $r externref)
      (call_ref (local.get $r))
    )
  )
  "type mismatch"
)
