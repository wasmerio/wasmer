(function() {
    var type_impls = Object.fromEntries([["wasmer_c_api",[]],["wasmer_wasix",[]],["wasmer_wasix_types",[]]]);
    if (window.register_type_impls) {
        window.register_type_impls(type_impls);
    } else {
        window.pending_type_impls = type_impls;
    }
})()
//{"start":55,"fragment_lengths":[19,20,26]}