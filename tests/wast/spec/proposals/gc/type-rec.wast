;; Static matching of recursive types

(module
  (rec (type $f1 (func)) (type (struct (field (ref $f1)))))
  (rec (type $f2 (func)) (type (struct (field (ref $f2)))))
  (func $f (type $f2))
  (global (ref $f1) (ref.func $f))
)

(module
  (rec (type $f1 (func)) (type (struct (field (ref $f1)))))
  (rec (type $f2 (func)) (type (struct (field (ref $f2)))))
  (rec
    (type $g1 (func))
    (type (struct (field (ref $f1) (ref $f1) (ref $f2) (ref $f2) (ref $g1))))
  )
  (rec
    (type $g2 (func))
    (type (struct (field (ref $f1) (ref $f2) (ref $f1) (ref $f2) (ref $g2))))
  )
  (func $g (type $g2))
  (global (ref $g1) (ref.func $g))
)

(assert_invalid
  (module
    (rec (type $f1 (func)) (type (struct (field (ref $f1)))))
    (rec (type $f2 (func)) (type (struct (field (ref $f1)))))
    (func $f (type $f2))
    (global (ref $f1) (ref.func $f))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (rec (type $f0 (func)) (type (struct (field (ref $f0)))))
    (rec (type $f1 (func)) (type (struct (field (ref $f0)))))
    (rec (type $f2 (func)) (type (struct (field (ref $f1)))))
    (func $f (type $f2))
    (global (ref $f1) (ref.func $f))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (rec (type $f1 (func)) (type (struct)))
    (rec (type (struct)) (type $f2 (func)))
    (global (ref $f1) (ref.func $f))
    (func $f (type $f2))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (rec (type $f1 (func)) (type (struct)))
    (rec (type $f2 (func)) (type (struct)) (type (func)))
    (global (ref $f1) (ref.func $f))
    (func $f (type $f2))
  )
  "type mismatch"
)


;; Link-time matching of recursive function types

(module $M
  (rec (type $f1 (func)) (type (struct)))
  (func (export "f") (type $f1))
)
(register "M" $M)

(module
  (rec (type $f2 (func)) (type (struct)))
  (func (import "M" "f") (type $f2))
)

(assert_unlinkable
  (module
    (rec (type (struct)) (type $f2 (func)))
    (func (import "M" "f") (type $f2))
  )
  "incompatible import type"
)

(assert_unlinkable
  (module
    (rec (type $f2 (func)))
    (func (import "M" "f") (type $f2))
  )
  "incompatible import type"
)


;; Dynamic matching of recursive function types

(module
  (rec (type $f1 (func)) (type (struct)))
  (rec (type $f2 (func)) (type (struct)))
  (table funcref (elem $f1))
  (func $f1 (type $f1))
  (func (export "run") (call_indirect (type $f2) (i32.const 0)))
)
(assert_return (invoke "run"))

(module
  (rec (type $f1 (func)) (type (struct)))
  (rec (type (struct)) (type $f2 (func)))
  (table funcref (elem $f1))
  (func $f1 (type $f1))
  (func (export "run") (call_indirect (type $f2) (i32.const 0)))
)
(assert_trap (invoke "run") "indirect call type mismatch")

(module
  (rec (type $f1 (func)) (type (struct)))
  (rec (type $f2 (func)))
  (table funcref (elem $f1))
  (func $f1 (type $f1))
  (func (export "run") (call_indirect (type $f2) (i32.const 0)))
)
(assert_trap (invoke "run") "indirect call type mismatch")


;; Implicit function types never pick up non-singleton recursive types

(module
  (rec (type $s (struct)))
  (rec (type $t (func (param (ref $s)))))
  (func $f (param (ref $s)))  ;; okay, type is equivalent to $t
  (global (ref $t) (ref.func $f))
)

(assert_invalid
  (module
    (rec
      (type $s (struct))
      (type $t (func (param (ref $s))))
    )
    (func $f (param (ref $s)))  ;; type is not equivalent to $t
    (global (ref $t) (ref.func $f))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (rec
      (type (struct))
      (type $t (func))
    )
    (func $f)  ;; type is not equivalent to $t
    (global (ref $t) (ref.func $f))
  )
  "type mismatch"
)
