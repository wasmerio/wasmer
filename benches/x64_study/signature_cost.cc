#define main boundary_example_main
#include "native_calls.cc"
#undef main

int main() {
    auto engine=wasm_engine_new(); require(engine);
    auto store=wasm_store_new(engine); require(store);
    {
        Export leaf(store,add_bytes);
        std::puts("round,case,ns");
        constexpr int n=250000;
        const auto expected_types=wasm_func_param_arity(leaf.func)+wasm_func_result_arity(leaf.func);
        for(int r=-1;r<8;r++) {
            size_t checked=0;
            auto start=std::chrono::steady_clock::now();
            for(int i=0;i<n;i++) {
                auto type=wasm_func_type(leaf.func);
                auto params=wasm_functype_params(type);
                auto results=wasm_functype_results(type);
                checked+=params->size+results->size;
                wasm_functype_delete(type);
            }
            auto elapsed=std::chrono::steady_clock::now()-start;
            require(checked==size_t(n)*expected_types);
            if(r>=0) std::printf("%d,signature-create-inspect-delete,%.4f\n",r,std::chrono::duration<double,std::nano>(elapsed).count()/n);
        }
    }
    wasm_store_delete(store); wasm_engine_delete(engine);
    return 0;
}
