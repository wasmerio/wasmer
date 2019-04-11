#include <cstddef>
#include <cstdint>
#include <llvm/ExecutionEngine/RuntimeDyld.h>
#include <iostream>
#include <sstream>
#include <exception>

typedef enum
{
    PROTECT_NONE,
    PROTECT_READ,
    PROTECT_READ_WRITE,
    PROTECT_READ_EXECUTE,
} mem_protect_t;

typedef enum
{
    RESULT_OK,
    RESULT_ALLOCATE_FAILURE,
    RESULT_PROTECT_FAILURE,
    RESULT_DEALLOC_FAILURE,
    RESULT_OBJECT_LOAD_FAILURE,
} result_t;

typedef result_t (*alloc_memory_t)(size_t size, mem_protect_t protect, uint8_t **ptr_out, size_t *size_out);
typedef result_t (*protect_memory_t)(uint8_t *ptr, size_t size, mem_protect_t protect);
typedef result_t (*dealloc_memory_t)(uint8_t *ptr, size_t size);
typedef uintptr_t (*lookup_vm_symbol_t)(const char *name_ptr, size_t length);
typedef void (*fde_visitor_t)(uint8_t *fde);
typedef result_t (*visit_fde_t)(uint8_t *fde, size_t size, fde_visitor_t visitor);

typedef void (*trampoline_t)(void *, void *, void *, void *);

typedef struct
{
    /* Memory management. */
    alloc_memory_t alloc_memory;
    protect_memory_t protect_memory;
    dealloc_memory_t dealloc_memory;

    lookup_vm_symbol_t lookup_vm_symbol;

    visit_fde_t visit_fde;
} callbacks_t;

struct WasmException
{
  public:
    virtual std::string description() const noexcept = 0;
};

struct UncatchableException : WasmException
{
  public:
    virtual std::string description() const noexcept override
    {
        return "Uncatchable exception";
    }
};

struct UserException : UncatchableException
{
  public:
    UserException(size_t data, size_t vtable) : data(data), vtable(vtable) {}

    virtual std::string description() const noexcept override
    {
        return "user exception";
    }

    // The parts of a `Box<dyn Any>`.
    size_t data, vtable;
};

struct WasmTrap : UncatchableException
{
  public:
    enum Type
    {
        Unreachable = 0,
        IncorrectCallIndirectSignature = 1,
        MemoryOutOfBounds = 2,
        CallIndirectOOB = 3,
        IllegalArithmetic = 4,
        Unknown,
    };

    WasmTrap(Type type) : type(type) {}

    virtual std::string description() const noexcept override
    {
        std::ostringstream ss;
        ss
            << "WebAssembly trap:" << '\n'
            << " - type: " << type << '\n';

        return ss.str();
    }

    Type type;

  private:
    friend std::ostream &operator<<(std::ostream &out, const Type &ty)
    {
        switch (ty)
        {
        case Type::Unreachable:
            out << "unreachable";
            break;
        case Type::IncorrectCallIndirectSignature:
            out << "incorrect call_indirect signature";
            break;
        case Type::MemoryOutOfBounds:
            out << "memory access out-of-bounds";
            break;
        case Type::CallIndirectOOB:
            out << "call_indirect out-of-bounds";
            break;
        case Type::IllegalArithmetic:
            out << "illegal arithmetic operation";
            break;
        case Type::Unknown:
        default:
            out << "unknown";
            break;
        }
        return out;
    }
};

struct CatchableException : WasmException
{
  public:
    CatchableException(uint32_t type_id, uint32_t value_num) : type_id(type_id), value_num(value_num) {}

    virtual std::string description() const noexcept override
    {
        return "catchable exception";
    }

    uint32_t type_id, value_num;
    uint64_t values[1];
};

struct WasmModule
{
  public:
    WasmModule(
        const uint8_t *object_start,
        size_t object_size,
        callbacks_t callbacks);

    void *get_func(llvm::StringRef name) const;

  private:
    std::unique_ptr<llvm::RuntimeDyld::MemoryManager> memory_manager;
    std::unique_ptr<llvm::object::ObjectFile> object_file;
    std::unique_ptr<llvm::RuntimeDyld> runtime_dyld;
};

extern "C"
{
    result_t module_load(const uint8_t *mem_ptr, size_t mem_size, callbacks_t callbacks, WasmModule **module_out)
    {
        *module_out = new WasmModule(mem_ptr, mem_size, callbacks);

        return RESULT_OK;
    }

    [[noreturn]] void throw_trap(WasmTrap::Type ty) {
        throw WasmTrap(ty);
    }

    void module_delete(WasmModule *module)
    {
        delete module;
    }

    // Throw a fat pointer that's assumed to be `*mut dyn Any` on the rust
    // side.
    [[noreturn]] void throw_any(size_t data, size_t vtable) {
        throw UserException(data, vtable);
    }

    bool invoke_trampoline(
        trampoline_t trampoline,
        void *ctx,
        void *func,
        void *params,
        void *results,
        WasmTrap::Type *trap_out,
        void *invoke_env) throw()
    {
        try
        {
            trampoline(ctx, func, params, results);
            return true;
        }
        catch (const WasmTrap &e)
        {
            *trap_out = e.type;
            return false;
        }
        catch (const WasmException &e)
        {
            *trap_out = WasmTrap::Type::Unknown;
            return false;
        }
        catch (...)
        {
            *trap_out = WasmTrap::Type::Unknown;
            return false;
        }
    }

    void *get_func_symbol(WasmModule *module, const char *name)
    {
        return module->get_func(llvm::StringRef(name));
    }
}