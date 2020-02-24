#pragma once

#include <cstddef>
#include <cstdint>
#include <exception>
#include <functional>
#include <iostream>
#include <memory>
#include <setjmp.h>
#include <sstream>

#include <llvm/ExecutionEngine/RuntimeDyld.h>

typedef enum {
  PROTECT_NONE,
  PROTECT_READ,
  PROTECT_READ_WRITE,
  PROTECT_READ_EXECUTE,
} mem_protect_t;

typedef enum {
  RESULT_OK,
  RESULT_ALLOCATE_FAILURE,
  RESULT_PROTECT_FAILURE,
  RESULT_DEALLOC_FAILURE,
  RESULT_OBJECT_LOAD_FAILURE,
} result_t;

typedef result_t (*alloc_memory_t)(size_t size, mem_protect_t protect,
                                   uint8_t **ptr_out, size_t *size_out);
typedef result_t (*protect_memory_t)(uint8_t *ptr, size_t size,
                                     mem_protect_t protect);
typedef result_t (*dealloc_memory_t)(uint8_t *ptr, size_t size);
typedef uintptr_t (*lookup_vm_symbol_t)(const char *name_ptr, size_t length);
typedef void (*fde_visitor_t)(uint8_t *fde);
typedef result_t (*visit_fde_t)(uint8_t *fde, size_t size,
                                fde_visitor_t visitor);

typedef void (*trampoline_t)(void *, void *, void *, void *);

typedef struct {
  /* Memory management. */
  alloc_memory_t alloc_memory;
  protect_memory_t protect_memory;
  dealloc_memory_t dealloc_memory;

  lookup_vm_symbol_t lookup_vm_symbol;

  visit_fde_t visit_fde;
} callbacks_t;

typedef struct {
  size_t data, vtable;
} box_any_t;

enum WasmTrapType {
  Unreachable = 0,
  IncorrectCallIndirectSignature = 1,
  MemoryOutOfBounds = 2,
  CallIndirectOOB = 3,
  IllegalArithmetic = 4,
  MisalignedAtomicAccess = 5,
  Unknown,
};

extern "C" void callback_trampoline(void *, void *);

struct MemoryManager : llvm::RuntimeDyld::MemoryManager {
public:
  MemoryManager(callbacks_t callbacks) : callbacks(callbacks) {}
  virtual ~MemoryManager() override;

  inline uint8_t *get_stack_map_ptr() const { return stack_map_ptr; }
  inline size_t get_stack_map_size() const { return stack_map_size; }
  inline uint8_t *get_code_ptr() const { return (uint8_t *)code_start_ptr; }
  inline size_t get_code_size() const { return code_size; }
  inline uint8_t *get_readwrite_section_ptr() const { return readwrite_section.base; }
  inline size_t get_readwrite_section_size() const { return readwrite_section.size; }

  virtual uint8_t *allocateCodeSection(uintptr_t size, unsigned alignment,
                                       unsigned section_id,
                                       llvm::StringRef section_name) override;
  virtual uint8_t *allocateDataSection(uintptr_t size, unsigned alignment,
                                       unsigned section_id,
                                       llvm::StringRef section_name,
                                       bool read_only) override;
  virtual void reserveAllocationSpace(uintptr_t code_size, uint32_t code_align,
                                      uintptr_t read_data_size,
                                      uint32_t read_data_align,
                                      uintptr_t read_write_data_size,
                                      uint32_t read_write_data_align) override;
  /* Turn on the `reserveAllocationSpace` callback. */
  virtual bool needsToReserveAllocationSpace() override;
  virtual void registerEHFrames(uint8_t *addr, uint64_t LoadAddr,
                                size_t size) override;
  virtual void deregisterEHFrames() override;
  virtual bool finalizeMemory(std::string *ErrMsg = nullptr) override;
  virtual void notifyObjectLoaded(llvm::RuntimeDyld &RTDyld,
                                  const llvm::object::ObjectFile &Obj) override;

private:
  struct Section {
    uint8_t *base;
    size_t size;
  };

  uint8_t *allocate_bump(Section &section, uintptr_t &bump_ptr, size_t size,
                         size_t align);

  Section code_section, read_section, readwrite_section;
  uintptr_t code_start_ptr;
  size_t code_size;
  uintptr_t code_bump_ptr, read_bump_ptr, readwrite_bump_ptr;
  uint8_t *eh_frame_ptr;
  size_t eh_frame_size;
  bool eh_frames_registered = false;

  callbacks_t callbacks;

  uint8_t *stack_map_ptr = nullptr;
  size_t stack_map_size = 0;
};

struct WasmErrorSink {
  WasmTrapType *trap_out;
  box_any_t *user_error;
};

struct WasmException : std::exception {
public:
  virtual std::string description() const noexcept { return "unknown"; }

  virtual const char *what() const noexcept override {
    return "wasm exception";
  }

  virtual void write_error(WasmErrorSink &out) const noexcept {
    *out.trap_out = WasmTrapType::Unknown;
  }
};

void catch_unwind(std::function<void()> &&f);
[[noreturn]] void unsafe_unwind(WasmException *exception);

struct UncatchableException : WasmException {
public:
  virtual std::string description() const noexcept override {
    return "Uncatchable exception";
  }
};

struct UserException : UncatchableException {
public:
  UserException(size_t data, size_t vtable) : error_data({data, vtable}) {}

  virtual std::string description() const noexcept override {
    return "user exception";
  }

  // The parts of a `Box<dyn Any>`.
  box_any_t error_data;

  virtual void write_error(WasmErrorSink &out) const noexcept override {
    *out.user_error = error_data;
  }
};

struct BreakpointException : UncatchableException {
public:
  BreakpointException(uintptr_t callback) : callback(callback) {}

  virtual std::string description() const noexcept override {
    return "breakpoint exception";
  }

  uintptr_t callback;

  virtual void write_error(WasmErrorSink &out) const noexcept override {
    puts("CB TRAMPOLINE");
    callback_trampoline(out.user_error, (void *)callback);
  }
};

struct WasmModule {
public:
  WasmModule(const uint8_t *object_start, size_t object_size,
             callbacks_t callbacks);

  void *get_func(llvm::StringRef name) const;
  uint8_t *get_stack_map_ptr() const;
  size_t get_stack_map_size() const;
  uint8_t *get_code_ptr() const;
  size_t get_code_size() const;
  uint8_t *get_readwrite_ptr() const;
  size_t get_readwrite_size() const;

  bool _init_failed = false;

private:
  std::unique_ptr<MemoryManager> memory_manager;
  std::unique_ptr<llvm::object::ObjectFile> object_file;
  std::unique_ptr<llvm::RuntimeDyld> runtime_dyld;
};

struct WasmTrap : UncatchableException {
public:
  WasmTrap(WasmTrapType type) : type(type) {}

  virtual std::string description() const noexcept override {
    std::ostringstream ss;
    ss << "WebAssembly trap:" << '\n' << " - type: " << type << '\n';

    return ss.str();
  }

  WasmTrapType type;

  virtual void write_error(WasmErrorSink &out) const noexcept override {
    *out.trap_out = type;
  }

private:
  friend std::ostream &operator<<(std::ostream &out, const WasmTrapType &ty) {
    switch (ty) {
    case WasmTrapType::Unreachable:
      out << "unreachable";
      break;
    case WasmTrapType::IncorrectCallIndirectSignature:
      out << "incorrect call_indirect signature";
      break;
    case WasmTrapType::MemoryOutOfBounds:
      out << "memory access out-of-bounds";
      break;
    case WasmTrapType::CallIndirectOOB:
      out << "call_indirect out-of-bounds";
      break;
    case WasmTrapType::IllegalArithmetic:
      out << "illegal arithmetic operation";
      break;
    case WasmTrapType::Unknown:
    default:
      out << "unknown";
      break;
    }
    return out;
  }
};

struct CatchableException : WasmException {
public:
  CatchableException(uint32_t type_id, uint32_t value_num)
      : type_id(type_id), value_num(value_num) {}

  virtual std::string description() const noexcept override {
    return "catchable exception";
  }

  uint32_t type_id, value_num;
  uint64_t values[1];
};

extern "C" {

result_t module_load(const uint8_t *mem_ptr, size_t mem_size,
                     callbacks_t callbacks, WasmModule **module_out) {
  *module_out = new WasmModule(mem_ptr, mem_size, callbacks);

  if ((*module_out)->_init_failed) {
    return RESULT_OBJECT_LOAD_FAILURE;
  }

  return RESULT_OK;
}

[[noreturn]] void throw_trap(WasmTrapType ty) {
  unsafe_unwind(new WasmTrap(ty));
}

void module_delete(WasmModule *module) { delete module; }

// Throw a fat pointer that's assumed to be `*mut dyn Any` on the rust
// side.
[[noreturn]] void throw_any(size_t data, size_t vtable) {
  unsafe_unwind(new UserException(data, vtable));
}

// Throw a pointer that's assumed to be codegen::BreakpointHandler on the
// rust side.
[[noreturn]] void throw_breakpoint(uintptr_t callback) {
  unsafe_unwind(new BreakpointException(callback));
}

bool cxx_invoke_trampoline(trampoline_t trampoline, void *ctx, void *func,
                       void *params, void *results, WasmTrapType *trap_out,
                       box_any_t *user_error, void *invoke_env) noexcept {
  try {
    catch_unwind([trampoline, ctx, func, params, results]() {
      trampoline(ctx, func, params, results);
    });
    return true;
  } catch (std::unique_ptr<WasmException> &e) {
    WasmErrorSink sink;
    sink.trap_out = trap_out;
    sink.user_error = user_error;
    e->write_error(sink);
    return false;
  } catch (...) {
    *trap_out = WasmTrapType::Unknown;
    return false;
  }
}

void *get_func_symbol(WasmModule *module, const char *name) {
  return module->get_func(llvm::StringRef(name));
}

const uint8_t *llvm_backend_get_stack_map_ptr(const WasmModule *module) {
  return module->get_stack_map_ptr();
}

size_t llvm_backend_get_stack_map_size(const WasmModule *module) {
  return module->get_stack_map_size();
}

const uint8_t *llvm_backend_get_code_ptr(const WasmModule *module) {
  return module->get_code_ptr();
}

size_t llvm_backend_get_code_size(const WasmModule *module) {
  return module->get_code_size();
}

const uint8_t *llvm_backend_get_readwrite_ptr(const WasmModule *module) {
  return module->get_readwrite_ptr();
}

size_t llvm_backend_get_readwrite_size(const WasmModule *module) {
  return module->get_readwrite_size();
}
}
