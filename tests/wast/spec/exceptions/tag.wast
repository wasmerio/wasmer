;; Test tag section

(module
  (tag)
  (tag (param i32))
  (tag (export "t2") (param i32))
  (tag $t3 (param i32 f32))
  (export "t3" (tag 3))
)

(register "test")

(module
  (tag $t0 (import "test" "t2") (param i32))
  (import "test" "t3" (tag $t1 (param i32 f32)))
)

(assert_invalid
  (module (tag (result i32)))
  "non-empty tag result type"
)
(assert_invalid
  (module (import "" "" (tag (result i32))))
  "non-empty tag result type"
)


;; Link-time typing

(module
  (rec
    (type $t1 (func))
    (type $t2 (func))
  )
  (tag (export "tag") (type $t1))
)

(register "M")

(module
  (rec
    (type $t1 (func))
    (type $t2 (func))
  )
  (tag (import "M" "tag") (type $t1))
)

(assert_unlinkable
  (module
    (rec
      (type $t1 (func))
      (type $t2 (func))
    )
    (tag (import "M" "tag") (type $t2))
  )
  "incompatible import type"
)

(assert_unlinkable
  (module
    (type $t (func))
    (tag (import "M" "tag") (type $t))
  )
  "incompatible import type"
)
