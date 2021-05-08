(module
  (type $t0 (struct))
  (type $t1 (struct (field i32)))
  (type $t1' (struct (field i32)))
  (type $t2 (struct (field i32 i32)))
  (type $t2' (struct (field i32 i32)))
  (type $t3 (struct (field i32 i32)))

  (global $t0 (rtt $t0) (rtt.canon $t0))
  (global $t0' (rtt $t0) (rtt.canon $t0))
  (global $t1 (rtt $t1) (rtt.sub $t1 (global.get $t0)))
  (global $t1' (rtt $t1') (rtt.sub $t1' (global.get $t0)))
  (global $t2 (rtt $t2) (rtt.sub $t2 (global.get $t1)))
  (global $t2' (rtt $t2') (rtt.sub $t2' (global.get $t1')))
  (global $t3 (rtt $t3) (rtt.sub $t3 (global.get $t0)))
  (global $t4 (rtt $t3) (rtt.sub $t3 (rtt.sub $t0 (global.get $t0))))

  (table 20 dataref)

  (func $init
    (table.set (i32.const 0) (struct.new_default $t0 (global.get $t0)))
    (table.set (i32.const 10) (struct.new_default $t0 (global.get $t0')))
    (table.set (i32.const 1) (struct.new_default $t1 (global.get $t1)))
    (table.set (i32.const 11) (struct.new_default $t1' (global.get $t1')))
    (table.set (i32.const 2) (struct.new_default $t2 (global.get $t2)))
    (table.set (i32.const 12) (struct.new_default $t2' (global.get $t2')))
    (table.set (i32.const 3) (struct.new_default $t3 (global.get $t3)))
    (table.set (i32.const 4) (struct.new_default $t3 (global.get $t4)))
  )

  (func (export "test-sub")
    (call $init)
    (block $l (result dataref)
      ;; must succeed
      (drop (block (result dataref) (br_on_cast 0 (ref.null data) (global.get $t0))))
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 0)) (global.get $t0))))
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 1)) (global.get $t0))))
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 2)) (global.get $t0))))
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 3)) (global.get $t0))))
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 4)) (global.get $t0))))

      (drop (block (result dataref) (br_on_cast 0 (ref.null data) (global.get $t1))))
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 1)) (global.get $t1))))
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 2)) (global.get $t1))))

      (drop (block (result dataref) (br_on_cast 0 (ref.null data) (global.get $t2))))
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 2)) (global.get $t2))))

      (drop (block (result dataref) (br_on_cast 0 (ref.null data) (global.get $t3))))
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 3)) (global.get $t3))))

      (drop (block (result dataref) (br_on_cast 0 (ref.null data) (global.get $t4))))
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 4)) (global.get $t4))))

      ;; must not succeed
      (br_on_cast $l (table.get (i32.const 0)) (global.get $t1))
      (br_on_cast $l (table.get (i32.const 3)) (global.get $t1))
      (br_on_cast $l (table.get (i32.const 4)) (global.get $t1))

      (br_on_cast $l (table.get (i32.const 0)) (global.get $t2))
      (br_on_cast $l (table.get (i32.const 1)) (global.get $t2))
      (br_on_cast $l (table.get (i32.const 3)) (global.get $t2))
      (br_on_cast $l (table.get (i32.const 4)) (global.get $t2))

      (br_on_cast $l (table.get (i32.const 0)) (global.get $t3))
      (br_on_cast $l (table.get (i32.const 1)) (global.get $t3))
      (br_on_cast $l (table.get (i32.const 2)) (global.get $t3))
      (br_on_cast $l (table.get (i32.const 4)) (global.get $t3))

      (br_on_cast $l (table.get (i32.const 0)) (global.get $t4))
      (br_on_cast $l (table.get (i32.const 1)) (global.get $t4))
      (br_on_cast $l (table.get (i32.const 2)) (global.get $t4))
      (br_on_cast $l (table.get (i32.const 3)) (global.get $t4))

      (return)
    )
    (unreachable)
  )

  (func (export "test-canon")
    (call $init)
    (block $l
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 0)) (global.get $t0'))))
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 1)) (global.get $t0'))))
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 2)) (global.get $t0'))))
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 3)) (global.get $t0'))))
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 4)) (global.get $t0'))))

      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 10)) (global.get $t0))))
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 11)) (global.get $t0))))
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 12)) (global.get $t0))))

      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 1)) (global.get $t1'))))
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 2)) (global.get $t1'))))

      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 11)) (global.get $t1))))
      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 12)) (global.get $t1))))

      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 2)) (global.get $t2'))))

      (drop (block (result dataref) (br_on_cast 0 (table.get (i32.const 12)) (global.get $t2))))

      (return)
    )
    (unreachable)
  )
)

(invoke "test-sub")
(invoke "test-canon")
