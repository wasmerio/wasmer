;; Test the element section

;; Syntax
(module
  (table $t 10 funcref)
  (func $f)
  (func $g)

  ;; Passive
  (elem funcref)
  (elem funcref (ref.func $f) (item ref.func $f) (item (ref.null func)) (ref.func $g))
  (elem func)
  (elem func $f $f $g $g)

  (elem $p1 funcref)
  (elem $p2 funcref (ref.func $f) (ref.func $f) (ref.null func) (ref.func $g))
  (elem $p3 func)
  (elem $p4 func $f $f $g $g)

  ;; Active
  (elem (table $t) (i32.const 0) funcref)
  (elem (table $t) (i32.const 0) funcref (ref.func $f) (ref.null func))
  (elem (table $t) (i32.const 0) func)
  (elem (table $t) (i32.const 0) func $f $g)
  (elem (table $t) (offset (i32.const 0)) funcref)
  (elem (table $t) (offset (i32.const 0)) func $f $g)
  (elem (table 0) (i32.const 0) func)
  (elem (table 0x0) (i32.const 0) func $f $f)
  (elem (table 0x000) (offset (i32.const 0)) func)
  (elem (table 0) (offset (i32.const 0)) func $f $f)
  (elem (table $t) (i32.const 0) func)
  (elem (table $t) (i32.const 0) func $f $f)
  (elem (table $t) (offset (i32.const 0)) func)
  (elem (table $t) (offset (i32.const 0)) func $f $f)
  (elem (offset (i32.const 0)))
  (elem (offset (i32.const 0)) funcref (ref.func $f) (ref.null func))
  (elem (offset (i32.const 0)) func $f $f)
  (elem (offset (i32.const 0)) $f $f)
  (elem (i32.const 0))
  (elem (i32.const 0) funcref (ref.func $f) (ref.null func))
  (elem (i32.const 0) func $f $f)
  (elem (i32.const 0) $f $f)
  (elem (i32.const 0) funcref (item (ref.func $f)) (item (ref.null func)))

  (elem $a1 (table $t) (i32.const 0) funcref)
  (elem $a2 (table $t) (i32.const 0) funcref (ref.func $f) (ref.null func))
  (elem $a3 (table $t) (i32.const 0) func)
  (elem $a4 (table $t) (i32.const 0) func $f $g)
  (elem $a9 (table $t) (offset (i32.const 0)) funcref)
  (elem $a10 (table $t) (offset (i32.const 0)) func $f $g)
  (elem $a11 (table 0) (i32.const 0) func)
  (elem $a12 (table 0x0) (i32.const 0) func $f $f)
  (elem $a13 (table 0x000) (offset (i32.const 0)) func)
  (elem $a14 (table 0) (offset (i32.const 0)) func $f $f)
  (elem $a15 (table $t) (i32.const 0) func)
  (elem $a16 (table $t) (i32.const 0) func $f $f)
  (elem $a17 (table $t) (offset (i32.const 0)) func)
  (elem $a18 (table $t) (offset (i32.const 0)) func $f $f)
  (elem $a19 (offset (i32.const 0)))
  (elem $a20 (offset (i32.const 0)) funcref (ref.func $f) (ref.null func))
  (elem $a21 (offset (i32.const 0)) func $f $f)
  (elem $a22 (offset (i32.const 0)) $f $f)
  (elem $a23 (i32.const 0))
  (elem $a24 (i32.const 0) funcref (ref.func $f) (ref.null func))
  (elem $a25 (i32.const 0) func $f $f)
  (elem $a26 (i32.const 0) $f $f)

  ;; Declarative
  (elem declare funcref)
  (elem declare funcref (ref.func $f) (ref.func $f) (ref.null func) (ref.func $g))
  (elem declare func)
  (elem declare func $f $f $g $g)

  (elem $d1 declare funcref)
  (elem $d2 declare funcref (ref.func $f) (ref.func $f) (ref.null func) (ref.func $g))
  (elem $d3 declare func)
  (elem $d4 declare func $f $f $g $g)
)

(module
  (func $f)
  (func $g)

  (table $t funcref (elem (ref.func $f) (ref.null func) (ref.func $g)))
)

(module
  (func $f)
  (func $g)

  (table $t 10 (ref func) (ref.func $f))
  (elem (i32.const 3) $g)
)


;; Basic use

(module
  (table 10 funcref)
  (func $f)
  (elem (i32.const 0) $f)
)
(module
  (import "spectest" "table" (table 10 funcref))
  (func $f)
  (elem (i32.const 0) $f)
)

(module
  (table 10 funcref)
  (func $f)
  (elem (i32.const 0) $f)
  (elem (i32.const 3) $f)
  (elem (i32.const 7) $f)
  (elem (i32.const 5) $f)
  (elem (i32.const 3) $f)
)
(module
  (import "spectest" "table" (table 10 funcref))
  (func $f)
  (elem (i32.const 9) $f)
  (elem (i32.const 3) $f)
  (elem (i32.const 7) $f)
  (elem (i32.const 3) $f)
  (elem (i32.const 5) $f)
)

(module
  (global (import "spectest" "global_i32") i32)
  (table 1000 funcref)
  (func $f)
  (elem (global.get 0) $f)
)

(module
  (global $g (import "spectest" "global_i32") i32)
  (table 1000 funcref)
  (func $f)
  (elem (global.get $g) $f)
)

(module
  (type $out-i32 (func (result i32)))
  (table 10 funcref)
  (elem (i32.const 7) $const-i32-a)
  (elem (i32.const 9) $const-i32-b)
  (func $const-i32-a (type $out-i32) (i32.const 65))
  (func $const-i32-b (type $out-i32) (i32.const 66))
  (func (export "call-7") (type $out-i32)
    (call_indirect (type $out-i32) (i32.const 7))
  )
  (func (export "call-9") (type $out-i32)
    (call_indirect (type $out-i32) (i32.const 9))
  )
)
(assert_return (invoke "call-7") (i32.const 65))
(assert_return (invoke "call-9") (i32.const 66))

;; Same as the above, but use ref.null to ensure the elements use exprs.
;; Note: some tools like wast2json avoid using exprs when possible.
(module
  (type $out-i32 (func (result i32)))
  (table 11 funcref)
  (elem (i32.const 6) funcref (ref.null func) (ref.func $const-i32-a))
  (elem (i32.const 9) funcref (ref.func $const-i32-b) (ref.null func))
  (func $const-i32-a (type $out-i32) (i32.const 65))
  (func $const-i32-b (type $out-i32) (i32.const 66))
  (func (export "call-7") (type $out-i32)
    (call_indirect (type $out-i32) (i32.const 7))
  )
  (func (export "call-9") (type $out-i32)
    (call_indirect (type $out-i32) (i32.const 9))
  )
)
(assert_return (invoke "call-7") (i32.const 65))
(assert_return (invoke "call-9") (i32.const 66))

(module
  (global i32 (i32.const 0))
  (table 1 funcref) (elem (global.get 0) $f) (func $f)
)
(module
  (global $g i32 (i32.const 0))
  (table 1 funcref) (elem (global.get $g) $f) (func $f)
)


;; Corner cases

(module
  (table 10 funcref)
  (func $f)
  (elem (i32.const 9) $f)
)
(module
  (import "spectest" "table" (table 10 funcref))
  (func $f)
  (elem (i32.const 9) $f)
)

(module
  (table 0 funcref)
  (elem (i32.const 0))
)
(module
  (import "spectest" "table" (table 0 funcref))
  (elem (i32.const 0))
)

(module
  (table 0 0 funcref)
  (elem (i32.const 0))
)

(module
  (table 20 funcref)
  (elem (i32.const 20))
)

(module
  (import "spectest" "table" (table 0 funcref))
  (func $f)
  (elem (i32.const 0) $f)
)

(module
  (import "spectest" "table" (table 0 100 funcref))
  (func $f)
  (elem (i32.const 0) $f)
)

(module
  (import "spectest" "table" (table 0 funcref))
  (func $f)
  (elem (i32.const 1) $f)
)

(module
  (import "spectest" "table" (table 0 30 funcref))
  (func $f)
  (elem (i32.const 1) $f)
)


;; Binary format variations

(module
  (func)
  (table 1 funcref)
  (elem (i32.const 0) func 0)
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\04\01"                ;; Table section: 1 table
    "\70\00\01"              ;; Table 0: [1..] funcref
  "\09\07\01"                ;; Elem section: 1 element segment
    "\00\41\00\0b\01\00"     ;; Segment 0: (i32.const 0) func 0
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)

(module
  (func)
  (table 1 funcref)
  (elem func 0)
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\04\01"                ;; Table section: 1 table
    "\70\00\01"              ;; Table 0: [1..] funcref
  "\09\05\01"                ;; Elem section: 1 element segment
    "\01\00\01\00"           ;; Segment 0: func 0
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)

(module
  (func)
  (table 1 funcref)
  (elem (table 0) (i32.const 0) func 0)
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\04\01"                ;; Table section: 1 table
    "\70\00\01"              ;; Table 0: [1..] funcref
  "\09\09\01"                ;; Elem section: 1 element segment
    "\02\00\41\00\0b\00\01\00"  ;; Segment 0: (table 0) (i32.const 0) func 0
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)

(module
  (func)
  (table 1 funcref)
  (elem declare func 0)
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\04\01"                ;; Table section: 1 table
    "\70\00\01"              ;; Table 0: [1..] funcref
  "\09\05\01"                ;; Elem section: 1 element segment
    "\03\00\01\00"           ;; Segment 0: declare func 0
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)

(module
  (func)
  (table 1 funcref)
  (elem (i32.const 0) (;;)(ref func) (ref.func 0))
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\04\01"                ;; Table section: 1 table
    "\70\00\01"              ;; Table 0: [1..] funcref
  "\09\09\01"                ;; Elem section: 1 element segment
    "\04\41\00\0b\01\d2\00\0b"  ;; Segment 0: (i32.const 0) (ref.func 0)
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)
(module
  (func)
  (table 1 funcref)
  (elem (i32.const 0) funcref (ref.null func))
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\04\01"                ;; Table section: 1 table
    "\70\00\01"              ;; Table 0: [1..] funcref
  "\09\09\01"                ;; Elem section: 1 element segment
    "\04\41\00\0b\01\d0\70\0b"  ;; Segment 0: (i32.const 0) (ref.null func)
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)

(module
  (func)
  (table 1 funcref)
  (elem (i32.const 0) funcref (ref.func 0))
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\04\01"                ;; Table section: 1 table
    "\70\00\01"              ;; Table 0: [1..] funcref
  "\09\07\01"                ;; Elem section: 1 element segment
    "\05\70\01\d2\00\0b"     ;; Segment 0: funcref (ref.func 0)
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)
(module
  (func)
  (table 1 funcref)
  (elem (i32.const 0) funcref (ref.null func))
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\04\01"                ;; Table section: 1 table
    "\70\00\01"              ;; Table 0: [1..] funcref
  "\09\07\01"                ;; Elem section: 1 element segment
    "\05\70\01\d0\70\0b"     ;; Segment 0: funcref (ref.null func)
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)

(module
  (func)
  (table 1 funcref)
  (elem (table 0) (i32.const 0) funcref (ref.func 0))
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\04\01"                ;; Table section: 1 table
    "\70\00\01"              ;; Table 0: [1..] funcref
  "\09\0b\01"                ;; Elem section: 1 element segment
    "\06\00\41\00\0b\70\01\d2\00\0b"  ;; Segment 0: (table 0) (i32.const 0) funcref (ref.func 0)
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)
(module
  (func)
  (table 1 funcref)
  (elem (table 0) (i32.const 0) funcref (ref.null func))
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\04\01"                ;; Table section: 1 table
    "\70\00\01"              ;; Table 0: [1..] funcref
  "\09\0b\01"                ;; Elem section: 1 element segment
    "\06\00\41\00\0b\70\01\d0\70\0b"  ;; Segment 0: (table 0) (i32.const 0) funcref (ref.null func)
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)

(module
  (func)
  (table 1 funcref)
  (elem declare funcref (ref.func 0))
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\04\01"                ;; Table section: 1 table
    "\70\00\01"              ;; Table 0: [1..] funcref
  "\09\07\01"                ;; Elem section: 1 element segment
    "\07\70\01\d2\00\0b"     ;; Segment 0: declare funcref (ref.func 0)
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)
(module
  (func)
  (table 1 funcref)
  (elem declare funcref (ref.null func))
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\04\01"                ;; Table section: 1 table
    "\70\00\01"              ;; Table 0: [1..] funcref
  "\09\07\01"                ;; Elem section: 1 element segment
    "\07\70\01\d0\70\0b"     ;; Segment 0: declare funcref (ref.null func)
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)


(module
  (func)
  (table 1 (ref func) (ref.func 0))
  (elem (i32.const 0) func 0)
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\0a\01"                ;; Table section: 1 table
    "\40\00\64\70\00\01\d2\00\0b"  ;; Table 0: [1..] (ref func) (ref.func 0)
  "\09\07\01"                ;; Elem section: 1 element segment
    "\00\41\00\0b\01\00"     ;; Segment 0: (i32.const 0) func 0
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)

(module
  (func)
  (table 1 (ref func) (ref.func 0))
  (elem func 0)
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\0a\01"                ;; Table section: 1 table
    "\40\00\64\70\00\01\d2\00\0b"  ;; Table 0: [1..] (ref func) (ref.func 0)
  "\09\05\01"                ;; Elem section: 1 element segment
    "\01\00\01\00"           ;; Segment 0: func 0
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)

(module
  (func)
  (table 1 (ref func) (ref.func 0))
  (elem (table 0) (i32.const 0) func 0)
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\0a\01"                ;; Table section: 1 table
    "\40\00\64\70\00\01\d2\00\0b"  ;; Table 0: [1..] (ref func) (ref.func 0)
  "\09\09\01"                ;; Elem section: 1 element segment
    "\02\00\41\00\0b\00\01\00"  ;; Segment 0: (table 0) (i32.const 0) func 0
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)

(module
  (func)
  (table 1 (ref func) (ref.func 0))
  (elem declare func 0)
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\0a\01"                ;; Table section: 1 table
    "\40\00\64\70\00\01\d2\00\0b"  ;; Table 0: [1..] (ref func) (ref.func 0)
  "\09\05\01"                ;; Elem section: 1 element segment
    "\03\00\01\00"           ;; Segment 0: declare func 0
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)

(assert_invalid
  (module
    (func)
    (table 1 (ref func) (ref.func 0))
    (elem (i32.const 0) funcref (ref.func 0))
  )
  "type mismatch"
)
(assert_invalid
  (module binary
    "\00asm" "\01\00\00\00"    ;; Magic
    "\01\04\01\60\00\00"       ;; Type section: 1 type
    "\03\02\01\00"             ;; Function section: 1 function
    "\04\0a\01"                ;; Table section: 1 table
      "\40\00\64\70\00\01\d2\00\0b"  ;; Table 0: [1..] (ref func) (ref.func 0)
    "\09\09\01"                ;; Elem section: 1 element segment
      "\04\41\00\0b\01\d2\00\0b"  ;; Segment 0: (i32.const 0) (ref.func 0)
    "\0a\04\01"                ;; Code section: 1 function
      "\02\00\0b"              ;; Function 0: empty
  )
  "type mismatch"
)

(module
  (func)
  (table 1 (ref func) (ref.func 0))
  (elem (ref func) (ref.func 0))
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\0a\01"                ;; Table section: 1 table
    "\40\00\64\70\00\01\d2\00\0b"  ;; Table 0: [1..] (ref func) (ref.func 0)
  "\09\08\01"                ;; Elem section: 1 element segment
    "\05\64\70\01\d2\00\0b"  ;; Segment 0: (ref func) (ref.func 0)
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)

(module
  (func)
  (table 1 (ref func) (ref.func 0))
  (elem (table 0) (i32.const 0) (ref func) (ref.func 0))
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\0a\01"                ;; Table section: 1 table
    "\40\00\64\70\00\01\d2\00\0b"  ;; Table 0: [1..] (ref func) (ref.func 0)
  "\09\0c\01"                ;; Elem section: 1 element segment
    "\06\00\41\00\0b\64\70\01\d2\00\0b"  ;; Segment 0: (table 0) (i32.const 0) (ref func) (ref.func 0)
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)

(module
  (func)
  (table 1 (ref func) (ref.func 0))
  (elem declare (ref func) (ref.func 0))
)
(module binary
  "\00asm" "\01\00\00\00"    ;; Magic
  "\01\04\01\60\00\00"       ;; Type section: 1 type
  "\03\02\01\00"             ;; Function section: 1 function
  "\04\0a\01"                ;; Table section: 1 table
    "\40\00\64\70\00\01\d2\00\0b"  ;; Table 0: [1..] (ref func) (ref.func 0)
  "\09\08\01"                ;; Elem section: 1 element segment
    "\07\64\70\01\d2\00\0b"  ;; Segment 0: declare (ref func) (ref.func 0)
  "\0a\04\01"                ;; Code section: 1 function
    "\02\00\0b"              ;; Function 0: empty
)


;; Invalid bounds for elements

(assert_trap
  (module
    (table 0 funcref)
    (func $f)
    (elem (i32.const 0) $f)
  )
  "out of bounds table access"
)

(assert_trap
  (module
    (table 0 0 funcref)
    (func $f)
    (elem (i32.const 0) $f)
  )
  "out of bounds table access"
)

(assert_trap
  (module
    (table 0 1 funcref)
    (func $f)
    (elem (i32.const 0) $f)
  )
  "out of bounds table access"
)

(assert_trap
  (module
    (table 0 funcref)
    (elem (i32.const 1))
  )
  "out of bounds table access"
)
(assert_trap
  (module
    (table 10 funcref)
    (func $f)
    (elem (i32.const 10) $f)
  )
  "out of bounds table access"
)
(assert_trap
  (module
    (import "spectest" "table" (table 10 funcref))
    (func $f)
    (elem (i32.const 10) $f)
  )
  "out of bounds table access"
)

(assert_trap
  (module
    (table 10 20 funcref)
    (func $f)
    (elem (i32.const 10) $f)
  )
  "out of bounds table access"
)
(assert_trap
  (module
    (import "spectest" "table" (table 10 funcref))
    (func $f)
    (elem (i32.const 10) $f)
  )
  "out of bounds table access"
)

(assert_trap
  (module
    (table 10 funcref)
    (func $f)
    (elem (i32.const -1) $f)
  )
  "out of bounds table access"
)
(assert_trap
  (module
    (import "spectest" "table" (table 10 funcref))
    (func $f)
    (elem (i32.const -1) $f)
  )
  "out of bounds table access"
)

(assert_trap
  (module
    (table 10 funcref)
    (func $f)
    (elem (i32.const -10) $f)
  )
  "out of bounds table access"
)
(assert_trap
  (module
    (import "spectest" "table" (table 10 funcref))
    (func $f)
    (elem (i32.const -10) $f)
  )
  "out of bounds table access"
)


;; Implicitly dropped elements

(module
  (table 10 funcref)
  (elem $e (i32.const 0) func $f)
  (func $f)
  (func (export "init")
    (table.init $e (i32.const 0) (i32.const 0) (i32.const 1))
  )
)
(assert_trap (invoke "init") "out of bounds table access")

(module
  (table 10 funcref)
  (elem $e declare func $f)
  (func $f)
  (func (export "init")
    (table.init $e (i32.const 0) (i32.const 0) (i32.const 1))
  )
)
(assert_trap (invoke "init") "out of bounds table access")


;; Element without table

(assert_invalid
  (module
    (func $f)
    (elem (i32.const 0) $f)
  )
  "unknown table"
)


;; Invalid offsets

(assert_invalid
  (module
    (table 1 funcref)
    (elem (i64.const 0))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (table 1 funcref)
    (elem (ref.null func))
  )
  "type mismatch"
)

(assert_invalid
  (module 
    (table 1 funcref)
    (elem (offset (;empty instruction sequence;)))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (table 1 funcref)
    (elem (offset (i32.const 0) (i32.const 0)))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (global (import "test" "global-i32") i32)
    (table 1 funcref)
    (elem (offset (global.get 0) (global.get 0)))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (global (import "test" "global-i32") i32)
    (table 1 funcref)
    (elem (offset (global.get 0) (i32.const 0)))
  )
  "type mismatch"
)


(assert_invalid
  (module
    (table 1 funcref)
    (elem (i32.ctz (i32.const 0)))
  )
  "constant expression required"
)

(assert_invalid
  (module
    (table 1 funcref)
    (elem (nop))
  )
  "constant expression required"
)

(assert_invalid
  (module
    (table 1 funcref)
    (elem (offset (nop) (i32.const 0)))
  )
  "constant expression required"
)

(assert_invalid
  (module
    (table 1 funcref)
    (elem (offset (i32.const 0) (nop)))
  )
  "constant expression required"
)

(assert_invalid
  (module
    (global $g (import "test" "g") (mut i32))
    (table 1 funcref)
    (elem (global.get $g))
  )
  "constant expression required"
)

(assert_invalid
   (module 
     (table 1 funcref)
     (elem (global.get 0))
   )
   "unknown global 0"
)

(assert_invalid
   (module
     (global (import "test" "global-i32") i32)
     (table 1 funcref)
     (elem (global.get 1))
   )
   "unknown global 1"
)

(assert_invalid
   (module 
     (global (import "test" "global-mut-i32") (mut i32))
     (table 1 funcref)
     (elem (global.get 0))
   )
   "constant expression required"
)


;; Invalid elements

(assert_invalid
  (module
    (table 1 funcref)
    (elem (i32.const 0) funcref (ref.null extern))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (table 1 funcref)
    (elem (i32.const 0) funcref (item (ref.null func) (ref.null func)))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (table 1 funcref)
    (elem (i32.const 0) funcref (i32.const 0))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (table 1 funcref)
    (elem (i32.const 0) funcref (item (i32.const 0)))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (table 1 funcref)
    (elem (i32.const 0) funcref (item (call $f)))
    (func $f (result funcref) (ref.null func))
  )
  "constant expression required"
)

(assert_invalid
  (module
    (table 1 funcref)
    (elem (i32.const 0) funcref (item (i32.add (i32.const 0) (i32.const 1))))
  )
  "constant expression required"
)


;; Two elements target the same slot

(module
  (type $out-i32 (func (result i32)))
  (table 10 funcref)
  (elem (i32.const 9) $const-i32-a)
  (elem (i32.const 9) $const-i32-b)
  (func $const-i32-a (type $out-i32) (i32.const 65))
  (func $const-i32-b (type $out-i32) (i32.const 66))
  (func (export "call-overwritten") (type $out-i32)
    (call_indirect (type $out-i32) (i32.const 9))
  )
)
(assert_return (invoke "call-overwritten") (i32.const 66))

(module
  (type $out-i32 (func (result i32)))
  (import "spectest" "table" (table 10 funcref))
  (elem (i32.const 9) $const-i32-a)
  (elem (i32.const 9) $const-i32-b)
  (func $const-i32-a (type $out-i32) (i32.const 65))
  (func $const-i32-b (type $out-i32) (i32.const 66))
  (func (export "call-overwritten-element") (type $out-i32)
    (call_indirect (type $out-i32) (i32.const 9))
  )
)
(assert_return (invoke "call-overwritten-element") (i32.const 66))


;; Element sections across multiple modules change the same table

(module $module1
  (type $out-i32 (func (result i32)))
  (table (export "shared-table") 10 funcref)
  (elem (i32.const 8) $const-i32-a)
  (elem (i32.const 9) $const-i32-b)
  (func $const-i32-a (type $out-i32) (i32.const 65))
  (func $const-i32-b (type $out-i32) (i32.const 66))
  (func (export "call-7") (type $out-i32)
    (call_indirect (type $out-i32) (i32.const 7))
  )
  (func (export "call-8") (type $out-i32)
    (call_indirect (type $out-i32) (i32.const 8))
  )
  (func (export "call-9") (type $out-i32)
    (call_indirect (type $out-i32) (i32.const 9))
  )
)

(register "module1" $module1)

(assert_trap (invoke $module1 "call-7") "uninitialized element")
(assert_return (invoke $module1 "call-8") (i32.const 65))
(assert_return (invoke $module1 "call-9") (i32.const 66))

(module $module2
  (type $out-i32 (func (result i32)))
  (import "module1" "shared-table" (table 10 funcref))
  (elem (i32.const 7) $const-i32-c)
  (elem (i32.const 8) $const-i32-d)
  (func $const-i32-c (type $out-i32) (i32.const 67))
  (func $const-i32-d (type $out-i32) (i32.const 68))
)

(assert_return (invoke $module1 "call-7") (i32.const 67))
(assert_return (invoke $module1 "call-8") (i32.const 68))
(assert_return (invoke $module1 "call-9") (i32.const 66))

(module $module3
  (type $out-i32 (func (result i32)))
  (import "module1" "shared-table" (table 10 funcref))
  (elem (i32.const 8) $const-i32-e)
  (elem (i32.const 9) $const-i32-f)
  (func $const-i32-e (type $out-i32) (i32.const 69))
  (func $const-i32-f (type $out-i32) (i32.const 70))
)

(assert_return (invoke $module1 "call-7") (i32.const 67))
(assert_return (invoke $module1 "call-8") (i32.const 69))
(assert_return (invoke $module1 "call-9") (i32.const 70))

;; Element segments must match element type of table

(assert_invalid
  (module (func $f) (table 1 externref) (elem (i32.const 0) $f))
  "type mismatch"
)

(assert_invalid
  (module (table 1 funcref) (elem (i32.const 0) externref (ref.null extern)))
  "type mismatch"
)

(assert_invalid
  (module
    (func $f)
    (table $t 1 externref)
    (elem $e funcref (ref.func $f))
    (func (table.init $t $e (i32.const 0) (i32.const 0) (i32.const 1))))
  "type mismatch"
)

(assert_invalid
  (module
    (table $t 1 funcref)
    (elem $e externref (ref.null extern))
    (func (table.init $t $e (i32.const 0) (i32.const 0) (i32.const 1))))
  "type mismatch"
)

;; Initializing a table with an externref-type element segment

(module $m
  (table $t (export "table") 2 externref)
  (func (export "get") (param $i i32) (result externref)
        (table.get $t (local.get $i)))
  (func (export "set") (param $i i32) (param $x externref)
        (table.set $t (local.get $i) (local.get $x))))

(register "exporter" $m)

(assert_return (invoke $m "get" (i32.const 0)) (ref.null extern))
(assert_return (invoke $m "get" (i32.const 1)) (ref.null extern))

(assert_return (invoke $m "set" (i32.const 0) (ref.extern 42)))
(assert_return (invoke $m "set" (i32.const 1) (ref.extern 137)))

(assert_return (invoke $m "get" (i32.const 0)) (ref.extern 42))
(assert_return (invoke $m "get" (i32.const 1)) (ref.extern 137))

(module
  (import "exporter" "table" (table $t 2 externref))
  (elem (i32.const 0) externref (ref.null extern)))

(assert_return (invoke $m "get" (i32.const 0)) (ref.null extern))
(assert_return (invoke $m "get" (i32.const 1)) (ref.extern 137))

;; Initializing a table with imported funcref global

(module $module4
  (func (result i32)
    i32.const 42
  )
  (global (export "f") funcref (ref.func 0))
)

(register "module4" $module4)

(module
  (import "module4" "f" (global funcref))
  (type $out-i32 (func (result i32)))
  (table 10 funcref)
  (elem (offset (i32.const 0)) funcref (global.get 0))
  (func (export "call_imported_elem") (type $out-i32)
    (call_indirect (type $out-i32) (i32.const 0))
  )
)

(assert_return (invoke "call_imported_elem") (i32.const 42))
