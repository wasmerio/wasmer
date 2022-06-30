#include "wasm.h"
#include "wasm.hh"

#include "wasm-v8.cc"

using namespace wasm;

extern "C" {

///////////////////////////////////////////////////////////////////////////////
// Auxiliaries

// Backing implementation

extern "C++" {

template<class T>
struct borrowed_vec {
  vec<T> it;
  borrowed_vec(vec<T>&& v) : it(std::move(v)) {}
  borrowed_vec(borrowed_vec<T>&& that) : it(std::move(that.it)) {}
  ~borrowed_vec() { it.release(); }
};

}  // extern "C++"


#define WASM_DEFINE_OWN(name, Name) \
  struct wasm_##name##_t : Name {}; \
  \
  void wasm_##name##_delete(wasm_##name##_t* x) { \
    delete x; \
  } \
  \
  extern "C++" inline auto hide_##name(Name* x) -> wasm_##name##_t* { \
    return static_cast<wasm_##name##_t*>(x); \
  } \
  extern "C++" inline auto hide_##name(const Name* x) -> const wasm_##name##_t* { \
    return static_cast<const wasm_##name##_t*>(x); \
  } \
  extern "C++" inline auto reveal_##name(wasm_##name##_t* x) -> Name* { \
    return x; \
  } \
  extern "C++" inline auto reveal_##name(const wasm_##name##_t* x) -> const Name* { \
    return x; \
  } \
  extern "C++" inline auto get_##name(own<Name>& x) -> wasm_##name##_t* { \
    return hide_##name(x.get()); \
  } \
  extern "C++" inline auto get_##name(const own<Name>& x) -> const wasm_##name##_t* { \
    return hide_##name(x.get()); \
  } \
  extern "C++" inline auto release_##name(own<Name>&& x) -> wasm_##name##_t* { \
    return hide_##name(x.release()); \
  } \
  extern "C++" inline auto adopt_##name(wasm_##name##_t* x) -> own<Name> { \
    return make_own(x); \
  }


// Vectors

#define WASM_DEFINE_VEC_BASE(name, Name, vec, plainvec, ptr_or_none) \
  static_assert( \
    sizeof(wasm_##name##_vec_t) == sizeof(vec<Name>), \
    "C/C++ incompatibility" \
  ); \
  static_assert( \
    sizeof(wasm_##name##_t ptr_or_none) == sizeof(vec<Name>::elem_type), \
    "C/C++ incompatibility" \
  ); \
  \
  extern "C++" inline auto hide_##name##_vec(vec<Name>& v) \
  -> wasm_##name##_vec_t* { \
    return reinterpret_cast<wasm_##name##_vec_t*>(&v); \
  } \
  extern "C++" inline auto hide_##name##_vec(const vec<Name>& v) \
  -> const wasm_##name##_vec_t* { \
    return reinterpret_cast<const wasm_##name##_vec_t*>(&v); \
  } \
  extern "C++" inline auto hide_##name##_vec(vec<Name>::elem_type* v) \
  -> wasm_##name##_t ptr_or_none* { \
    return reinterpret_cast<wasm_##name##_t ptr_or_none*>(v); \
  } \
  extern "C++" inline auto hide_##name##_vec(const vec<Name>::elem_type* v) \
  -> wasm_##name##_t ptr_or_none const* { \
    return reinterpret_cast<wasm_##name##_t ptr_or_none const*>(v); \
  } \
  extern "C++" inline auto reveal_##name##_vec(wasm_##name##_vec_t* v) \
  -> plainvec<Name*>* { \
    return reinterpret_cast<plainvec<Name*>*>(v); \
  } \
  extern "C++" inline auto reveal_##name##_vec(const wasm_##name##_vec_t* v) \
  -> const plainvec<Name*>* { \
    return reinterpret_cast<const plainvec<Name*>*>(v); \
  } \
  extern "C++" inline auto reveal_##name##_vec(wasm_##name##_t ptr_or_none* v) \
  -> vec<Name>::elem_type* { \
    return reinterpret_cast<vec<Name>::elem_type*>(v); \
  } \
  extern "C++" inline auto reveal_##name##_vec(wasm_##name##_t ptr_or_none const* v) \
  -> const vec<Name>::elem_type* { \
    return reinterpret_cast<const vec<Name>::elem_type*>(v); \
  } \
  extern "C++" inline auto get_##name##_vec(vec<Name>& v) \
  -> wasm_##name##_vec_t { \
    wasm_##name##_vec_t v2 = { v.size(), hide_##name##_vec(v.get()) }; \
    return v2; \
  } \
  extern "C++" inline auto get_##name##_vec(const vec<Name>& v) \
  -> const wasm_##name##_vec_t { \
    wasm_##name##_vec_t v2 = { \
      v.size(), const_cast<wasm_##name##_t ptr_or_none*>(hide_##name##_vec(v.get())) }; \
    return v2; \
  } \
  extern "C++" inline auto release_##name##_vec(vec<Name>&& v) \
  -> wasm_##name##_vec_t { \
    wasm_##name##_vec_t v2 = { v.size(), hide_##name##_vec(v.release()) }; \
    return v2; \
  } \
  extern "C++" inline auto adopt_##name##_vec(wasm_##name##_vec_t* v) \
  -> vec<Name> { \
    return vec<Name>::adopt(v->size, reveal_##name##_vec(v->data)); \
  } \
  extern "C++" inline auto borrow_##name##_vec(const wasm_##name##_vec_t* v) \
  -> borrowed_vec<vec<Name>::elem_type> { \
    return borrowed_vec<vec<Name>::elem_type>(vec<Name>::adopt(v->size, reveal_##name##_vec(v->data))); \
  } \
  \
  void wasm_##name##_vec_new_uninitialized( \
    wasm_##name##_vec_t* out, size_t size \
  ) { \
    *out = release_##name##_vec(vec<Name>::make_uninitialized(size)); \
  } \
  void wasm_##name##_vec_new_empty(wasm_##name##_vec_t* out) { \
    wasm_##name##_vec_new_uninitialized(out, 0); \
  } \
  \
  void wasm_##name##_vec_delete(wasm_##name##_vec_t* v) { \
    adopt_##name##_vec(v); \
  }

// Vectors with no ownership management of elements
#define WASM_DEFINE_VEC_PLAIN(name, Name) \
  WASM_DEFINE_VEC_BASE(name, Name, vec, vec, ) \
  \
  void wasm_##name##_vec_new( \
    wasm_##name##_vec_t* out, \
    size_t size, \
    const wasm_##name##_t data[] \
  ) { \
    auto v2 = vec<Name>::make_uninitialized(size); \
    if (v2.size() != 0) { \
      memcpy(v2.get(), data, size * sizeof(wasm_##name##_t)); \
    } \
    *out = release_##name##_vec(std::move(v2)); \
  } \
  \
  void wasm_##name##_vec_copy( \
    wasm_##name##_vec_t* out, const wasm_##name##_vec_t* v \
  ) { \
    wasm_##name##_vec_new(out, v->size, v->data); \
  }

// Vectors that own their elements
#define WASM_DEFINE_VEC_OWN(name, Name) \
  WASM_DEFINE_VEC_BASE(name, Name, ownvec, vec, *) \
  \
  void wasm_##name##_vec_new( \
    wasm_##name##_vec_t* out, \
    size_t size, \
    wasm_##name##_t* const data[] \
  ) { \
    auto v2 = ownvec<Name>::make_uninitialized(size); \
    for (size_t i = 0; i < v2.size(); ++i) { \
      v2[i] = adopt_##name(data[i]); \
    } \
    *out = release_##name##_vec(std::move(v2)); \
  } \
  \
  void wasm_##name##_vec_copy( \
    wasm_##name##_vec_t* out, const wasm_##name##_vec_t* v \
  ) { \
    auto v2 = ownvec<Name>::make_uninitialized(v->size); \
    for (size_t i = 0; i < v2.size(); ++i) { \
      v2[i] = adopt_##name(wasm_##name##_copy(v->data[i])); \
    } \
    *out = release_##name##_vec(std::move(v2)); \
  }

extern "C++" {
template<class T>
inline auto is_empty(T* p) -> bool { return !p; }
}


// Byte vectors

using byte = byte_t;
WASM_DEFINE_VEC_PLAIN(byte, byte)


///////////////////////////////////////////////////////////////////////////////
// Runtime Environment

// Configuration

WASM_DEFINE_OWN(config, Config)

wasm_config_t* wasm_config_new() {
  return release_config(Config::make());
}


// Engine

WASM_DEFINE_OWN(engine, Engine)

wasm_engine_t* wasm_engine_new() {
  return release_engine(Engine::make());
}

wasm_engine_t* wasm_engine_new_with_config(wasm_config_t* config) {
  return release_engine(Engine::make(adopt_config(config)));
}


// Stores

WASM_DEFINE_OWN(store, Store)

wasm_store_t* wasm_store_new(wasm_engine_t* engine) {
  return release_store(Store::make(engine));
};


///////////////////////////////////////////////////////////////////////////////
// Type Representations

// Type attributes

extern "C++" inline auto hide_mutability(Mutability mutability) -> wasm_mutability_t {
  return static_cast<wasm_mutability_t>(mutability);
}

extern "C++" inline auto reveal_mutability(wasm_mutability_t mutability) -> Mutability {
  return static_cast<Mutability>(mutability);
}


extern "C++" inline auto hide_limits(const Limits& limits) -> const wasm_limits_t* {
  return reinterpret_cast<const wasm_limits_t*>(&limits);
}

extern "C++" inline auto reveal_limits(wasm_limits_t limits) -> Limits {
  return Limits(limits.min, limits.max);
}


extern "C++" inline auto hide_valkind(ValKind kind) -> wasm_valkind_t {
  return static_cast<wasm_valkind_t>(kind);
}

extern "C++" inline auto reveal_valkind(wasm_valkind_t kind) -> ValKind {
  return static_cast<ValKind>(kind);
}


extern "C++" inline auto hide_externkind(ExternKind kind) -> wasm_externkind_t {
  return static_cast<wasm_externkind_t>(kind);
}

extern "C++" inline auto reveal_externkind(wasm_externkind_t kind) -> ExternKind {
  return static_cast<ExternKind>(kind);
}



// Generic

#define WASM_DEFINE_TYPE(name, Name) \
  WASM_DEFINE_OWN(name, Name) \
  WASM_DEFINE_VEC_OWN(name, Name) \
  \
  wasm_##name##_t* wasm_##name##_copy(wasm_##name##_t* t) { \
    return release_##name(t->copy()); \
  }


// Value Types

WASM_DEFINE_TYPE(valtype, ValType)

wasm_valtype_t* wasm_valtype_new(wasm_valkind_t k) {
  return release_valtype(ValType::make(reveal_valkind(k)));
}

wasm_valkind_t wasm_valtype_kind(const wasm_valtype_t* t) {
  return hide_valkind(t->kind());
}


// Function Types

WASM_DEFINE_TYPE(functype, FuncType)

wasm_functype_t* wasm_functype_new(
  wasm_valtype_vec_t* params, wasm_valtype_vec_t* results
) {
  return release_functype(
    FuncType::make(adopt_valtype_vec(params), adopt_valtype_vec(results)));
}

const wasm_valtype_vec_t* wasm_functype_params(const wasm_functype_t* ft) {
  return hide_valtype_vec(ft->params());
}

const wasm_valtype_vec_t* wasm_functype_results(const wasm_functype_t* ft) {
  return hide_valtype_vec(ft->results());
}


// Global Types

WASM_DEFINE_TYPE(globaltype, GlobalType)

wasm_globaltype_t* wasm_globaltype_new(
  wasm_valtype_t* content, wasm_mutability_t mutability
) {
  return release_globaltype(GlobalType::make(
    adopt_valtype(content),
    reveal_mutability(mutability)
  ));
}

const wasm_valtype_t* wasm_globaltype_content(const wasm_globaltype_t* gt) {
  return hide_valtype(gt->content());
}

wasm_mutability_t wasm_globaltype_mutability(const wasm_globaltype_t* gt) {
  return hide_mutability(gt->mutability());
}


// Table Types

WASM_DEFINE_TYPE(tabletype, TableType)

wasm_tabletype_t* wasm_tabletype_new(
  wasm_valtype_t* element, const wasm_limits_t* limits
) {
  return release_tabletype(TableType::make(adopt_valtype(element), reveal_limits(*limits)));
}

const wasm_valtype_t* wasm_tabletype_element(const wasm_tabletype_t* tt) {
  return hide_valtype(tt->element());
}

const wasm_limits_t* wasm_tabletype_limits(const wasm_tabletype_t* tt) {
  return hide_limits(tt->limits());
}


// Memory Types

WASM_DEFINE_TYPE(memorytype, MemoryType)

wasm_memorytype_t* wasm_memorytype_new(const wasm_limits_t* limits) {
  return release_memorytype(MemoryType::make(reveal_limits(*limits)));
}

const wasm_limits_t* wasm_memorytype_limits(const wasm_memorytype_t* mt) {
  return hide_limits(mt->limits());
}


// Extern Types

WASM_DEFINE_TYPE(externtype, ExternType)

wasm_externkind_t wasm_externtype_kind(const wasm_externtype_t* et) {
  return hide_externkind(et->kind());
}

wasm_externtype_t* wasm_functype_as_externtype(wasm_functype_t* ft) {
  return hide_externtype(static_cast<ExternType*>(ft));
}
wasm_externtype_t* wasm_globaltype_as_externtype(wasm_globaltype_t* gt) {
  return hide_externtype(static_cast<ExternType*>(gt));
}
wasm_externtype_t* wasm_tabletype_as_externtype(wasm_tabletype_t* tt) {
  return hide_externtype(static_cast<ExternType*>(tt));
}
wasm_externtype_t* wasm_memorytype_as_externtype(wasm_memorytype_t* mt) {
  return hide_externtype(static_cast<ExternType*>(mt));
}

const wasm_externtype_t* wasm_functype_as_externtype_const(
  const wasm_functype_t* ft
) {
  return hide_externtype(static_cast<const ExternType*>(ft));
}
const wasm_externtype_t* wasm_globaltype_as_externtype_const(
  const wasm_globaltype_t* gt
) {
  return hide_externtype(static_cast<const ExternType*>(gt));
}
const wasm_externtype_t* wasm_tabletype_as_externtype_const(
  const wasm_tabletype_t* tt
) {
  return hide_externtype(static_cast<const ExternType*>(tt));
}
const wasm_externtype_t* wasm_memorytype_as_externtype_const(
  const wasm_memorytype_t* mt
) {
  return hide_externtype(static_cast<const ExternType*>(mt));
}

wasm_functype_t* wasm_externtype_as_functype(wasm_externtype_t* et) {
  return et->kind() == ExternKind::FUNC
    ? hide_functype(static_cast<FuncType*>(reveal_externtype(et))) : nullptr;
}
wasm_globaltype_t* wasm_externtype_as_globaltype(wasm_externtype_t* et) {
  return et->kind() == ExternKind::GLOBAL
    ? hide_globaltype(static_cast<GlobalType*>(reveal_externtype(et))) : nullptr;
}
wasm_tabletype_t* wasm_externtype_as_tabletype(wasm_externtype_t* et) {
  return et->kind() == ExternKind::TABLE
    ? hide_tabletype(static_cast<TableType*>(reveal_externtype(et))) : nullptr;
}
wasm_memorytype_t* wasm_externtype_as_memorytype(wasm_externtype_t* et) {
  return et->kind() == ExternKind::MEMORY
    ? hide_memorytype(static_cast<MemoryType*>(reveal_externtype(et))) : nullptr;
}

const wasm_functype_t* wasm_externtype_as_functype_const(
  const wasm_externtype_t* et
) {
  return et->kind() == ExternKind::FUNC
    ? hide_functype(static_cast<const FuncType*>(reveal_externtype(et))) : nullptr;
}
const wasm_globaltype_t* wasm_externtype_as_globaltype_const(
  const wasm_externtype_t* et
) {
  return et->kind() == ExternKind::GLOBAL
    ? hide_globaltype(static_cast<const GlobalType*>(reveal_externtype(et))) : nullptr;
}
const wasm_tabletype_t* wasm_externtype_as_tabletype_const(
  const wasm_externtype_t* et
) {
  return et->kind() == ExternKind::TABLE
    ? hide_tabletype(static_cast<const TableType*>(reveal_externtype(et))) : nullptr;
}
const wasm_memorytype_t* wasm_externtype_as_memorytype_const(
  const wasm_externtype_t* et
) {
  return et->kind() == ExternKind::MEMORY
    ? hide_memorytype(static_cast<const MemoryType*>(reveal_externtype(et))) : nullptr;
}


// Import Types

WASM_DEFINE_TYPE(importtype, ImportType)

wasm_importtype_t* wasm_importtype_new(
  wasm_name_t* module, wasm_name_t* name, wasm_externtype_t* type
) {
  return release_importtype(
    ImportType::make(adopt_byte_vec(module), adopt_byte_vec(name), adopt_externtype(type)));
}

const wasm_name_t* wasm_importtype_module(const wasm_importtype_t* it) {
  return hide_byte_vec(it->module());
}

const wasm_name_t* wasm_importtype_name(const wasm_importtype_t* it) {
  return hide_byte_vec(it->name());
}

const wasm_externtype_t* wasm_importtype_type(const wasm_importtype_t* it) {
  return hide_externtype(it->type());
}


// Export Types

WASM_DEFINE_TYPE(exporttype, ExportType)

wasm_exporttype_t* wasm_exporttype_new(
  wasm_name_t* name, wasm_externtype_t* type
) {
  return release_exporttype(
    ExportType::make(adopt_byte_vec(name), adopt_externtype(type)));
}

const wasm_name_t* wasm_exporttype_name(const wasm_exporttype_t* et) {
  return hide_byte_vec(et->name());
}

const wasm_externtype_t* wasm_exporttype_type(const wasm_exporttype_t* et) {
  return hide_externtype(et->type());
}


///////////////////////////////////////////////////////////////////////////////
// Runtime Values

// References

#define WASM_DEFINE_REF_BASE(name, Name) \
  WASM_DEFINE_OWN(name, Name) \
  \
  wasm_##name##_t* wasm_##name##_copy(const wasm_##name##_t* t) { \
    return release_##name(t->copy()); \
  } \
  \
  bool wasm_##name##_same(const wasm_##name##_t* t1, const wasm_##name##_t* t2) { \
    return t1->same(t2); \
  } \
  \
  void* wasm_##name##_get_host_info(const wasm_##name##_t* r) { \
    return r->get_host_info(); \
  } \
  void wasm_##name##_set_host_info(wasm_##name##_t* r, void* info) { \
    r->set_host_info(info); \
  } \
  void wasm_##name##_set_host_info_with_finalizer( \
    wasm_##name##_t* r, void* info, void (*finalizer)(void*) \
  ) { \
    r->set_host_info(info, finalizer); \
  }

#define WASM_DEFINE_REF(name, Name) \
  WASM_DEFINE_REF_BASE(name, Name) \
  \
  wasm_ref_t* wasm_##name##_as_ref(wasm_##name##_t* r) { \
    return hide_ref(static_cast<Ref*>(reveal_##name(r))); \
  } \
  wasm_##name##_t* wasm_ref_as_##name(wasm_ref_t* r) { \
    return hide_##name(static_cast<Name*>(reveal_ref(r))); \
  } \
  \
  const wasm_ref_t* wasm_##name##_as_ref_const(const wasm_##name##_t* r) { \
    return hide_ref(static_cast<const Ref*>(reveal_##name(r))); \
  } \
  const wasm_##name##_t* wasm_ref_as_##name##_const(const wasm_ref_t* r) { \
    return hide_##name(static_cast<const Name*>(reveal_ref(r))); \
  }

#define WASM_DEFINE_SHARABLE_REF(name, Name) \
  WASM_DEFINE_REF(name, Name) \
  WASM_DEFINE_OWN(shared_##name, Shared<Name>)


WASM_DEFINE_REF_BASE(ref, Ref)


// Values

extern "C++" {

inline auto is_empty(wasm_val_t v) -> bool {
 return !is_ref(reveal_valkind(v.kind)) || !v.of.ref;
}

inline auto hide_val(Val v) -> wasm_val_t {
  wasm_val_t v2 = { hide_valkind(v.kind()) };
  switch (v.kind()) {
    case ValKind::I32: v2.of.i32 = v.i32(); break;
    case ValKind::I64: v2.of.i64 = v.i64(); break;
    case ValKind::F32: v2.of.f32 = v.f32(); break;
    case ValKind::F64: v2.of.f64 = v.f64(); break;
    case ValKind::ANYREF:
    case ValKind::FUNCREF: v2.of.ref = hide_ref(v.ref()); break;
    default: assert(false);
  }
  return v2;
}

inline auto release_val(Val v) -> wasm_val_t {
  wasm_val_t v2 = { hide_valkind(v.kind()) };
  switch (v.kind()) {
    case ValKind::I32: v2.of.i32 = v.i32(); break;
    case ValKind::I64: v2.of.i64 = v.i64(); break;
    case ValKind::F32: v2.of.f32 = v.f32(); break;
    case ValKind::F64: v2.of.f64 = v.f64(); break;
    case ValKind::ANYREF:
    case ValKind::FUNCREF: v2.of.ref = release_ref(v.release_ref()); break;
    default: assert(false);
  }
  return v2;
}

inline auto adopt_val(wasm_val_t v) -> Val {
  switch (reveal_valkind(v.kind)) {
    case ValKind::I32: return Val(v.of.i32);
    case ValKind::I64: return Val(v.of.i64);
    case ValKind::F32: return Val(v.of.f32);
    case ValKind::F64: return Val(v.of.f64);
    case ValKind::ANYREF:
    case ValKind::FUNCREF: return Val(adopt_ref(v.of.ref));
    default: assert(false);
  }
}

struct borrowed_val {
  Val it;
  borrowed_val(Val&& v) : it(std::move(v)) {}
  borrowed_val(borrowed_val&& that) : it(std::move(that.it)) {}
  ~borrowed_val() { if (it.is_ref()) it.release_ref().release(); }
};

inline auto borrow_val(const wasm_val_t* v) -> borrowed_val {
  Val v2;
  switch (reveal_valkind(v->kind)) {
    case ValKind::I32: v2 = Val(v->of.i32); break;
    case ValKind::I64: v2 = Val(v->of.i64); break;
    case ValKind::F32: v2 = Val(v->of.f32); break;
    case ValKind::F64: v2 = Val(v->of.f64); break;
    case ValKind::ANYREF:
    case ValKind::FUNCREF: v2 = Val(adopt_ref(v->of.ref)); break;
    default: assert(false);
  }
  return borrowed_val(std::move(v2));
}

}  // extern "C++"


WASM_DEFINE_VEC_BASE(val, Val, vec, vec, )

void wasm_val_vec_new(
  wasm_val_vec_t* out, size_t size, wasm_val_t const data[]
) {
  auto v2 = vec<Val>::make_uninitialized(size);
  for (size_t i = 0; i < v2.size(); ++i) {
    v2[i] = adopt_val(data[i]);
  }
  *out = release_val_vec(std::move(v2));
}

void wasm_val_vec_copy(wasm_val_vec_t* out, const wasm_val_vec_t* v) {
  auto v2 = vec<Val>::make_uninitialized(v->size);
  for (size_t i = 0; i < v2.size(); ++i) {
    wasm_val_t val;
    wasm_val_copy(&v->data[i], &val);
    v2[i] = adopt_val(val);
  }
  *out = release_val_vec(std::move(v2));
}


void wasm_val_delete(wasm_val_t* v) {
  if (is_ref(reveal_valkind(v->kind))) {
    adopt_ref(v->of.ref);
  }
}

void wasm_val_copy(wasm_val_t* out, const wasm_val_t* v) {
  *out = *v;
  if (is_ref(reveal_valkind(v->kind))) {
    out->of.ref = v->of.ref ? release_ref(v->of.ref->copy()) : nullptr;
  }
}


///////////////////////////////////////////////////////////////////////////////
// Runtime Objects

// Frames

WASM_DEFINE_OWN(frame, Frame)
WASM_DEFINE_VEC_OWN(frame, Frame)

wasm_frame_t* wasm_frame_copy(const wasm_frame_t* frame) {
  return release_frame(frame->copy());
}

wasm_instance_t* wasm_frame_instance(const wasm_frame_t* frame);
// Defined below along with wasm_instance_t.

uint32_t wasm_frame_func_index(const wasm_frame_t* frame) {
  return reveal_frame(frame)->func_index();
}

size_t wasm_frame_func_offset(const wasm_frame_t* frame) {
  return reveal_frame(frame)->func_offset();
}

size_t wasm_frame_module_offset(const wasm_frame_t* frame) {
  return reveal_frame(frame)->module_offset();
}


// Traps

WASM_DEFINE_REF(trap, Trap)

wasm_trap_t* wasm_trap_new(wasm_store_t* store, const wasm_message_t* message) {
  auto message_ = borrow_byte_vec(message);
  return release_trap(Trap::make(store, message_.it));
}

void wasm_trap_message(const wasm_trap_t* trap, wasm_message_t* out) {
  *out = release_byte_vec(reveal_trap(trap)->message());
}

wasm_frame_t* wasm_trap_origin(const wasm_trap_t* trap) {
  return release_frame(reveal_trap(trap)->origin());
}

void wasm_trap_trace(const wasm_trap_t* trap, wasm_frame_vec_t* out) {
  *out = release_frame_vec(reveal_trap(trap)->trace());
}


// Foreign Objects

WASM_DEFINE_REF(foreign, Foreign)

wasm_foreign_t* wasm_foreign_new(wasm_store_t* store) {
  return release_foreign(Foreign::make(store));
}


// Modules

WASM_DEFINE_SHARABLE_REF(module, Module)

bool wasm_module_validate(wasm_store_t* store, const wasm_byte_vec_t* binary) {
  auto binary_ = borrow_byte_vec(binary);
  return Module::validate(store, binary_.it);
}

wasm_module_t* wasm_module_new(
  wasm_store_t* store, const wasm_byte_vec_t* binary
) {
  auto binary_ = borrow_byte_vec(binary);
  return release_module(Module::make(store, binary_.it));
}


void wasm_module_imports(
  const wasm_module_t* module, wasm_importtype_vec_t* out
) {
  *out = release_importtype_vec(reveal_module(module)->imports());
}

void wasm_module_exports(
  const wasm_module_t* module, wasm_exporttype_vec_t* out
) {
  *out = release_exporttype_vec(reveal_module(module)->exports());
}

void wasm_module_serialize(const wasm_module_t* module, wasm_byte_vec_t* out) {
  *out = release_byte_vec(reveal_module(module)->serialize());
}

wasm_module_t* wasm_module_deserialize(
  wasm_store_t* store, const wasm_byte_vec_t* binary
) {
  auto binary_ = borrow_byte_vec(binary);
  return release_module(Module::deserialize(store, binary_.it));
}

wasm_shared_module_t* wasm_module_share(const wasm_module_t* module) {
  return release_shared_module(reveal_module(module)->share());
}

wasm_module_t* wasm_module_obtain(wasm_store_t* store, const wasm_shared_module_t* shared) {
  return release_module(Module::obtain(store, shared));
}


// Function Instances

WASM_DEFINE_REF(func, Func)

extern "C++" {

auto wasm_callback(
  void* env, const vec<Val>& args, vec<Val>& results
) -> own<Trap> {
  auto f = reinterpret_cast<wasm_func_callback_t>(env);
  return adopt_trap(f(hide_val_vec(args), hide_val_vec(results)));
}

struct wasm_callback_env_t {
  wasm_func_callback_with_env_t callback;
  void* env;
  void (*finalizer)(void*);
};

auto wasm_callback_with_env(
  void* env, const vec<Val>& args, vec<Val>& results
) -> own<Trap> {
  auto t = static_cast<wasm_callback_env_t*>(env);
  return adopt_trap(t->callback(t->env, hide_val_vec(args), hide_val_vec(results)));
}

void wasm_callback_env_finalizer(void* env) {
  auto t = static_cast<wasm_callback_env_t*>(env);
  if (t->finalizer) t->finalizer(t->env);
  delete t;
}

}  // extern "C++"

wasm_func_t* wasm_func_new(
  wasm_store_t* store, const wasm_functype_t* type,
  wasm_func_callback_t callback
) {
  return release_func(Func::make(
    store, type, wasm_callback, reinterpret_cast<void*>(callback)));
}

wasm_functype_t* wasm_func_type(const wasm_func_t* func) {
  return release_functype(func->type());
}

size_t wasm_func_param_arity(const wasm_func_t* func) {
  return func->param_arity();
}

size_t wasm_func_result_arity(const wasm_func_t* func) {
  return func->result_arity();
}

wasm_trap_t* wasm_func_call(
  const wasm_func_t* func, const wasm_val_vec_t* args, wasm_val_vec_t* results
) {
  auto args_ = borrow_val_vec(args);
  auto results_ = borrow_val_vec(results);
  return release_trap(func->call(args_.it, results_.it));
}


// Global Instances

WASM_DEFINE_REF(global, Global)

wasm_global_t* wasm_global_new(
  wasm_store_t* store, const wasm_globaltype_t* type, const wasm_val_t* val
) {
  auto val_ = borrow_val(val);
  return release_global(Global::make(store, type, val_.it));
}

wasm_globaltype_t* wasm_global_type(const wasm_global_t* global) {
  return release_globaltype(global->type());
}

void wasm_global_get(const wasm_global_t* global, wasm_val_t* out) {
  *out = release_val(global->get());
}

void wasm_global_set(wasm_global_t* global, const wasm_val_t* val) {
  auto val_ = borrow_val(val);
  global->set(val_.it);
}


// Table Instances

WASM_DEFINE_REF(table, Table)

wasm_table_t* wasm_table_new(
  wasm_store_t* store, const wasm_tabletype_t* type, wasm_ref_t* ref
) {
  return release_table(Table::make(store, type, ref));
}

wasm_tabletype_t* wasm_table_type(const wasm_table_t* table) {
  return release_tabletype(table->type());
}

wasm_ref_t* wasm_table_get(const wasm_table_t* table, wasm_table_size_t index) {
  return release_ref(table->get(index));
}

bool wasm_table_set(
  wasm_table_t* table, wasm_table_size_t index, wasm_ref_t* ref
) {
  return table->set(index, ref);
}

wasm_table_size_t wasm_table_size(const wasm_table_t* table) {
  return table->size();
}

bool wasm_table_grow(
  wasm_table_t* table, wasm_table_size_t delta, wasm_ref_t* ref
) {
  return table->grow(delta, ref);
}


// Memory Instances

WASM_DEFINE_REF(memory, Memory)

wasm_memory_t* wasm_memory_new(
  wasm_store_t* store, const wasm_memorytype_t* type
) {
  return release_memory(Memory::make(store, type));
}

wasm_memorytype_t* wasm_memory_type(const wasm_memory_t* memory) {
  return release_memorytype(memory->type());
}

wasm_byte_t* wasm_memory_data(wasm_memory_t* memory) {
  return memory->data();
}

size_t wasm_memory_data_size(const wasm_memory_t* memory) {
  return memory->data_size();
}

wasm_memory_pages_t wasm_memory_size(const wasm_memory_t* memory) {
  return memory->size();
}

bool wasm_memory_grow(wasm_memory_t* memory, wasm_memory_pages_t delta) {
  return memory->grow(delta);
}


// Externals

WASM_DEFINE_REF(extern, Extern)
WASM_DEFINE_VEC_OWN(extern, Extern)

wasm_externkind_t wasm_extern_kind(const wasm_extern_t* external) {
  return hide_externkind(external->kind());
}
wasm_externtype_t* wasm_extern_type(const wasm_extern_t* external) {
  return release_externtype(external->type());
}

wasm_extern_t* wasm_func_as_extern(wasm_func_t* func) {
  return hide_extern(static_cast<Extern*>(reveal_func(func)));
}
wasm_extern_t* wasm_global_as_extern(wasm_global_t* global) {
  return hide_extern(static_cast<Extern*>(reveal_global(global)));
}
wasm_extern_t* wasm_table_as_extern(wasm_table_t* table) {
  return hide_extern(static_cast<Extern*>(reveal_table(table)));
}
wasm_extern_t* wasm_memory_as_extern(wasm_memory_t* memory) {
  return hide_extern(static_cast<Extern*>(reveal_memory(memory)));
}

const wasm_extern_t* wasm_func_as_extern_const(const wasm_func_t* func) {
  return hide_extern(static_cast<const Extern*>(reveal_func(func)));
}
const wasm_extern_t* wasm_global_as_extern_const(const wasm_global_t* global) {
  return hide_extern(static_cast<const Extern*>(reveal_global(global)));
}
const wasm_extern_t* wasm_table_as_extern_const(const wasm_table_t* table) {
  return hide_extern(static_cast<const Extern*>(reveal_table(table)));
}
const wasm_extern_t* wasm_memory_as_extern_const(const wasm_memory_t* memory) {
  return hide_extern(static_cast<const Extern*>(reveal_memory(memory)));
}

wasm_func_t* wasm_extern_as_func(wasm_extern_t* external) {
  return hide_func(external->func());
}
wasm_global_t* wasm_extern_as_global(wasm_extern_t* external) {
  return hide_global(external->global());
}
wasm_table_t* wasm_extern_as_table(wasm_extern_t* external) {
  return hide_table(external->table());
}
wasm_memory_t* wasm_extern_as_memory(wasm_extern_t* external) {
  return hide_memory(external->memory());
}

const wasm_func_t* wasm_extern_as_func_const(const wasm_extern_t* external) {
  return hide_func(external->func());
}
const wasm_global_t* wasm_extern_as_global_const(const wasm_extern_t* external) {
  return hide_global(external->global());
}
const wasm_table_t* wasm_extern_as_table_const(const wasm_extern_t* external) {
  return hide_table(external->table());
}
const wasm_memory_t* wasm_extern_as_memory_const(const wasm_extern_t* external) {
  return hide_memory(external->memory());
}


// Module Instances

WASM_DEFINE_REF(instance, Instance)

wasm_instance_t* wasm_instance_new(
  wasm_store_t* store,
  const wasm_module_t* module,
  const wasm_extern_vec_t* imports,
  wasm_trap_t** trap
) {
  own<Trap> error;
  auto imports_ = reveal_extern_vec(imports);
  auto instance =
    release_instance(Instance::make(store, module, *imports_, &error));
  if (trap) *trap = hide_trap(error.release());
  return instance;
}

void wasm_instance_exports(
  const wasm_instance_t* instance, wasm_extern_vec_t* out
) {
  *out = release_extern_vec(instance->exports());
}


wasm_instance_t* wasm_frame_instance(const wasm_frame_t* frame) {
  return hide_instance(reveal_frame(frame)->instance());
}

}  // extern "C"
