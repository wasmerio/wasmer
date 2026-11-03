;; Table element type mismatch and externref table initialization tests.

(module
  (table $table 1 externref)
  (elem $segment externref (ref.null extern))

  (func (export "init")
    i32.const 0
    i32.const 0
    i32.const 1
    table.init $table $segment))

(assert_return (invoke "init"))

;; `table.grow` init value must match the table element type.

(assert_invalid
  (module
    (table $table 0 1 externref)
    (func (result i32)
      (table.grow $table (ref.null func) (i32.const 1))))
  "type mismatch")

(assert_invalid
  (module
    (table $table 0 1 funcref)
    (func (param externref) (result i32)
      (table.grow $table (local.get 0) (i32.const 1))))
  "type mismatch")

;; `table.set` value must match the table element type.

(assert_invalid
  (module
    (table $table 1 externref)
    (func
      (table.set $table (i32.const 0) (ref.null func))))
  "type mismatch")

(assert_invalid
  (module
    (table $table 1 funcref)
    (func (param externref)
      (table.set $table (i32.const 0) (local.get 0))))
  "type mismatch")

;; `table.copy` source and destination element types must match.

(assert_invalid
  (module
    (table $externrefs 1 externref)
    (table $funcrefs 1 funcref)
    (func
      (table.copy $externrefs $funcrefs
        (i32.const 0) (i32.const 0) (i32.const 1))))
  "type mismatch")
