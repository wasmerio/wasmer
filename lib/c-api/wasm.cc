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
#define DEFINE_C_VEC_SPECIALIZATION(name)                                      \
  template <> struct c_vec<wasm_##name##_t> {                                  \
    using type = wasm_##name##_vec_t;                                          \
    static constexpr auto new_empty = wasm_##name##_vec_new_empty;             \
    static constexpr auto new_uninitialized =                                  \
        wasm_##name##_vec_new_uninitialized;                                   \
    static constexpr auto new_ = wasm_##name##_vec_new;                        \
    static constexpr auto copy = wasm_##name##_vec_copy;                       \
    static constexpr auto delete_ = wasm_##name##_vec_delete;                  \
  };
// This list should match all WASM_DECLARE_VEC(type) in wasm.h.
DEFINE_C_VEC_SPECIALIZATION(byte)
DEFINE_C_VEC_SPECIALIZATION(val)
DEFINE_C_VEC_SPECIALIZATION(frame)
DEFINE_C_VEC_SPECIALIZATION(extern)
DEFINE_C_VEC_SPECIALIZATION(valtype)
DEFINE_C_VEC_SPECIALIZATION(functype)
DEFINE_C_VEC_SPECIALIZATION(globaltype)
DEFINE_C_VEC_SPECIALIZATION(tabletype)
DEFINE_C_VEC_SPECIALIZATION(memorytype)
DEFINE_C_VEC_SPECIALIZATION(externtype)
DEFINE_C_VEC_SPECIALIZATION(importtype)
DEFINE_C_VEC_SPECIALIZATION(exporttype)

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
        functype(std::move(functype)),
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
      : WasmerExternTypeImpl(
            wasm_globaltype_as_externtype_const(globaltype.get())),
        globaltype(std::move(globaltype)),
        valtype(c_valtype_to_cxx_ownvaltype(
            wasm_globaltype_content(globaltype.get()))) {}

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

class WASM_API_EXTERN WasmerMemoryType
    : public From<MemoryType, WasmerMemoryType> {
public:
  explicit WasmerMemoryType(c_own<const wasm_memorytype_t> memorytype)
      : memorytype(std::move(memorytype)) {}

  static auto make(Limits limits) -> own<MemoryType> {
    auto l = cxx_limits_to_c_limits(limits);
    return make_own(new WasmerMemoryType(make_c_own(wasm_memorytype_new(&l))));
  }
  auto copy() const -> own<MemoryType> {
    return make_own(new WasmerMemoryType(
        make_c_own(wasm_memorytype_copy(memorytype.get()))));
  }

  auto limits() const -> const Limits &;

  c_own<const wasm_memorytype_t> memorytype;
};

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

  auto copy() const -> own<ExternType> {
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
public:
  explicit WasmerImportType(c_own<const wasm_importtype_t> &&importtype)
      : importtype(std::move(importtype)),
        externtype(
            WasmerExternType::make(wasm_importtype_type(importtype.get()))),
        module_(c_vec_to_cxx_vec(wasm_importtype_module(importtype.get()),
                                 identity)),
        name_(c_vec_to_cxx_vec(wasm_importtype_name(importtype.get()),
                               identity)) {}

  static byte_t identity(byte_t x) { return x; }

  static auto make(Name &&module, Name &&name, own<ExternType> &&externtype)
      -> own<ImportType> {
    auto c_module = cxx_vec_to_c_vec(std::move(module), identity);
    auto c_name = cxx_vec_to_c_vec(std::move(name), identity);
    auto c_externtype = wasm_externtype_copy(
        WasmerExternType::from(externtype.get())->externtype);
    return make_own(new WasmerImportType(
        make_c_own(wasm_importtype_new(&c_module, &c_name, c_externtype))));
  }
  auto copy() const -> own<ImportType> {
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
};
} // namespace

class WASM_API_EXTERN WasmerRef : WasmerRefWrapper,
                                  public From<Ref, WasmerRef> {
public:
  auto copy() const -> own<Ref>;
  /*
  auto copy() const -> own<Ref> {
    switch (kind) {
    case TRAP:
      return static_cast<WasmerTrap>(this)->copy();
    // TODO
    }
  }
  */
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
  static auto make(Store *, const Message &msg) -> own<Trap>;
  auto copy() const -> own<Trap>;

  auto message() const -> Message {
    wasm_message_t message;
    wasm_trap_message(trap.get(), &message);
    auto ret = c_vec_to_cxx_vec(&message, identity);
    wasm_name_delete(&message);
    return ret;
  }
  auto origin() const -> own<Frame> {
    return make_own(new WasmerFrame(make_c_own(wasm_trap_origin(trap.get()))));
  }
  auto trace() const -> ownvec<Frame> {
    wasm_frame_vec_t trace;
    wasm_trap_trace(trap.get(), &trace);
    return c_vec_to_cxx_ownvec(&trace, c_frame_to_cxx_frame);
  }

  c_own<wasm_trap_t> trap;
};

auto Trap::message() const -> Message {
  return WasmerTrap::from(this)->message();
};

auto Trap::origin() const -> own<Frame> {
  return WasmerTrap::from(this)->origin();
}
auto Trap::trace() const -> ownvec<Frame> {
  return WasmerTrap::from(this)->trace();
}

class WASM_API_EXTERN WasmerModule : WasmerRefWrapper,
                                     public From<Module, WasmerModule> {
public:
  static auto validate(Store *store, const vec<byte_t> &binary) -> bool;
  static auto make(Store *store, const vec<byte_t> &binary) -> own<Module>;
  auto copy() const -> own<Module>;

  auto imports() const -> ownvec<ImportType>;
  auto exports() const -> ownvec<ExportType>;

  auto share() const -> own<Shared<Module>>;
  static auto obtain(Store *store, const Shared<Module> *shared_module)
      -> own<Module>;

  auto serialize() const -> vec<byte_t>;
  static auto deserialize(Store *store, const vec<byte_t> &bytes)
      -> own<Module>;
};

auto Module::validate(Store *store, const vec<byte_t> &binary) -> bool {
  return WasmerModule::validate(store, binary);
}
auto Module::make(Store *store, const vec<byte_t> &binary) -> own<Module> {
  return WasmerModule::make(store, binary);
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
  return WasmerModule::obtain(store, shared_module);
}

auto Module::serialize() const -> vec<byte_t> {
  return WasmerModule::from(this)->serialize();
}
auto deserialize(Store *store, const vec<byte_t> &bytes) -> own<Module> {
  return WasmerModule::deserialize(store, bytes);
}

class WASM_API_EXTERN WasmerForeign : WasmerRefWrapper,
                                      public From<Foreign, WasmerForeign> {
public:
  static auto make(Store *store) -> own<Foreign>;

  auto copy() const -> own<Foreign>;
};

auto Foreign::make(Store *store) -> own<Foreign> {
  return WasmerForeign::make(store);
}
auto Foreign::copy() const -> own<Foreign> {
  return WasmerForeign::from(this)->copy();
}

namespace {
struct WasmerExternWrapper : WasmerRefWrapper {
  const wasm_func_t *func() const {
    return kind() == ExternKind::FUNC ? wasm_extern_as_func_const(extern_())
                                      : nullptr;
  }
  const wasm_global_t *global() const {
    return kind() == ExternKind::GLOBAL ? wasm_extern_as_global_const(extern_())
                                        : nullptr;
  }
  const wasm_table_t *table() const {
    return kind() == ExternKind::TABLE ? wasm_extern_as_table_const(extern_())
                                       : nullptr;
  }
  const wasm_memory_t *memory() const {
    return kind() == ExternKind::MEMORY ? wasm_extern_as_memory_const(extern_())
                                        : nullptr;
  }
  auto kind() const -> ExternKind {
    return c_externkind_to_cxx_externkind(wasm_extern_kind(extern_()));
  }
};
} // namespace

class WasmerExtern : WasmerExternWrapper, public From<Extern, WasmerExtern> {
public:
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

public:
  static auto make(Store *store, const FuncType *functype, callback cb)
      -> own<Func>;
  static auto make(Store *store, const FuncType *functype, callback_with_env cb,
                   void *env, void (*finalizer)(void *) = nullptr) -> own<Func>;
  auto copy() const -> own<Func>;

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

  c_own<wasm_func_t> func;
};

void Func::destroy() { delete WasmerFunc::from(this); }

auto Func::make(Store *store, const FuncType *functype, callback cb)
    -> own<Func> {
  return WasmerFunc::make(store, functype, cb);
}
auto Func::make(Store *store, const FuncType *functype, callback_with_env cb,
                void *env, void (*finalizer)(void *)) -> own<Func> {
  return WasmerFunc::make(store, functype, cb, env, finalizer);
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
public:
  static auto make(WasmerStore *store, const WasmerModule *module,
                   const vec<WasmerExtern *> &imports,
                   own<WasmerTrap> *trap = nullptr) -> own<WasmerInstance>;

  auto copy() const -> own<Instance>;

  auto exports() const -> ownvec<Extern>;
};

auto Instance::make(Store *store, const Module *module,
                    const vec<Extern *> &imports, own<Trap> *trap)
    -> own<Instance> {
  vec<WasmerExtern *> wasmer_imports =
      vec<WasmerExtern *>::make_uninitialized(imports.size());
  for (int i = 0; i != imports.size(); ++i) {
    wasmer_imports[i] = WasmerExtern::from(imports[i]);
  }
  own<WasmerTrap> wasmer_trap;
  auto instance =
      WasmerInstance::make(WasmerStore::from(store), WasmerModule::from(module),
                           wasmer_imports, trap ? &wasmer_trap : nullptr);
  if (trap)
    *trap = std::move(wasmer_trap);
  return instance;
}

void Instance::destroy() { delete WasmerInstance::from(this); }

auto Instance::copy() const -> own<Instance> {
  return WasmerInstance::from(this)->copy();
}

auto Instance::exports() const -> ownvec<Extern> {
  return WasmerInstance::from(this)->exports();
}
