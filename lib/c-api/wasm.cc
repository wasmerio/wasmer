// Implementation of the wasm-c-api C++ API for wasmer on top of the Wasmer C++
// API and the wasm C API.

#include "wasm.hh"
#include "wasm.h"

#include "wasmer.hh"

using namespace wasm;

class WASM_API_EXTERN WasmerConfig : public Config {
public:
  WasmerConfig() : Config{} {}

  static WasmerConfig *from(void *base) {
    return reinterpret_cast<WasmerConfig *>(base);
  }

private:
  // TODO: custom config state
};

Config::~Config() {}

void Config::operator delete(void *ptr) { delete WasmerConfig::from(ptr); }

auto Config::make() -> own<Config> { return make_own(new WasmerConfig); }

// TODO: add custom config calls

class WASM_API_EXTERN WasmerEngine : public Engine {
public:
  WasmerEngine() : Engine{} {}

  static WasmerEngine *from(void *base) {
    return reinterpret_cast<WasmerEngine *>(base);
  }

private:
  // TODO: wasmer engine
};

Engine::~Engine() {}

void Engine::operator delete(void *ptr) { delete WasmerEngine::from(ptr); }

auto Engine::make(own<Config> &&config) -> own<Engine> {
  return make_own(new WasmerEngine);
}

class WASM_API_EXTERN WasmerStore : public Store {
public:
  WasmerStore() : Store{} {}

  static WasmerStore *from(void *base) {
    return reinterpret_cast<WasmerStore *>(base);
  }

private:
  // TODO: wasmer store
};

Store::~Store() {}

void Store::operator delete(void *ptr) { delete WasmerStore::from(ptr); }

class WASM_API_EXTERN WasmerValType : public ValType {
public:
  explicit WasmerValType(ValKind m_kind) : ValType{}, m_kind(m_kind) {}

  auto copy() const -> own<ValType> {
    return make_own(new WasmerValType(kind()));
  }

  auto kind() const -> ValKind { return m_kind; }

  static const WasmerValType *from(const ValType *base) {
    return static_cast<const WasmerValType *>(base);
  }
  static WasmerValType *from(void *base) {
    return reinterpret_cast<WasmerValType *>(base);
  }

private:
  ValKind m_kind;
};

ValType::~ValType() {}

auto ValType::make(ValKind kind) -> own<ValType> {
  return make_own(new WasmerValType(kind));
}

void ValType::operator delete(void *ptr) { delete WasmerValType::from(ptr); }

auto ValType::copy() const -> own<ValType> {
  return WasmerValType::from(this)->copy();
}

auto ValType::kind() const -> ValKind {
  return WasmerValType::from(this)->kind();
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

template <typename T, typename C, typename S1, typename S2>
ownvec<T> c_vec_to_cxx_ownvec(C c_vec, S2 (*convert)(S1)) {
  ownvec<T> v = ownvec<T>::make_uninitialized(c_vec->size);
  for (int i = 0; i < c_vec->size; ++i) {
    v[i] = convert(c_vec->data[i]);
  }
  return v;
}

static own<ValType> c_valtype_to_cxx_ownvaltype(const wasm_valtype_t *c) {
  return ValType::make(c_valkind_to_cxx_valkind(wasm_valtype_kind(c)));
}

static wasm_valtype_t *cxx_valtype_to_c_valtype(const ValType *val_type) {
  return wasm_valtype_new(cxx_valkind_to_c_valkind(val_type->kind()));
}

class WasmerFuncType : public FuncType {
public:
  explicit WasmerFuncType(const wasm_functype_t *m_func_type)
      : FuncType{}, m_func_type(const_cast<wasm_functype_t *>(m_func_type)),
        m_params(c_vec_to_cxx_ownvec<ValType>(wasm_functype_params(m_func_type),
                                              c_valtype_to_cxx_ownvaltype)),
        m_results(c_vec_to_cxx_ownvec<ValType>(
            wasm_functype_results(m_func_type), c_valtype_to_cxx_ownvaltype)) {}
  ~WasmerFuncType() { wasm_functype_delete(m_func_type); }

  auto copy() const -> own<FuncType> {
    return make_own(new WasmerFuncType(wasm_functype_copy(m_func_type)));
  }

  auto params() const -> const ownvec<ValType> & { return m_params; }
  auto results() const -> const ownvec<ValType> & { return m_results; }

  static const WasmerFuncType *from(const FuncType *base) {
    return static_cast<const WasmerFuncType *>(base);
  }

private:
  wasm_functype_t *m_func_type;
  ownvec<ValType> m_params;
  ownvec<ValType> m_results;
};

FuncType::~FuncType() {}

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

class WASM_API_EXTERN WasmerGlobalType : public GlobalType {
public:
  explicit WasmerGlobalType(const wasm_globaltype_t *m_global_type)
      : GlobalType{},
        m_global_type(const_cast<wasm_globaltype_t *>(m_global_type)),
        m_valtype(
            c_valtype_to_cxx_ownvaltype(wasm_globaltype_content(m_global_type))
                .release()) {}

  explicit WasmerGlobalType(own<ValType> &&m_valtype, Mutability mutability)
      : GlobalType{}, m_global_type(wasm_globaltype_new(
                          cxx_valtype_to_c_valtype(m_valtype.get()),
                          cxx_mutability_to_c_mutability(mutability))),
        m_valtype(m_valtype.release()) {}

  ~WasmerGlobalType() {
    wasm_globaltype_delete(m_global_type);
    delete WasmerValType::from(m_valtype);
  }

  auto copy() const -> own<GlobalType> {
    return make_own(new WasmerGlobalType(wasm_globaltype_copy(m_global_type)));
  }

  auto content() const -> const ValType * { return m_valtype; }
  auto mutability() const -> Mutability {
    return c_mutability_to_cxx_mutability(
        wasm_globaltype_mutability(m_global_type));
  }

  static const WasmerGlobalType *from(const GlobalType *base) {
    return static_cast<const WasmerGlobalType *>(base);
  }

private:
  wasm_globaltype_t *m_global_type;
  ValType *m_valtype;
};

GlobalType::~GlobalType() {}

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

// TODO: table, memory

class WASM_API_EXTERN WasmerExternType : public ExternType {
public:
  // We take ownership of `m_extern_type` and will delete it.
  explicit WasmerExternType(wasm_externtype_t *m_extern_type)
      : ExternType{}, m_extern_type(m_extern_type) {}

  ~WasmerExternType() { wasm_externtype_delete(m_extern_type); }

  auto copy() const -> own<ExternType> {
    return make_own(new WasmerExternType(wasm_externtype_copy(m_extern_type)));
  }

  auto kind() const -> ExternKind {
    return c_externkind_to_cxx_externkind(wasm_externtype_kind(m_extern_type));
  }

  auto func() -> FuncType * {
    return new WasmerFuncType(wasm_externtype_as_functype(m_extern_type));
  }
  auto global() -> GlobalType * {
    return new WasmerGlobalType(wasm_externtype_as_globaltype(m_extern_type));
  }
  auto table() -> TableType *;
  auto memory() -> MemoryType *;

  auto func() const -> const FuncType * {
    return new WasmerFuncType(wasm_externtype_as_functype_const(m_extern_type));
  }
  auto global() const -> const GlobalType * {
    return new WasmerGlobalType(
        wasm_externtype_as_globaltype_const(m_extern_type));
  }
  auto table() const -> const TableType *;
  auto memory() const -> const MemoryType *;

  static const WasmerExternType *from(const ExternType *base) {
    return static_cast<const WasmerExternType *>(base);
  }
  static WasmerExternType *from(ExternType *base) {
    return static_cast<WasmerExternType *>(base);
  }
  static WasmerExternType *from(void *base) {
    return reinterpret_cast<WasmerExternType *>(base);
  }

private:
  wasm_externtype_t *m_extern_type;
};

ExternType::~ExternType() {}

void ExternType::operator delete(void *ptr) {
  delete WasmerExternType::from(ptr);
}

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
