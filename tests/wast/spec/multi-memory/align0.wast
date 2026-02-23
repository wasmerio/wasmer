;; Test aligned and unaligned read/write

(module
  (memory $mem0 0)
  (memory $mem1 1)
  (memory $mem2 0)

  ;; $default: natural alignment, $1: align=1, $2: align=2, $4: align=4, $8: align=8

  (func (export "f32_align_switch") (param i32) (result f32)
    (local f32 f32)
    (local.set 1 (f32.const 10.0))
    (block $4
      (block $2
        (block $1
          (block $default
            (block $0
              (br_table $0 $default $1 $2 $4 (local.get 0))
            ) ;; 0
            (f32.store $mem1 (i32.const 0) (local.get 1))
            (local.set 2 (f32.load $mem1 (i32.const 0)))
            (br $4)
          ) ;; default
          (f32.store $mem1 align=1 (i32.const 0) (local.get 1))
          (local.set 2 (f32.load $mem1 align=1 (i32.const 0)))
          (br $4)
        ) ;; 1
        (f32.store $mem1 align=2 (i32.const 0) (local.get 1))
        (local.set 2 (f32.load $mem1 align=2 (i32.const 0)))
        (br $4)
      ) ;; 2
      (f32.store $mem1 align=4 (i32.const 0) (local.get 1))
      (local.set 2 (f32.load $mem1 align=4 (i32.const 0)))
    ) ;; 4
    (local.get 2)
  )
)

(assert_return (invoke "f32_align_switch" (i32.const 0)) (f32.const 10.0))
(assert_return (invoke "f32_align_switch" (i32.const 1)) (f32.const 10.0))
(assert_return (invoke "f32_align_switch" (i32.const 2)) (f32.const 10.0))
(assert_return (invoke "f32_align_switch" (i32.const 3)) (f32.const 10.0))

