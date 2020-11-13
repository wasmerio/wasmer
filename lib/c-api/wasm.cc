// Implementation of the wasm-c-api C++ API for wasmer on top of the Wasmer C++
// API and the wasm C API.

#include "wasm.hh"
#include "wasm.h"

#include "wasmer.hh"

using namespace wasm;

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

template <typename T> struct c_vec;
template <> struct c_vec<wasm_valtype_t> {
  using type = wasm_valtype_vec_t;
  static constexpr auto new_empty = wasm_valtype_vec_new_empty;
  static constexpr auto new_uninitialized = wasm_valtype_vec_new_uninitialized;
  static constexpr auto new_ = wasm_valtype_vec_new;
  static constexpr auto copy = wasm_valtype_vec_copy;
  static constexpr auto delete_ = wasm_valtype_vec_delete;
};

template <typename T, typename S1, typename S2>
typename c_vec<S2>::type cxx_ownvec_to_c_vec(ownvec<T> &&cxx_vec,
                                             S2 *(*convert)(S1)) {
  typename c_vec<S2>::type v;
  c_vec<S2>::new_uninitialized(&v, cxx_vec.size());
  for (int i = 0, e = cxx_vec.size(); i != e; ++i) {
    v.data[i] = convert(cxx_vec[i].release());
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
  explicit WasmerEngine(own<Config> &&config)
      : engine(wasm_engine_new_with_config(
            WasmerConfig::from(std::move(config))->config.release())) {}

  c_own<wasm_engine_t> engine;
};

void Engine::destroy() { delete WasmerEngine::from(this); }

auto Engine::make(own<Config> &&config) -> own<Engine> {
  return make_own(new WasmerEngine(std::move(config)));
}

class WASM_API_EXTERN WasmerStore : public From<Store, WasmerStore> {
public:
  explicit WasmerStore(Engine *engine)
      : store(wasm_store_new(WasmerEngine::from(engine)->engine.get())) {}

  c_own<wasm_store_t> store;
};

void Store::destroy() { delete WasmerStore::from(this); }

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

  auto copy() const -> own<ValType> {
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
struct ExternKindWrapper {
  ExternKind kind;
};
} // namespace

template <typename T, typename Derived>
struct WasmerExternTypeImpl : public From<T, Derived>,
                              public ExternKindWrapper {
  WasmerExternTypeImpl(ExternKind kind) : ExternKindWrapper{kind} {}
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
      : WasmerExternTypeImpl(ExternKind::FUNC), functype(std::move(functype)),
        params_(c_vec_to_cxx_ownvec(wasm_functype_params(functype.get()),
                                    c_valtype_to_cxx_ownvaltype)),
        results_(c_vec_to_cxx_ownvec(wasm_functype_results(functype.get()),
                                     c_valtype_to_cxx_ownvaltype)) {}

  static auto make(ownvec<ValType> &&params, ownvec<ValType> &&results)
      -> own<FuncType> {
    wasm_valtype_vec_t c_params =
        cxx_ownvec_to_c_vec(std::move(params), cxx_valtype_to_c_valtype);
    wasm_valtype_vec_t c_results =
        cxx_ownvec_to_c_vec(std::move(results), cxx_valtype_to_c_valtype);
    return make_own(new WasmerFuncType(
        make_c_own(wasm_functype_new(&c_params, &c_results))));
  }

  auto copy() const -> own<FuncType> {
    return make_own(
        new WasmerFuncType(make_c_own(wasm_functype_copy(functype.get()))));
  }

  auto params() const -> const ownvec<ValType> & { return params_; }
  auto results() const -> const ownvec<ValType> & { return results_; }

  c_own<const wasm_functype_t> functype;
  ownvec<ValType> params_;
  ownvec<ValType> results_;
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
      : WasmerExternTypeImpl(ExternKind::GLOBAL),
        globaltype(std::move(globaltype)),
        valtype(c_valtype_to_cxx_ownvaltype(
            wasm_globaltype_content(globaltype.get()))) {}

  explicit WasmerGlobalType(own<ValType> &&valtype, Mutability mutability)
      : WasmerExternTypeImpl(ExternKind::GLOBAL),
        globaltype(
            wasm_globaltype_new(cxx_valtype_to_c_valtype(valtype.get()),
                                cxx_mutability_to_c_mutability(mutability))),
        valtype(std::move(valtype)) {}

  auto copy() const -> own<GlobalType> {
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
  return make_own(new WasmerGlobalType(std::move(valtype), mutability));
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
      : WasmerExternTypeImpl(ExternKind::TABLE),
        tabletype(std::move(tabletype)),
        element_(c_valtype_to_cxx_ownvaltype(
            wasm_tabletype_element(tabletype.get()))),
        limits_(
            c_limits_to_cxx_limits(*wasm_tabletype_limits(tabletype.get()))) {}

  static auto make(own<ValType> &&element, Limits limits) -> own<TableType> {
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

auto TableType::make(own<ValType> &&element, Limits limits) -> own<TableType> {
  return WasmerTableType::make(std::move(element), limits);
}

// TODO: memory

class WASM_API_EXTERN WasmerExternType
    : public From<ExternType, WasmerExternType> {
public:
  explicit WasmerExternType(wasm_externtype_t *externtype)
      : externtype(externtype) {}

  auto copy() const -> own<ExternType> {
    return make_own(new WasmerExternType(wasm_externtype_copy(externtype)));
  }

  auto kind() const -> ExternKind {
    return c_externkind_to_cxx_externkind(wasm_externtype_kind(externtype));
  }

  auto func() -> FuncType *;
  auto global() -> GlobalType *;
  auto table() -> TableType *;
  auto memory() -> MemoryType *;

  auto func() const -> const FuncType *;
  auto global() const -> const GlobalType *;
  auto table() const -> const TableType *;
  auto memory() const -> const MemoryType *;

  static const WasmerExternType *from(const ExternType *base) {
    return static_cast<const WasmerExternType *>(base);
  }
  static WasmerExternType *from(ExternType *base) {
    return static_cast<WasmerExternType *>(base);
  }

  wasm_externtype_t *externtype;
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
