;; Syntactic types (validation time)

;; Simple types.

(module
  (type $t1 (func (param f32 f32) (result f32)))
  (type $t2 (func (param $x f32) (param $y f32) (result f32)))

  (func $f1 (param $r (ref $t1)) (call $f2 (local.get $r)))
  (func $f2 (param $r (ref $t2)) (call $f1 (local.get $r)))
)


;; Indirect types.

(module
  (type $s0 (func (param i32) (result f32)))
  (type $s1 (func (param i32 (ref $s0)) (result (ref $s0))))
  (type $s2 (func (param i32 (ref $s0)) (result (ref $s0))))
  (type $t1 (func (param (ref $s1)) (result (ref $s2))))
  (type $t2 (func (param (ref $s2)) (result (ref $s1))))

  (func $f1 (param $r (ref $t1)) (call $f2 (local.get $r)))
  (func $f2 (param $r (ref $t2)) (call $f1 (local.get $r)))
)


;; Recursive types.

(module
  (rec (type $t1 (func (param i32 (ref $t1)))))
  (rec (type $t2 (func (param i32 (ref $t2)))))

  (func $f1 (param $r (ref $t1)) (call $f2 (local.get $r)))
  (func $f2 (param $r (ref $t2)) (call $f1 (local.get $r)))
)

(module
  (type $t1 (func (param i32 (ref $t1))))
  (type $t2 (func (param i32 (ref $t2))))

  (func $f1 (param $r (ref $t1)) (call $f2 (local.get $r)))
  (func $f2 (param $r (ref $t2)) (call $f1 (local.get $r)))
)


;; Isomorphic recursive types.

(module
  (rec
    (type $t0 (func (param i32 (ref $t1))))
    (type $t1 (func (param i32 (ref $t0))))
  )
  (rec
    (type $t2 (func (param i32 (ref $t3))))
    (type $t3 (func (param i32 (ref $t2))))
  )

  (func $f0 (param $r (ref $t0))
    (call $f2 (local.get $r))
  )
  (func $f1 (param $r (ref $t1))
    (call $f3 (local.get $r))
  )
  (func $f2 (param $r (ref $t2))
    (call $f0 (local.get $r))
  )
  (func $f3 (param $r (ref $t3))
    (call $f1 (local.get $r))
  )
)


;; Invalid recursion.

(assert_invalid
  (module
    (type $t1 (func (param (ref $t2))))
    (type $t2 (func (param (ref $t1))))
  )
  "unknown type"
)


;; Semantic types (run time)

;; Simple types.

(module
  (type $t1 (func (param f32 f32)))
  (type $t2 (func (param $x f32) (param $y f32)))

  (func $f1 (type $t1))
  (func $f2 (type $t2))
  (table funcref (elem $f1 $f2))

  (func (export "run")
    (call_indirect (type $t1) (f32.const 1) (f32.const 2) (i32.const 1))
    (call_indirect (type $t2) (f32.const 1) (f32.const 2) (i32.const 0))
  )
)
(assert_return (invoke "run"))


;; Indirect types.

(module
  (type $s0 (func (param i32)))
  (type $s1 (func (param i32 (ref $s0))))
  (type $s2 (func (param i32 (ref $s0))))
  (type $t1 (func (param (ref $s1))))
  (type $t2 (func (param (ref $s2))))

  (func $s1 (type $s1))
  (func $s2 (type $s2))
  (func $f1 (type $t1))
  (func $f2 (type $t2))
  (table funcref (elem $f1 $f2 $s1 $s2))

  (func (export "run")
    (call_indirect (type $t1) (ref.func $s1) (i32.const 0))
    (call_indirect (type $t1) (ref.func $s1) (i32.const 1))
    (call_indirect (type $t1) (ref.func $s2) (i32.const 0))
    (call_indirect (type $t1) (ref.func $s2) (i32.const 1))
    (call_indirect (type $t2) (ref.func $s1) (i32.const 0))
    (call_indirect (type $t2) (ref.func $s1) (i32.const 1))
    (call_indirect (type $t2) (ref.func $s2) (i32.const 0))
    (call_indirect (type $t2) (ref.func $s2) (i32.const 1))
  )
)
(assert_return (invoke "run"))


;; Recursive types.

(module
  (rec (type $t1 (func (result (ref null $t1)))))
  (rec (type $t2 (func (result (ref null $t2)))))

  (func $f1 (type $t1) (ref.null $t1))
  (func $f2 (type $t2) (ref.null $t2))
  (table funcref (elem $f1 $f2))

  (func (export "run")
    (block (result (ref null $t1)) (call_indirect (type $t1) (i32.const 0)))
    (block (result (ref null $t1)) (call_indirect (type $t2) (i32.const 0)))
    (block (result (ref null $t2)) (call_indirect (type $t1) (i32.const 0)))
    (block (result (ref null $t2)) (call_indirect (type $t2) (i32.const 0)))
    (block (result (ref null $t1)) (call_indirect (type $t1) (i32.const 1)))
    (block (result (ref null $t1)) (call_indirect (type $t2) (i32.const 1)))
    (block (result (ref null $t2)) (call_indirect (type $t1) (i32.const 1)))
    (block (result (ref null $t2)) (call_indirect (type $t2) (i32.const 1)))
    (br 0)
  )
)
(assert_return (invoke "run"))


;; Isomorphic recursive types.

(module
  (rec
    (type $t1 (func (param i32 (ref $t1))))
    (type $t2 (func (param i32 (ref $t3))))
    (type $t3 (func (param i32 (ref $t2))))
  )

  (rec
    (type $u1 (func (param i32 (ref $u1))))
    (type $u2 (func (param i32 (ref $u3))))
    (type $u3 (func (param i32 (ref $u2))))
  )

  (func $f1 (type $t1))
  (func $f2 (type $t2))
  (func $f3 (type $t3))
  (table funcref (elem $f1 $f2 $f3))

  (func (export "run")
    (call_indirect (type $t1) (i32.const 1) (ref.func $f1) (i32.const 0))
    (call_indirect (type $t2) (i32.const 1) (ref.func $f3) (i32.const 1))
    (call_indirect (type $t3) (i32.const 1) (ref.func $f2) (i32.const 2))
    (call_indirect (type $u1) (i32.const 1) (ref.func $f1) (i32.const 0))
    (call_indirect (type $u2) (i32.const 1) (ref.func $f3) (i32.const 1))
    (call_indirect (type $u3) (i32.const 1) (ref.func $f2) (i32.const 2))
  )
)
(assert_return (invoke "run"))


;; Semantic types (link time)

;; Simple types.

(module
  (type $t1 (func (param f32 f32) (result f32)))
  (func (export "f") (param (ref $t1)))
)
(register "M")
(module
  (type $t2 (func (param $x f32) (param $y f32) (result f32)))
  (func (import "M" "f") (param (ref $t2)))
)


;; Indirect types.

(module
  (type $s0 (func (param i32) (result f32)))
  (type $s1 (func (param i32 (ref $s0)) (result (ref $s0))))
  (type $s2 (func (param i32 (ref $s0)) (result (ref $s0))))
  (type $t1 (func (param (ref $s1)) (result (ref $s2))))
  (type $t2 (func (param (ref $s2)) (result (ref $s1))))
  (func (export "f1") (param (ref $t1)))
  (func (export "f2") (param (ref $t1)))
)
(register "N")
(module
  (type $s0 (func (param i32) (result f32)))
  (type $s1 (func (param i32 (ref $s0)) (result (ref $s0))))
  (type $s2 (func (param i32 (ref $s0)) (result (ref $s0))))
  (type $t1 (func (param (ref $s1)) (result (ref $s2))))
  (type $t2 (func (param (ref $s2)) (result (ref $s1))))
  (func (import "N" "f1") (param (ref $t1)))
  (func (import "N" "f1") (param (ref $t2)))
  (func (import "N" "f2") (param (ref $t1)))
  (func (import "N" "f2") (param (ref $t1)))
)


;; Recursive types.

(module
  (rec (type $t1 (func (param i32 (ref $t1)))))
  (func (export "f") (param (ref $t1)))
)
(register "Mr1")
(module
  (rec (type $t2 (func (param i32 (ref $t2)))))
  (func (import "Mr1" "f") (param (ref $t2)))
)


;; Isomorphic recursive types.

(module
  (rec
    (type $t1 (func (param i32 (ref $t1))))
    (type $t2 (func (param i32 (ref $t3))))
    (type $t3 (func (param i32 (ref $t2))))
  )
  (func (export "f1") (param (ref $t1)))
  (func (export "f2") (param (ref $t2)))
  (func (export "f3") (param (ref $t3)))
)
(register "Mr2")
(module
  (rec
    (type $t1 (func (param i32 (ref $t1))))
    (type $t2 (func (param i32 (ref $t3))))
    (type $t3 (func (param i32 (ref $t2))))
  )
  (func (import "Mr2" "f1") (param (ref $t1)))
  (func (import "Mr2" "f2") (param (ref $t2)))
  (func (import "Mr2" "f3") (param (ref $t3)))
)

(module
  (rec
    (type $t1 (func (param i32 (ref $t3))))
    (type $t2 (func (param i32 (ref $t1))))
    (type $t3 (func (param i32 (ref $t2))))
  )
  (func (export "f1") (param (ref $t1)))
  (func (export "f2") (param (ref $t2)))
  (func (export "f3") (param (ref $t3)))
)
(register "Mr3")
(module
  (rec
    (type $t1 (func (param i32 (ref $t3))))
    (type $t2 (func (param i32 (ref $t1))))
    (type $t3 (func (param i32 (ref $t2))))
  )
  (func (import "Mr3" "f1") (param (ref $t1)))
  (func (import "Mr3" "f2") (param (ref $t2)))
  (func (import "Mr3" "f3") (param (ref $t3)))
)

(module
  (rec
    (type $t1 (func (param i32 (ref $u1))))
    (type $u1 (func (param f32 (ref $t1))))
  )

  (rec
    (type $t2 (func (param i32 (ref $u3))))
    (type $u2 (func (param f32 (ref $t3))))
    (type $t3 (func (param i32 (ref $u2))))
    (type $u3 (func (param f32 (ref $t2))))
  )

  (func (export "f1") (param (ref $t1)))
  (func (export "f2") (param (ref $t2)))
  (func (export "f3") (param (ref $t3)))
)
(register "Mr4")
(module
  (rec
    (type $t1 (func (param i32 (ref $u1))))
    (type $u1 (func (param f32 (ref $t1))))
  )

  (rec
    (type $t2 (func (param i32 (ref $u3))))
    (type $u2 (func (param f32 (ref $t3))))
    (type $t3 (func (param i32 (ref $u2))))
    (type $u3 (func (param f32 (ref $t2))))
  )

  (func (import "Mr4" "f1") (param (ref $t1)))
  (func (import "Mr4" "f2") (param (ref $t2)))
  (func (import "Mr4" "f3") (param (ref $t3)))
)
