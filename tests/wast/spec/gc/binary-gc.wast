(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01"                     ;; Type section id
    "\04"                     ;; Type section length
    "\01"                     ;; Types vector length
    "\5e"                     ;; Array type, -0x22
    "\78"                     ;; Storage type: i8 or -0x08
    "\02"                     ;; Mutability, should be 0 or 1, but isn't
  )
  "malformed mutability"
)
