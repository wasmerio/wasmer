(module
  (func $fg) (func (call $fg))
  (func $03) (func (call $03))
  (func $!?@#a$%^&*b-+_.:9'`|/\<=>~) (func (call $!?@#a$%^&*b-+_.:9'`|/\<=>~))
  (func $" random \t \n stuff ") (func (call $" random \t \n stuff "))
  (func $" ") (func (call $" "))

  (func $fh) (func (call $"fh"))
  (func $"fi") (func (call $fi))
  (func $!?@#a$%^&*-+_.:9'`|/\<=>~) (func (call $"!?@#a$%^&*-+_.:9'`|/\\<=>~"))

  (func $"\41B") (func (call $"AB") (call $"A\42") (call $"\41\42") (call $"\u{41}\u{42}"))
  (func $"\t") (func (call $"\09") (call $"\u{09}"))
  (func $"") (func (call $"\ef\98\9a\ef\92\a9") (call $"\u{f61a}\u{f4a9}"))

  (func
    block $l1 (br $"l1") end $"l1"
    block $007 (br $"007") end $"007"
    block $!?@#a$%^&*-+_.:9'`|/\<=>~ end $"!?@#a$%^&*-+_.:9'`|/\\<=>~"
    (i32.const 0) if $"\41B" (br $AB) else $"A\42" end $"\u{41}\u{42}"
    (i32.const 0) if $"\t" else $"\09" end $"\u{09}"
    (i32.const 0) if $" " else $"\ef\98\9a\ef\92\a9 " end $"\u{f61a}\u{f4a9} "
  )
)

(assert_malformed (module quote "(func $)") "empty identifier")
(assert_malformed (module quote "(func $\"\")") "empty identifier")
(assert_malformed (module quote "(func $ \"a\")") "empty identifier")
(assert_malformed (module quote "(func $\"a\nb\")") "empty identifier")
(assert_malformed (module quote "(func $\"a\tb\")") "empty identifier")
(assert_malformed (module quote "(func $\"\\ef\")") "malformed UTF-8")
