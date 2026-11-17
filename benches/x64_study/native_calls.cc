#include <wasm.h>
#include <array>
#include <chrono>
#include <cstdio>
#include <cstdlib>

// Two cached exports with the same bytecode as the Wasmer/Wasmtime study.
// run(a,b) returns h.call(a+1,b)+1.
static const unsigned char add_bytes[] = {0,97,115,109,1,0,0,0,1,7,1,96,2,127,127,1,127,3,2,1,0,7,7,1,3,97,100,100,0,0,10,9,1,7,0,32,0,32,1,106,11};
static const unsigned char wrap_bytes[] = {0,97,115,109,1,0,0,0,1,7,1,96,2,127,127,1,127,2,10,1,1,104,4,99,97,108,108,0,0,3,2,1,0,7,7,1,3,114,117,110,0,1,10,16,1,14,0,32,0,65,1,106,32,1,16,0,65,1,106,11};

static void require(bool ok) { if (!ok) std::abort(); }
static wasm_val_t i32(int v) { wasm_val_t val{}; val.kind=WASM_I32; val.of.i32=v; return val; }

struct Export {
    wasm_module_t* module;
    wasm_instance_t* instance;
    wasm_extern_vec_t exports;
    wasm_func_t* func;
    template<size_t N>
    Export(wasm_store_t* store, const unsigned char (&bytes)[N], wasm_func_t* host=nullptr) {
        wasm_byte_vec_t code{N,reinterpret_cast<char*>(const_cast<unsigned char*>(bytes))};
        module=wasm_module_new(store,&code);
        require(module);
        wasm_extern_t* import=host?wasm_func_as_extern(host):nullptr;
        wasm_extern_vec_t imports{host?size_t(1):size_t(0), host?&import:nullptr};
        wasm_trap_t* trap=nullptr;
        instance=wasm_instance_new(store,module,&imports,&trap);
        require(instance && !trap);
        wasm_instance_exports(instance,&exports);
        require(exports.size==1);
        func=wasm_extern_as_func(exports.data[0]);
        require(func);
    }
    ~Export() {wasm_extern_vec_delete(&exports); wasm_instance_delete(instance); wasm_module_delete(module);}
};

struct Invocation {
    wasm_func_t* function;
    wasm_val_vec_t output;
    explicit Invocation(wasm_func_t* fn):function(fn) {
        wasm_val_vec_new_uninitialized(&output,1);
        output.data[0]=i32(0);
    }
    ~Invocation() {wasm_val_vec_delete(&output);}
    int call(int a,int b){
        // V8 15.9's C binding adopts the input vector on every call and
        // releases the output vector back to the caller.
        wasm_val_vec_t args;
        wasm_val_vec_new_uninitialized(&args,2);
        args.data[0]=i32(a); args.data[1]=i32(b);
        require(!wasm_func_call(function,&args,&output));
        return output.data[0].of.i32;
    }
};

static wasm_trap_t* host(void* env,const wasm_val_vec_t* args,wasm_val_vec_t* ret){
    int a=args->data[0].of.i32,b=args->data[1].of.i32;
    ret->data[0]=i32(env?static_cast<Invocation*>(env)->call(a,b):a+b);
    return nullptr;
}

static void sample(int round,const char* label,Invocation& invoke,int extra,int n){
    int64_t sum=0;
    auto start=std::chrono::steady_clock::now();
    for(int i=0;i<n;i++) sum+=invoke.call(i&1023,27);
    auto elapsed=std::chrono::steady_clock::now()-start;
    int64_t expected=int64_t(n)*(27+extra)+int64_t(n/1024)*1023*512+int64_t(n%1024)*(n%1024-1)/2;
    require(sum==expected);
    if(round>=0) std::printf("%d,%s,%.4f\n",round,label,std::chrono::duration<double,std::nano>(elapsed).count()/n);
}

int main() {
    auto engine=wasm_engine_new(); require(engine);
    auto store=wasm_store_new(engine); require(store);
    auto type=wasm_functype_new_2_1(wasm_valtype_new_i32(),wasm_valtype_new_i32(),wasm_valtype_new_i32());
    {
        Export leaf(store,add_bytes); Invocation direct(leaf.func);
        auto add=wasm_func_new_with_env(store,type,host,nullptr,nullptr);
        auto first=wasm_func_new_with_env(store,type,host,&direct,nullptr);
        Export imported(store,wrap_bytes,add); Invocation import_only(imported.func);
        Export wrapped(store,wrap_bytes,first); Invocation nested(wrapped.func);
        auto second=wasm_func_new_with_env(store,type,host,&nested,nullptr);
        Export twice(store,wrap_bytes,second); Invocation deeper(twice.func);
        std::array<Invocation*,4> calls={&direct,&import_only,&nested,&deeper};
        const char* names[]={"direct","import-only","nested-1","nested-2"};
        int extras[]={0,2,2,4};
        std::puts("round,case,ns");
        for(int r=-1;r<8;r++) for(int j=0;j<4;j++) {
            int i=(j+r+4)%4;
            sample(r,names[i],*calls[i],extras[i],r<0?10000:300000);
        }
        wasm_func_delete(second); wasm_func_delete(first); wasm_func_delete(add);
    }
    wasm_functype_delete(type); wasm_store_delete(store); wasm_engine_delete(engine);
}
