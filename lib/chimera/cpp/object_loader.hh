#include <cstddef>
#include <cstdint>
#include <llvm/ExecutionEngine/RuntimeDyld.h>
#include <iostream>
#include <sstream>
#include <exception>

#if defined(__linux__)
const std::string STACKMAP_SECTION_NAME = "__llvm_stackmaps";
#elif defined(__MACH__)
const std::string STACKMAP_SECTION_NAME = ".llvm_stackmaps";
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
    MemoryManager(callbacks_t callbacks) : callbacks(callbacks) {}

    virtual ~MemoryManager() override
    {
        deregisterEHFrames();
        // Deallocate all of the allocated memory.
        callbacks.dealloc_memory(code_section.base, code_section.size);
        callbacks.dealloc_memory(read_section.base, read_section.size);
        callbacks.dealloc_memory(readwrite_section.base, readwrite_section.size);
    }

    virtual uint8_t *allocateCodeSection(uintptr_t size, unsigned alignment, unsigned section_id, llvm::StringRef section_name) override
    {
        return allocate_bump(code_section, code_bump_ptr, size, alignment);
    }

    virtual uint8_t *allocateDataSection(uintptr_t size, unsigned alignment, unsigned section_id, llvm::StringRef section_name, bool read_only) override
    {
        // Allocate from the read-only section or the read-write section, depending on if this allocation
        // should be read-only or not.
        if (read_only)
        {
            auto ptr = allocate_bump(read_section, read_bump_ptr, size, alignment);

            if (section_name == STACKMAP_SECTION_NAME)
            {
                stackmap.base = ptr;
                stackmap.size = size;
            }

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
        auto aligner = [](uintptr_t ptr, size_t align) {
            if (ptr == 0)
            {
                return align;
            }
            return (ptr + align - 1) & ~(align - 1);
        };

        uint8_t *code_ptr_out = nullptr;
        size_t code_size_out = 0;
        auto code_result = callbacks.alloc_memory(aligner(code_size, 4096), PROTECT_READ_WRITE, &code_ptr_out, &code_size_out);
        assert(code_result == RESULT_OK);
        code_section = View{code_ptr_out, code_size_out};
        code_bump_ptr = (uintptr_t)code_ptr_out;

        uint8_t *read_ptr_out = nullptr;
        size_t read_size_out = 0;
        auto read_result = callbacks.alloc_memory(aligner(read_data_size, 4096), PROTECT_READ_WRITE, &read_ptr_out, &read_size_out);
        assert(read_result == RESULT_OK);
        read_section = View{read_ptr_out, read_size_out};
        read_bump_ptr = (uintptr_t)read_ptr_out;

        uint8_t *readwrite_ptr_out = nullptr;
        size_t readwrite_size_out = 0;
        auto readwrite_result = callbacks.alloc_memory(aligner(read_write_data_size, 4096), PROTECT_READ_WRITE, &readwrite_ptr_out, &readwrite_size_out);
        assert(readwrite_result == RESULT_OK);
        readwrite_section = View{readwrite_ptr_out, readwrite_size_out};
        readwrite_bump_ptr = (uintptr_t)readwrite_ptr_out;
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
        auto code_result = callbacks.protect_memory(code_section.base, code_section.size, mem_protect_t::PROTECT_READ_EXECUTE);
        if (code_result != RESULT_OK)
        {
            return false;
        }

        auto read_result = callbacks.protect_memory(read_section.base, read_section.size, mem_protect_t::PROTECT_READ);
        if (read_result != RESULT_OK)
        {
            return false;
        }

        // The readwrite section is already mapped as read-write.

        return false;
    }

    virtual void notifyObjectLoaded(llvm::RuntimeDyld &RTDyld, const llvm::object::ObjectFile &Obj) override {}

    struct View
    {
        uint8_t *base;
        size_t size;
    };

    View eh_frames, stackmap;

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
    uintptr_t code_bump_ptr, read_bump_ptr, readwrite_bump_ptr;
    bool eh_frames_registered = false;

    callbacks_t callbacks;
};

struct WasmModule
{
  public:
    WasmModule(
        const uint8_t *object_start,
        size_t object_size,
        callbacks_t callbacks);

    void *get_func(llvm::StringRef name) const;

    std::unique_ptr<MemoryManager> memory_manager;

  private:
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

    void *get_func_symbol(WasmModule *module, const char *name)
    {
        return module->get_func(llvm::StringRef(name));
    }

    uint8_t *get_stackmap(WasmModule *module, size_t *size_out)
    {
        auto stackmap = module->memory_manager->stackmap;
        *size_out = stackmap.size;
        return stackmap.base;
    }

    void module_delete(WasmModule *module)
    {
        delete module;
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