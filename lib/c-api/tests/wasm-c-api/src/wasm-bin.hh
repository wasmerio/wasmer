#ifndef __WASM_BIN_HH
#define __WASM_BIN_HH

#include "wasm.hh"

namespace wasm {
namespace bin {

auto u32_size(uint32_t) -> size_t;
auto u64_size(uint64_t) -> size_t;
void encode_u32(char*& ptr, uint32_t n);
void encode_u64(char*& ptr, uint64_t n);
auto u32(const byte_t*& pos) -> uint32_t;
auto u64(const byte_t*& pos) -> uint64_t;

auto wrapper(const FuncType*) -> vec<byte_t>;
auto wrapper(const GlobalType*) -> vec<byte_t>;

auto imports(const vec<byte_t>& binary) -> ownvec<ImportType>;
auto exports(const vec<byte_t>& binary) -> ownvec<ExportType>;

}  // namespace bin
}  // namespace wasm

#endif  // #ifdef __WASM_BIN_HH
