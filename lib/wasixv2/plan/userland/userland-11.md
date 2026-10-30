# V15 static real programs

Work in progress: no runtime results claimed yet. Runtime base b145215a694,
tests ed854e9, libc2 6bc05b3b and ABI 07cd32e. Source builds and artifacts live
under ~/wasixv2-tools/userland-11; full logs and chronological state are in
~/wasixv2-orch/logs/v15progs. Never commit generated binaries.

Use userland-11.sh one package at a time, with WASIXCC_SYSROOT_PREFIX pointing
to the exact-base libc2 build, BINARYEN set, and WASIXV2_PROBE_CLI pointing to
the source-built release CLI. Run long jobs in systemd user units; use nice19,
CPUs4-7, jobs3, CARGO_INCREMENTAL=0. Recipe order: openssl, curl, ncurses,
ffmpeg, vim, htop, python-native, python, nginx. All target programs are static
and asyncify. python-native is a same-version native build generator only.

The Python static profile uses Modules/Setup.local and the detected upstream
Setup.stdlib. ctypes/FFI and shared-only test extensions are explicitly disabled;
no dynamic loading, callback emulation or reflection changes are included.
Mandatory /dev cross-configuration answers come from a real target probe.
Sources and SHA256 pins are in userland-11.sources. Follow existing userland9/10
recipes for guest shell utilities and panic-rejecting output capture.
