(module
  (func (export "new") (param $i i32) (result (ref i31))
    (ref.i31 (local.get $i))
  )

  (func (export "get_u") (param $i i32) (result i32)
    (i31.get_u (ref.i31 (local.get $i)))
  )
  (func (export "get_s") (param $i i32) (result i32)
    (i31.get_s (ref.i31 (local.get $i)))
  )

  (func (export "get_u-null") (result i32)
    (i31.get_u (ref.null i31))
  )
  (func (export "get_s-null") (result i32)
    (i31.get_u (ref.null i31))
  )

  (global $i (ref i31) (ref.i31 (i32.const 2)))
  (global $m (mut (ref i31)) (ref.i31 (i32.const 3)))

  (func (export "get_globals") (result i32 i32)
    (i31.get_u (global.get $i))
    (i31.get_u (global.get $m))
  )

  (func (export "set_global") (param i32)
    (global.set $m (ref.i31 (local.get 0)))
  )
)

(assert_return (invoke "new" (i32.const 1)) (ref.i31))

(assert_return (invoke "get_u" (i32.const 0)) (i32.const 0))
(assert_return (invoke "get_u" (i32.const 100)) (i32.const 100))
(assert_return (invoke "get_u" (i32.const -1)) (i32.const 0x7fff_ffff))
(assert_return (invoke "get_u" (i32.const 0x3fff_ffff)) (i32.const 0x3fff_ffff))
(assert_return (invoke "get_u" (i32.const 0x4000_0000)) (i32.const 0x4000_0000))
(assert_return (invoke "get_u" (i32.const 0x7fff_ffff)) (i32.const 0x7fff_ffff))
(assert_return (invoke "get_u" (i32.const 0xaaaa_aaaa)) (i32.const 0x2aaa_aaaa))
(assert_return (invoke "get_u" (i32.const 0xcaaa_aaaa)) (i32.const 0x4aaa_aaaa))

(assert_return (invoke "get_s" (i32.const 0)) (i32.const 0))
(assert_return (invoke "get_s" (i32.const 100)) (i32.const 100))
(assert_return (invoke "get_s" (i32.const -1)) (i32.const -1))
(assert_return (invoke "get_s" (i32.const 0x3fff_ffff)) (i32.const 0x3fff_ffff))
(assert_return (invoke "get_s" (i32.const 0x4000_0000)) (i32.const -0x4000_0000))
(assert_return (invoke "get_s" (i32.const 0x7fff_ffff)) (i32.const -1))
(assert_return (invoke "get_s" (i32.const 0xaaaa_aaaa)) (i32.const 0x2aaa_aaaa))
(assert_return (invoke "get_s" (i32.const 0xcaaa_aaaa)) (i32.const 0xcaaa_aaaa))

(assert_trap (invoke "get_u-null") "null i31 reference")
(assert_trap (invoke "get_s-null") "null i31 reference")

(assert_return (invoke "get_globals") (i32.const 2) (i32.const 3))

(invoke "set_global" (i32.const 1234))
(assert_return (invoke "get_globals") (i32.const 2) (i32.const 1234))

(module $tables_of_i31ref
  (table $table 3 10 i31ref)
  (elem (table $table) (i32.const 0) i31ref (item (ref.i31 (i32.const 999)))
                                            (item (ref.i31 (i32.const 888)))
                                            (item (ref.i31 (i32.const 777))))

  (func (export "size") (result i32)
    table.size $table
  )

  (func (export "get") (param i32) (result i32)
    (i31.get_u (table.get $table (local.get 0)))
  )

  (func (export "grow") (param i32 i32) (result i32)
    (table.grow $table (ref.i31 (local.get 1)) (local.get 0))
  )

  (func (export "fill") (param i32 i32 i32)
    (table.fill $table (local.get 0) (ref.i31 (local.get 1)) (local.get 2))
  )

  (func (export "copy") (param i32 i32 i32)
    (table.copy $table $table (local.get 0) (local.get 1) (local.get 2))
  )

  (elem $elem i31ref (item (ref.i31 (i32.const 123)))
                     (item (ref.i31 (i32.const 456)))
                     (item (ref.i31 (i32.const 789))))
  (func (export "init") (param i32 i32 i32)
    (table.init $table $elem (local.get 0) (local.get 1) (local.get 2))
  )
)

;; Initial state.
(assert_return (invoke "size") (i32.const 3))
(assert_return (invoke "get" (i32.const 0)) (i32.const 999))
(assert_return (invoke "get" (i32.const 1)) (i32.const 888))
(assert_return (invoke "get" (i32.const 2)) (i32.const 777))

;; Grow from size 3 to size 5.
(assert_return (invoke "grow" (i32.const 2) (i32.const 333)) (i32.const 3))
(assert_return (invoke "size") (i32.const 5))
(assert_return (invoke "get" (i32.const 3)) (i32.const 333))
(assert_return (invoke "get" (i32.const 4)) (i32.const 333))

;; Fill table[2..4] = 111.
(invoke "fill" (i32.const 2) (i32.const 111) (i32.const 2))
(assert_return (invoke "get" (i32.const 2)) (i32.const 111))
(assert_return (invoke "get" (i32.const 3)) (i32.const 111))

;; Copy from table[0..2] to table[3..5].
(invoke "copy" (i32.const 3) (i32.const 0) (i32.const 2))
(assert_return (invoke "get" (i32.const 3)) (i32.const 999))
(assert_return (invoke "get" (i32.const 4)) (i32.const 888))

;; Initialize the passive element at table[1..4].
(invoke "init" (i32.const 1) (i32.const 0) (i32.const 3))
(assert_return (invoke "get" (i32.const 1)) (i32.const 123))
(assert_return (invoke "get" (i32.const 2)) (i32.const 456))
(assert_return (invoke "get" (i32.const 3)) (i32.const 789))

(module $env
  (global (export "g") i32 (i32.const 42))
)
(register "env")

(module $i31ref_of_global_table_initializer
  (global $g (import "env" "g") i32)
  (table $t 3 3 (ref i31) (ref.i31 (global.get $g)))
  (func (export "get") (param i32) (result i32)
    (i31.get_u (local.get 0) (table.get $t))
  )
)

(assert_return (invoke "get" (i32.const 0)) (i32.const 42))
(assert_return (invoke "get" (i32.const 1)) (i32.const 42))
(assert_return (invoke "get" (i32.const 2)) (i32.const 42))

(module $i31ref_of_global_global_initializer
  (global $g0 (import "env" "g") i32)
  (global $g1 i31ref (ref.i31 (global.get $g0)))
  (func (export "get") (result i32)
    (i31.get_u (global.get $g1))
  )
)

(assert_return (invoke "get") (i32.const 42))

(module $anyref_global_of_i31ref
  (global $c anyref (ref.i31 (i32.const 1234)))
  (global $m (mut anyref) (ref.i31 (i32.const 5678)))

  (func (export "get_globals") (result i32 i32)
    (i31.get_u (ref.cast i31ref (global.get $c)))
    (i31.get_u (ref.cast i31ref (global.get $m)))
  )

  (func (export "set_global") (param i32)
    (global.set $m (ref.i31 (local.get 0)))
  )
)

(assert_return (invoke "get_globals") (i32.const 1234) (i32.const 5678))
(invoke "set_global" (i32.const 0))
(assert_return (invoke "get_globals") (i32.const 1234) (i32.const 0))

(module $anyref_table_of_i31ref
  (table $table 3 10 anyref)
  (elem (table $table) (i32.const 0) i31ref (item (ref.i31 (i32.const 999)))
                                            (item (ref.i31 (i32.const 888)))
                                            (item (ref.i31 (i32.const 777))))

  (func (export "size") (result i32)
    table.size $table
  )

  (func (export "get") (param i32) (result i32)
    (i31.get_u (ref.cast i31ref (table.get $table (local.get 0))))
  )

  (func (export "grow") (param i32 i32) (result i32)
    (table.grow $table (ref.i31 (local.get 1)) (local.get 0))
  )

  (func (export "fill") (param i32 i32 i32)
    (table.fill $table (local.get 0) (ref.i31 (local.get 1)) (local.get 2))
  )

  (func (export "copy") (param i32 i32 i32)
    (table.copy $table $table (local.get 0) (local.get 1) (local.get 2))
  )

  (elem $elem i31ref (item (ref.i31 (i32.const 123)))
                     (item (ref.i31 (i32.const 456)))
                     (item (ref.i31 (i32.const 789))))
  (func (export "init") (param i32 i32 i32)
    (table.init $table $elem (local.get 0) (local.get 1) (local.get 2))
  )
)

;; Initial state.
(assert_return (invoke "size") (i32.const 3))
(assert_return (invoke "get" (i32.const 0)) (i32.const 999))
(assert_return (invoke "get" (i32.const 1)) (i32.const 888))
(assert_return (invoke "get" (i32.const 2)) (i32.const 777))

;; Grow from size 3 to size 5.
(assert_return (invoke "grow" (i32.const 2) (i32.const 333)) (i32.const 3))
(assert_return (invoke "size") (i32.const 5))
(assert_return (invoke "get" (i32.const 3)) (i32.const 333))
(assert_return (invoke "get" (i32.const 4)) (i32.const 333))

;; Fill table[2..4] = 111.
(invoke "fill" (i32.const 2) (i32.const 111) (i32.const 2))
(assert_return (invoke "get" (i32.const 2)) (i32.const 111))
(assert_return (invoke "get" (i32.const 3)) (i32.const 111))

;; Copy from table[0..2] to table[3..5].
(invoke "copy" (i32.const 3) (i32.const 0) (i32.const 2))
(assert_return (invoke "get" (i32.const 3)) (i32.const 999))
(assert_return (invoke "get" (i32.const 4)) (i32.const 888))

;; Initialize the passive element at table[1..4].
(invoke "init" (i32.const 1) (i32.const 0) (i32.const 3))
(assert_return (invoke "get" (i32.const 1)) (i32.const 123))
(assert_return (invoke "get" (i32.const 2)) (i32.const 456))
(assert_return (invoke "get" (i32.const 3)) (i32.const 789))
