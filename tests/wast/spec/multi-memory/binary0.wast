;; Unsigned LEB128 can have non-minimal length
(module binary
  "\00asm" "\01\00\00\00"
  "\05\07\02"                          ;; Memory section with 2 entries
  "\00\82\00"                          ;; no max, minimum 2
  "\00\82\00"                          ;; no max, minimum 2
)
(module binary
  "\00asm" "\01\00\00\00"
  "\05\13\03"                          ;; Memory section with 3 entries
  "\00\83\80\80\80\00"                 ;; no max, minimum 3
  "\00\84\80\80\80\00"                 ;; no max, minimum 4
  "\00\85\80\80\80\00"                 ;; no max, minimum 5
)

(module binary
  "\00asm" "\01\00\00\00"
  "\05\05\02"                          ;; Memory section with 2 entries
  "\00\00"                             ;; no max, minimum 0
  "\00\00"                             ;; no max, minimum 0
  "\0b\06\01"                          ;; Data section with 1 entry
  "\00"                                ;; Memory index 0
  "\41\00\0b\00"                       ;; (i32.const 0) with contents ""
)

(module binary
  "\00asm" "\01\00\00\00"
  "\05\05\02"                          ;; Memory section with 2 entries
  "\00\00"                             ;; no max, minimum 0
  "\00\01"                             ;; no max, minimum 1
  "\0b\07\01"                          ;; Data section with 1 entry
  "\02\01"                             ;; Memory index 1
  "\41\00\0b\00"                       ;; (i32.const 0) with contents ""
)

(module binary
  "\00asm" "\01\00\00\00"
  "\05\05\02"                          ;; Memory section with 2 entries
  "\00\00"                             ;; no max, minimum 0
  "\00\01"                             ;; no max, minimum 1
  "\0b\0a\01"                          ;; Data section with 1 entry
  "\02\81\80\80\00"                    ;; Memory index 1
  "\41\00\0b\00"                       ;; (i32.const 0) with contents ""
)

;; Unsigned LEB128 must not be overlong
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\05\10\02"                          ;; Memory section with 2 entries
    "\00\01"                             ;; no max, minimum 1
    "\00\82\80\80\80\80\80\80\80\80\80\80\00"  ;; no max, minimum 2 with one byte too many
  )
  "integer representation too long"
)

;; 2 memories declared, 1 given
(assert_malformed
  (module binary
      "\00asm" "\01\00\00\00"
      "\05\03\02"                           ;; memory section with inconsistent count (1 declared, 0 given)
      "\00\00"                              ;; memory 0 (missed)
      ;; "\00\00"                           ;; memory 1 (missing)
  )
  "unexpected end of section or function"
)

