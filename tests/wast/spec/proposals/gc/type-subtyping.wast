;; Definitions

(module
  (type $e0 (sub (array i32)))
  (type $e1 (sub $e0 (array i32)))

  (type $e2 (sub (array anyref)))
  (type $e3 (sub (array (ref null $e0))))
  (type $e4 (sub (array (ref $e1))))

  (type $m1 (sub (array (mut i32))))
  (type $m2 (sub $m1 (array (mut i32))))
)

(module
  (type $e0 (sub (struct)))
  (type $e1 (sub $e0 (struct)))
  (type $e2 (sub $e1 (struct (field i32))))
  (type $e3 (sub $e2 (struct (field i32 (ref null $e0)))))
  (type $e4 (sub $e3 (struct (field i32 (ref $e0) (mut i64)))))
  (type $e5 (sub $e4 (struct (field i32 (ref $e1) (mut i64)))))
)

(module
  (type $s (sub (struct)))
  (type $s' (sub $s (struct)))

  (type $f1 (sub (func (param (ref $s')) (result anyref))))
  (type $f2 (sub $f1 (func (param (ref $s)) (result (ref any)))))
  (type $f3 (sub $f2 (func (param (ref null $s)) (result (ref $s)))))
  (type $f4 (sub $f3 (func (param (ref null struct)) (result (ref $s')))))
)


;; Recursive definitions

(module
  (type $t (sub (struct (field anyref))))
  (rec (type $r (sub $t (struct (field (ref $r))))))
  (type $t' (sub $r (struct (field (ref $r) i32))))
)

(module
  (rec
    (type $r1 (sub (struct (field i32 (ref $r1)))))
  )
  (rec
    (type $r2 (sub $r1 (struct (field i32 (ref $r3)))))
    (type $r3 (sub $r1 (struct (field i32 (ref $r2)))))
  )
)

(module
  (rec
    (type $a1 (sub (struct (field i32 (ref $a2)))))
    (type $a2 (sub (struct (field i64 (ref $a1)))))
  )
  (rec
    (type $b1 (sub $a2 (struct (field i64 (ref $a1) i32))))
    (type $b2 (sub $a1 (struct (field i32 (ref $a2) i32))))
    (type $b3 (sub $a2 (struct (field i64 (ref $b2) i32))))
  )
)


;; Subsumption

(module
  (rec
    (type $t1 (sub (func (param i32 (ref $t3)))))
    (type $t2 (sub $t1 (func (param i32 (ref $t2)))))
    (type $t3 (sub $t2 (func (param i32 (ref $t1)))))
  )

  (func $f1 (param $r (ref $t1))
    (call $f1 (local.get $r))
  )
  (func $f2 (param $r (ref $t2))
    (call $f1 (local.get $r))
    (call $f2 (local.get $r))
  )
  (func $f3 (param $r (ref $t3))
    (call $f1 (local.get $r))
    (call $f2 (local.get $r))
    (call $f3 (local.get $r))
  )
)

(module
  (rec
    (type $t1 (sub (func (result i32 (ref $u1)))))
    (type $u1 (sub (func (result f32 (ref $t1)))))
  )

  (rec
    (type $t2 (sub $t1 (func (result i32 (ref $u3)))))
    (type $u2 (sub $u1 (func (result f32 (ref $t3)))))
    (type $t3 (sub $t1 (func (result i32 (ref $u2)))))
    (type $u3 (sub $u1 (func (result f32 (ref $t2)))))
  )

  (func $f1 (param $r (ref $t1))
    (call $f1 (local.get $r))
  )
  (func $f2 (param $r (ref $t2))
    (call $f1 (local.get $r))
    (call $f2 (local.get $r))
  )
  (func $f3 (param $r (ref $t3))
    (call $f1 (local.get $r))
    (call $f3 (local.get $r))
  )
)

(module
  (rec (type $f1 (sub (func))) (type (struct (field (ref $f1)))))
  (rec (type $f2 (sub (func))) (type (struct (field (ref $f2)))))
  (rec (type $g1 (sub $f1 (func))) (type (struct)))
  (rec (type $g2 (sub $f2 (func))) (type (struct)))
  (func $g (type $g2))
  (global (ref $g1) (ref.func $g))
)

(module
  (rec (type $f1 (sub (func))) (type $s1 (sub (struct (field (ref $f1))))))
  (rec (type $f2 (sub (func))) (type $s2 (sub (struct (field (ref $f2))))))
  (rec
    (type $g1 (sub $f1 (func)))
    (type (sub $s1 (struct (field (ref $f1) (ref $f1) (ref $f2) (ref $f2) (ref $g1)))))
  )
  (rec
    (type $g2 (sub $f2 (func)))
    (type (sub $s2 (struct (field (ref $f1) (ref $f2) (ref $f1) (ref $f2) (ref $g2)))))
  )
  (func $g (type $g2))
  (global (ref $g1) (ref.func $g))
)

(assert_invalid
  (module
    (rec (type $f1 (sub (func))) (type (struct (field (ref $f1)))))
    (rec (type $f2 (sub (func))) (type (struct (field (ref $f1)))))
    (rec (type $g1 (sub $f1 (func))) (type (struct)))
    (rec (type $g2 (sub $f2 (func))) (type (struct)))
    (func $g (type $g2))
    (global (ref $g1) (ref.func $g))
  )
  "type mismatch"
)

(module
  (rec (type $f1 (sub (func))) (type (struct (field (ref $f1)))))
  (rec (type $f2 (sub (func))) (type (struct (field (ref $f2)))))
  (rec (type $g (sub $f1 (func))) (type (struct)))
  (func $g (type $g))
  (global (ref $f1) (ref.func $g))
)

(module
  (rec (type $f1 (sub (func))) (type $s1 (sub (struct (field (ref $f1))))))
  (rec (type $f2 (sub (func))) (type $s2 (sub (struct (field (ref $f2))))))
  (rec
    (type $g1 (sub $f1 (func)))
    (type (sub $s1 (struct (field (ref $f1) (ref $f1) (ref $f2) (ref $f2) (ref $g1)))))
  )
  (rec
    (type $g2 (sub $f2 (func)))
    (type (sub $s2 (struct (field (ref $f1) (ref $f2) (ref $f1) (ref $f2) (ref $g2)))))
  )
  (rec (type $h (sub $g2 (func))) (type (struct)))
  (func $h (type $h))
  (global (ref $f1) (ref.func $h))
  (global (ref $g1) (ref.func $h))
)


(module
  (rec (type $f11 (sub (func (result (ref func))))) (type $f12 (sub $f11 (func (result (ref $f11))))))
  (rec (type $f21 (sub (func (result (ref func))))) (type $f22 (sub $f21 (func (result (ref $f21))))))
  (func $f11 (type $f11) (unreachable))
  (func $f12 (type $f12) (unreachable))
  (global (ref $f11) (ref.func $f11))
  (global (ref $f21) (ref.func $f11))
  (global (ref $f12) (ref.func $f12))
  (global (ref $f22) (ref.func $f12))
)

(module
  (rec (type $f11 (sub (func (result (ref func))))) (type $f12 (sub $f11 (func (result (ref $f11))))))
  (rec (type $f21 (sub (func (result (ref func))))) (type $f22 (sub $f21 (func (result (ref $f21))))))
  (rec (type $g11 (sub $f11 (func (result (ref func))))) (type $g12 (sub $g11 (func (result (ref $g11))))))
  (rec (type $g21 (sub $f21 (func (result (ref func))))) (type $g22 (sub $g21 (func (result (ref $g21))))))
  (func $g11 (type $g11) (unreachable))
  (func $g12 (type $g12) (unreachable))
  (global (ref $f11) (ref.func $g11))
  (global (ref $f21) (ref.func $g11))
  (global (ref $f11) (ref.func $g12))
  (global (ref $f21) (ref.func $g12))
  (global (ref $g11) (ref.func $g11))
  (global (ref $g21) (ref.func $g11))
  (global (ref $g12) (ref.func $g12))
  (global (ref $g22) (ref.func $g12))
)

(assert_invalid
  (module
    (rec (type $f11 (sub (func))) (type $f12 (sub $f11 (func))))
    (rec (type $f21 (sub (func))) (type $f22 (sub $f11 (func))))
    (func $f (type $f21))
    (global (ref $f11) (ref.func $f))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (rec (type $f01 (sub (func))) (type $f02 (sub $f01 (func))))
    (rec (type $f11 (sub (func))) (type $f12 (sub $f01 (func))))
    (rec (type $f21 (sub (func))) (type $f22 (sub $f11 (func))))
    (func $f (type $f21))
    (global (ref $f11) (ref.func $f))
  )
  "type mismatch"
)


;; Runtime types

(module
  (type $t0 (sub (func (result (ref null func)))))
  (rec (type $t1 (sub $t0 (func (result (ref null $t1))))))
  (rec (type $t2 (sub $t1 (func (result (ref null $t2))))))

  (func $f0 (type $t0) (ref.null func))
  (func $f1 (type $t1) (ref.null $t1))
  (func $f2 (type $t2) (ref.null $t2))
  (table funcref (elem $f0 $f1 $f2))

  (func (export "run")
    (block (result (ref null func)) (call_indirect (type $t0) (i32.const 0)))
    (block (result (ref null func)) (call_indirect (type $t0) (i32.const 1)))
    (block (result (ref null func)) (call_indirect (type $t0) (i32.const 2)))
    (block (result (ref null $t1)) (call_indirect (type $t1) (i32.const 1)))
    (block (result (ref null $t1)) (call_indirect (type $t1) (i32.const 2)))
    (block (result (ref null $t2)) (call_indirect (type $t2) (i32.const 2)))

    (block (result (ref null $t0)) (ref.cast (ref $t0) (table.get (i32.const 0))))
    (block (result (ref null $t0)) (ref.cast (ref $t0) (table.get (i32.const 1))))
    (block (result (ref null $t0)) (ref.cast (ref $t0) (table.get (i32.const 2))))
    (block (result (ref null $t1)) (ref.cast (ref $t1) (table.get (i32.const 1))))
    (block (result (ref null $t1)) (ref.cast (ref $t1) (table.get (i32.const 2))))
    (block (result (ref null $t2)) (ref.cast (ref $t2) (table.get (i32.const 2))))
    (br 0)
  )

  (func (export "fail1")
    (block (result (ref null $t1)) (call_indirect (type $t1) (i32.const 0)))
    (br 0)
  )
  (func (export "fail2")
    (block (result (ref null $t1)) (call_indirect (type $t2) (i32.const 0)))
    (br 0)
  )
  (func (export "fail3")
    (block (result (ref null $t1)) (call_indirect (type $t2) (i32.const 1)))
    (br 0)
  )

  (func (export "fail4")
    (ref.cast (ref $t1) (table.get (i32.const 0)))
    (br 0)
  )
  (func (export "fail5")
    (ref.cast (ref $t2) (table.get (i32.const 0)))
    (br 0)
  )
  (func (export "fail6")
    (ref.cast (ref $t2) (table.get (i32.const 1)))
    (br 0)
  )
)
(assert_return (invoke "run"))
(assert_trap (invoke "fail1") "indirect call")
(assert_trap (invoke "fail2") "indirect call")
(assert_trap (invoke "fail3") "indirect call")
(assert_trap (invoke "fail4") "cast")
(assert_trap (invoke "fail5") "cast")
(assert_trap (invoke "fail6") "cast")

(module
  (type $t1 (sub (func)))
  (type $t2 (sub final (func)))

  (func $f1 (type $t1))
  (func $f2 (type $t2))
  (table funcref (elem $f1 $f2))

  (func (export "fail1")
    (block (call_indirect (type $t1) (i32.const 1)))
  )
  (func (export "fail2")
    (block (call_indirect (type $t2) (i32.const 0)))
  )

  (func (export "fail3")
    (ref.cast (ref $t1) (table.get (i32.const 1)))
    (drop)
  )
  (func (export "fail4")
    (ref.cast (ref $t2) (table.get (i32.const 0)))
    (drop)
  )
)
(assert_trap (invoke "fail1") "indirect call")
(assert_trap (invoke "fail2") "indirect call")
(assert_trap (invoke "fail3") "cast")
(assert_trap (invoke "fail4") "cast")

(module
  (type $t1 (sub (func)))
  (type $t2 (sub $t1 (func)))
  (type $t3 (sub $t2 (func)))
  (type $t4 (sub final (func)))

  (func $f2 (type $t2))
  (func $f3 (type $t3))
  (table (ref null $t2) (elem $f2 $f3))

  (func (export "run")
    (call_indirect (type $t1) (i32.const 0))
    (call_indirect (type $t1) (i32.const 1))
    (call_indirect (type $t2) (i32.const 0))
    (call_indirect (type $t2) (i32.const 1))
    (call_indirect (type $t3) (i32.const 1))
  )

  (func (export "fail1")
    (call_indirect (type $t3) (i32.const 0))
  )
  (func (export "fail2")
    (call_indirect (type $t4) (i32.const 0))
  )
)
(assert_return (invoke "run"))
(assert_trap (invoke "fail1") "indirect call")
(assert_trap (invoke "fail2") "indirect call")

(module
  (rec (type $f1 (sub (func))) (type (struct (field (ref $f1)))))
  (rec (type $f2 (sub (func))) (type (struct (field (ref $f2)))))
  (rec (type $g1 (sub $f1 (func))) (type (struct)))
  (rec (type $g2 (sub $f2 (func))) (type (struct)))
  (func $g (type $g2)) (elem declare func $g)
  (func (export "run") (result i32)
    (ref.test (ref $g1) (ref.func $g))
  )
)
(assert_return (invoke "run") (i32.const 1))

(module
  (rec (type $f1 (sub (func))) (type $s1 (sub (struct (field (ref $f1))))))
  (rec (type $f2 (sub (func))) (type $s2 (sub (struct (field (ref $f2))))))
  (rec
    (type $g1 (sub $f1 (func)))
    (type (sub $s1 (struct (field (ref $f1) (ref $f1) (ref $f2) (ref $f2) (ref $g1)))))
  )
  (rec
    (type $g2 (sub $f2 (func)))
    (type (sub $s2 (struct (field (ref $f1) (ref $f2) (ref $f1) (ref $f2) (ref $g2)))))
  )
  (func $g (type $g2)) (elem declare func $g)
  (func (export "run") (result i32)
    (ref.test (ref $g1) (ref.func $g))
  )
)
(assert_return (invoke "run") (i32.const 1))

(module
  (rec (type $f1 (sub (func))) (type (struct (field (ref $f1)))))
  (rec (type $f2 (sub (func))) (type (struct (field (ref $f1)))))
  (rec (type $g1 (sub $f1 (func))) (type (struct)))
  (rec (type $g2 (sub $f2 (func))) (type (struct)))
  (func $g (type $g2)) (elem declare func $g)
  (func (export "run") (result i32)
    (ref.test (ref $g1) (ref.func $g))
  )
)
(assert_return (invoke "run") (i32.const 0))

(module
  (rec (type $f1 (sub (func))) (type (struct (field (ref $f1)))))
  (rec (type $f2 (sub (func))) (type (struct (field (ref $f2)))))
  (rec (type $g (sub $f1 (func))) (type (struct)))
  (func $g (type $g)) (elem declare func $g)
  (func (export "run") (result i32)
    (ref.test (ref $f1) (ref.func $g))
  )
)
(assert_return (invoke "run") (i32.const 1))

(module
  (rec (type $f1 (sub (func))) (type $s1 (sub (struct (field (ref $f1))))))
  (rec (type $f2 (sub (func))) (type $s2 (sub (struct (field (ref $f2))))))
  (rec
    (type $g1 (sub $f1 (func)))
    (type (sub $s1 (struct (field (ref $f1) (ref $f1) (ref $f2) (ref $f2) (ref $g1)))))
  )
  (rec
    (type $g2 (sub $f2 (func)))
    (type (sub $s2 (struct (field (ref $f1) (ref $f2) (ref $f1) (ref $f2) (ref $g2)))))
  )
  (rec (type $h (sub $g2 (func))) (type (struct)))
  (func $h (type $h)) (elem declare func $h)
  (func (export "run") (result i32 i32)
    (ref.test (ref $f1) (ref.func $h))
    (ref.test (ref $g1) (ref.func $h))
  )
)
(assert_return (invoke "run") (i32.const 1) (i32.const 1))


(module
  (rec (type $f11 (sub (func (result (ref func))))) (type $f12 (sub $f11 (func (result (ref $f11))))))
  (rec (type $f21 (sub (func (result (ref func))))) (type $f22 (sub $f21 (func (result (ref $f21))))))
  (func $f11 (type $f11) (unreachable)) (elem declare func $f11)
  (func $f12 (type $f12) (unreachable)) (elem declare func $f12)
  (func (export "run") (result i32 i32 i32 i32)
    (ref.test (ref $f11) (ref.func $f11))
    (ref.test (ref $f21) (ref.func $f11))
    (ref.test (ref $f12) (ref.func $f12))
    (ref.test (ref $f22) (ref.func $f12))
  )
)
(assert_return (invoke "run")
  (i32.const 1) (i32.const 1) (i32.const 1) (i32.const 1)
)

(module
  (rec (type $f11 (sub (func (result (ref func))))) (type $f12 (sub $f11 (func (result (ref $f11))))))
  (rec (type $f21 (sub (func (result (ref func))))) (type $f22 (sub $f21 (func (result (ref $f21))))))
  (rec (type $g11 (sub $f11 (func (result (ref func))))) (type $g12 (sub $g11 (func (result (ref $g11))))))
  (rec (type $g21 (sub $f21 (func (result (ref func))))) (type $g22 (sub $g21 (func (result (ref $g21))))))
  (func $g11 (type $g11) (unreachable)) (elem declare func $g11)
  (func $g12 (type $g12) (unreachable)) (elem declare func $g12)
  (func (export "run") (result i32 i32 i32 i32 i32 i32 i32 i32)
    (ref.test (ref $f11) (ref.func $g11))
    (ref.test (ref $f21) (ref.func $g11))
    (ref.test (ref $f11) (ref.func $g12))
    (ref.test (ref $f21) (ref.func $g12))
    (ref.test (ref $g11) (ref.func $g11))
    (ref.test (ref $g21) (ref.func $g11))
    (ref.test (ref $g12) (ref.func $g12))
    (ref.test (ref $g22) (ref.func $g12))
  )
)
(assert_return (invoke "run")
  (i32.const 1) (i32.const 1) (i32.const 1) (i32.const 1)
  (i32.const 1) (i32.const 1) (i32.const 1) (i32.const 1)
)

(module
  (rec (type $f11 (sub (func))) (type $f12 (sub $f11 (func))))
  (rec (type $f21 (sub (func))) (type $f22 (sub $f11 (func))))
  (func $f (type $f21)) (elem declare func $f)
  (func (export "run") (result i32)
    (ref.test (ref $f11) (ref.func $f))
  )
)
(assert_return (invoke "run") (i32.const 0))

(module
  (rec (type $f01 (sub (func))) (type $f02 (sub $f01 (func))))
  (rec (type $f11 (sub (func))) (type $f12 (sub $f01 (func))))
  (rec (type $f21 (sub (func))) (type $f22 (sub $f11 (func))))
  (func $f (type $f21)) (elem declare func $f)
  (func (export "run") (result i32)
    (ref.test (ref $f11) (ref.func $f))
  )
)
(assert_return (invoke "run") (i32.const 0))



;; Linking

(module
  (type $t0 (sub (func (result (ref null func)))))
  (rec (type $t1 (sub $t0 (func (result (ref null $t1))))))
  (rec (type $t2 (sub $t1 (func (result (ref null $t2))))))

  (func (export "f0") (type $t0) (ref.null func))
  (func (export "f1") (type $t1) (ref.null $t1))
  (func (export "f2") (type $t2) (ref.null $t2))
)
(register "M")

(module
  (type $t0 (sub (func (result (ref null func)))))
  (rec (type $t1 (sub $t0 (func (result (ref null $t1))))))
  (rec (type $t2 (sub $t1 (func (result (ref null $t2))))))

  (func (import "M" "f0") (type $t0))
  (func (import "M" "f1") (type $t0))
  (func (import "M" "f1") (type $t1))
  (func (import "M" "f2") (type $t0))
  (func (import "M" "f2") (type $t1))
  (func (import "M" "f2") (type $t2))
)

(assert_unlinkable
  (module
    (type $t0 (sub (func (result (ref null func)))))
    (rec (type $t1 (sub $t0 (func (result (ref null $t1))))))
    (rec (type $t2 (sub $t1 (func (result (ref null $t2))))))
    (func (import "M" "f0") (type $t1))
  )
  "incompatible import type"
)

(assert_unlinkable
  (module
    (type $t0 (sub (func (result (ref null func)))))
    (rec (type $t1 (sub $t0 (func (result (ref null $t1))))))
    (rec (type $t2 (sub $t1 (func (result (ref null $t2))))))
    (func (import "M" "f0") (type $t2))
  )
  "incompatible import type"
)

(assert_unlinkable
  (module
    (type $t0 (sub (func (result (ref null func)))))
    (rec (type $t1 (sub $t0 (func (result (ref null $t1))))))
    (rec (type $t2 (sub $t1 (func (result (ref null $t2))))))
    (func (import "M" "f1") (type $t2))
  )
  "incompatible import type"
)

(module
  (type $t1 (sub (func)))
  (type $t2 (sub final (func)))
  (func (export "f1") (type $t1))
  (func (export "f2") (type $t2))
)
(register "M2")

(assert_unlinkable
  (module
    (type $t1 (sub (func)))
    (type $t2 (sub final (func)))
    (func (import "M2" "f1") (type $t2))
  )
  "incompatible import type"
)
(assert_unlinkable
  (module
    (type $t1 (sub (func)))
    (type $t2 (sub final (func)))
    (func (import "M2" "f2") (type $t1))
  )
  "incompatible import type"
)


(module
  (rec (type $f2 (sub (func))) (type (struct (field (ref $f2)))))
  (rec (type $g2 (sub $f2 (func))) (type (struct)))
  (func (export "g") (type $g2))
)
(register "M3")
(module
  (rec (type $f1 (sub (func))) (type (struct (field (ref $f1)))))
  (rec (type $g1 (sub $f1 (func))) (type (struct)))
  (func (import "M3" "g") (type $g1))
)

(module
  (rec (type $f1 (sub (func))) (type $s1 (sub (struct (field (ref $f1))))))
  (rec (type $f2 (sub (func))) (type $s2 (sub (struct (field (ref $f2))))))
  (rec
    (type $g2 (sub $f2 (func)))
    (type (sub $s2 (struct (field (ref $f1) (ref $f2) (ref $f1) (ref $f2) (ref $g2)))))
  )
  (func (export "g") (type $g2))
)
(register "M4")
(module
  (rec (type $f1 (sub (func))) (type $s1 (sub (struct (field (ref $f1))))))
  (rec (type $f2 (sub (func))) (type $s2 (sub (struct (field (ref $f2))))))
  (rec
    (type $g1 (sub $f1 (func)))
    (type (sub $s1 (struct (field (ref $f1) (ref $f1) (ref $f2) (ref $f2) (ref $g1)))))
  )
  (func (import "M4" "g") (type $g1))
)

(module
  (rec (type $f1 (sub (func))) (type (struct (field (ref $f1)))))
  (rec (type $f2 (sub (func))) (type (struct (field (ref $f1)))))
  (rec (type $g2 (sub $f2 (func))) (type (struct)))
  (func (export "g") (type $g2))
)
(register "M5")
(assert_unlinkable
  (module
    (rec (type $f1 (sub (func))) (type (struct (field (ref $f1)))))
    (rec (type $g1 (sub $f1 (func))) (type (struct)))
    (func (import "M5" "g") (type $g1))
  )
  "incompatible import"
)

(module
  (rec (type $f1 (sub (func))) (type (struct (field (ref $f1)))))
  (rec (type $f2 (sub (func))) (type (struct (field (ref $f2)))))
  (rec (type $g (sub $f1 (func))) (type (struct)))
  (func (export "g") (type $g))
)
(register "M6")
(module
  (rec (type $f1 (sub (func))) (type (struct (field (ref $f1)))))
  (rec (type $f2 (sub (func))) (type (struct (field (ref $f2)))))
  (rec (type $g (sub $f1 (func))) (type (struct)))
  (func (import "M6" "g") (type $f1))
)

(module
  (rec (type $f1 (sub (func))) (type $s1 (sub (struct (field (ref $f1))))))
  (rec (type $f2 (sub (func))) (type $s2 (sub (struct (field (ref $f2))))))
  (rec
    (type $g2 (sub $f2 (func)))
    (type (sub $s2 (struct (field (ref $f1) (ref $f2) (ref $f1) (ref $f2) (ref $g2)))))
  )
  (rec (type $h (sub $g2 (func))) (type (struct)))
  (func (export "h") (type $h))
)
(register "M7")
(module
  (rec (type $f1 (sub (func))) (type $s1 (sub (struct (field (ref $f1))))))
  (rec (type $f2 (sub (func))) (type $s2 (sub (struct (field (ref $f2))))))
  (rec
    (type $g1 (sub $f1 (func)))
    (type (sub $s1 (struct (field (ref $f1) (ref $f1) (ref $f2) (ref $f2) (ref $g1)))))
  )
  (rec (type $h (sub $g1 (func))) (type (struct)))
  (func (import "M7" "h") (type $f1))
  (func (import "M7" "h") (type $g1))
)


(module
  (rec (type $f11 (sub (func (result (ref func))))) (type $f12 (sub $f11 (func (result (ref $f11))))))
  (rec (type $f21 (sub (func (result (ref func))))) (type $f22 (sub $f21 (func (result (ref $f21))))))
  (func (export "f11") (type $f11) (unreachable))
  (func (export "f12") (type $f12) (unreachable))
)
(register "M8")
(module
  (rec (type $f11 (sub (func (result (ref func))))) (type $f12 (sub $f11 (func (result (ref $f11))))))
  (rec (type $f21 (sub (func (result (ref func))))) (type $f22 (sub $f21 (func (result (ref $f21))))))
  (func (import "M8" "f11") (type $f11))
  (func (import "M8" "f11") (type $f21))
  (func (import "M8" "f12") (type $f12))
  (func (import "M8" "f12") (type $f22))
)

(module
  (rec (type $f11 (sub (func (result (ref func))))) (type $f12 (sub $f11 (func (result (ref $f11))))))
  (rec (type $f21 (sub (func (result (ref func))))) (type $f22 (sub $f21 (func (result (ref $f21))))))
  (rec (type $g11 (sub $f11 (func (result (ref func))))) (type $g12 (sub $g11 (func (result (ref $g11))))))
  (rec (type $g21 (sub $f21 (func (result (ref func))))) (type $g22 (sub $g21 (func (result (ref $g21))))))
  (func (export "g11") (type $g11) (unreachable))
  (func (export "g12") (type $g12) (unreachable))
)
(register "M9")
(module
  (rec (type $f11 (sub (func (result (ref func))))) (type $f12 (sub $f11 (func (result (ref $f11))))))
  (rec (type $f21 (sub (func (result (ref func))))) (type $f22 (sub $f21 (func (result (ref $f21))))))
  (rec (type $g11 (sub $f11 (func (result (ref func))))) (type $g12 (sub $g11 (func (result (ref $g11))))))
  (rec (type $g21 (sub $f21 (func (result (ref func))))) (type $g22 (sub $g21 (func (result (ref $g21))))))
  (func (import "M9" "g11") (type $f11))
  (func (import "M9" "g11") (type $f21))
  (func (import "M9" "g12") (type $f11))
  (func (import "M9" "g12") (type $f21))
  (func (import "M9" "g11") (type $g11))
  (func (import "M9" "g11") (type $g21))
  (func (import "M9" "g12") (type $g12))
  (func (import "M9" "g12") (type $g22))
)

(module
  (rec (type $f11 (sub (func))) (type $f12 (sub $f11 (func))))
  (rec (type $f21 (sub (func))) (type $f22 (sub $f11 (func))))
  (func (export "f") (type $f21))
)
(register "M10")
(assert_unlinkable
  (module
    (rec (type $f11 (sub (func))) (type $f12 (sub $f11 (func))))
    (func (import "M10" "f") (type $f11))
  )
  "incompatible import"
)

(module
  (rec (type $f01 (sub (func))) (type $f02 (sub $f01 (func))))
  (rec (type $f11 (sub (func))) (type $f12 (sub $f01 (func))))
  (rec (type $f21 (sub (func))) (type $f22 (sub $f11 (func))))
  (func (export "f") (type $f21))
)
(register "M11")
(assert_unlinkable
  (module
    (rec (type $f01 (sub (func))) (type $f02 (sub $f01 (func))))
    (rec (type $f11 (sub (func))) (type $f12 (sub $f01 (func))))
    (func (import "M11" "f") (type $f11))
  )
  "incompatible import"
)



;; Finality violation

(assert_invalid
  (module
    (type $t (func))
    (type $s (sub $t (func)))
  )
  "sub type"
)

(assert_invalid
  (module
    (type $t (struct))
    (type $s (sub $t (struct)))
  )
  "sub type"
)

(assert_invalid
  (module
    (type $t (sub final (func)))
    (type $s (sub $t (func)))
  )
  "sub type"
)

(assert_invalid
  (module
    (type $t (sub (func)))
    (type $s (sub final $t (func)))
    (type $u (sub $s (func)))
  )
  "sub type"
)



;; Invalid subtyping definitions

(assert_invalid
  (module
    (type $a0 (sub (array i32)))
    (type $s0 (sub $a0 (struct)))
  )
  "sub type"
)

(assert_invalid
  (module
    (type $f0 (sub (func (param i32) (result i32))))
    (type $s0 (sub $f0 (struct)))
  )
  "sub type"
)

(assert_invalid
  (module
    (type $s0 (sub (struct)))
    (type $a0 (sub $s0 (array i32)))
  )
  "sub type"
)

(assert_invalid
  (module
    (type $f0 (sub (func (param i32) (result i32))))
    (type $a0 (sub $f0 (array i32)))
  )
  "sub type"
)

(assert_invalid
  (module
    (type $s0 (sub (struct)))
    (type $f0 (sub $s0 (func (param i32) (result i32))))
  )
  "sub type"
)

(assert_invalid
  (module
    (type $a0 (sub (array i32)))
    (type $f0 (sub $a0 (func (param i32) (result i32))))
  )
  "sub type"
)

(assert_invalid
  (module
    (type $a0 (sub (array i32)))
    (type $a1 (sub $a0 (array i64)))
  )
  "sub type"
)

(assert_invalid
  (module
    (type $s0 (sub (struct (field i32))))
    (type $s1 (sub $s0 (struct (field i64))))
  )
  "sub type"
)

(assert_invalid
  (module
    (type $f0 (sub (func)))
    (type $f1 (sub $f0 (func (param i32))))
  )
  "sub type"
)
