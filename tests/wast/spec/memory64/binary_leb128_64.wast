(module binary
  "\00asm" "\01\00\00\00"
  "\01\04\01\60\00\00"             ;; Type section
  "\03\02\01\00"                   ;; Function section
  "\05\03\01\04\00"                ;; Memory section (flags: i64)
  "\0a\13\01"                      ;; Code section
  ;; function 0
  "\11\00"                         ;; local type count
  "\42\00"                         ;; i64.const 0
  "\28"                            ;; i32.load
  "\02"                            ;; alignment 2
  "\ff\ff\ff\ff\ff\ff\ff\ff\ff\01" ;; offset 2^64 - 1
  "\1a"                            ;; drop
  "\0b"                            ;; end
)
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"             ;; Type section
    "\03\02\01\00"                   ;; Function section
    "\05\03\01\04\00"                ;; Memory section (flags: i64)
    "\0a\13\01"                      ;; Code section
    ;; function 0
    "\11\00"                         ;; local type count
    "\42\00"                         ;; i64.const 0
    "\28"                            ;; i32.load
    "\02"                            ;; alignment 2
    "\ff\ff\ff\ff\ff\ff\ff\ff\ff\02" ;; offset 2^64 (one unused bit set)
    "\1a"                            ;; drop
    "\0b"                            ;; end
  )
  "integer too large"
)
