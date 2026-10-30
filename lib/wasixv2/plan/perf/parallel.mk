# Real zlib C compilation and archive build, on a fresh tmpfs directory per run.
# clang runs as a guest program on WASIX; native clang emits the same wasm32 ABI.
SOURCES = adler32 compress crc32 deflate gzclose gzlib gzread gzwrite infback inffast inflate inftrees trees uncompr zutil
OBJECTS = $(SOURCES:=.o)
all: libz.a
libz.a: $(OBJECTS)
	$(AR) rcs $@ $^
%.o: $(SRC)/%.c
	$(CC) -O2 -I$(SRC) -c $< -o $@
