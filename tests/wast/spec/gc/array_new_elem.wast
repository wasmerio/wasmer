;;;; Expression-style element segments.

(module
  (type $arr (array i31ref))

  (elem $e i31ref
    (ref.i31 (i32.const 0xaa))
    (ref.i31 (i32.const 0xbb))
    (ref.i31 (i32.const 0xcc))
    (ref.i31 (i32.const 0xdd)))

  (func (export "array-new-elem") (param i32 i32) (result (ref $arr))
    (array.new_elem $arr $e (local.get 0) (local.get 1))
  )
)

;; In-bounds element segment accesses.
(assert_return (invoke "array-new-elem" (i32.const 0) (i32.const 0)) (ref.array))
(assert_return (invoke "array-new-elem" (i32.const 0) (i32.const 4)) (ref.array))
(assert_return (invoke "array-new-elem" (i32.const 1) (i32.const 2)) (ref.array))
(assert_return (invoke "array-new-elem" (i32.const 4) (i32.const 0)) (ref.array))

;; Out-of-bounds element segment accesses.
(assert_trap (invoke "array-new-elem" (i32.const 0) (i32.const 5)) "out of bounds table access")
(assert_trap (invoke "array-new-elem" (i32.const 5) (i32.const 0)) "out of bounds table access")
(assert_trap (invoke "array-new-elem" (i32.const 1) (i32.const 4)) "out of bounds table access")
(assert_trap (invoke "array-new-elem" (i32.const 4) (i32.const 1)) "out of bounds table access")

(module
  (type $arr (array i31ref))

  (elem $e i31ref
    (ref.i31 (i32.const 0xaa))
    (ref.i31 (i32.const 0xbb))
    (ref.i31 (i32.const 0xcc))
    (ref.i31 (i32.const 0xdd)))

  (func (export "array-new-elem-contents") (result i32 i32)
    (local (ref $arr))
    (local.set 0 (array.new_elem $arr $e (i32.const 1) (i32.const 2)))
    (i31.get_u (array.get $arr (local.get 0) (i32.const 0)))
    (i31.get_u (array.get $arr (local.get 0) (i32.const 1)))
  )
)

;; Array is initialized with the correct contents.
(assert_return (invoke "array-new-elem-contents") (i32.const 0xbb) (i32.const 0xcc))

;;;; MVP-style function-index segments.

(module
  (type $arr (array funcref))

  (elem $e func $aa $bb $cc $dd)
  (func $aa (result i32) (i32.const 0xaa))
  (func $bb (result i32) (i32.const 0xbb))
  (func $cc (result i32) (i32.const 0xcc))
  (func $dd (result i32) (i32.const 0xdd))

  (func (export "array-new-elem") (param i32 i32) (result (ref $arr))
    (array.new_elem $arr $e (local.get 0) (local.get 1))
  )
)

;; In-bounds element segment accesses.
(assert_return (invoke "array-new-elem" (i32.const 0) (i32.const 0)) (ref.array))
(assert_return (invoke "array-new-elem" (i32.const 0) (i32.const 4)) (ref.array))
(assert_return (invoke "array-new-elem" (i32.const 1) (i32.const 2)) (ref.array))
(assert_return (invoke "array-new-elem" (i32.const 4) (i32.const 0)) (ref.array))

;; Out-of-bounds element segment accesses.
(assert_trap (invoke "array-new-elem" (i32.const 0) (i32.const 5)) "out of bounds table access")
(assert_trap (invoke "array-new-elem" (i32.const 5) (i32.const 0)) "out of bounds table access")
(assert_trap (invoke "array-new-elem" (i32.const 1) (i32.const 4)) "out of bounds table access")
(assert_trap (invoke "array-new-elem" (i32.const 4) (i32.const 1)) "out of bounds table access")

(module
  (type $f (func (result i32)))
  (type $arr (array funcref))

  (elem $e func $aa $bb $cc $dd)
  (func $aa (result i32) (i32.const 0xaa))
  (func $bb (result i32) (i32.const 0xbb))
  (func $cc (result i32) (i32.const 0xcc))
  (func $dd (result i32) (i32.const 0xdd))

  (table $t 2 2 funcref)

  (func (export "array-new-elem-contents") (result i32 i32)
    (local (ref $arr))
    (local.set 0 (array.new_elem $arr $e (i32.const 1) (i32.const 2)))

    (table.set $t (i32.const 0) (array.get $arr (local.get 0) (i32.const 0)))
    (table.set $t (i32.const 1) (array.get $arr (local.get 0) (i32.const 1)))

    (call_indirect (type $f) (i32.const 0))
    (call_indirect (type $f) (i32.const 1))

  )
)

;; Array is initialized with the correct contents.
(assert_return (invoke "array-new-elem-contents") (i32.const 0xbb) (i32.const 0xcc))
