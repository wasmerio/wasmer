#include <cstdint>

#include <llvm/Support/raw_ostream.h>

#include <lld/Common/Driver.h>
#include <lld/Common/ErrorHandler.h>

extern "C" {

void wasmer_lld_wrapper_link(const char **filenames, uint32_t count) {
  llvm::ArrayRef<const char *> files(filenames, count);

  if (!lld::elf::link(files, false, llvm::outs(), llvm::errs()))
    lld::exitLld(0);
}

}
