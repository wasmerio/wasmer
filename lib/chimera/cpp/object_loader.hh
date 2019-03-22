#include <cstddef>
#include <cstdint>
#include <llvm/ExecutionEngine/RuntimeDyld.h>
#include <iostream>
#include <sstream>
#include <exception>

#if defined(__linux__)
const std::string STACKMAP_SECTION_NAME = ".llvm_stackmaps";
#elif defined(__MACH__)
const std::string STACKMAP_SECTION_NAME = "__llvm_stackmaps";
#endif

extern "C" void __register_frame(uint8_t *);
extern "C" void __deregister_frame(uint8_t *);

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

typedef uint8_t *(*alloc_t)(size_t size, size_t align);
typedef void (*dealloc_t)(uint8_t *ptr, size_t size, size_t align);
typedef uint8_t *(*create_code_t)(void *pool, uint32_t code_size, uint32_t *offset_out);

typedef uintptr_t (*lookup_vm_symbol_t)(const char *name_ptr, size_t length);
typedef void (*fde_visitor_t)(uint8_t *fde);
typedef result_t (*visit_fde_t)(uint8_t *fde, size_t size, fde_visitor_t visitor);

typedef void (*trampoline_t)(void *, void *, void *, void *);

typedef struct
{
    alloc_t alloc;
    dealloc_t dealloc;
    create_code_t create_code;
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
    UserException(std::string msg) : msg(msg) {}

    virtual std::string description() const noexcept override
    {
        return std::string("user exception: ") + msg;
    }

  private:
    std::string msg;
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

struct MemoryManager : llvm::RuntimeDyld::MemoryManager
{
  public:
    MemoryManager(callbacks_t callbacks, void *pool) : callbacks(callbacks), pool(pool) {}

    virtual ~MemoryManager() override
    {
        deregisterEHFrames();
        // Deallocate all of the allocated memory.
        callbacks.dealloc(read_section.base, read_section.size, read_align);
        callbacks.dealloc(readwrite_section.base, readwrite_section.size, readwrite_align);
    }

    virtual uint8_t *allocateCodeSection(uintptr_t size, unsigned alignment, unsigned section_id, llvm::StringRef section_name) override
    {
        if (!allocate_code_was_called)
        {
            allocate_code_was_called = true;
        }
        else
        {
            std::cerr << "allocateCodeSection was called more than once" << std::endl;
            exit(1);
        }

        return code_section.base;
    }

    virtual uint8_t *allocateDataSection(uintptr_t size, unsigned alignment, unsigned section_id, llvm::StringRef section_name, bool read_only) override
    {
        // Allocate from the read-only section or the read-write section, depending on if this allocation
        // should be read-only or not.
        if (read_only)
        {
            auto ptr = allocate_bump(read_section, read_bump_ptr, size, alignment);

            return ptr;
        }
        else
        {
            auto ptr = allocate_bump(readwrite_section, readwrite_bump_ptr, size, alignment);

            if (section_name == STACKMAP_SECTION_NAME)
            {
                stackmap.base = ptr;
                stackmap.size = size;
            }

            return ptr;
        }
    }

    virtual void reserveAllocationSpace(
        uintptr_t code_size,
        uint32_t code_align,
        uintptr_t read_data_size,
        uint32_t read_data_align,
        uintptr_t read_write_data_size,
        uint32_t read_write_data_align) override
    {
        auto code_ptr = callbacks.create_code(pool, code_size, &code_offset);
        code_section = View{code_ptr, code_size};

        auto read_ptr = callbacks.alloc(read_data_size, read_data_align);
        assert(read_ptr);
        read_section = View{read_ptr, read_data_size};
        read_bump_ptr = (uintptr_t)read_ptr;
        read_align = read_data_align;

        auto readwrite_ptr = callbacks.alloc(read_write_data_size, read_write_data_align);
        assert(readwrite_ptr);
        readwrite_section = View{readwrite_ptr, read_write_data_size};
        readwrite_bump_ptr = (uintptr_t)readwrite_ptr;
        readwrite_align = read_write_data_align;
    }

    /* Turn on the `reserveAllocationSpace` callback. */
    virtual bool needsToReserveAllocationSpace() override
    {
        return true;
    }

    virtual void registerEHFrames(uint8_t *addr, uint64_t LoadAddr, size_t size) override
    {
        eh_frames.base = addr;
        eh_frames.size = size;
        eh_frames_registered = true;
        callbacks.visit_fde(addr, size, __register_frame);
    }

    virtual void deregisterEHFrames() override
    {
        if (eh_frames_registered)
        {
            callbacks.visit_fde(eh_frames.base, eh_frames.size, __deregister_frame);
        }
    }

    virtual bool finalizeMemory(std::string *ErrMsg = nullptr) override
    {
        return false;
    }

    virtual void notifyObjectLoaded(llvm::RuntimeDyld &RTDyld, const llvm::object::ObjectFile &Obj) override {}

    struct View
    {
        uint8_t *base;
        size_t size;
    };

    View eh_frames, stackmap = {0};
    uint32_t code_offset;

  private:
    uint8_t *allocate_bump(View &section, uintptr_t &bump_ptr, size_t size, size_t align)
    {
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

    View code_section, read_section, readwrite_section;
    uintptr_t read_bump_ptr, readwrite_bump_ptr;
    size_t read_align, readwrite_align;
    bool eh_frames_registered = false;

    callbacks_t callbacks;
    void *pool;
    bool allocate_code_was_called = false;
};

struct WasmFunction
{
  public:
    WasmFunction(
        const uint8_t *object_start,
        size_t object_size,
        callbacks_t callbacks,
        void *pool);

    std::unique_ptr<MemoryManager> memory_manager;

  private:
    std::unique_ptr<llvm::object::ObjectFile> object_file;
    std::unique_ptr<llvm::RuntimeDyld> runtime_dyld;
};

extern "C"
{
    result_t function_load(const uint8_t *mem_ptr, size_t mem_size, callbacks_t callbacks, void *pool, WasmFunction **function_out, uint32_t *code_offset_out)
    {
        auto function = new WasmFunction(mem_ptr, mem_size, callbacks, pool);
        *function_out = function;
        *code_offset_out = function->memory_manager->code_offset;

        return RESULT_OK;
    }

    uint8_t *get_stackmap(WasmFunction *function, size_t *size_out)
    {
        auto stackmap = function->memory_manager->stackmap;
        *size_out = stackmap.size;
        return stackmap.base;
    }

    void function_delete(WasmFunction *function)
    {
        delete function;
    }

    [[noreturn]] void throw_trap(WasmTrap::Type ty) {
        throw WasmTrap(ty);
    }

    bool invoke_trampoline(
        trampoline_t trampoline,
        void *ctx,
        void *func,
        void *params,
        void *results,
        WasmTrap::Type *trap_out) throw()
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
}