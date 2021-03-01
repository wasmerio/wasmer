all: build-probes

build-probes: src/probes.h

src/probes.h: src/probes.d
	dtrace -o src/probes.h -h -s src/probes.d
