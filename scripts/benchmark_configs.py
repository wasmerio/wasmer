# Shared Wasmer configurations used by benchmark scripts.
WASMER_CONFIGS = (
    ("Wasmer 7 LLVM (w/ pass-params)", "wasmer-7", "-l --enable-pass-params-opt"),
    ("Wasmer LLVM", "wasmer-next", "-l"),
    # ("Wasmer LLVM (w/ non-volatile)", "wasmer-next", "-l"),
    # ("Wasmer LLVM (w/ non-volatile + -O2)", "wasmer-next-O2", "-l"),
    # (
    #     "Wasmer LLVM (w/ non-volatile + embedded Globals and Tables)",
    #     "wasmer-next-embed",
    #     "-l",
    # ),
    # (
    #     "Wasmer LLVM (w/ non-volatile + embedded Globals and Tables + O2)",
    #     "wasmer-next-embed-O2",
    #     "-l",
    # ),
)
