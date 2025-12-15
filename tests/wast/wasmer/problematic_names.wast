(module
    (func $.L_oops_llvm_local
        return
    )

    (func $llvm.oops_llvm_intrinsic
        return
    )

    (func (export "test_it") (result i32)
        call $llvm.oops_llvm_intrinsic
        call $.L_oops_llvm_local
        i32.const 1
    )
)

(assert_return (invoke "test_it") (i32.const 1))
