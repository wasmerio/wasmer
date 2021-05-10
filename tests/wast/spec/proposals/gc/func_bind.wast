(module
  (type $unop (func (param i32) (result i32)))

  (elem func $add)
  (func $add (param i32 i32) (result i32) (i32.add (local.get 0) (local.get 1)))

  (func $mk-adder (param $i i32) (result (ref $unop))
    (func.bind (type $unop) (local.get $i) (ref.func $add))
  )

  (global $f (mut (ref null $unop)) (ref.null $unop))

  (func (export "make") (param $i i32)
    (global.set $f (call $mk-adder (local.get $i)))
  )

  (func (export "call") (param $j i32) (result i32)
    (call_ref (local.get $j) (global.get $f))
  )

  (func (export "call2") (param $i i32) (param $j i32) (param $k i32) (result i32)
    (call $mk-adder (local.get $k))
    (let (result i32) (local $f (ref $unop))  ;; binds $f to top of stack
      (i32.mul
        (call_ref (local.get $i) (local.get $f))
        (call_ref (local.get $j) (local.get $f))
      )
    )
  )

  (func (export "null") (result i32)
    (func.bind (type $unop) (i32.const 1) (ref.null $unop))
    (drop)
  )
)

(assert_trap (invoke "call" (i32.const 0)) "null function")

(assert_return (invoke "make" (i32.const 3)))
(assert_return (invoke "call" (i32.const 2)) (i32.const 5))
(assert_return (invoke "call" (i32.const 10)) (i32.const 13))

(assert_return (invoke "make" (i32.const 0)))
(assert_return (invoke "call" (i32.const 10)) (i32.const 10))

(assert_return (invoke "make" (i32.const -3)))
(assert_return (invoke "call" (i32.const 10)) (i32.const 7))

(assert_return (invoke "call2" (i32.const 2) (i32.const 3) (i32.const 0)) (i32.const 6))
(assert_return (invoke "call2" (i32.const 2) (i32.const 5) (i32.const 1)) (i32.const 18))
(assert_return (invoke "call2" (i32.const 2) (i32.const 5) (i32.const 7)) (i32.const 108))

(assert_trap (invoke "null") "null function")

(module
  (elem declare func $p $f)
  (func $p (import "spectest" "print_f64_f64") (param f64 f64))
  (func $f (param f64 f64 f64 f64) (result f64)
    (f64.const 0)
    (f64.add (f64.mul (local.get 0) (f64.const 1000)))
    (f64.add (f64.mul (local.get 1) (f64.const 100)))
    (f64.add (f64.mul (local.get 2) (f64.const 10)))
    (f64.add (f64.mul (local.get 3) (f64.const 1)))
  )

  (type $p0 (func (param)))
  (type $p1 (func (param f64)))
  (type $p2 (func (param f64 f64)))
  (type $f0 (func (param) (result f64)))
  (type $f1 (func (param f64) (result f64)))
  (type $f2 (func (param f64 f64) (result f64)))
  (type $f3 (func (param f64 f64 f64) (result f64)))
  (type $f4 (func (param f64 f64 f64 f64) (result f64)))

  (table $tp 30 funcref)
  (table $tf 50 funcref)

  (func (export "call-p0") (param $i i32)
    (call_indirect $tp (type $p0) (local.get $i))
  )
  (func (export "call-p1") (param $i i32) (param f64)
    (call_indirect $tp (type $p1) (local.get 1) (local.get $i))
  )
  (func (export "call-p2") (param $i i32) (param f64 f64)
    (call_indirect $tp (type $p2) (local.get 1) (local.get 2) (local.get $i))
  )

  (func (export "call-f0") (param $i i32) (result f64)
    (call_indirect $tf (type $f0) (local.get $i))
  )
  (func (export "call-f1") (param $i i32) (param f64) (result f64)
    (call_indirect $tf (type $f1) (local.get 1) (local.get $i))
  )
  (func (export "call-f2") (param $i i32) (param f64 f64) (result f64)
    (call_indirect $tf (type $f2) (local.get 1) (local.get 2) (local.get $i))
  )
  (func (export "call-f3") (param $i i32) (param f64 f64 f64) (result f64)
    (call_indirect $tf (type $f3) (local.get 1) (local.get 2) (local.get 3) (local.get $i))
  )
  (func (export "call-f4") (param $i i32) (param f64 f64 f64 f64) (result f64)
    (call_indirect $tf (type $f4) (local.get 1) (local.get 2) (local.get 3) (local.get 4) (local.get $i))
  )

  (func (export "init")
    ;; Host closures with arity 2
    (table.set $tp (i32.const 20)
      (ref.func $p)
    )
    (table.set $tp (i32.const 21)
      (func.bind (type $p2)
        (ref.func $p))
    )

    ;; Host closures with arity 1
    (table.set $tp (i32.const 10)
      (func.bind (type $p1) (f64.const 1)
        (ref.func $p))
    )
    (table.set $tp (i32.const 11)
      (func.bind (type $p1) (f64.const 1)
        (func.bind (type $p2)
          (ref.func $p)))
    )
    (table.set $tp (i32.const 12)
      (func.bind (type $p1)
        (func.bind (type $p1) (f64.const 1)
          (func.bind (type $p2)
            (ref.func $p))))
    )

    ;; Host closures with arity 0
    (table.set $tp (i32.const 00)
      (func.bind (type $p0) (f64.const 1) (f64.const 2)
        (ref.func $p))
    )
    (table.set $tp (i32.const 01)
      (func.bind (type $p0) (f64.const 2)
       (func.bind (type $p1) (f64.const 1)
         (ref.func $p)))
    )
    (table.set $tp (i32.const 02)
      (func.bind (type $p0)
        (func.bind (type $p0) (f64.const 2)
          (func.bind (type $p1)
            (func.bind (type $p1) (f64.const 1)
              (func.bind (type $p2)
                (func.bind (type $p2)
                  (ref.func $p)))))))
    )

    ;; Wasm closures with arity 4
    (table.set $tf (i32.const 40)
      (ref.func $f)
    )
    (table.set $tf (i32.const 41)
      (func.bind (type $f4)
        (ref.func $f))
    )

    ;; Wasm closures with arity 3
    (table.set $tf (i32.const 30)
      (func.bind (type $f3) (f64.const 1)
        (ref.func $f))
    )
    (table.set $tf (i32.const 31)
      (func.bind (type $f3) (f64.const 1)
        (func.bind (type $f4)
          (ref.func $f)))
    )
    (table.set $tf (i32.const 32)
      (func.bind (type $f3)
        (func.bind (type $f3) (f64.const 1)
          (func.bind (type $f4)
            (ref.func $f))))
    )

    ;; Wasm closures with arity 2
    (table.set $tf (i32.const 20)
      (func.bind (type $f2) (f64.const 1) (f64.const 2)
        (ref.func $f))
    )
    (table.set $tf (i32.const 21)
      (func.bind (type $f2) (f64.const 2)
        (func.bind (type $f3) (f64.const 1)
          (ref.func $f)))
    )
    (table.set $tf (i32.const 22)
      (func.bind (type $f2)
        (func.bind (type $f2) (f64.const 2)
          (func.bind (type $f3)
            (func.bind (type $f3) (f64.const 1)
              (func.bind (type $f4)
                (func.bind (type $f4)
                  (ref.func $f)))))))
    )

    ;; Wasm closures with arity 1
    (table.set $tf (i32.const 10)
      (func.bind (type $f1) (f64.const 1) (f64.const 2) (f64.const 3)
        (ref.func $f))
    )
    (table.set $tf (i32.const 11)
      (func.bind (type $f1) (f64.const 2) (f64.const 3)
        (func.bind (type $f3) (f64.const 1)
          (ref.func $f)))
    )
    (table.set $tf (i32.const 12)
      (func.bind (type $f1) (f64.const 3)
        (func.bind (type $f2) (f64.const 1) (f64.const 2)
          (ref.func $f)))
    )
    (table.set $tf (i32.const 13)
      (func.bind (type $f1) (f64.const 3)
        (func.bind (type $f2) (f64.const 2)
          (func.bind (type $f3) (f64.const 1)
            (ref.func $f))))
    )
    (table.set $tf (i32.const 14)
      (func.bind (type $f1)
        (func.bind (type $f1)
          (func.bind (type $f1) (f64.const 3)
            (func.bind (type $f2)
              (func.bind (type $f2)
                (func.bind (type $f2) (f64.const 2)
                  (func.bind (type $f3)
                    (func.bind (type $f3)
                      (func.bind (type $f3) (f64.const 1)
                        (func.bind (type $f4)
                          (func.bind (type $f4)
                            (ref.func $f))))))))))))
    )

    ;; Wasm closures with arity 0
    (table.set $tf (i32.const 00)
      (func.bind (type $f0) (f64.const 1) (f64.const 2) (f64.const 3) (f64.const 4)
        (ref.func $f))
    )
    (table.set $tf (i32.const 01)
      (func.bind (type $f0) (f64.const 2) (f64.const 3) (f64.const 4)
        (func.bind (type $f3) (f64.const 1)
          (ref.func $f)))
    )
    (table.set $tf (i32.const 02)
      (func.bind (type $f0) (f64.const 3) (f64.const 4)
        (func.bind (type $f2) (f64.const 1) (f64.const 2)
          (ref.func $f)))
    )
    (table.set $tf (i32.const 03)
      (func.bind (type $f0) (f64.const 4)
        (func.bind (type $f1) (f64.const 1) (f64.const 2) (f64.const 3)
          (ref.func $f)))
    )
    (table.set $tf (i32.const 04)
      (func.bind (type $f0) (f64.const 3) (f64.const 4)
        (func.bind (type $f2) (f64.const 2)
          (func.bind (type $f3) (f64.const 1)
            (ref.func $f))))
    )
    (table.set $tf (i32.const 05)
      (func.bind (type $f0) (f64.const 4)
        (func.bind (type $f1) (f64.const 2) (f64.const 3)
          (func.bind (type $f3) (f64.const 1)
            (ref.func $f))))
    )
    (table.set $tf (i32.const 06)
      (func.bind (type $f0) (f64.const 4)
        (func.bind (type $f1) (f64.const 3)
          (func.bind (type $f2) (f64.const 1) (f64.const 2)
            (ref.func $f))))
    )
    (table.set $tf (i32.const 07)
      (func.bind (type $f0) (f64.const 4)
        (func.bind (type $f1) (f64.const 3)
          (func.bind (type $f2) (f64.const 2)
            (func.bind (type $f3) (f64.const 1)
              (func.bind (type $f4)
                (ref.func $f))))))
    )
    (table.set $tf (i32.const 08)
      (func.bind (type $f0)
        (func.bind (type $f0)
          (func.bind (type $f0) (f64.const 4)
            (func.bind (type $f1)
              (func.bind (type $f1)
                (func.bind (type $f1) (f64.const 2) (f64.const 3)
                  (func.bind (type $f3)
                    (func.bind (type $f3)
                      (func.bind (type $f3) (f64.const 1)
                        (func.bind (type $f4)
                          (func.bind (type $f4)
                            (ref.func $f))))))))))))
    )
  )
)

(invoke "init")

(assert_return (invoke "call-p0" (i32.const 00)))
(assert_return (invoke "call-p0" (i32.const 01)))
(assert_return (invoke "call-p0" (i32.const 02)))
(assert_return (invoke "call-p1" (i32.const 10) (f64.const 2)))
(assert_return (invoke "call-p1" (i32.const 11) (f64.const 2)))
(assert_return (invoke "call-p1" (i32.const 12) (f64.const 2)))
(assert_return (invoke "call-p2" (i32.const 20) (f64.const 1) (f64.const 2)))
(assert_return (invoke "call-p2" (i32.const 21) (f64.const 1) (f64.const 2)))

(assert_return (invoke "call-f0" (i32.const 00)) (f64.const 1234))
(assert_return (invoke "call-f0" (i32.const 01)) (f64.const 1234))
(assert_return (invoke "call-f0" (i32.const 02)) (f64.const 1234))
(assert_return (invoke "call-f0" (i32.const 03)) (f64.const 1234))
(assert_return (invoke "call-f0" (i32.const 04)) (f64.const 1234))
(assert_return (invoke "call-f0" (i32.const 05)) (f64.const 1234))
(assert_return (invoke "call-f0" (i32.const 06)) (f64.const 1234))
(assert_return (invoke "call-f0" (i32.const 07)) (f64.const 1234))
(assert_return (invoke "call-f0" (i32.const 08)) (f64.const 1234))
(assert_return (invoke "call-f1" (i32.const 10) (f64.const 4)) (f64.const 1234))
(assert_return (invoke "call-f1" (i32.const 11) (f64.const 4)) (f64.const 1234))
(assert_return (invoke "call-f1" (i32.const 12) (f64.const 4)) (f64.const 1234))
(assert_return (invoke "call-f1" (i32.const 13) (f64.const 4)) (f64.const 1234))
(assert_return (invoke "call-f1" (i32.const 14) (f64.const 4)) (f64.const 1234))
(assert_return (invoke "call-f2" (i32.const 20) (f64.const 3) (f64.const 4)) (f64.const 1234))
(assert_return (invoke "call-f2" (i32.const 21) (f64.const 3) (f64.const 4)) (f64.const 1234))
(assert_return (invoke "call-f2" (i32.const 22) (f64.const 3) (f64.const 4)) (f64.const 1234))
(assert_return (invoke "call-f3" (i32.const 30) (f64.const 2) (f64.const 3) (f64.const 4)) (f64.const 1234))
(assert_return (invoke "call-f3" (i32.const 31) (f64.const 2) (f64.const 3) (f64.const 4)) (f64.const 1234))
(assert_return (invoke "call-f3" (i32.const 32) (f64.const 2) (f64.const 3) (f64.const 4)) (f64.const 1234))
(assert_return (invoke "call-f4" (i32.const 40) (f64.const 1) (f64.const 2) (f64.const 3) (f64.const 4)) (f64.const 1234))
(assert_return (invoke "call-f4" (i32.const 41) (f64.const 1) (f64.const 2) (f64.const 3) (f64.const 4)) (f64.const 1234))


(; Once we allow subtyping on function types:
;; The runtime type of a closure is its internal one,
;; not the static bind annotation.
(module
  (type $ii (func (param i32) (result i32)))
  (type $fl (func (param i32 anyref) (result (ref null $ii))))
  (type $fu (func (param i32 (ref $ii)) (result anyref)))
  (type $fl' (func (param anyref) (result (ref null $ii))))
  (type $fu' (func (param (ref $ii)) (result anyref)))

  (elem declare func $sqr $f)
  (func $sqr (param i32) (result i32) (i32.mul (local.get 0) (local.get 0)))
  (func $f (type $fl) (ref.func $sqr))

  (table $t 10 funcref)

  (func (export "run") (result i32)
    (table.set $t (i32.const 0) (func.bind (type $fu) (ref.func $f)))
    (table.set $t (i32.const 1) (func.bind (type $fu') (i32.const 0) (ref.func $f)))

    (i32.add
      (call_ref (i32.const 2)
        (call_indirect $t (type $fl) (i32.const 0) (ref.null $fl) (i32.const 0))
      )
      (call_ref (i32.const 3)
        (call_indirect $t (type $fl') (ref.null $fl') (i32.const 1))
      )
    )
  )
)

(assert_return (invoke "run") (i32.const 13))
;)

(; Instead, for now: ;)
(assert_invalid
  (module
    (type $ii (func (param i32) (result i32)))
    (type $fl (func (param i32 funcref) (result (ref null $ii))))
    (type $fu (func (param i32 (ref $ii)) (result funcref)))

    (elem declare func $sqr $f)
    (func $sqr (param i32) (result i32) (i32.mul (local.get 0) (local.get 0)))
    (func $f (type $fl) (ref.func $sqr))

    (func (drop (func.bind (type $fu) (ref.func $f))))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (type $ii (func (param i32) (result i32)))
    (type $fl (func (param i32 funcref) (result (ref null $ii))))
    (type $fu' (func (param (ref $ii)) (result funcref)))

    (elem declare func $sqr $f)
    (func $sqr (param i32) (result i32) (i32.mul (local.get 0) (local.get 0)))
    (func $f (type $fl) (ref.func $sqr))

    (func (drop (func.bind (type $fu') (i32.const 0) (ref.func $f))))
  )
  "type mismatch"
)


;; Null and unreachable typing.

(module
  (type $t (func))
  (func (export "null") (result (ref $t))
    (ref.null $t)
    (func.bind)
  )
)
(assert_trap (invoke "null") "null function")

(module
  (type $t (func (param f32)))
  (func (export "null") (result (ref $t))
    (ref.null $t)
    (func.bind (type $t))
  )
)
(assert_trap (invoke "null") "null function")

(module
  (type $t0 (func))
  (type $t1 (func (param i64)))
  (func (export "null") (result (ref $t0))
    (i64.const 0)
    (ref.null $t1)
    (func.bind)
  )
)
(assert_trap (invoke "null") "null function")

(module
  (type $t0 (func))
  (type $t1 (func (param i64)))
  (func (export "null") (result (ref $t0))
    (i64.const 0)
    (ref.null $t1)
    (func.bind (type $t0))
  )
)
(assert_trap (invoke "null") "null function")

(assert_invalid
  (module
    (type $t (func (result f32)))
    (func (export "null") (result i32)
      (ref.null $t)
      (func.bind)
    )
  )
  "type mismatch"
)


(module
  (type $t (func (param f32)))
  (func (export "unreachable") (result (ref $t))
    (unreachable)
    (func.bind (type $t))
  )
)
(assert_trap (invoke "unreachable") "unreachable")

(module
  (type $t (func (param f32) (result i32)))
  (elem declare func $f)
  (func $f (param i32 f32) (result i32) (local.get 0))

  (func (export "unreachable") (result (ref $t))
    (unreachable)
    (ref.func $f)
    (func.bind (type $t))
  )
)
(assert_trap (invoke "unreachable") "unreachable")

(assert_invalid
  (module
    (type $t (func (param f32)))
    (elem declare func $f)
    (func $f (param i32 f32) (result i32) (local.get 0))

    (func (export "unreachable") (result (ref $t))
      (unreachable)
      (i64.const 0)
      (ref.func $f)
      (func.bind (type $t))
    )
  )
  "type mismatch"
)

(assert_invalid
  (module
    (type $t (func))
    (elem declare func $f)
    (func $f (param i32) (result i32) (local.get 0))

    (func (export "unreachable") (result (ref $t))
      (unreachable)
      (ref.func $f)
      (func.bind (type $t))
      (drop)
      (i64.const 0)
    )
  )
  "type mismatch"
)

(assert_invalid
  (module
    (func (export "null") (result i32)
      (unreachable)
      (func.bind)
    )
  )
  "type mismatch"
)

(assert_invalid
  (module
    (type $t (func (param f32)))
    (elem declare func $f)
    (func $f (param i32 i32) (result i32) (local.get 0))

    (func (export "unreachable") (result (ref $t))
      (unreachable)
      (ref.func $f)
      (func.bind (type $t))
    )
  )
  "type mismatch"
)

(assert_invalid
  (module
    (type $t (func))
    (func $f (param $r externref)
      (func.bind (type $t) (local.get $r))
      (drop)
    )
  )
  "type mismatch"
)
