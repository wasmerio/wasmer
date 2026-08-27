(function() {
    var type_impls = Object.fromEntries([["wasmer",[]],["wasmer_types",[]],["wasmer_vm",[]],["wasmer_wasix",[]]]);
    if (window.register_type_impls) {
        window.register_type_impls(type_impls);
    } else {
        window.pending_type_impls = type_impls;
    }
})()
//{"start":55,"fragment_lengths":[13,20,17,20]}