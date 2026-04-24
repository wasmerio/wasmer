# Current state of the RISCV support

Only Cranelift and LLVM compiler are supported.
Singlepass can be done, but no ressources are allocated on this task for now.

Both LLVM and Cranelift support are quite new, and so it is expected to have a few things not working well.

LLVM code needs a hack to force the ABI to "lp64d", and some tested with funciton & float/double values are still not working correctly and have be disable for now.

On Cranelift, SIMD is not supported as the CPU doesn't have official SIMD/Vector extension for now, and no Workaround is in place.

Test have be conducted on actual hardware, with a Vision Fixe 2 board running Debian. Some previous tests have also be done on a Vison Five 1 running Fedora (with LLVM only).