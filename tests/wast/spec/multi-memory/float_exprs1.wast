;; Test that plain summation is not reassociated, and that Kahan summation
;; isn't optimized into plain summation.

(module
  (memory 0 0)
  (memory 0 0)
  (memory 0 0)
  (memory 0 0)
  (memory 0 0)
  (memory $m (data
    "\c4\c5\57\24\a5\84\c8\0b\6d\b8\4b\2e\f2\76\17\1c\ca\4a\56\1e\1b\6e\71\22"
    "\5d\17\1e\6e\bf\cd\14\5c\c7\21\55\51\39\9c\1f\b2\51\f0\a3\93\d7\c1\2c\ae"
    "\7e\a8\28\3a\01\21\f4\0a\58\93\f8\42\77\9f\83\39\6a\5f\ba\f7\0a\d8\51\6a"
    "\34\ca\ad\c6\34\0e\d8\26\dc\4c\33\1c\ed\29\90\a8\78\0f\d1\ce\76\31\23\83"
    "\b8\35\e8\f2\44\b0\d3\a1\fc\bb\32\e1\b0\ba\69\44\09\d6\d9\7d\ff\2e\c0\5a"
    "\36\14\33\14\3e\a9\fa\87\6d\8b\bc\ce\9d\a7\fd\c4\e9\85\3f\dd\d7\e1\18\a6"
    "\50\26\72\6e\3f\73\0f\f8\12\93\23\34\61\76\12\48\c0\9b\05\93\eb\ac\86\de"
    "\94\3e\55\e8\8c\e8\dd\e4\fc\95\47\be\56\03\21\20\4c\e6\bf\7b\f6\7f\d5\ba"
    "\73\1c\c1\14\8f\c4\27\96\b3\bd\33\ff\78\41\5f\c0\5a\ce\f6\67\6e\73\9a\17"
    "\66\70\03\f8\ce\27\a3\52\b2\9f\3b\bf\fb\ae\ed\d3\5a\f8\37\57\f0\f5\6e\ef"
    "\b1\4d\70\3d\54\a7\01\9a\85\08\48\91\f5\9d\0c\60\87\5b\d9\54\1e\51\6d\88"
    "\8e\08\8c\a5\71\3a\56\08\67\46\8f\8f\13\2a\2c\ec\2c\1f\b4\62\2b\6f\41\0a"
    "\c4\65\42\a2\31\6b\2c\7d\3e\bb\75\ac\86\97\30\d9\48\cd\9a\1f\56\c4\c6\e4"
    "\12\c0\9d\fb\ee\02\8c\ce\1c\f2\1e\a1\78\23\db\c4\1e\49\03\d3\71\cc\08\50"
    "\c5\d8\5c\ed\d5\b5\65\ac\b5\c9\21\d2\c9\29\76\de\f0\30\1a\5b\3c\f2\3b\db"
    "\3a\39\82\3a\16\08\6f\a8\f1\be\69\69\99\71\a6\05\d3\14\93\2a\16\f2\2f\11"
    "\c7\7e\20\bb\91\44\ee\f8\e4\01\53\c0\b9\7f\f0\bf\f0\03\9c\6d\b1\df\a2\44"
    "\01\6d\6b\71\2b\5c\b3\21\19\46\5e\8f\db\91\d3\7c\78\6b\b7\12\00\8f\eb\bd"
    "\8a\f5\d4\2e\c4\c1\1e\df\73\63\59\47\49\03\0a\b7\cf\24\cf\9c\0e\44\7a\9e"
    "\14\fb\42\bf\9d\39\30\9e\a0\ab\2f\d1\ae\9e\6a\83\43\e3\55\7d\85\bf\63\8a"
    "\f8\96\10\1f\fe\6d\e7\22\1b\e1\69\46\8a\44\c8\c8\f9\0c\2b\19\07\a5\02\3e"
    "\f2\30\10\9a\85\8a\5f\ef\81\45\a0\77\b1\03\10\73\4b\ae\98\9d\47\bf\9a\2d"
    "\3a\d5\0f\03\66\e3\3d\53\d9\40\ce\1f\6f\32\2f\21\2b\23\21\6c\62\d4\a7\3e"
    "\a8\ce\28\31\2d\00\3d\67\5e\af\a0\cf\2e\d2\b9\6b\84\eb\69\08\3c\62\36\be"
    "\12\fd\36\7f\88\3e\ad\bc\0b\c0\41\c4\50\b6\e3\50\31\e8\ce\e2\96\65\55\9c"
    "\16\46\e6\b0\2d\3a\e8\81\05\b0\bf\34\f7\bc\10\1c\fb\cc\3c\f1\85\97\42\9f"
    "\eb\14\8d\3c\bf\d7\17\88\49\9d\8b\2b\b2\3a\83\d1\4f\04\9e\a1\0f\ad\08\9d"
    "\54\af\d1\82\c3\ec\32\2f\02\8f\05\21\2d\a2\b7\e4\f4\6f\2e\81\2b\0b\9c\fc"
    "\cb\fe\74\02\f9\db\f4\f3\ea\00\a8\ec\d1\99\74\26\dd\d6\34\d5\25\b1\46\dd"
    "\9c\aa\71\f5\60\b0\88\c8\e0\0b\59\5a\25\4f\29\66\f9\e3\2e\fe\e9\da\e5\18"
    "\4f\27\62\f4\ce\a4\21\95\74\c7\57\64\27\9a\4c\fd\54\7d\61\ce\c3\ac\87\46"
    "\9c\fa\ff\09\ca\79\97\67\24\74\ca\d4\21\83\26\25\19\12\37\64\19\e5\65\e0"
    "\74\75\8e\dd\c8\ef\74\c7\d8\21\2b\79\04\51\46\65\60\03\5d\fa\d8\f4\65\a4"
    "\9e\5d\23\da\d7\8a\92\80\a4\de\78\3c\f1\57\42\6d\cd\c9\2f\d5\a4\9e\ab\40"
    "\f4\cb\1b\d7\a3\ca\fc\eb\a7\01\b2\9a\69\4e\46\9b\18\4e\dd\79\a7\aa\a6\52"
    "\39\1e\ef\30\cc\9b\bd\5b\ee\4c\21\6d\30\00\72\b0\46\5f\08\cf\c5\b9\e0\3e"
    "\c2\b3\0c\dc\8e\64\de\19\42\79\cf\43\ea\43\5d\8e\88\f7\ab\15\dc\3f\c8\67"
    "\20\db\b8\64\b1\47\1f\de\f2\cb\3f\59\9f\d8\46\90\dc\ae\2f\22\f9\e2\31\89"
    "\d9\9c\1c\4c\d3\a9\4a\57\84\9c\9f\ea\2c\3c\ae\3c\c3\1e\8b\e5\4e\17\01\25"
    "\db\34\46\5f\15\ea\05\0c\7c\d9\45\8c\19\d0\73\8a\96\16\dd\44\f9\05\b7\5b"
    "\71\b0\e6\21\36\5f\75\89\91\73\75\ab\7d\ae\d3\73\ec\37\c6\ea\55\75\ef\ea"
    "\ab\8b\7b\11\dc\6d\1a\b2\6a\c4\25\cf\aa\e3\9f\49\49\89\cb\37\9b\0a\a7\01"
    "\60\70\dc\b7\c8\83\e1\42\f5\be\ad\62\94\ad\8d\a1"
  ))
  (memory 0 0)
  (memory 0 0)
  (memory 0 0)

  (func (export "f32.kahan_sum") (param $p i32) (param $n i32) (result f32)
    (local $sum f32)
    (local $c f32)
    (local $t f32)
    (block $exit
      (loop $top
        (local.set $t
          (f32.sub
            (f32.sub
              (local.tee $sum
                (f32.add
                  (local.get $c)
                  (local.tee $t
                    (f32.sub (f32.load $m (local.get $p)) (local.get $t))
                  )
                )
              )
              (local.get $c)
            )
            (local.get $t)
          )
        )
        (local.set $p (i32.add (local.get $p) (i32.const 4)))
        (local.set $c (local.get $sum))
        (br_if $top (local.tee $n (i32.add (local.get $n) (i32.const -1))))
      )
    )
    (local.get $sum)
  )

  (func (export "f32.plain_sum") (param $p i32) (param $n i32) (result f32)
    (local $sum f32)
    (block $exit
      (loop $top
        (local.set $sum (f32.add (local.get $sum) (f32.load $m (local.get $p))))
        (local.set $p (i32.add (local.get $p) (i32.const 4)))
        (local.set $n (i32.add (local.get $n) (i32.const -1)))
        (br_if $top (local.get $n))
      )
    )
    (local.get $sum)
  )
)

(assert_return (invoke "f32.kahan_sum" (i32.const 0) (i32.const 256)) (f32.const -0x1.101a1ap+104))
(assert_return (invoke "f32.plain_sum" (i32.const 0) (i32.const 256)) (f32.const -0x1.a0343ap+103))

