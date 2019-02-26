#include "object_loader.hh"
#include <llvm/ExecutionEngine/RuntimeDyld.h>
#include <iostream>

class MemoryManager : llvm::RuntimeDyld::MemoryManager {
public:
    MemoryManager() {}

    virtual ~MemoryManager() {}

    virtual uint8_t* allocateCodeSection(uintptr_t Size, unsigned Alignment, unsigned SectionID, llvm::StringRef SectionName) override {
        return nullptr;
    }

    virtual uint8_t* allocateDataSection(uintptr_t Size, unsigned Alignment, unsigned SectionID, llvm::StringRef SectionName, bool isReadOnly) override {
        return nullptr;
    }

    virtual void reserveAllocationSpace(
        uintptr_t code_size,
        uint32_t code_align,
        uintptr_t read_data_size,
        uint32_t read_data_align,
        uintptr_t read_write_data_size,
        uint32_t read_write_data_align
    ) override {
        
    }

    /* Turn on the `reserveAllocationSpace` callback. */
    virtual bool needsToReserveAllocationSpace() override {
        return true;
    }

    virtual void registerEHFrames(uint8_t* Addr, uint64_t LoadAddr, size_t Size) override {

    }

    virtual void deregisterEHFrames() override {

    }

    virtual bool finalizeMemory(std::string *ErrMsg = nullptr) override {
        

        return false;
    }

    virtual void notifyObjectLoaded(llvm::RuntimeDyld &RTDyld, const llvm::object::ObjectFile &Obj) override {
        
    }
private:
    struct Section {
        uint8_t* base;
        size_t num_pages;
        size_t num_commited_bytes;
    };

    uint8_t *image_base;
    size_t num_allocated_pages;

    Section code_section, read_section, readwrite_section;


};

class SymbolLookup : llvm::JITSymbolResolver {
public:
    virtual llvm::Expected<LookupResult> lookup(const LookupSet& symbols) override {
        LookupResult result;

        for (auto symbol : symbols) {
            result.emplace(symbol, symbol_lookup(symbol));
        }

        return result;
    }

    virtual llvm::Expected<LookupFlagsResult> lookupFlags(const LookupSet& symbols) override {
        LookupFlagsResult result;

        for (auto symbol : symbols) {
            result.emplace(symbol, symbol_lookup(symbol).getFlags());
        }

        return result;
    }

private:
    llvm::JITEvaluatedSymbol symbol_lookup(llvm::StringRef name) {
        std::cout << "symbol name: " << (std::string)name << std::endl;
        uint64_t addr = 0;

        return llvm::JITEvaluatedSymbol(addr, llvm::JITSymbolFlags::None);
    }
};