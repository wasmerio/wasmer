#include <cstddef>
#include <cstdint>

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
typedef uintptr_t (*lookup_vm_symbol_t)(char* name_ptr, size_t name_size);

typedef struct {
    /* Memory management. */
    alloc_memory_t alloc_memory;
    protect_memory_t protect_memory;
    dealloc_memory_t dealloc_memory;

    lookup_vm_symbol_t lookup_vm_symbol;
} callbacks_t;

extern "C" {
    result_t object_load(uint8_t* mem_ptr, size_t mem_size, callbacks_t* callbacks) {
        return RESULT_OK;
    }

    void test_cpp() {
    }
}