;; Test table section structure

(module (table 0 funcref))
(module (table 1 funcref))
(module (table 0 0 funcref))
(module (table 0 1 funcref))
(module (table 1 256 funcref))
(module (table 0 65536 funcref))
(module (table 0 0xffff_ffff funcref))

(module (table 0 funcref) (table 0 funcref))
(module (table (import "spectest" "table") 0 funcref) (table 0 funcref))

(assert_invalid (module (elem (i32.const 0))) "unknown table")
(assert_invalid (module (elem (i32.const 0) $f) (func $f)) "unknown table")


(assert_invalid
  (module (table 1 0 funcref))
  "size minimum must not be greater than maximum"
)
(assert_invalid
  (module (table 0xffff_ffff 0 funcref))
  "size minimum must not be greater than maximum"
)

(assert_malformed
  (module quote "(table 0x1_0000_0000 funcref)")
  "i32 constant out of range"
)
(assert_malformed
  (module quote "(table 0x1_0000_0000 0x1_0000_0000 funcref)")
  "i32 constant out of range"
)
(assert_malformed
  (module quote "(table 0 0x1_0000_0000 funcref)")
  "i32 constant out of range"
)


;; Duplicate table identifiers

(assert_malformed (module quote
  "(table $foo 1 funcref)"
  "(table $foo 1 funcref)")
  "duplicate table")
(assert_malformed (module quote
  "(import \"\" \"\" (table $foo 1 funcref))"
  "(table $foo 1 funcref)")
  "duplicate table")
(assert_malformed (module quote
  "(import \"\" \"\" (table $foo 1 funcref))"
  "(import \"\" \"\" (table $foo 1 funcref))")
  "duplicate table")
