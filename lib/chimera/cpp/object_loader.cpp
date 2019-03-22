#include "object_loader.hh"
#include <iostream>
#include <memory>

struct SymbolLookup : llvm::JITSymbolResolver
{
  public:
    SymbolLookup(callbacks_t callbacks) : callbacks(callbacks) {}

    virtual llvm::Expected<LookupResult> lookup(const LookupSet &symbols) override
    {
        LookupResult result;

        for (auto symbol : symbols)
        {
            result.emplace(symbol, symbol_lookup(symbol));
        }

        return result;
    }

    virtual llvm::Expected<LookupFlagsResult> lookupFlags(const LookupSet &symbols) override
    {
        LookupFlagsResult result;

        for (auto symbol : symbols)
        {
            result.emplace(symbol, symbol_lookup(symbol).getFlags());
        }

        return result;
    }

  private:
    llvm::JITEvaluatedSymbol symbol_lookup(llvm::StringRef name)
    {
        uint64_t addr = callbacks.lookup_vm_symbol(name.data(), name.size());

        return llvm::JITEvaluatedSymbol(addr, llvm::JITSymbolFlags::None);
    }

    callbacks_t callbacks;
};

WasmFunction::WasmFunction(
    const uint8_t *object_start,
    size_t object_size,
    callbacks_t callbacks,
    void *pool,
    uint32_t func_index) : memory_manager(std::unique_ptr<MemoryManager>(new MemoryManager(callbacks, pool, func_index)))
{
    object_file = llvm::cantFail(llvm::object::ObjectFile::createObjectFile(llvm::MemoryBufferRef(
        llvm::StringRef((const char *)object_start, object_size), "object")));

    SymbolLookup symbol_resolver(callbacks);
    runtime_dyld = std::unique_ptr<llvm::RuntimeDyld>(new llvm::RuntimeDyld(*memory_manager, symbol_resolver));

    runtime_dyld->setProcessAllSections(true);

    runtime_dyld->loadObject(*object_file);
    runtime_dyld->finalizeWithMemoryManagerLocking();

    if (runtime_dyld->hasError())
    {
        std::cout << "RuntimeDyld error: " << (std::string)runtime_dyld->getErrorString() << std::endl;
        abort();
    }
}
