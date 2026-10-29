#!/usr/bin/env bash
# iproute2 7.3.0 for WASIX v2 (asyncify): ip (link, addr, route, netns, ...) and ss, which
# LTP's network shell tests and `ip netns` setups call. Built with SHARED_LIBS=n: ip looks up
# the link types it knows (veth, vlan, ...) in its own static symbol table instead of with
# dlopen/dlsym. tc is not built: the kernel has no queueing disciplines to configure.
# Needs bison on the build host (ss's filter parser).
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/iproute2
VER=7.3.0
TARBALL=$(fetch https://www.kernel.org/pub/linux/utils/net/iproute2/iproute2-$VER.tar.xz \
  5c1789d290bba048a6135ed1ab81b0402dd0fb528d1f1421765c42b1fc3d6289)

SRC=$PKG/src
unpack "$TARBALL" "$SRC"
apply_patches "$SRC" "$PKG/patches"
cd "$SRC"
# iproute2 builds in its tree. Its configure probes the compiler and pkg-config for the
# optional libraries (libmnl, libelf, libbpf, libcap, selinux), none of which the sysroot has.
PKG_CONFIG=false ./configure --prefix=/usr
make -j"$JOBS" SHARED_LIBS=n CC="$CC" HOSTCC="$CC_FOR_BUILD" AR="$AR" \
  CCOPTS="-g -O2" SUBDIRS="lib ip misc" V=1
install_wasm ip/ip ip
install_wasm misc/ss ss
