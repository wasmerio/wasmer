#include "wasm-bin.hh"

#include <cstring>

namespace wasm {
namespace bin {

////////////////////////////////////////////////////////////////////////////////
// Encoding

void encode_header(char*& ptr) {
  std::memcpy(ptr, "\x00""asm\x01\x00\x00\x00", 8);
  ptr += 8;
}

auto u64_size(uint64_t n) -> size_t {
  bool done = false;
  size_t size = 0;
  do {
    ++size;
    done = n <= 0x7f;
    n = n >> 7;
  } while (!done);
  return size;
}

auto u32_size(uint64_t n) -> size_t {
  return u64_size(n);
}

void encode_u64(char*& ptr, uint64_t n) {
  bool done = false;
  do {
    done = n <= 0x7f;
    *ptr++ = (n & 0x7f) | (done ? 0x00 : 0x80);
    n = n >> 7;
  } while (!done);
}

void encode_u32(char*& ptr, uint32_t n) {
  encode_u64(ptr, n);
}

void encode_size32(char*& ptr, size_t n) {
  assert(n <= 0xffffffff);
  for (int i = 0; i < 5; ++i) {
    *ptr++ = (n & 0x7f) | (i == 4 ? 0x00 : 0x80);
    n = n >> 7;
  }
}


void encode_valtype(char*& ptr, const ValType* type) {
  switch (type->kind()) {
    case ValKind::I32: *ptr++ = 0x7f; break;
    case ValKind::I64: *ptr++ = 0x7e; break;
    case ValKind::F32: *ptr++ = 0x7d; break;
    case ValKind::F64: *ptr++ = 0x7c; break;
    case ValKind::FUNCREF: *ptr++ = 0x70; break;
    case ValKind::ANYREF: *ptr++ = 0x6f; break;
    default: assert(false);
  }
}

auto zero_size(const ValType* type) -> size_t {
  switch (type->kind()) {
    case ValKind::I32: return 1;
    case ValKind::I64: return 1;
    case ValKind::F32: return 4;
    case ValKind::F64: return 8;
    case ValKind::FUNCREF: return 0;
    case ValKind::ANYREF: return 0;
    default: assert(false);
  }
}

void encode_const_zero(char*& ptr, const ValType* type) {
  switch (type->kind()) {
    case ValKind::I32: *ptr++ = 0x41; break;
    case ValKind::I64: *ptr++ = 0x42; break;
    case ValKind::F32: *ptr++ = 0x43; break;
    case ValKind::F64: *ptr++ = 0x44; break;
    case ValKind::FUNCREF: *ptr++ = 0xd0; break;
    case ValKind::ANYREF: *ptr++ = 0xd0; break;
    default: assert(false);
  }
  for (int i = 0; i < zero_size(type); ++i) *ptr++ = 0;
}


auto wrapper(const FuncType* type) -> vec<byte_t> {
  auto in_arity = type->params().size();
  auto out_arity = type->results().size();
  auto size = 39 + in_arity + out_arity;
  auto binary = vec<byte_t>::make_uninitialized(size);
  auto ptr = binary.get();

  encode_header(ptr);

  *ptr++ = 0x01;  // type section
  encode_size32(ptr, 12 + in_arity + out_arity);  // size
  *ptr++ = 1;  // length
  *ptr++ = 0x60;  // function
  encode_size32(ptr, in_arity);
  for (size_t i = 0; i < in_arity; ++i) {
    encode_valtype(ptr, type->params()[i].get());
  }
  encode_size32(ptr, out_arity);
  for (size_t i = 0; i < out_arity; ++i) {
    encode_valtype(ptr, type->results()[i].get());
  }

  *ptr++ = 0x02;  // import section
  *ptr++ = 5;  // size
  *ptr++ = 1;  // length
  *ptr++ = 0;  // module length
  *ptr++ = 0;  // name length
  *ptr++ = 0x00;  // func
  *ptr++ = 0;  // type index

  *ptr++ = 0x07;  // export section
  *ptr++ = 4;  // size
  *ptr++ = 1;  // length
  *ptr++ = 0;  // name length
  *ptr++ = 0x00;  // func
  *ptr++ = 0;  // func index

  assert(ptr - binary.get() == size);
  return binary;
}

auto wrapper(const GlobalType* type) -> vec<byte_t> {
  auto size = 25 + zero_size(type->content());
  auto binary = vec<byte_t>::make_uninitialized(size);
  auto ptr = binary.get();

  encode_header(ptr);

  *ptr++ = 0x06;  // global section
  encode_size32(ptr, 5 + zero_size(type->content()));  // size
  *ptr++ = 1;  // length
  encode_valtype(ptr, type->content());
  *ptr++ = (type->mutability() == Mutability::VAR);
  encode_const_zero(ptr, type->content());
  *ptr++ = 0x0b;  // end

  *ptr++ = 0x07;  // export section
  *ptr++ = 4;  // size
  *ptr++ = 1;  // length
  *ptr++ = 0;  // name length
  *ptr++ = 0x03;  // global
  *ptr++ = 0;  // func index

  assert(ptr - binary.get() == size);
  return binary;
}


////////////////////////////////////////////////////////////////////////////////
// Decoding

// Numbers

auto u32(const byte_t*& pos) -> uint32_t {
  uint32_t n = 0;
  uint32_t shift = 0;
  byte_t b;
  do {
    b = *pos++;
    n += (b & 0x7f) << shift;
    shift += 7;
  } while ((b & 0x80) != 0);
  return n;
}

auto u64(const byte_t*& pos) -> uint64_t {
  uint64_t n = 0;
  uint64_t shift = 0;
  byte_t b;
  do {
    b = *pos++;
    n += (b & 0x7f) << shift;
    shift += 7;
  } while ((b & 0x80) != 0);
  return n;
}

void u32_skip(const byte_t*& pos) {
  bin::u32(pos);
}


// Names

auto name(const byte_t*& pos) -> Name {
  auto size = bin::u32(pos);
  auto start = pos;
  auto name = Name::make_uninitialized(size);
  std::memcpy(name.get(), start, size);
  pos += size;
  return name;
}

void name_skip(const byte_t*& pos) {
  auto size = bin::u32(pos);
  pos += size;
}


// Types

auto valtype(const byte_t*& pos) -> own<wasm::ValType> {
  switch (*pos++) {
    case 0x7f: return ValType::make(ValKind::I32);
    case 0x7e: return ValType::make(ValKind::I64);
    case 0x7d: return ValType::make(ValKind::F32);
    case 0x7c: return ValType::make(ValKind::F64);
    case 0x70: return ValType::make(ValKind::FUNCREF);
    case 0x6f: return ValType::make(ValKind::ANYREF);
    default:
      // TODO(wasm+): support new value types
      assert(false);
  }
}

auto mutability(const byte_t*& pos) -> Mutability {
  return *pos++ ? Mutability::VAR : Mutability::CONST;
}

auto limits(const byte_t*& pos) -> Limits {
  auto tag = *pos++;
  auto min = bin::u32(pos);
  if ((tag & 0x01) == 0) {
    return Limits(min);
  } else {
    auto max = bin::u32(pos);
    return Limits(min, max);
  }
}

auto stacktype(const byte_t*& pos) -> ownvec<ValType> {
  size_t size = bin::u32(pos);
  auto v = ownvec<ValType>::make_uninitialized(size);
  for (uint32_t i = 0; i < size; ++i) v[i] = bin::valtype(pos);
  return v;
}

auto functype(const byte_t*& pos) -> own<FuncType> {
  assert(*pos == 0x60);
  ++pos;
  auto params = bin::stacktype(pos);
  auto results = bin::stacktype(pos);
  return FuncType::make(std::move(params), std::move(results));
}

auto globaltype(const byte_t*& pos) -> own<GlobalType> {
  auto content = bin::valtype(pos);
  auto mutability = bin::mutability(pos);
  return GlobalType::make(std::move(content), mutability);
}

auto tabletype(const byte_t*& pos) -> own<TableType> {
  auto elem = bin::valtype(pos);
  auto limits = bin::limits(pos);
  return TableType::make(std::move(elem), limits);
}

auto memorytype(const byte_t*& pos) -> own<MemoryType> {
  auto limits = bin::limits(pos);
  return MemoryType::make(limits);
}


void mutability_skip(const byte_t*& pos) {
  ++pos;
}

void limits_skip(const byte_t*& pos) {
  auto tag = *pos++;
  bin::u32_skip(pos);
  if ((tag & 0x01) != 0) bin::u32_skip(pos);
}

void valtype_skip(const byte_t*& pos) {
  // TODO(wasm+): support new value types
  ++pos;
}

void globaltype_skip(const byte_t*& pos) {
  bin::valtype_skip(pos);
  bin::mutability_skip(pos);
}

void tabletype_skip(const byte_t*& pos) {
  bin::valtype_skip(pos);
  bin::limits_skip(pos);
}

void memorytype_skip(const byte_t*& pos) {
  bin::limits_skip(pos);
}


// Expressions

void expr_skip(const byte_t*& pos) {
  switch (*pos++ & 0xff) {
    case 0x41:  // i32.const
    case 0x42:  // i64.const
    case 0x23:  // get_global
    case 0xd2: {  // ref.func
      bin::u32_skip(pos);
    } break;
    case 0x43: {  // f32.const
      pos += 4;
    } break;
    case 0x44: {  // f64.const
      pos += 8;
    } break;
    case 0xd0: {  // ref.null
    } break;
    default: {
      assert(false);
    }
  }
  ++pos;  // end
}


// Sections

enum sec_t : byte_t {
  SEC_TYPE = 1,
  SEC_IMPORT = 2,
  SEC_FUNC = 3,
  SEC_TABLE = 4,
  SEC_MEMORY = 5,
  SEC_GLOBAL = 6,
  SEC_EXPORT = 7
};

auto section(const vec<byte_t>& binary, bin::sec_t sec) -> const byte_t* {
  const byte_t* end = binary.get() + binary.size();
  const byte_t* pos = binary.get() + 8;  // skip header
  while (pos < end && *pos++ != sec) {
    auto size = bin::u32(pos);
    pos += size;
  }
  if (pos == end) return nullptr;
  bin::u32_skip(pos);
  return pos;
}

auto section_end(const vec<byte_t>& binary, bin::sec_t sec) -> const byte_t* {
  const byte_t* end = binary.get() + binary.size();
  const byte_t* pos = binary.get() + 8;  // skip header
  while (pos < end && *pos != sec) {
    ++pos;
    auto size = bin::u32(pos);
    pos += size;
  }
  if (pos == end) return nullptr;
  ++pos;
  auto size = bin::u32(pos);
  return pos + size;
}


// Type section

auto types(const vec<byte_t>& binary) -> ownvec<FuncType> {
  auto pos = bin::section(binary, SEC_TYPE);
  if (pos == nullptr) return ownvec<FuncType>::make();
  size_t size = bin::u32(pos);
  // TODO(wasm+): support new deftypes
  auto v = ownvec<FuncType>::make_uninitialized(size);
  for (uint32_t i = 0; i < size; ++i) {
    v[i] = bin::functype(pos);
  }
  assert(pos = bin::section_end(binary, SEC_TYPE));
  return v;
}


// Import section

auto imports(
  const vec<byte_t>& binary, const ownvec<FuncType>& types
) -> ownvec<ImportType> {
  auto pos = bin::section(binary, SEC_IMPORT);
  if (pos == nullptr) return ownvec<ImportType>::make();
  size_t size = bin::u32(pos);
  auto v = ownvec<ImportType>::make_uninitialized(size);
  for (uint32_t i = 0; i < size; ++i) {
    auto module = bin::name(pos);
    auto name = bin::name(pos);
    own<ExternType> type;
    switch (*pos++) {
      case 0x00: type = types[bin::u32(pos)]->copy(); break;
      case 0x01: type = bin::tabletype(pos); break;
      case 0x02: type = bin::memorytype(pos); break;
      case 0x03: type = bin::globaltype(pos); break;
      default: assert(false);
    }
    v[i] = ImportType::make(
      std::move(module), std::move(name), std::move(type));
  }
  assert(pos = bin::section_end(binary, SEC_IMPORT));
  return v;
}

auto count(const ownvec<ImportType>& imports, ExternKind kind) -> uint32_t {
  uint32_t n = 0;
  for (uint32_t i = 0; i < imports.size(); ++i) {
    if (imports[i]->type()->kind() == kind) ++n;
  }
  return n;
}


// Function section

auto funcs(
  const vec<byte_t>& binary,
  const ownvec<ImportType>& imports, const ownvec<FuncType>& types
) -> ownvec<FuncType> {
  auto pos = bin::section(binary, SEC_FUNC);
  size_t size = pos != nullptr ? bin::u32(pos) : 0;
  auto v = ownvec<FuncType>::make_uninitialized(
    size + count(imports, ExternKind::FUNC));
  size_t j = 0;
  for (uint32_t i = 0; i < imports.size(); ++i) {
    auto et = imports[i]->type();
    if (et->kind() == ExternKind::FUNC) {
      v[j++] = et->func()->copy();
    }
  }
  if (pos != nullptr) {
    for (; j < v.size(); ++j) {
      v[j] = types[bin::u32(pos)]->copy();
    }
    assert(pos = bin::section_end(binary, SEC_FUNC));
  }
  return v;
}


// Global section

auto globals(
  const vec<byte_t>& binary, const ownvec<ImportType>& imports
) -> ownvec<GlobalType> {
  auto pos = bin::section(binary, SEC_GLOBAL);
  size_t size = pos != nullptr ? bin::u32(pos) : 0;
  auto v = ownvec<GlobalType>::make_uninitialized(
    size + count(imports, ExternKind::GLOBAL));
  size_t j = 0;
  for (uint32_t i = 0; i < imports.size(); ++i) {
    auto et = imports[i]->type();
    if (et->kind() == ExternKind::GLOBAL) {
      v[j++] = et->global()->copy();
    }
  }
  if (pos != nullptr) {
    for (; j < v.size(); ++j) {
      v[j] = bin::globaltype(pos);
      expr_skip(pos);
    }
    assert(pos = bin::section_end(binary, SEC_GLOBAL));
  }
  return v;
}


// Table section

auto tables(
  const vec<byte_t>& binary, const ownvec<ImportType>& imports
) -> ownvec<TableType> {
  auto pos = bin::section(binary, SEC_TABLE);
  size_t size = pos != nullptr ? bin::u32(pos) : 0;
  auto v = ownvec<TableType>::make_uninitialized(
    size + count(imports, ExternKind::TABLE));
  size_t j = 0;
  for (uint32_t i = 0; i < imports.size(); ++i) {
    auto et = imports[i]->type();
    if (et->kind() == ExternKind::TABLE) {
      v[j++] = et->table()->copy();
    }
  }
  if (pos != nullptr) {
    for (; j < v.size(); ++j) {
      v[j] = bin::tabletype(pos);
    }
    assert(pos = bin::section_end(binary, SEC_TABLE));
  }
  return v;
}


// Memory section

auto memories(
  const vec<byte_t>& binary, const ownvec<ImportType>& imports
) -> ownvec<MemoryType> {
  auto pos = bin::section(binary, SEC_MEMORY);
  size_t size = pos != nullptr ? bin::u32(pos) : 0;
  auto v = ownvec<MemoryType>::make_uninitialized(
    size + count(imports, ExternKind::MEMORY));
  size_t j = 0;
  for (uint32_t i = 0; i < imports.size(); ++i) {
    auto et = imports[i]->type();
    if (et->kind() == ExternKind::MEMORY) {
      v[j++] = et->memory()->copy();
    }
  }
  if (pos != nullptr) {
    for (; j < v.size(); ++j) {
      v[j] = bin::memorytype(pos);
    }
    assert(pos = bin::section_end(binary, SEC_MEMORY));
  }
  return v;
}


// Export section

auto exports(const vec<byte_t>& binary,
  const ownvec<FuncType>& funcs, const ownvec<GlobalType>& globals,
  const ownvec<TableType>& tables, const ownvec<MemoryType>& memories
) -> ownvec<ExportType> {
  auto pos = bin::section(binary, SEC_EXPORT);
  if (pos == nullptr) return ownvec<ExportType>::make();
  size_t size = bin::u32(pos);
  auto exports = ownvec<ExportType>::make_uninitialized(size);
  for (uint32_t i = 0; i < size; ++i) {
    auto name = bin::name(pos);
    auto tag = *pos++;
    auto index = bin::u32(pos);
    own<ExternType> type;
    switch (tag) {
      case 0x00: type = funcs[index]->copy(); break;
      case 0x01: type = tables[index]->copy(); break;
      case 0x02: type = memories[index]->copy(); break;
      case 0x03: type = globals[index]->copy(); break;
      default: assert(false);
    }
    exports[i] = ExportType::make(std::move(name), std::move(type));
  }
  assert(pos = bin::section_end(binary, SEC_EXPORT));
  return exports;
}

auto imports(const vec<byte_t>& binary) -> ownvec<ImportType> {
  return bin::imports(binary, bin::types(binary));
}

auto exports(const vec<byte_t>& binary) -> ownvec<ExportType> {
  auto types = bin::types(binary);
  auto imports = bin::imports(binary, types);
  auto funcs = bin::funcs(binary, imports, types);
  auto globals = bin::globals(binary, imports);
  auto tables = bin::tables(binary, imports);
  auto memories = bin::memories(binary, imports);
  return bin::exports(binary, funcs, globals, tables, memories);
}

}  // namespace bin
}  // namespace wasm
