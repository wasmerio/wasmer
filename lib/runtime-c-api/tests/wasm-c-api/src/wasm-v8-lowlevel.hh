#ifndef __WASM_V8_LOWLEVEL_HH
#define __WASM_V8_LOWLEVEL_HH

#include "v8.h"

namespace v8 {
namespace wasm {

auto object_isolate(v8::Local<v8::Object>) -> v8::Isolate*;
auto object_isolate(const v8::Persistent<v8::Object>&) -> v8::Isolate*;

auto object_is_module(v8::Local<v8::Object>) -> bool;
auto object_is_instance(v8::Local<v8::Object>) -> bool;
auto object_is_func(v8::Local<v8::Object>) -> bool;
auto object_is_global(v8::Local<v8::Object>) -> bool;
auto object_is_table(v8::Local<v8::Object>) -> bool;
auto object_is_memory(v8::Local<v8::Object>) -> bool;
auto object_is_error(v8::Local<v8::Object>) -> bool;

auto foreign_new(v8::Isolate*, void*) -> v8::Local<v8::Value>;
auto foreign_get(v8::Local<v8::Value>) -> void*;

auto managed_new(v8::Isolate*, void*, void (*)(void*)) -> v8::Local<v8::Value>;
auto managed_get(v8::Local<v8::Value>) -> void*;

enum val_kind_t { I32, I64, F32, F64, ANYREF = 128, FUNCREF };
auto func_type_param_arity(v8::Local<v8::Object> global) -> uint32_t;
auto func_type_result_arity(v8::Local<v8::Object> global) -> uint32_t;
auto func_type_param(v8::Local<v8::Object> global, size_t) -> val_kind_t;
auto func_type_result(v8::Local<v8::Object> global, size_t) -> val_kind_t;

auto global_type_content(v8::Local<v8::Object> global) -> val_kind_t;
auto global_type_mutable(v8::Local<v8::Object> global) -> bool;

auto table_type_min(v8::Local<v8::Object> table) -> uint32_t;
auto table_type_max(v8::Local<v8::Object> table) -> uint32_t;

auto memory_type_min(v8::Local<v8::Object> memory) -> uint32_t;
auto memory_type_max(v8::Local<v8::Object> memory) -> uint32_t;

auto module_binary_size(v8::Local<v8::Object> module) -> size_t;
auto module_binary(v8::Local<v8::Object> module) -> const char*;
auto module_serialize_size(v8::Local<v8::Object> module) -> size_t;
auto module_serialize(v8::Local<v8::Object> module, char*, size_t) -> bool;
auto module_deserialize(v8::Isolate*, const char*, size_t, const char*, size_t) -> v8::MaybeLocal<v8::Object>;

auto instance_module(v8::Local<v8::Object> instance) -> v8::Local<v8::Object>;
auto instance_exports(v8::Local<v8::Object> instance) -> v8::Local<v8::Object>;

enum extern_kind_t { EXTERN_FUNC, EXTERN_GLOBAL, EXTERN_TABLE, EXTERN_MEMORY };
auto extern_kind(v8::Local<v8::Object> external) -> extern_kind_t;

auto func_instance(v8::Local<v8::Function>) -> v8::Local<v8::Object>;

auto global_get_i32(v8::Local<v8::Object> global) -> int32_t;
auto global_get_i64(v8::Local<v8::Object> global) -> int64_t;
auto global_get_f32(v8::Local<v8::Object> global) -> float;
auto global_get_f64(v8::Local<v8::Object> global) -> double;
auto global_get_ref(v8::Local<v8::Object> global) -> v8::Local<v8::Value>;
void global_set_i32(v8::Local<v8::Object> global, int32_t);
void global_set_i64(v8::Local<v8::Object> global, int64_t);
void global_set_f32(v8::Local<v8::Object> global, float);
void global_set_f64(v8::Local<v8::Object> global, double);
void global_set_ref(v8::Local<v8::Object> global, v8::Local<v8::Value>);

auto table_get(v8::Local<v8::Object> table, size_t index) -> v8::MaybeLocal<v8::Value>;
auto table_set(v8::Local<v8::Object> table, size_t index, v8::Local<v8::Value>) -> bool;
auto table_size(v8::Local<v8::Object> table) -> size_t;
auto table_grow(v8::Local<v8::Object> table, size_t delta, v8::Local<v8::Value>) -> bool;

auto memory_data(v8::Local<v8::Object> memory) -> char*;
auto memory_data_size(v8::Local<v8::Object> memory)-> size_t;
auto memory_size(v8::Local<v8::Object> memory) -> uint32_t;
auto memory_grow(v8::Local<v8::Object> memory, uint32_t delta) -> bool;

}  // namespace wasm
}  // namespace v8

#endif  // #define __WASM_V8_LOWLEVEL_HH
