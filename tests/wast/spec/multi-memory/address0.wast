;; Load i32 data with different offset/align arguments

(module
  (memory $mem0 0)
  (memory $mem1 1)
  (data (memory $mem1) (i32.const 0) "abcdefghijklmnopqrstuvwxyz")

  (func (export "8u_good1") (param $i i32) (result i32)
    (i32.load8_u $mem1 offset=0 (local.get $i))                   ;; 97 'a'
  )
  (func (export "8u_good2") (param $i i32) (result i32)
    (i32.load8_u $mem1 align=1 (local.get $i))                    ;; 97 'a'
  )
  (func (export "8u_good3") (param $i i32) (result i32)
    (i32.load8_u $mem1 offset=1 align=1 (local.get $i))           ;; 98 'b'
  )
  (func (export "8u_good4") (param $i i32) (result i32)
    (i32.load8_u $mem1 offset=2 align=1 (local.get $i))           ;; 99 'c'
  )
  (func (export "8u_good5") (param $i i32) (result i32)
    (i32.load8_u $mem1 offset=25 align=1 (local.get $i))          ;; 122 'z'
  )

  (func (export "8s_good1") (param $i i32) (result i32)
    (i32.load8_s $mem1 offset=0 (local.get $i))                   ;; 97 'a'
  )
  (func (export "8s_good2") (param $i i32) (result i32)
    (i32.load8_s $mem1 align=1 (local.get $i))                    ;; 97 'a'
  )
  (func (export "8s_good3") (param $i i32) (result i32)
    (i32.load8_s $mem1 offset=1 align=1 (local.get $i))           ;; 98 'b'
  )
  (func (export "8s_good4") (param $i i32) (result i32)
    (i32.load8_s $mem1 offset=2 align=1 (local.get $i))           ;; 99 'c'
  )
  (func (export "8s_good5") (param $i i32) (result i32)
    (i32.load8_s $mem1 offset=25 align=1 (local.get $i))          ;; 122 'z'
  )

  (func (export "16u_good1") (param $i i32) (result i32)
    (i32.load16_u $mem1 offset=0 (local.get $i))                  ;; 25185 'ab'
  )
  (func (export "16u_good2") (param $i i32) (result i32)
    (i32.load16_u $mem1 align=1 (local.get $i))                   ;; 25185 'ab'
  )
  (func (export "16u_good3") (param $i i32) (result i32)
    (i32.load16_u $mem1 offset=1 align=1 (local.get $i))          ;; 25442 'bc'
  )
  (func (export "16u_good4") (param $i i32) (result i32)
    (i32.load16_u $mem1 offset=2 align=2 (local.get $i))          ;; 25699 'cd'
  )
  (func (export "16u_good5") (param $i i32) (result i32)
    (i32.load16_u $mem1 offset=25 align=2 (local.get $i))         ;; 122 'z\0'
  )

  (func (export "16s_good1") (param $i i32) (result i32)
    (i32.load16_s $mem1 offset=0 (local.get $i))                  ;; 25185 'ab'
  )
  (func (export "16s_good2") (param $i i32) (result i32)
    (i32.load16_s $mem1 align=1 (local.get $i))                   ;; 25185 'ab'
  )
  (func (export "16s_good3") (param $i i32) (result i32)
    (i32.load16_s $mem1 offset=1 align=1 (local.get $i))          ;; 25442 'bc'
  )
  (func (export "16s_good4") (param $i i32) (result i32)
    (i32.load16_s $mem1 offset=2 align=2 (local.get $i))          ;; 25699 'cd'
  )
  (func (export "16s_good5") (param $i i32) (result i32)
    (i32.load16_s $mem1 offset=25 align=2 (local.get $i))         ;; 122 'z\0'
  )

  (func (export "32_good1") (param $i i32) (result i32)
    (i32.load $mem1 offset=0 (local.get $i))                      ;; 1684234849 'abcd'
  )
  (func (export "32_good2") (param $i i32) (result i32)
    (i32.load $mem1 align=1 (local.get $i))                       ;; 1684234849 'abcd'
  )
  (func (export "32_good3") (param $i i32) (result i32)
    (i32.load $mem1 offset=1 align=1 (local.get $i))              ;; 1701077858 'bcde'
  )
  (func (export "32_good4") (param $i i32) (result i32)
    (i32.load $mem1 offset=2 align=2 (local.get $i))              ;; 1717920867 'cdef'
  )
  (func (export "32_good5") (param $i i32) (result i32)
    (i32.load $mem1 offset=25 align=4 (local.get $i))             ;; 122 'z\0\0\0'
  )

  (func (export "8u_bad") (param $i i32)
    (drop (i32.load8_u $mem1 offset=4294967295 (local.get $i)))
  )
  (func (export "8s_bad") (param $i i32)
    (drop (i32.load8_s $mem1 offset=4294967295 (local.get $i)))
  )
  (func (export "16u_bad") (param $i i32)
    (drop (i32.load16_u $mem1 offset=4294967295 (local.get $i)))
  )
  (func (export "16s_bad") (param $i i32)
    (drop (i32.load16_s $mem1 offset=4294967295 (local.get $i)))
  )
  (func (export "32_bad") (param $i i32)
    (drop (i32.load $mem1 offset=4294967295 (local.get $i)))
  )
)

(assert_return (invoke "8u_good1" (i32.const 0)) (i32.const 97))
(assert_return (invoke "8u_good2" (i32.const 0)) (i32.const 97))
(assert_return (invoke "8u_good3" (i32.const 0)) (i32.const 98))
(assert_return (invoke "8u_good4" (i32.const 0)) (i32.const 99))
(assert_return (invoke "8u_good5" (i32.const 0)) (i32.const 122))

(assert_return (invoke "8s_good1" (i32.const 0)) (i32.const 97))
(assert_return (invoke "8s_good2" (i32.const 0)) (i32.const 97))
(assert_return (invoke "8s_good3" (i32.const 0)) (i32.const 98))
(assert_return (invoke "8s_good4" (i32.const 0)) (i32.const 99))
(assert_return (invoke "8s_good5" (i32.const 0)) (i32.const 122))

(assert_return (invoke "16u_good1" (i32.const 0)) (i32.const 25185))
(assert_return (invoke "16u_good2" (i32.const 0)) (i32.const 25185))
(assert_return (invoke "16u_good3" (i32.const 0)) (i32.const 25442))
(assert_return (invoke "16u_good4" (i32.const 0)) (i32.const 25699))
(assert_return (invoke "16u_good5" (i32.const 0)) (i32.const 122))

(assert_return (invoke "16s_good1" (i32.const 0)) (i32.const 25185))
(assert_return (invoke "16s_good2" (i32.const 0)) (i32.const 25185))
(assert_return (invoke "16s_good3" (i32.const 0)) (i32.const 25442))
(assert_return (invoke "16s_good4" (i32.const 0)) (i32.const 25699))
(assert_return (invoke "16s_good5" (i32.const 0)) (i32.const 122))

(assert_return (invoke "32_good1" (i32.const 0)) (i32.const 1684234849))
(assert_return (invoke "32_good2" (i32.const 0)) (i32.const 1684234849))
(assert_return (invoke "32_good3" (i32.const 0)) (i32.const 1701077858))
(assert_return (invoke "32_good4" (i32.const 0)) (i32.const 1717920867))
(assert_return (invoke "32_good5" (i32.const 0)) (i32.const 122))

(assert_return (invoke "8u_good1" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "8u_good2" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "8u_good3" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "8u_good4" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "8u_good5" (i32.const 65507)) (i32.const 0))

(assert_return (invoke "8s_good1" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "8s_good2" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "8s_good3" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "8s_good4" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "8s_good5" (i32.const 65507)) (i32.const 0))

(assert_return (invoke "16u_good1" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "16u_good2" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "16u_good3" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "16u_good4" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "16u_good5" (i32.const 65507)) (i32.const 0))

(assert_return (invoke "16s_good1" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "16s_good2" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "16s_good3" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "16s_good4" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "16s_good5" (i32.const 65507)) (i32.const 0))

(assert_return (invoke "32_good1" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "32_good2" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "32_good3" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "32_good4" (i32.const 65507)) (i32.const 0))
(assert_return (invoke "32_good5" (i32.const 65507)) (i32.const 0))

(assert_return (invoke "8u_good1" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "8u_good2" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "8u_good3" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "8u_good4" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "8u_good5" (i32.const 65508)) (i32.const 0))

(assert_return (invoke "8s_good1" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "8s_good2" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "8s_good3" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "8s_good4" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "8s_good5" (i32.const 65508)) (i32.const 0))

(assert_return (invoke "16u_good1" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "16u_good2" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "16u_good3" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "16u_good4" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "16u_good5" (i32.const 65508)) (i32.const 0))

(assert_return (invoke "16s_good1" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "16s_good2" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "16s_good3" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "16s_good4" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "16s_good5" (i32.const 65508)) (i32.const 0))

(assert_return (invoke "32_good1" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "32_good2" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "32_good3" (i32.const 65508)) (i32.const 0))
(assert_return (invoke "32_good4" (i32.const 65508)) (i32.const 0))
(assert_trap (invoke "32_good5" (i32.const 65508)) "out of bounds memory access")

(assert_trap (invoke "8u_good3" (i32.const -1)) "out of bounds memory access")
(assert_trap (invoke "8s_good3" (i32.const -1)) "out of bounds memory access")
(assert_trap (invoke "16u_good3" (i32.const -1)) "out of bounds memory access")
(assert_trap (invoke "16s_good3" (i32.const -1)) "out of bounds memory access")
(assert_trap (invoke "32_good3" (i32.const -1)) "out of bounds memory access")
(assert_trap (invoke "32_good3" (i32.const -1)) "out of bounds memory access")

(assert_trap (invoke "8u_bad" (i32.const 0)) "out of bounds memory access")
(assert_trap (invoke "8s_bad" (i32.const 0)) "out of bounds memory access")
(assert_trap (invoke "16u_bad" (i32.const 0)) "out of bounds memory access")
(assert_trap (invoke "16s_bad" (i32.const 0)) "out of bounds memory access")
(assert_trap (invoke "32_bad" (i32.const 0)) "out of bounds memory access")

(assert_trap (invoke "8u_bad" (i32.const 1)) "out of bounds memory access")
(assert_trap (invoke "8s_bad" (i32.const 1)) "out of bounds memory access")
(assert_trap (invoke "16u_bad" (i32.const 1)) "out of bounds memory access")
(assert_trap (invoke "16s_bad" (i32.const 1)) "out of bounds memory access")
(assert_trap (invoke "32_bad" (i32.const 1)) "out of bounds memory access")
