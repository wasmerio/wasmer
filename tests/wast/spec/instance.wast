;; Instantiation is generative

(module definition $M
  (global (export "glob") (mut i32) (i32.const 0))
  (table (export "tab") 10 funcref (ref.null func))
  (memory (export "mem") 1)
  (tag (export "tag"))
)

(module instance $I1 $M)
(module instance $I2 $M)
(register "I1" $I1)
(register "I2" $I2)

(module
  (import "I1" "glob" (global $glob1 (mut i32)))
  (import "I2" "glob" (global $glob2 (mut i32)))
  (import "I1" "tab" (table $tab1 10 funcref))
  (import "I2" "tab" (table $tab2 10 funcref))
  (import "I1" "mem" (memory $mem1 1))
  (import "I2" "mem" (memory $mem2 1))
  (import "I1" "tag" (tag $tag1))
  (import "I2" "tag" (tag $tag2))

  (func $f)
  (elem declare func $f)

  (func (export "glob") (result i32)
    (global.set $glob1 (i32.const 1))
    (global.get $glob2)
  )
  (func (export "tab") (result funcref)
    (table.set $tab1 (i32.const 0) (ref.func $f))
    (table.get $tab2 (i32.const 0))
  )
  (func (export "mem") (result i32)
    (i32.store $mem1 (i32.const 0) (i32.const 1))
    (i32.load $mem2 (i32.const 0))
  )
  (func (export "tag") (result i32)
    (block $on_tag1
      (block $on_other
        (try_table (catch $tag1 $on_tag1) (catch_all $on_other)
          (throw $tag2)
        )
        (unreachable)
      )
      (return (i32.const 0))
    )
    (return (i32.const 1))
  )
)

(assert_return (invoke "glob") (i32.const 0))
(assert_return (invoke "tab") (ref.null))
(assert_return (invoke "mem") (i32.const 0))
(assert_return (invoke "tag") (i32.const 0))


;; Import is not generative

(module
  (import "I1" "glob" (global $glob1 (mut i32)))
  (import "I1" "glob" (global $glob2 (mut i32)))
  (import "I1" "tab" (table $tab1 10 funcref))
  (import "I1" "tab" (table $tab2 10 funcref))
  (import "I1" "mem" (memory $mem1 1))
  (import "I1" "mem" (memory $mem2 1))
  (import "I1" "tag" (tag $tag1))
  (import "I1" "tag" (tag $tag2))

  (func $f)
  (elem declare func $f)

  (func (export "glob") (result i32)
    (global.set $glob1 (i32.const 1))
    (global.get $glob2)
  )
  (func (export "tab") (result funcref)
    (table.set $tab1 (i32.const 0) (ref.func $f))
    (table.get $tab2 (i32.const 0))
  )
  (func (export "mem") (result i32)
    (i32.store $mem1 (i32.const 0) (i32.const 1))
    (i32.load $mem2 (i32.const 0))
  )
  (func (export "tag") (result i32)
    (block $on_tag1
      (block $on_other
        (try_table (catch $tag1 $on_tag1) (catch_all $on_other)
          (throw $tag2)
        )
        (unreachable)
      )
      (return (i32.const 0))
    )
    (return (i32.const 1))
  )
)

(assert_return (invoke "glob") (i32.const 1))
(assert_return (invoke "tab") (ref.func))
(assert_return (invoke "mem") (i32.const 1))
(assert_return (invoke "tag") (i32.const 1))


;; Export is not generative

(module definition $N
  (global $glob (mut i32) (i32.const 0))
  (table $tab 10 funcref (ref.null func))
  (memory $mem 1)
  (tag $tag)

  (export "glob1" (global $glob))
  (export "glob2" (global $glob))
  (export "tab1" (table $tab))
  (export "tab2" (table $tab))
  (export "mem1" (memory $mem))
  (export "mem2" (memory $mem))
  (export "tag1" (tag $tag))
  (export "tag2" (tag $tag))
)

(module instance $I $N)
(register "I" $I)

(module
  (import "I" "glob1" (global $glob1 (mut i32)))
  (import "I" "glob2" (global $glob2 (mut i32)))
  (import "I" "tab1" (table $tab1 10 funcref))
  (import "I" "tab2" (table $tab2 10 funcref))
  (import "I" "mem1" (memory $mem1 1))
  (import "I" "mem2" (memory $mem2 1))
  (import "I" "tag1" (tag $tag1))
  (import "I" "tag2" (tag $tag2))

  (func $f)
  (elem declare func $f)

  (func (export "glob") (result i32)
    (global.set $glob1 (i32.const 1))
    (global.get $glob2)
  )
  (func (export "tab") (result funcref)
    (table.set $tab1 (i32.const 0) (ref.func $f))
    (table.get $tab2 (i32.const 0))
  )
  (func (export "mem") (result i32)
    (i32.store $mem1 (i32.const 0) (i32.const 1))
    (i32.load $mem2 (i32.const 0))
  )
  (func (export "tag") (result i32)
    (block $on_tag1
      (block $on_other
        (try_table (catch $tag1 $on_tag1) (catch_all $on_other)
          (throw $tag2)
        )
        (unreachable)
      )
      (return (i32.const 0))
    )
    (return (i32.const 1))
  )
)

(assert_return (invoke "glob") (i32.const 1))
(assert_return (invoke "tab") (ref.func))
(assert_return (invoke "mem") (i32.const 1))
(assert_return (invoke "tag") (i32.const 1))
