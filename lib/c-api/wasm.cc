// Implementation of the wasm-c-api C++ API for wasmer on top of the Wasmer C++
// API and the wasm C API.

#include <variant>

#include "wasm.h"
#include "wasm.hh"

#include "wasmer.hh"

using namespace wasm;

// TODO: these are all missing from wasmer's C API implementation
extern "C" {
WASM_API_EXTERN void wasm_config_delete(wasm_config_t *) { abort(); }
WASM_API_EXTERN void wasm_foreign_delete(wasm_foreign_t *) { abort(); }
WASM_API_EXTERN wasm_ref_t *wasm_module_as_ref(wasm_module_t *ptr) {
  return (wasm_ref_t *)ptr;
}
WASM_API_EXTERN wasm_ref_t *wasm_foreign_as_ref(wasm_foreign_t *ptr) {
  return (wasm_ref_t *)ptr;
}
WASM_API_EXTERN wasm_ref_t *wasm_func_as_ref(wasm_func_t *ptr) {
  return (wasm_ref_t *)wasm_func_as_extern(ptr);
}
WASM_API_EXTERN wasm_ref_t *wasm_instance_as_ref(wasm_instance_t *ptr) {
  return (wasm_ref_t *)ptr;
}
WASM_API_EXTERN wasm_ref_t *wasm_trap_as_ref(wasm_trap_t *ptr) {
  return (wasm_ref_t *)ptr;
}
WASM_API_EXTERN wasm_extern_t *wasm_ref_as_extern(wasm_ref_t *ptr) {
  return (wasm_extern_t *)ptr;
}
WASM_API_EXTERN const wasm_extern_t *
wasm_ref_as_extern_const(const wasm_ref_t *ptr) {
  return (wasm_extern_t *)ptr;
}
WASM_API_EXTERN wasm_globaltype_t *
wasm_globaltype_copy(const wasm_globaltype_t *) {
  abort();
}
WASM_API_EXTERN wasm_memorytype_t *
wasm_memorytype_copy(const wasm_memorytype_t *) {
  abort();
}
WASM_API_EXTERN wasm_importtype_t *
wasm_importtype_copy(const wasm_importtype_t *) {
  abort();
}
WASM_API_EXTERN wasm_exporttype_t *
wasm_exporttype_copy(const wasm_exporttype_t *) {
  abort();
}
WASM_API_EXTERN wasm_module_t *wasm_module_copy(const wasm_module_t *) {
  abort();
}
WASM_API_EXTERN wasm_foreign_t *wasm_foreign_copy(const wasm_foreign_t *) {
  abort();
}
WASM_API_EXTERN wasm_func_t *wasm_func_copy(const wasm_func_t *) { abort(); }
WASM_API_EXTERN wasm_instance_t *wasm_instance_copy(const wasm_instance_t *) {
  abort();
}
WASM_API_EXTERN wasm_trap_t *wasm_trap_copy(const wasm_trap_t *) { abort(); }
WASM_API_EXTERN void *wasm_ref_get_host_info(const wasm_ref_t *) { abort(); }
WASM_API_EXTERN void wasm_ref_set_host_info(wasm_ref_t *, void *) { abort(); }
WASM_API_EXTERN void wasm_ref_set_host_info_with_finalizer(wasm_ref_t *, void *,
                                                           void (*)(void *)) {
  abort();
}
WASM_API_EXTERN wasm_foreign_t *wasm_foreign_new(wasm_store_t *) { abort(); }
}

namespace {
struct wasmer_delete_c_type {
#define WASMER_DECLARE_DELETE(name)                                            \
  void operator()(wasm_##name##_t *ptr) { wasm_##name##_delete(ptr); }         \
  void operator()(const wasm_##name##_t *ptr) {                                \
    wasm_##name##_delete(const_cast<wasm_##name##_t *>(ptr));                  \
  }
  // This list should match all WASM_DECLARE_OWN(type) in wasm.h.
  WASMER_DECLARE_DELETE(config)
  WASMER_DECLARE_DELETE(engine)
  WASMER_DECLARE_DELETE(store)
  WASMER_DECLARE_DELETE(frame)
  WASMER_DECLARE_DELETE(valtype)
  WASMER_DECLARE_DELETE(functype)
  WASMER_DECLARE_DELETE(globaltype)
  WASMER_DECLARE_DELETE(tabletype)
  WASMER_DECLARE_DELETE(memorytype)
  WASMER_DECLARE_DELETE(externtype)
  WASMER_DECLARE_DELETE(importtype)
  WASMER_DECLARE_DELETE(exporttype)
  WASMER_DECLARE_DELETE(ref)
  WASMER_DECLARE_DELETE(trap)
  WASMER_DECLARE_DELETE(foreign)
  WASMER_DECLARE_DELETE(func)
  WASMER_DECLARE_DELETE(global)
  WASMER_DECLARE_DELETE(table)
  WASMER_DECLARE_DELETE(memory)
  WASMER_DECLARE_DELETE(extern)
  WASMER_DECLARE_DELETE(instance)
  WASMER_DECLARE_DELETE(module)
#undef WASMER_DECLARE_DELETE
};
} // namespace

// A smart pointer for the wasm C API types.
template <typename T> using c_own = std::unique_ptr<T, wasmer_delete_c_type>;
template <typename T> auto make_c_own(T *p) -> c_own<T> { return c_own<T>(p); }

// Type traits for own<T>. Use `remove_own<S>::type` to get T when S = own<T>.
template <typename T> struct remove_own { typedef T type; };
template <typename T> struct remove_own<own<T>> { typedef T type; };

// Conversion from a C wasm_##name##_vec_t* to a C++ ownvec<T>.
template <typename C, typename S1, typename S2>
ownvec<typename remove_own<S2>::type> c_vec_to_cxx_ownvec(C c_vec,
                                                          S2 (*convert)(S1)) {
  auto v =
      ownvec<typename remove_own<S2>::type>::make_uninitialized(c_vec->size);
  for (int i = 0; i < c_vec->size; ++i) {
    v[i] = convert(c_vec->data[i]);
  }
  return v;
}

// Conversion from a C wasm_##name##_vec_t* to a C++ vec<T>.
template <typename C, typename S1, typename S2>
vec<S2> c_vec_to_cxx_vec(C c_vec, S2 (*convert)(S1)) {
  auto v = vec<S2>::make_uninitialized(c_vec->size);
  for (int i = 0; i < c_vec->size; ++i) {
    v[i] = convert(c_vec->data[i]);
  }
  return v;
}

template <typename T> struct c_vec;
#define DEFINE_C_VEC_SPECIALIZATION(name, ptr_or_none)                         \
  template <> struct c_vec<wasm_##name##_t ptr_or_none> {                      \
    using type = wasm_##name##_vec_t;                                          \
    static constexpr auto new_empty = wasm_##name##_vec_new_empty;             \
    static constexpr auto new_uninitialized =                                  \
        wasm_##name##_vec_new_uninitialized;                                   \
    static constexpr auto new_ = wasm_##name##_vec_new;                        \
    static constexpr auto copy = wasm_##name##_vec_copy;                       \
    static constexpr auto delete_ = wasm_##name##_vec_delete;                  \
  };
// This list should match all WASM_DECLARE_VEC(type) in wasm.h.
DEFINE_C_VEC_SPECIALIZATION(byte, )
DEFINE_C_VEC_SPECIALIZATION(val, *)
DEFINE_C_VEC_SPECIALIZATION(frame, *)
DEFINE_C_VEC_SPECIALIZATION(extern, *)
DEFINE_C_VEC_SPECIALIZATION(valtype, *)
DEFINE_C_VEC_SPECIALIZATION(functype, *)
DEFINE_C_VEC_SPECIALIZATION(globaltype, *)
DEFINE_C_VEC_SPECIALIZATION(tabletype, *)
DEFINE_C_VEC_SPECIALIZATION(memorytype, *)
DEFINE_C_VEC_SPECIALIZATION(externtype, *)
DEFINE_C_VEC_SPECIALIZATION(importtype, *)
DEFINE_C_VEC_SPECIALIZATION(exporttype, *)

template <typename T, typename S1, typename S2>
typename c_vec<S2>::type cxx_ownvec_to_c_vec(ownvec<T> &&cxx_vec,
                                             S2 (*convert)(S1)) {
  typename c_vec<S2>::type v;
  c_vec<S2>::new_uninitialized(&v, cxx_vec.size());
  for (int i = 0, e = cxx_vec.size(); i != e; ++i) {
    v.data[i] = convert(cxx_vec[i].release());
  }
  return v;
}

template <typename T, typename S1, typename S2>
typename c_vec<S2>::type cxx_vec_to_c_vec(vec<T> &&cxx_vec, S2 (*convert)(S1)) {
  typename c_vec<S2>::type v;
  c_vec<S2>::new_uninitialized(&v, cxx_vec.size());
  for (int i = 0, e = cxx_vec.size(); i != e; ++i) {
    v.data[i] = convert(cxx_vec[i]);
  }
  return v;
}

template <typename Base, typename Derived> struct From : Base {
  static auto from(own<Base> &&base) -> own<Derived> {
    return make_own(from(base.release()));
  }
  /*
  static auto from(own<const Base> &&base) -> own<const Derived> {
    return make_own(from(base.release()));
  }
  static auto from(Base &base) -> Derived &{
    return static_cast<Derived &>(base);
  }
  static auto from(const Base &base) -> const Derived &{
    return static_cast<const Derived &>(base);
  }
  */
  static auto from(Base *base) -> Derived * {
    return static_cast<Derived *>(base);
  }
  static auto from(const Base *base) -> const Derived * {
    return static_cast<const Derived *>(base);
  }
};

class WASM_API_EXTERN WasmerConfig : public From<Config, WasmerConfig> {
public:
  explicit WasmerConfig() : config(wasm_config_new()) {}

  c_own<wasm_config_t> config;
};

void Config::destroy() { delete WasmerConfig::from(this); }

auto Config::make() -> own<Config> { return make_own(new WasmerConfig); }

// TODO: add custom config calls

class WASM_API_EXTERN WasmerEngine : public From<Engine, WasmerEngine> {
public:
  explicit WasmerEngine(c_own<wasm_engine_t> &&engine)
      : engine(std::move(engine)) {}

  c_own<wasm_engine_t> engine;
};

void Engine::destroy() { delete WasmerEngine::from(this); }

auto Engine::make(own<Config> &&config) -> own<Engine> {
  return make_own(new WasmerEngine(make_c_own(wasm_engine_new_with_config(
      WasmerConfig::from(std::move(config))->config.release()))));
}

class WASM_API_EXTERN WasmerStore : public From<Store, WasmerStore> {
public:
  explicit WasmerStore(c_own<wasm_store_t> &&store) : store(std::move(store)) {}

  static auto make(WasmerEngine *engine) -> own<WasmerStore> {
    return make_own(
        new WasmerStore(make_c_own(wasm_store_new(engine->engine.get()))));
  }

  c_own<wasm_store_t> store;
};

void Store::destroy() { delete WasmerStore::from(this); }

auto Store::make(Engine *engine) -> own<Store> {
  return WasmerStore::make(WasmerEngine::from(engine));
}

static ValKind c_valkind_to_cxx_valkind(wasm_valkind_t valkind) {
  switch (valkind) {
  case WASM_I32:
    return ValKind::I32;
  case WASM_I64:
    return ValKind::I64;
  case WASM_F32:
    return ValKind::F32;
  case WASM_F64:
    return ValKind::F64;
  case WASM_ANYREF:
    return ValKind::ANYREF;
  case WASM_FUNCREF:
    return ValKind::FUNCREF;
  }
}

static wasm_valkind_t cxx_valkind_to_c_valkind(ValKind valkind) {
  switch (valkind) {
  case ValKind::I32:
    return WASM_I32;
  case ValKind::I64:
    return WASM_I64;
  case ValKind::F32:
    return WASM_F32;
  case ValKind::F64:
    return WASM_F64;
  case ValKind::ANYREF:
    return WASM_ANYREF;
  case ValKind::FUNCREF:
    return WASM_FUNCREF;
  }
}

class WASM_API_EXTERN WasmerValType : public From<ValType, WasmerValType> {
public:
  explicit WasmerValType(ValKind kind)
      : valtype(wasm_valtype_new(cxx_valkind_to_c_valkind(kind))) {}

  auto copy() const -> own<WasmerValType> {
    return make_own(new WasmerValType(kind()));
  }

  auto kind() const -> ValKind {
    return c_valkind_to_cxx_valkind(wasm_valtype_kind(valtype.get()));
  }

  c_own<wasm_valtype_t> valtype;
};

auto ValType::make(ValKind kind) -> own<ValType> {
  return make_own(new WasmerValType(kind));
}

void ValType::destroy() { delete WasmerValType::from(this); }

auto ValType::copy() const -> own<ValType> {
  return WasmerValType::from(this)->copy();
}

auto ValType::kind() const -> ValKind {
  return WasmerValType::from(this)->kind();
}

ExternKind c_externkind_to_cxx_externkind(wasm_externkind_t kind) {
  switch (kind) {
  case WASM_EXTERN_FUNC:
    return ExternKind::FUNC;
  case WASM_EXTERN_GLOBAL:
    return ExternKind::GLOBAL;
  case WASM_EXTERN_TABLE:
    return ExternKind::TABLE;
  case WASM_EXTERN_MEMORY:
    return ExternKind::MEMORY;
  }
}

class WasmerExternType;

namespace {
struct ExternTypeWrapper {
  wasm_externkind_t c_kind() const { return wasm_externtype_kind(externtype); }
  const wasm_functype_t *func() const {
    return wasm_externtype_as_functype_const(externtype);
  }
  const wasm_globaltype_t *global() const {
    return wasm_externtype_as_globaltype_const(externtype);
  }
  const wasm_tabletype_t *table() const {
    return wasm_externtype_as_tabletype_const(externtype);
  }
  const wasm_memorytype_t *memory() const {
    return wasm_externtype_as_memorytype_const(externtype);
  }

  const wasm_externtype_t *externtype;
};
} // namespace

template <typename T, typename Derived>
struct WasmerExternTypeImpl : public From<T, Derived>,
                              public ExternTypeWrapper {
  WasmerExternTypeImpl(const wasm_externtype_t *externtype)
      : ExternTypeWrapper{externtype} {}
};

static own<ValType> c_valtype_to_cxx_ownvaltype(const wasm_valtype_t *c) {
  return ValType::make(c_valkind_to_cxx_valkind(wasm_valtype_kind(c)));
}

static wasm_valtype_t *cxx_ownvaltype_to_c_valtype(own<ValType> &&val_type) {
  return wasm_valtype_new(cxx_valkind_to_c_valkind(val_type->kind()));
}

static wasm_valtype_t *cxx_valtype_to_c_valtype(const ValType *val_type) {
  return wasm_valtype_new(cxx_valkind_to_c_valkind(val_type->kind()));
}

class WasmerFuncType : public WasmerExternTypeImpl<FuncType, WasmerFuncType> {
public:
  explicit WasmerFuncType(c_own<const wasm_functype_t> &&functype)
      : WasmerExternTypeImpl(wasm_functype_as_externtype_const(functype.get())),
        params_(c_vec_to_cxx_ownvec(wasm_functype_params(functype.get()),
                                    c_valtype_to_cxx_ownvaltype)),
        results_(c_vec_to_cxx_ownvec(wasm_functype_results(functype.get()),
                                     c_valtype_to_cxx_ownvaltype)),
        functype(std::move(functype)) {}

  static auto make(ownvec<ValType> &&params, ownvec<ValType> &&results)
      -> own<FuncType> {
    wasm_valtype_vec_t c_params =
        cxx_ownvec_to_c_vec(std::move(params), cxx_valtype_to_c_valtype);
    wasm_valtype_vec_t c_results =
        cxx_ownvec_to_c_vec(std::move(results), cxx_valtype_to_c_valtype);
    return make_own(new WasmerFuncType(
        make_c_own(wasm_functype_new(&c_params, &c_results))));
  }

  auto copy() const -> own<WasmerFuncType> {
    return make_own(
        new WasmerFuncType(make_c_own(wasm_functype_copy(functype.get()))));
  }

  auto params() const -> const ownvec<ValType> & { return params_; }
  auto results() const -> const ownvec<ValType> & { return results_; }

  ownvec<ValType> params_;
  ownvec<ValType> results_;
  c_own<const wasm_functype_t> functype;
};

auto FuncType::make(ownvec<ValType> &&params, ownvec<ValType> &&results)
    -> own<FuncType> {
  return WasmerFuncType::make(std::move(params), std::move(results));
}

void FuncType::destroy() { delete WasmerFuncType::from(this); }

auto FuncType::copy() const -> own<FuncType> {
  return WasmerFuncType::from(this)->copy();
}

auto FuncType::params() const -> const ownvec<ValType> & {
  return WasmerFuncType::from(this)->params();
}

auto FuncType::results() const -> const ownvec<ValType> & {
  return WasmerFuncType::from(this)->results();
}

Mutability c_mutability_to_cxx_mutability(wasm_mutability_t mutability) {
  switch (mutability) {
  case WASM_CONST:
    return Mutability::CONST;
  case WASM_VAR:
    return Mutability::VAR;
  }
}

wasm_mutability_t cxx_mutability_to_c_mutability(Mutability mutability) {
  switch (mutability) {
  case Mutability::CONST:
    return WASM_CONST;
  case Mutability::VAR:
    return WASM_VAR;
  }
}

class WASM_API_EXTERN WasmerGlobalType
    : public WasmerExternTypeImpl<GlobalType, WasmerGlobalType> {
public:
  explicit WasmerGlobalType(c_own<const wasm_globaltype_t> &&globaltype)
      : WasmerExternTypeImpl(
            wasm_globaltype_as_externtype_const(globaltype.get())),
        globaltype(std::move(globaltype)),
        valtype(c_valtype_to_cxx_ownvaltype(
            wasm_globaltype_content(globaltype.get()))) {}

  auto copy() const -> own<WasmerGlobalType> {
    return make_own(new WasmerGlobalType(
        make_c_own(wasm_globaltype_copy(globaltype.get()))));
  }

  auto content() const -> const ValType * { return valtype.get(); }

  auto mutability() const -> Mutability {
    return c_mutability_to_cxx_mutability(
        wasm_globaltype_mutability(globaltype.get()));
  }

  c_own<const wasm_globaltype_t> globaltype;
  own<ValType> valtype;
};

void GlobalType::destroy() { delete WasmerGlobalType::from(this); }

auto GlobalType::make(own<ValType> &&valtype, Mutability mutability)
    -> own<GlobalType> {
  auto element = cxx_valtype_to_c_valtype(valtype.get());
  auto cxx_mutability = cxx_mutability_to_c_mutability(mutability);
  return make_own(new WasmerGlobalType(
      make_c_own(wasm_globaltype_new(element, cxx_mutability))));
}

auto GlobalType::copy() const -> own<GlobalType> {
  return WasmerGlobalType::from(this)->copy();
}

auto GlobalType::content() const -> const ValType * {
  return WasmerGlobalType::from(this)->content();
}
auto GlobalType::mutability() const -> Mutability {
  return WasmerGlobalType::from(this)->mutability();
}

static Limits c_limits_to_cxx_limits(wasm_limits_t limits) {
  return Limits(limits.min, limits.max);
}
static wasm_limits_t cxx_limits_to_c_limits(Limits limits) {
  return wasm_limits_t{limits.min, limits.max};
}

class WASM_API_EXTERN WasmerTableType
    : public WasmerExternTypeImpl<TableType, WasmerTableType> {
public:
  explicit WasmerTableType(c_own<const wasm_tabletype_t> &&tabletype)
      : WasmerExternTypeImpl(
            wasm_tabletype_as_externtype_const(tabletype.get())),
        tabletype(std::move(tabletype)),
        element_(c_valtype_to_cxx_ownvaltype(
            wasm_tabletype_element(tabletype.get()))),
        limits_(
            c_limits_to_cxx_limits(*wasm_tabletype_limits(tabletype.get()))) {}

  static auto make(own<ValType> &&element, Limits limits)
      -> own<WasmerTableType> {
    auto c_element =
        make_c_own(cxx_ownvaltype_to_c_valtype(std::move(element)));
    auto c_limits = cxx_limits_to_c_limits(limits);
    return make_own(new WasmerTableType(
        make_c_own(wasm_tabletype_new(c_element.get(), &c_limits))));
  }

  auto element() const -> const ValType * { return element_.get(); }

  auto limits() const -> const Limits & { return limits_; }

  c_own<const wasm_tabletype_t> tabletype;
  own<ValType> element_;
  Limits limits_;
};

void TableType::destroy() { delete WasmerTableType::from(this); }

auto TableType::make(own<ValType> &&element, Limits limits) -> own<TableType> {
  return WasmerTableType::make(std::move(element), limits);
}

class WASM_API_EXTERN WasmerMemoryType
    : public From<MemoryType, WasmerMemoryType> {
public:
  explicit WasmerMemoryType(c_own<const wasm_memorytype_t> memorytype)
      : memorytype(std::move(memorytype)),
        limits_(
            c_limits_to_cxx_limits(*wasm_memorytype_limits(memorytype.get()))) {
  }

  static auto make(Limits limits) -> own<MemoryType> {
    auto l = cxx_limits_to_c_limits(limits);
    return make_own(new WasmerMemoryType(make_c_own(wasm_memorytype_new(&l))));
  }
  auto copy() const -> own<WasmerMemoryType> {
    return make_own(new WasmerMemoryType(
        make_c_own(wasm_memorytype_copy(memorytype.get()))));
  }

  auto limits() const -> const Limits & { return limits_; }

  c_own<const wasm_memorytype_t> memorytype;
  Limits limits_;
};

void MemoryType::destroy() { delete WasmerMemoryType::from(this); }

auto MemoryType::make(Limits limits) -> own<MemoryType> {
  return WasmerMemoryType::make(limits);
}
auto MemoryType::copy() const -> own<MemoryType> {
  return WasmerMemoryType::from(this)->copy();
}

auto MemoryType::limits() const -> const Limits & {
  return WasmerMemoryType::from(this)->limits();
}

class WASM_API_EXTERN WasmerExternType
    : public WasmerExternTypeImpl<ExternType, WasmerExternType> {
protected:
  explicit WasmerExternType(const wasm_externtype_t *externtype)
      : WasmerExternTypeImpl(externtype) {}

public:
  static auto make(const wasm_externtype_t *externtype) -> own<ExternType> {
    auto owned_externtype = wasm_externtype_copy(externtype);
    switch (wasm_externtype_kind(owned_externtype)) {
    case WASM_EXTERN_FUNC:
      return make_own(new WasmerFuncType(
          make_c_own(wasm_externtype_as_functype_const(owned_externtype))));
    case WASM_EXTERN_GLOBAL:
      return make_own(new WasmerGlobalType(
          make_c_own(wasm_externtype_as_globaltype_const(owned_externtype))));
    case WASM_EXTERN_TABLE:
      return make_own(new WasmerTableType(
          make_c_own(wasm_externtype_as_tabletype_const(owned_externtype))));
    case WASM_EXTERN_MEMORY:
      return make_own(new WasmerMemoryType(
          make_c_own(wasm_externtype_as_memorytype_const(owned_externtype))));
    }
  }

  auto copy() const -> own<WasmerExternType> {
    return make_own(new WasmerExternType(wasm_externtype_copy(externtype)));
  }

  auto kind() const -> ExternKind {
    return c_externkind_to_cxx_externkind(wasm_externtype_kind(externtype));
  }

  auto func() -> FuncType * {
    return kind() == ExternKind::FUNC
               ? static_cast<FuncType *>(static_cast<ExternType *>(this))
               : nullptr;
  }
  auto global() -> GlobalType * {
    return kind() == ExternKind::GLOBAL
               ? static_cast<GlobalType *>(static_cast<ExternType *>(this))
               : nullptr;
  }
  auto table() -> TableType * {
    return kind() == ExternKind::TABLE
               ? static_cast<TableType *>(static_cast<ExternType *>(this))
               : nullptr;
  }
  auto memory() -> MemoryType * {
    return kind() == ExternKind::FUNC
               ? static_cast<MemoryType *>(static_cast<ExternType *>(this))
               : nullptr;
  }

  auto func() const -> const FuncType * {
    return kind() == ExternKind::FUNC
               ? static_cast<const FuncType *>(
                     static_cast<const ExternType *>(this))
               : nullptr;
  }
  auto global() const -> const GlobalType * {
    return kind() == ExternKind::GLOBAL
               ? static_cast<const GlobalType *>(
                     static_cast<const ExternType *>(this))
               : nullptr;
  }
  auto table() const -> const TableType * {
    return kind() == ExternKind::TABLE
               ? static_cast<const TableType *>(
                     static_cast<const ExternType *>(this))
               : nullptr;
  }
  auto memory() const -> const MemoryType * {
    return kind() == ExternKind::FUNC
               ? static_cast<const MemoryType *>(
                     static_cast<const ExternType *>(this))
               : nullptr;
  }
};

void ExternType::destroy() { delete WasmerExternType::from(this); }

auto ExternType::copy() const -> own<ExternType> {
  return WasmerExternType::from(this)->copy();
}

auto ExternType::kind() const -> ExternKind {
  return WasmerExternType::from(this)->kind();
}

auto ExternType::func() -> FuncType * {
  return WasmerExternType::from(this)->func();
}
auto ExternType::global() -> GlobalType * {
  return WasmerExternType::from(this)->global();
}
auto ExternType::table() -> TableType * {
  return WasmerExternType::from(this)->table();
}
auto ExternType::memory() -> MemoryType * {
  return WasmerExternType::from(this)->memory();
}

auto ExternType::func() const -> const FuncType * {
  return WasmerExternType::from(this)->func();
}
auto ExternType::global() const -> const GlobalType * {
  return WasmerExternType::from(this)->global();
}
auto ExternType::table() const -> const TableType * {
  return WasmerExternType::from(this)->table();
}
auto ExternType::memory() const -> const MemoryType * {
  return WasmerExternType::from(this)->memory();
}

class WASM_API_EXTERN WasmerImportType
    : public From<ImportType, WasmerImportType> {
  static byte_t identity(byte_t x) { return x; }

public:
  explicit WasmerImportType(c_own<const wasm_importtype_t> &&importtype)
      : importtype(std::move(importtype)),
        externtype(
            WasmerExternType::make(wasm_importtype_type(importtype.get()))),
        module_(c_vec_to_cxx_vec(wasm_importtype_module(importtype.get()),
                                 identity)),
        name_(c_vec_to_cxx_vec(wasm_importtype_name(importtype.get()),
                               identity)) {}

  static auto make(Name &&module, Name &&name, own<ExternType> &&externtype)
      -> own<ImportType> {
    auto c_module = cxx_vec_to_c_vec(std::move(module), identity);
    auto c_name = cxx_vec_to_c_vec(std::move(name), identity);
    auto c_externtype = wasm_externtype_copy(
        WasmerExternType::from(externtype.get())->externtype);
    return make_own(new WasmerImportType(
        make_c_own(wasm_importtype_new(&c_module, &c_name, c_externtype))));
  }
  auto copy() const -> own<WasmerImportType> {
    return make_own(new WasmerImportType(
        make_c_own(wasm_importtype_copy(importtype.get()))));
  }

  auto module() const -> const Name & { return module_; }
  auto name() const -> const Name & { return name_; }
  auto type() const -> const ExternType * { return externtype.get(); }

  c_own<const wasm_importtype_t> importtype;
  own<ExternType> externtype;
  Name module_;
  Name name_;
};

void ImportType::destroy() { delete WasmerImportType::from(this); }

auto ImportType::copy() const -> own<ImportType> {
  return WasmerImportType::from(this)->copy();
}

auto ImportType::module() const -> const Name & {
  return WasmerImportType::from(this)->module();
}

auto ImportType::name() const -> const Name & {
  return WasmerImportType::from(this)->name();
}

auto ImportType::type() const -> const ExternType * {
  return WasmerImportType::from(this)->type();
}

class WASM_API_EXTERN WasmerExportType
    : public From<ExportType, WasmerExportType> {
public:
  explicit WasmerExportType(c_own<const wasm_exporttype_t> &&exporttype)
      : exporttype(std::move(exporttype)),
        name_(
            c_vec_to_cxx_vec(wasm_exporttype_name(exporttype.get()), identity)),
        externtype(
            WasmerExternType::make(wasm_exporttype_type(exporttype.get()))) {}

  static byte_t identity(byte_t x) { return x; }

  static auto make(Name &&name, own<ExternType> &&externtype)
      -> own<ExportType> {
    auto c_name = cxx_vec_to_c_vec(std::move(name), identity);
    auto c_externtype = wasm_externtype_copy(
        WasmerExternType::from(externtype.get())->externtype);
    return make_own(new WasmerExportType(
        make_c_own(wasm_exporttype_new(&c_name, c_externtype))));
  }

  auto copy() const -> own<ExportType> {
    return make_own(new WasmerExportType(
        make_c_own(wasm_exporttype_copy(exporttype.get()))));
  }

  auto name() const -> const Name & { return name_; }
  auto type() const -> const ExternType * { return externtype.get(); }

  c_own<const wasm_exporttype_t> exporttype;
  Name name_;
  own<ExternType> externtype;
};

void ExportType::destroy() { delete WasmerExportType::from(this); }

auto ExportType::make(Name &&name, own<ExternType> &&externtype)
    -> own<ExportType> {
  return WasmerExportType::make(std::move(name), std::move(externtype));
}
auto ExportType::copy() const -> own<ExportType> {
  return WasmerExportType::from(this)->copy();
}

auto ExportType::name() const -> const Name & {
  return WasmerExportType::from(this)->name();
}
auto ExportType::type() const -> const ExternType * {
  return WasmerExportType::from(this)->type();
}

namespace {
struct WasmerRefWrapper {
  const wasm_trap_t *trap() const { return wasm_ref_as_trap_const(ref); }
  const wasm_foreign_t *foreign() const {
    return wasm_ref_as_foreign_const(ref);
  }
  const wasm_func_t *func() const { return wasm_ref_as_func_const(ref); }
  const wasm_global_t *global() const { return wasm_ref_as_global_const(ref); }
  const wasm_table_t *table() const { return wasm_ref_as_table_const(ref); }
  const wasm_memory_t *memory() const { return wasm_ref_as_memory_const(ref); }
  const wasm_extern_t *extern_() const { return wasm_ref_as_extern_const(ref); }
  const wasm_instance_t *instance() const {
    return wasm_ref_as_instance_const(ref);
  }
  const wasm_module_t *module() const { return wasm_ref_as_module_const(ref); }

  wasm_ref_t *const ref;
  enum WasmerRefWrapperKind {
    TRAP,
    FOREIGN,
    FUNC,
    GLOBAL,
    TABLE,
    MEMORY,
    EXTERN,
    INSTANCE,
    MODULE
  } kind;

  explicit WasmerRefWrapper(wasm_ref_t *ref, WasmerRefWrapperKind kind)
      : ref(ref), kind(kind) {}
};
} // namespace

class WASM_API_EXTERN WasmerRef : WasmerRefWrapper,
                                  public From<Ref, WasmerRef> {
public:
  auto copy() const -> own<Ref>;

  auto same(const Ref *that) const -> bool {
    return ref == WasmerRef::from(that)->ref;
  }

  auto get_host_info() const -> void * { return wasm_ref_get_host_info(ref); }
  void set_host_info(void *info, void (*finalizer)(void *) = nullptr) {
    if (finalizer) {
      wasm_ref_set_host_info(ref, info);
    } else {
      wasm_ref_set_host_info_with_finalizer(ref, info, finalizer);
    }
  }
};

void Ref::destroy() { delete WasmerRef::from(this); }

auto Ref::copy() const -> own<Ref> { return WasmerRef::from(this)->copy(); }
auto Ref::same(const Ref *that) const -> bool {
  return WasmerRef::from(this)->same(that);
}

auto Ref::get_host_info() const -> void * {
  return WasmerRef::from(this)->get_host_info();
}
void Ref::set_host_info(void *info, void (*finalizer)(void *)) {
  return WasmerRef::from(this)->set_host_info(info, finalizer);
}

class WASM_API_EXTERN WasmerFrame : public From<Frame, WasmerFrame> {
public:
  explicit WasmerFrame(c_own<wasm_frame_t> &&frame) : frame(std::move(frame)) {}

  auto copy() const -> own<Frame> {
    return make_own(new WasmerFrame(make_c_own(wasm_frame_copy(frame.get()))));
  }

  auto instance() const -> Instance * {
    // TODO: return make_own(new
    // WasmerInstance(make_c_own(wasm_frame_instance(frame.get()))));
    return nullptr;
  }
  auto func_index() const -> uint32_t {
    return wasm_frame_func_index(frame.get());
  }
  auto func_offset() const -> size_t {
    return wasm_frame_func_offset(frame.get());
  }
  auto module_offset() const -> size_t {
    return wasm_frame_module_offset(frame.get());
  }

  c_own<wasm_frame_t> frame;
};

void Frame::destroy() { delete WasmerFrame::from(this); }

auto Frame::copy() const -> own<Frame> {
  return WasmerFrame::from(this)->copy();
}

auto Frame::instance() const -> Instance * {
  return WasmerFrame::from(this)->instance();
}
auto Frame::func_index() const -> uint32_t {
  return WasmerFrame::from(this)->func_index();
}
auto Frame::func_offset() const -> size_t {
  return WasmerFrame::from(this)->func_offset();
}
auto Frame::module_offset() const -> size_t {
  return WasmerFrame::from(this)->module_offset();
}

class WASM_API_EXTERN WasmerTrap : WasmerRefWrapper,
                                   public From<Trap, WasmerTrap> {
  static auto identity(byte_t x) -> byte_t { return x; }

  static auto c_frame_to_cxx_frame(wasm_frame_t *c_frame) -> own<Frame> {
    return make_own(new WasmerFrame(make_c_own(c_frame)));
  }

public:
  explicit WasmerTrap(c_own<wasm_trap_t> &&trap)
      : WasmerRefWrapper(wasm_trap_as_ref(trap.get()), TRAP),
        trap(std::move(trap)) {}

  static auto make(WasmerStore *, const Message &msg) -> own<WasmerTrap>;
  auto copy() const -> own<WasmerTrap> {
    return make_own(new WasmerTrap(make_c_own(wasm_trap_copy(trap.get()))));
  }

  auto message() const -> Message {
    wasm_message_t message;
    wasm_trap_message(trap.get(), &message);
    auto ret = c_vec_to_cxx_vec(&message, identity);
    wasm_name_delete(&message);
    return ret;
  }
  auto origin() const -> own<WasmerFrame> {
    return make_own(new WasmerFrame(make_c_own(wasm_trap_origin(trap.get()))));
  }
  auto trace() const -> ownvec<Frame> {
    wasm_frame_vec_t trace;
    wasm_trap_trace(trap.get(), &trace);
    return c_vec_to_cxx_ownvec(&trace, c_frame_to_cxx_frame);
  }

  c_own<wasm_trap_t> trap;
};

void Trap::destroy() { delete WasmerTrap::from(this); }

auto Trap::message() const -> Message {
  return WasmerTrap::from(this)->message();
}

auto Trap::origin() const -> own<Frame> {
  return WasmerTrap::from(this)->origin();
}
auto Trap::trace() const -> ownvec<Frame> {
  return WasmerTrap::from(this)->trace();
}

class WASM_API_EXTERN WasmerModule : WasmerRefWrapper,
                                     public From<Module, WasmerModule> {

  static byte_t identity(byte_t x) { return x; }

public:
  explicit WasmerModule(c_own<wasm_module_t> &&module)
      : WasmerRefWrapper(wasm_module_as_ref(module.get()), MODULE),
        module(std::move(module)) {}

  static auto validate(WasmerStore *store, const vec<byte_t> &binary) -> bool {
    // TODO: we should be able to remove 'binary.copy()' here
    auto c_binary = cxx_vec_to_c_vec(binary.copy(), identity);
    return wasm_module_validate(store->store.get(), &c_binary);
  }
  static auto make(WasmerStore *store, const vec<byte_t> &binary)
      -> own<WasmerModule> {
    // TODO: we should be able to remove 'binary.copy()' here
    auto c_binary = cxx_vec_to_c_vec(binary.copy(), identity);
    return make_own(new WasmerModule(
        make_c_own(wasm_module_new(store->store.get(), &c_binary))));
  }

  auto copy() const -> own<WasmerModule> {
    return make_own(
        new WasmerModule(make_c_own(wasm_module_copy(module.get()))));
  }

  auto imports() const -> ownvec<ImportType> { abort(); }
  auto exports() const -> ownvec<ExportType> { abort(); }

  auto share() const -> own<Shared<Module>> { abort(); }
  static auto obtain(WasmerStore *store, const Shared<Module> *shared_module)
      -> own<Module> {
    abort();
  }

  auto serialize() const -> vec<byte_t> { abort(); }
  static auto deserialize(WasmerStore *store, const vec<byte_t> &bytes)
      -> own<Module> {
    abort();
  }

  c_own<wasm_module_t> module;
};

void Module::destroy() { delete WasmerModule::from(this); }

auto Module::validate(Store *store, const vec<byte_t> &binary) -> bool {
  return WasmerModule::validate(WasmerStore::from(store), binary);
}
auto Module::make(Store *store, const vec<byte_t> &binary) -> own<Module> {
  return WasmerModule::make(WasmerStore::from(store), binary);
}
auto Module::copy() const -> own<Module> {
  return WasmerModule::from(this)->copy();
}

auto Module::imports() const -> ownvec<ImportType> {
  return WasmerModule::from(this)->imports();
}
auto Module::exports() const -> ownvec<ExportType> {
  return WasmerModule::from(this)->exports();
}

auto Module::share() const -> own<Shared<Module>> {
  return WasmerModule::from(this)->share();
}
auto Module::obtain(Store *store, const Shared<Module> *shared_module)
    -> own<Module> {
  return WasmerModule::obtain(WasmerStore::from(store), shared_module);
}

auto Module::serialize() const -> vec<byte_t> {
  return WasmerModule::from(this)->serialize();
}
auto deserialize(Store *store, const vec<byte_t> &bytes) -> own<Module> {
  return WasmerModule::deserialize(WasmerStore::from(store), bytes);
}

class WASM_API_EXTERN WasmerForeign : WasmerRefWrapper,
                                      public From<Foreign, WasmerForeign> {
public:
  explicit WasmerForeign(c_own<wasm_foreign_t> &&foreign)
      : WasmerRefWrapper(wasm_foreign_as_ref(foreign.get()), FOREIGN),
        foreign(std::move(foreign)) {}

  static auto make(WasmerStore *store) -> own<WasmerForeign> {
    return make_own(
        new WasmerForeign(make_c_own(wasm_foreign_new(store->store.get()))));
  }

  auto copy() const -> own<WasmerForeign> {
    return make_own(
        new WasmerForeign(make_c_own(wasm_foreign_copy(foreign.get()))));
  }

  c_own<wasm_foreign_t> foreign;
};

void Foreign::destroy() { delete WasmerForeign::from(this); }

auto Foreign::make(Store *store) -> own<Foreign> {
  return WasmerForeign::make(WasmerStore::from(store));
}
auto Foreign::copy() const -> own<Foreign> {
  return WasmerForeign::from(this)->copy();
}

namespace {
struct WasmerExternWrapper : WasmerRefWrapper {
  using WasmerRefWrapper::WasmerRefWrapper;
  void sanity_check() const {
    ExternKind kind =
        c_externkind_to_cxx_externkind(wasm_extern_kind(extern_()));
    assert(kind == ExternKind::FUNC || kind == ExternKind::GLOBAL ||
           kind == ExternKind::TABLE || kind == ExternKind::MEMORY);
  }
  const wasm_func_t *func() const {
    sanity_check();
    return kind() == ExternKind::FUNC ? wasm_extern_as_func_const(extern_())
                                      : nullptr;
  }
  const wasm_global_t *global() const {
    sanity_check();
    return kind() == ExternKind::GLOBAL ? wasm_extern_as_global_const(extern_())
                                        : nullptr;
  }
  const wasm_table_t *table() const {
    sanity_check();
    return kind() == ExternKind::TABLE ? wasm_extern_as_table_const(extern_())
                                       : nullptr;
  }
  const wasm_memory_t *memory() const {
    sanity_check();
    return kind() == ExternKind::MEMORY ? wasm_extern_as_memory_const(extern_())
                                        : nullptr;
  }
  auto kind() const -> ExternKind {
    sanity_check();
    return c_externkind_to_cxx_externkind(wasm_extern_kind(extern_()));
  }
};
} // namespace

class WasmerExtern : WasmerExternWrapper, public From<Extern, WasmerExtern> {
public:
  using WasmerExternWrapper::extern_;

  void destroy();

  auto copy() const -> own<Extern>;

  auto kind() const -> ExternKind { return WasmerExternWrapper::kind(); }
  auto type() const -> own<ExternType>;

  auto func() -> Func *;
  auto global() -> Global *;
  auto table() -> Table *;
  auto memory() -> Memory *;

  auto func() const -> const Func *;
  auto global() const -> const Global *;
  auto table() const -> const Table *;
  auto memory() const -> const Memory *;
};

void Extern::destroy() { WasmerExtern::from(this)->destroy(); }

auto Extern::copy() const -> own<Extern> {
  return WasmerExtern::from(this)->copy();
}

auto Extern::kind() const -> ExternKind {
  return WasmerExtern::from(this)->kind();
}
auto Extern::type() const -> own<ExternType> {
  return WasmerExtern::from(this)->type();
}

auto Extern::func() -> Func * { return WasmerExtern::from(this)->func(); }
auto Extern::global() -> Global * { return WasmerExtern::from(this)->global(); }
auto Extern::table() -> Table * { return WasmerExtern::from(this)->table(); }
auto Extern::memory() -> Memory * { return WasmerExtern::from(this)->memory(); }

auto Extern::func() const -> const Func * {
  return WasmerExtern::from(this)->func();
}
auto Extern::global() const -> const Global * {
  return WasmerExtern::from(this)->global();
}
auto Extern::table() const -> const Table * {
  return WasmerExtern::from(this)->table();
}
auto Extern::memory() const -> const Memory * {
  return WasmerExtern::from(this)->memory();
}

class WasmerFunc : WasmerExternWrapper, public From<Func, WasmerFunc> {
  // TODO: this should be at the top level
  static auto cxx_val_to_c_val(Val cxx_val) -> wasm_val_t {
    wasm_val_t c_val;
    switch (cxx_val.kind()) {
    case ValKind::I32:
      c_val.kind = WASM_I32;
      c_val.of.i32 = cxx_val.i32();
      return c_val;
    case ValKind::I64:
      c_val.kind = WASM_I64;
      c_val.of.i64 = cxx_val.i64();
      return c_val;
    case ValKind::F32:
      c_val.kind = WASM_F32;
      c_val.of.f32 = cxx_val.f32();
      return c_val;
    case ValKind::F64:
      c_val.kind = WASM_F64;
      c_val.of.f64 = cxx_val.i64();
      return c_val;
    case ValKind::ANYREF:
    case ValKind::FUNCREF:
      assert(false);
      // return WASM_REF_VAL(v.ref());
    }
  };

  struct WasmerFuncEnv {
    callback cb;
    static wasm_trap_t *shim(void *env, const wasm_val_vec_t *args,
                             wasm_val_vec_t *results) {
      abort();
    }
    static void finalizer(void *ptr) {
      delete static_cast<WasmerFuncEnvWithEnv *>(ptr);
    }
  };

  struct WasmerFuncEnvWithEnv {
    callback_with_env cb;
    void (*c_finalizer)(void *);
    void *env;
    static wasm_trap_t *shim(void *env, const wasm_val_vec_t *args,
                             wasm_val_vec_t *results) {
      abort();
    }
    static void finalizer(void *ptr) {
      auto self = static_cast<WasmerFuncEnvWithEnv *>(ptr);
      self->c_finalizer(self->env);
      delete self;
    }
  };

public:
  explicit WasmerFunc(
      c_own<wasm_func_t> &&func,
      std::unique_ptr<std::variant<WasmerFuncEnv, WasmerFuncEnvWithEnv>>
          func_env)
      : WasmerExternWrapper(wasm_func_as_ref(func.get()), FUNC),
        func_env(std::move(func_env)), func(std::move(func)) {}

  static auto make(WasmerStore *store, const WasmerFuncType *functype,
                   callback cb) -> own<Func> {
    auto func_env =
        std::make_unique<std::variant<WasmerFuncEnv, WasmerFuncEnvWithEnv>>(
            WasmerFuncEnv{cb});
    auto &inner = std::get<WasmerFuncEnv>(*func_env);
    return make_own(new WasmerFunc(
        make_c_own(wasm_func_new_with_env(store->store.get(),
                                          functype->functype.get(), inner.shim,
                                          func_env.get(), inner.finalizer)),
        std::move(func_env)));
  }
  static auto make(WasmerStore *store, const WasmerFuncType *functype,
                   callback_with_env cb, void *env,
                   void (*finalizer)(void *) = nullptr) -> own<Func> {
    auto func_env =
        std::make_unique<std::variant<WasmerFuncEnv, WasmerFuncEnvWithEnv>>(
            WasmerFuncEnvWithEnv{cb, finalizer, env});
    auto &inner = std::get<WasmerFuncEnvWithEnv>(*func_env);
    return make_own(new WasmerFunc(
        make_c_own(wasm_func_new_with_env(store->store.get(),
                                          functype->functype.get(), inner.shim,
                                          func_env.get(), inner.finalizer)),
        std::move(func_env)));
  }

  auto copy() const -> own<Func> {
    auto func_env =
        std::make_unique<std::variant<WasmerFuncEnv, WasmerFuncEnvWithEnv>>(
            *this->func_env);
    return make_own(new WasmerFunc(make_c_own(wasm_func_copy(func.get())),
                                   std::move(func_env)));
  }

  auto type() const -> own<FuncType> {
    return make_own(new WasmerFuncType(make_c_own(wasm_func_type(func.get()))));
  }
  auto param_arity() const -> size_t {
    return wasm_func_param_arity(func.get());
  }
  auto result_arity() const -> size_t {
    return wasm_func_result_arity(func.get());
  }

  auto call(const vec<Val> &cxx_args, vec<Val> &cxx_results) const
      -> own<Trap> {
    /*
    auto c_args = cxx_vec_to_c_vec(cxx_args, cxx_val_to_c_val);
    wasm_val_vec_t c_results;
    wasm_val_vec_new_uninitialized(&c_results, cxx_results.size());
    auto trap = make_own(new WasmerTrap(make_c_own(wasm_func_call(func.get(),
    c_args, c_results)))); c_vec_to_cxx_vec( return
    */
  }

  std::unique_ptr<std::variant<WasmerFuncEnv, WasmerFuncEnvWithEnv>> func_env;
  c_own<wasm_func_t> func;
};

void Func::destroy() { delete WasmerFunc::from(this); }

auto Func::make(Store *store, const FuncType *functype, callback cb)
    -> own<Func> {
  return WasmerFunc::make(WasmerStore::from(store),
                          WasmerFuncType::from(functype), cb);
}
auto Func::make(Store *store, const FuncType *functype, callback_with_env cb,
                void *env, void (*finalizer)(void *)) -> own<Func> {
  return WasmerFunc::make(WasmerStore::from(store),
                          WasmerFuncType::from(functype), cb, env, finalizer);
}
auto Func::copy() const -> own<Func> { return WasmerFunc::from(this)->copy(); }

auto Func::type() const -> own<FuncType> {
  return WasmerFunc::from(this)->type();
}
auto Func::param_arity() const -> size_t {
  return WasmerFunc::from(this)->param_arity();
}
auto Func::result_arity() const -> size_t {
  return WasmerFunc::from(this)->result_arity();
}

auto Func::call(const vec<Val> &args, vec<Val> &results) const -> own<Trap> {
  return WasmerFunc::from(this)->call(args, results);
}

class WASM_API_EXTERN WasmerInstance : WasmerRefWrapper,
                                       public From<Instance, WasmerInstance> {

  static wasm_extern_t *cxx_extern_to_c_extern(Extern *extern_) {
    // TODO: const correctness
    return const_cast<wasm_extern_t *>(WasmerExtern::from(extern_)->extern_());
  }

public:
  explicit WasmerInstance(c_own<wasm_instance_t> &&instance)
      : WasmerRefWrapper(wasm_instance_as_ref(instance.get()), INSTANCE),
        instance(std::move(instance)) {}

  static auto make(WasmerStore *store, const WasmerModule *module,
                   const vec<Extern *> &imports, own<WasmerTrap> &trap)
      -> own<WasmerInstance> {
    wasm_trap_t *c_trap = nullptr;
    auto c_imports = cxx_vec_to_c_vec(imports.copy(), cxx_extern_to_c_extern);
    auto instance = make_own(new WasmerInstance(make_c_own(wasm_instance_new(
        store->store.get(), module->module.get(), &c_imports, &c_trap))));
    trap = make_own(new WasmerTrap(make_c_own(c_trap)));
    return instance;
  }

  auto copy() const -> own<WasmerInstance> {
    return make_own(
        new WasmerInstance(make_c_own(wasm_instance_copy(instance.get()))));
  }

  auto exports() const -> ownvec<Extern> { abort(); }

  c_own<wasm_instance_t> instance;
};

void Instance::destroy() { delete WasmerInstance::from(this); }

auto Instance::make(Store *store, const Module *module,
                    const vec<Extern *> &imports, own<Trap> *trap)
    -> own<Instance> {
  /*vec<WasmerExtern *> wasmer_imports =
      vec<WasmerExtern *>::make_uninitialized(imports.size());
  for (int i = 0; i != imports.size(); ++i) {
    wasmer_imports[i] = WasmerExtern::from(imports[i]);
  }
  */
  own<WasmerTrap> wasmer_trap;
  auto instance =
      WasmerInstance::make(WasmerStore::from(store), WasmerModule::from(module),
                           imports, wasmer_trap);
  if (trap)
    *trap = std::move(wasmer_trap);
  return instance;
}

auto Instance::copy() const -> own<Instance> {
  return WasmerInstance::from(this)->copy();
}

auto Instance::exports() const -> ownvec<Extern> {
  return WasmerInstance::from(this)->exports();
}

void WasmerExtern::destroy() {
  auto extern_ = static_cast<Extern *>(this);
  switch (kind()) {
  case ExternKind::FUNC:
    delete static_cast<WasmerFunc *>(extern_);
    return;
  // case ExternKind::GLOBAL:
  //  delete static_cast<WasmerGlobal *>(extern_);
  //  return;
  // case ExternKind::TABLE:
  //  delete static_cast<WasmerTable *>(extern_);
  //  return;
  // case ExternKind::MEMORY:
  //  delete static_cast<WasmerMemory *>(extern_);
  //  return;
  default:
    abort();
  }
}

auto WasmerExtern::copy() const -> own<Extern> {
  auto extern_ = static_cast<const Extern *>(this);
  switch (kind()) {
  case ExternKind::FUNC:
    return static_cast<const WasmerFunc *>(extern_)->copy();
  // case ExternKind::GLOBAL: return static_cast<const WasmerGlobal
  // *>(extern_)->copy(); case ExternKind::TABLE: return static_cast<const
  // WasmerTable *>(extern_)->copy(); case ExternKind::MEMORY: return
  // static_cast<const WasmerMemory *>(extern_)->copy();
  default:
    abort();
  }
}

auto WasmerExtern::func() -> Func * {
  auto extern_ = static_cast<Extern *>(this);
  return static_cast<WasmerFunc *>(extern_);
}
auto WasmerExtern::global() -> Global * {
  // auto extern_ = static_cast<Extern *>(this);
  // return static_cast<WasmerGlobal *>(extern_);
  abort();
}
auto WasmerExtern::table() -> Table * {
  // auto extern_ = static_cast<Extern *>(this);
  // return static_cast<WasmerTable *>(extern_);
  abort();
}
auto WasmerExtern::memory() -> Memory * {
  // auto extern_ = static_cast<Extern *>(this);
  // return static_cast<WasmerMemory *>(extern_);
  abort();
}

auto WasmerExtern::func() const -> const Func * {
  auto extern_ = static_cast<const Extern *>(this);
  return static_cast<const WasmerFunc *>(extern_);
}
auto WasmerExtern::global() const -> const Global * {
  // auto extern_ = static_cast<const Extern *>(this);
  // return static_cast<const WasmerGlobal *>(extern_);
  abort();
}
auto WasmerExtern::table() const -> const Table * {
  // auto extern_ = static_cast<const Extern *>(this);
  // return static_cast<const WasmerTable *>(extern_);
  abort();
}
auto WasmerExtern::memory() const -> const Memory * {
  // auto extern_ = static_cast<const Extern *>(this);
  // return static_cast<const WasmerMemory *>(extern_);
  abort();
}

auto WasmerExtern::type() const -> own<ExternType> {
  auto extern_ = static_cast<const Extern *>(this);
  switch (kind()) {
  case ExternKind::FUNC:
    return static_cast<const WasmerFunc *>(extern_)->type();
  // case ExternKind::GLOBAL: return static_cast<const WasmerGlobal
  // *>(extern_)->type(); case ExternKind::TABLE: return static_cast<const
  // WasmerTable *>(extern_)->type(); case ExternKind::MEMORY: return
  // static_cast<const WasmerMemory *>(extern_)->type();
  default:
    abort();
  }
}

auto WasmerRef::copy() const -> own<Ref> {
  auto ref = static_cast<const Ref *>(this);
  switch (kind) {
  case TRAP:
    return static_cast<const WasmerTrap *>(ref)->copy();
  case FOREIGN:
    return static_cast<const WasmerForeign *>(ref)->copy();
  case FUNC:
    return static_cast<const WasmerFunc *>(ref)->copy();
  // case GLOBAL:
  //  return static_cast<const WasmerGlobal *>(ref)->copy();
  // case TABLE:
  //  return static_cast<const WasmerTable *>(ref)->copy();
  // case MEMORY:
  //  return static_cast<const WasmerMemory *>(ref)->copy();
  case EXTERN:
    return static_cast<const WasmerExtern *>(ref)->copy();
  case INSTANCE:
    return static_cast<const WasmerInstance *>(ref)->copy();
  case MODULE:
    return static_cast<const WasmerModule *>(ref)->copy();
  default:
    abort();
  }
}
