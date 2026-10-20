(module
  (type $tag (func (param i32)))
  (tag $e (type $tag))
  (global $thrown (mut i32) (i32.const 0))
  (func (export "run") (result i32)
    i32.const 0
    loop (param i32)
      try_table (catch $e 0)
        global.get $thrown
        if
        else
          i32.const 1
          global.set $thrown
          i32.const 42
          throw $e
        end
      end
      drop
    end
    global.get $thrown))

(assert_return (invoke "run") (i32.const 1))
