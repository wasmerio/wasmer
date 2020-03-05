#include "object_loader.hh"
#include <iostream>
#include <memory>
#include <setjmp.h>

extern "C" void __register_frame(uint8_t *);
extern "C" void __deregister_frame(uint8_t *);

MemoryManager::~MemoryManager() {
  deregisterEHFrames();
  // Deallocate all of the allocated memory.
  callbacks.dealloc_memory(code_section.base, code_section.size);
  callbacks.dealloc_memory(read_section.base, read_section.size);
  callbacks.dealloc_memory(readwrite_section.base, readwrite_section.size);
}
void unwinding_setjmp(jmp_buf stack_out, void (*func)(void *), void *userdata) {
  if (setjmp(stack_out)) {

  } else {
    func(userdata);
  }
}

[[noreturn]] void unwinding_longjmp(jmp_buf stack_in) { longjmp(stack_in, 42); }

struct UnwindPoint {
  UnwindPoint *prev;
  jmp_buf stack;
  std::function<void()> *f;
  std::unique_ptr<WasmException> exception;
};

static thread_local UnwindPoint *unwind_state = nullptr;

static void unwind_payload(void *_point) {
  UnwindPoint *point = (UnwindPoint *)_point;
  (*point->f)();
}

void catch_unwind(std::function<void()> &&f) {
  UnwindPoint current;
  current.prev = unwind_state;
  current.f = &f;
  unwind_state = &current;

  unwinding_setjmp(current.stack, unwind_payload, (void *)&current);

  unwind_state = current.prev;
  if (current.exception) {
    throw std::move(current.exception);
  }
}

void unsafe_unwind(WasmException *exception) {
  UnwindPoint *state = unwind_state;
  if (state) {
    state->exception.reset(exception);
    unwinding_longjmp(state->stack);
  } else {
    abort();
  }
}

uint8_t *MemoryManager::allocateCodeSection(uintptr_t size, unsigned alignment,
                                            unsigned section_id,
                                            llvm::StringRef section_name) {
  return allocate_bump(code_section, code_bump_ptr, size, alignment);
}

uint8_t *MemoryManager::allocateDataSection(uintptr_t size, unsigned alignment,
                                            unsigned section_id,
                                            llvm::StringRef section_name,
                                            bool read_only) {
  // Allocate from the read-only section or the read-write section, depending
  // on if this allocation should be read-only or not.
  uint8_t *ret;
  if (read_only) {
    ret = allocate_bump(read_section, read_bump_ptr, size, alignment);
  } else {
    ret = allocate_bump(readwrite_section, readwrite_bump_ptr, size, alignment);
  }
  if (section_name.equals(llvm::StringRef("__llvm_stackmaps")) ||
      section_name.equals(llvm::StringRef(".llvm_stackmaps"))) {
    stack_map_ptr = ret;
    stack_map_size = size;
  }
  return ret;
}

void MemoryManager::reserveAllocationSpace(uintptr_t code_size,
                                           uint32_t code_align,
                                           uintptr_t read_data_size,
                                           uint32_t read_data_align,
                                           uintptr_t read_write_data_size,
                                           uint32_t read_write_data_align) {
  auto aligner = [](uintptr_t ptr, size_t align) {
    if (ptr == 0) {
      return align;
    }
    return (ptr + align - 1) & ~(align - 1);
  };
  uint8_t *code_ptr_out = nullptr;
  size_t code_size_out = 0;
  auto code_result =
      callbacks.alloc_memory(aligner(code_size, 4096), PROTECT_READ_WRITE,
                             &code_ptr_out, &code_size_out);
  assert(code_result == RESULT_OK);
  code_section = Section{code_ptr_out, code_size_out};
  code_bump_ptr = (uintptr_t)code_ptr_out;
  code_start_ptr = (uintptr_t)code_ptr_out;
  this->code_size = code_size;

  uint8_t *read_ptr_out = nullptr;
  size_t read_size_out = 0;
  auto read_result =
      callbacks.alloc_memory(aligner(read_data_size, 4096), PROTECT_READ_WRITE,
                             &read_ptr_out, &read_size_out);
  assert(read_result == RESULT_OK);
  read_section = Section{read_ptr_out, read_size_out};
  read_bump_ptr = (uintptr_t)read_ptr_out;

  uint8_t *readwrite_ptr_out = nullptr;
  size_t readwrite_size_out = 0;
  auto readwrite_result = callbacks.alloc_memory(
      aligner(read_write_data_size, 4096), PROTECT_READ_WRITE,
      &readwrite_ptr_out, &readwrite_size_out);
  assert(readwrite_result == RESULT_OK);
  readwrite_section = Section{readwrite_ptr_out, readwrite_size_out};
  readwrite_bump_ptr = (uintptr_t)readwrite_ptr_out;
}

bool MemoryManager::needsToReserveAllocationSpace() { return true; }

void MemoryManager::registerEHFrames(uint8_t *addr, uint64_t LoadAddr,
                                     size_t size) {
// We don't know yet how to do this on Windows, so we hide this on compilation
// so we can compile and pass spectests on unix systems
#ifndef _WIN32
  eh_frame_ptr = addr;
  eh_frame_size = size;
  eh_frames_registered = true;
  callbacks.visit_fde(addr, size, __register_frame);
#endif
}

void MemoryManager::deregisterEHFrames() {
// We don't know yet how to do this on Windows, so we hide this on compilation
// so we can compile and pass spectests on unix systems
#ifndef _WIN32
  if (eh_frames_registered) {
    callbacks.visit_fde(eh_frame_ptr, eh_frame_size, __deregister_frame);
  }
#endif
}

bool MemoryManager::finalizeMemory(std::string *ErrMsg) {
  auto code_result =
      callbacks.protect_memory(code_section.base, code_section.size,
                               mem_protect_t::PROTECT_READ_EXECUTE);
  if (code_result != RESULT_OK) {
    return false;
  }

  auto read_result = callbacks.protect_memory(
      read_section.base, read_section.size, mem_protect_t::PROTECT_READ);
  if (read_result != RESULT_OK) {
    return false;
  }

  // The readwrite section is already mapped as read-write.

  return false;
}

void MemoryManager::notifyObjectLoaded(llvm::RuntimeDyld &RTDyld,
                                       const llvm::object::ObjectFile &Obj) {}

uint8_t *MemoryManager::allocate_bump(Section &section, uintptr_t &bump_ptr,
                                      size_t size, size_t align) {
  auto aligner = [](uintptr_t &ptr, size_t align) {
    ptr = (ptr + align - 1) & ~(align - 1);
  };

  // Align the bump pointer to the requires alignment.
  aligner(bump_ptr, align);

  auto ret_ptr = bump_ptr;
  bump_ptr += size;

  assert(bump_ptr <= (uintptr_t)section.base + section.size);

  return (uint8_t *)ret_ptr;
}

struct SymbolLookup : llvm::JITSymbolResolver {
public:
  SymbolLookup(callbacks_t callbacks) : callbacks(callbacks) {}

  void lookup(const LookupSet &symbols, OnResolvedFunction OnResolved) {
    LookupResult result;

    for (auto symbol : symbols) {
      result.emplace(symbol, symbol_lookup(symbol));
    }

    OnResolved(result);
  }

  llvm::Expected<LookupSet> getResponsibilitySet(const LookupSet &Symbols) {
    const std::set<llvm::StringRef> empty;
    return empty;
  }

private:
  llvm::JITEvaluatedSymbol symbol_lookup(llvm::StringRef name) {
    uint64_t addr = callbacks.lookup_vm_symbol(name.data(), name.size());

    return llvm::JITEvaluatedSymbol(addr, llvm::JITSymbolFlags::None);
  }

  callbacks_t callbacks;
};

WasmModule::WasmModule(const uint8_t *object_start, size_t object_size,
                       callbacks_t callbacks)
    : memory_manager(
          std::unique_ptr<MemoryManager>(new MemoryManager(callbacks))) {

  if (auto created_object_file =
          llvm::object::ObjectFile::createObjectFile(llvm::MemoryBufferRef(
              llvm::StringRef((const char *)object_start, object_size),
              "object"))) {
    object_file = cantFail(std::move(created_object_file));
    SymbolLookup symbol_resolver(callbacks);
    runtime_dyld = std::unique_ptr<llvm::RuntimeDyld>(
        new llvm::RuntimeDyld(*memory_manager, symbol_resolver));

    runtime_dyld->setProcessAllSections(true);

    runtime_dyld->loadObject(*object_file);
    runtime_dyld->finalizeWithMemoryManagerLocking();

    if (runtime_dyld->hasError()) {
      _init_failed = true;
      return;
    }
  } else {
    _init_failed = true;
  }
}

void *WasmModule::get_func(llvm::StringRef name) const {
  auto symbol = runtime_dyld->getSymbol(name);
  return (void *)symbol.getAddress();
}

uint8_t *WasmModule::get_stack_map_ptr() const {
  return memory_manager->get_stack_map_ptr();
}

size_t WasmModule::get_stack_map_size() const {
  return memory_manager->get_stack_map_size();
}

uint8_t *WasmModule::get_code_ptr() const {
  return memory_manager->get_code_ptr();
}

size_t WasmModule::get_code_size() const {
  return memory_manager->get_code_size();
}

uint8_t *WasmModule::get_readwrite_ptr() const {
  return memory_manager->get_readwrite_section_ptr();
}

size_t WasmModule::get_readwrite_size() const {
  return memory_manager->get_readwrite_section_size();
}

uint8_t *WasmModule::get_read_ptr() const {
  return memory_manager->get_read_section_ptr();
}

size_t WasmModule::get_read_size() const {
  return memory_manager->get_read_section_size();
}
