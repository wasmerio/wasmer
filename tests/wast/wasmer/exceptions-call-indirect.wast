(module $env
  (func (export "add_one") (param i32) (result i32)
    local.get 0
    i32.const 1
    i32.add))
(register "env" $env)

(module $test
  (type $t0 (func (param i32) (result i32)))
  (import "env" "add_one" (func $add_one (type $t0)))
  (memory 1 1)
  (table 2 funcref)
  (elem (i32.const 0) func $add_one $add_ten)
  (tag $err (param i32))

  (func $add_ten (type $t0) (param i32) (result i32)
    local.get 0
    i32.const 10
    i32.add)

  (func (export "dispatch") (param i32 i32) (result i32)
    (block $catch (result i32)
      (try_table (result i32) (catch $err $catch)
        local.get 0
        local.get 1
        call_indirect (type $t0)))))

(assert_return (invoke $test "dispatch" (i32.const 41) (i32.const 0)) (i32.const 42))
(assert_return (invoke $test "dispatch" (i32.const 41) (i32.const 1)) (i32.const 51))
