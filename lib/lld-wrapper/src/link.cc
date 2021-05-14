#include <cstdint>
#include <memory>

#include <llvm/ADT/StringRef.h>
#include <llvm/Support/MemoryBuffer.h>
#include <llvm/Support/raw_ostream.h>

#include <lld/Common/Driver.h>
#include <lld/Common/ErrorHandler.h>
#include <lld/Core/PassManager.h>
#include <lld/Core/Resolver.h>
#include <lld/Core/Simple.h>
#include <lld/ReaderWriter/MachOLinkingContext.h>

extern "C" {

void wasmer_lld_wrapper_macho_link(const char **start, uint32_t *length) {
  lld::MachOLinkingContext Ctx;
  Ctx.configure(llvm::MachO::MH_DYLIB, Ctx.Arch::arch_x86_64, Ctx.OS::macOSX, 0,
                true);
  Ctx.setPIE(true);
  Ctx.setUndefinedMode(Ctx.UndefinedMode::dynamicLookup);
  Ctx.setOutputPath("/tmp/wasmer-lld.dyld");

  while (*start) {
    auto Object =
        llvm::MemoryBuffer::getMemBufferCopy(llvm::StringRef(*start, *length));
    auto File = std::make_unique<lld::SimpleFile>(
        "in-memory(wasm function)", lld::File::kindNormalizedObject);
    File->setSharedMemoryBuffer(
        std::shared_ptr<llvm::MemoryBuffer>(std::move(Object)));
    File->parse();
    Ctx.getNodes().emplace_back(
        std::make_unique<lld::FileNode>(std::move(File)));
    ++start, ++length;
  }
  Ctx.finalizeInputFiles();

  lld::Resolver Resolver(Ctx);
  Resolver.resolve();
  auto Merged = Resolver.resultFile();

  lld::PassManager PM;
  Ctx.addPasses(PM);
  // Ignore returned errors.
  (void)PM.runOnFile(*Merged);

  // Ignore returned errors.
  (void)Ctx.writeFile(*Merged);
}

void wasmer_lld_wrapper_link(const char **filenames, uint32_t count) {
  llvm::ArrayRef<const char *> files(filenames, count);

  if (!lld::elf::link(files, false, llvm::outs(), llvm::errs()))
    lld::exitLld(0);
}
}
