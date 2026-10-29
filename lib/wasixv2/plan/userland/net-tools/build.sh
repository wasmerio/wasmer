#!/usr/bin/env bash
# net-tools 2.10 for WASIX v2 (asyncify): ifconfig, route, netstat, arp and hostname, which
# LTP's ioctl variants of the network tests (netns_*.sh -I) and older scripts call. Only the
# UNIX, INET and INET6 families and Ethernet-like hardware are configured in.
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/net-tools
VER=2.10
TARBALL=$(fetch https://downloads.sourceforge.net/project/net-tools/net-tools-$VER.tar.xz \
  b262435a5241e89bfa51c3cabd5133753952f7a7b7b93f32e08cb9d96f580d69)

SRC=$PKG/src
unpack "$TARBALL" "$SRC"
apply_patches "$SRC" "$PKG/patches"
cd "$SRC"
# configure.sh asks about every option of config.in in turn: yes to these, no to the rest.
YES=" HAVE_AFUNIX HAVE_AFINET HAVE_AFINET6 HAVE_HWETHER HAVE_HWTUNNEL HAVE_HWSIT HAVE_HWEUI64
  HAVE_ARP_TOOLS HAVE_HOSTNAME_TOOLS "
grep -E "^bool " config.in | awk '{print $(NF-1)}' | while read -r opt; do
  case "$YES" in *" $opt "*) echo y ;; *) echo n ;; esac
done | ./configure.sh config.in > /dev/null
PROGS=(ifconfig route netstat arp hostname)
# CFLAGS through the environment: the Makefile adds its include paths to it.
export CFLAGS="-g -O2"
MAKEARGS=(CC="$CC" AR="$AR")
# The generated header and the support libraries first, then only the programs wanted.
make "${MAKEARGS[@]}" version.h
make -j"$JOBS" "${MAKEARGS[@]}" subdirs
make -j"$JOBS" "${MAKEARGS[@]}" "${PROGS[@]}"
for p in "${PROGS[@]}"; do install_wasm "$p" "$p"; done
