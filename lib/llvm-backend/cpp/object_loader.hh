#include <cstddef>
#include <cstdint>
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

typedef result_t (*alloc_memory_t)(size_t size, mem_protect_t protect, uint8_t** ptr_out, size_t* size_out);
typedef result_t (*protect_memory_t)(uint8_t* ptr, size_t size, mem_protect_t protect);
typedef result_t (*dealloc_memory_t)(uint8_t* ptr, size_t size);
typedef uintptr_t (*lookup_vm_symbol_t)(const char* name_ptr, size_t length);

typedef struct {
    /* Memory management. */
    alloc_memory_t alloc_memory;
    protect_memory_t protect_memory;
    dealloc_memory_t dealloc_memory;

    lookup_vm_symbol_t lookup_vm_symbol;
} callbacks_t;

class WasmModule {
public:
    WasmModule(
        const uint8_t *object_start,
        size_t object_size,
        callbacks_t callbacks
    );

    void *get_func(llvm::StringRef name) const;
private:
    llvm::RuntimeDyld::MemoryManager* memory_manager;
    std::unique_ptr<llvm::object::ObjectFile> object_file;
    std::unique_ptr<llvm::RuntimeDyld> runtime_dyld;
};

extern "C" {
    result_t module_load(const uint8_t* mem_ptr, size_t mem_size, callbacks_t callbacks, WasmModule** module_out) {
        *module_out = new WasmModule(mem_ptr, mem_size, callbacks);

        return RESULT_OK;
    }

    void module_delete(WasmModule* module) {
        delete module;
    }

    void* get_func_symbol(WasmModule* module, const char* name) {
        return module->get_func(llvm::StringRef(name));
    }
}