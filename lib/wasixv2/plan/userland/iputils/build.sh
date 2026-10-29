#!/usr/bin/env bash
# iputils 20240905 for WASIX v2 (asyncify): ping, and ping6, the link through which ping
# defaults to IPv6 (it picks the family by the last letter of its name). LTP's network tests
# (netns_*, net.ipv6, net.multicast) ping across namespaces. ping opens an ICMP datagram
# socket where ping_group_range allows it and a raw one with CAP_NET_RAW otherwise. Without
# libcap (ping then drops nothing it was not given), libidn2 or gettext. arping, clockdiff and
# tracepath are not built: they need packet-level access the kernel does not have.
# Needs meson on the build host: MESON names it (default: meson on PATH).
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/iputils
VER=20240905
TARBALL=$(fetch https://github.com/iputils/iputils/releases/download/$VER/iputils-$VER.tar.xz \
  1ff0c762578d5b0c1c5fbbd081f80f3137ba4f115e5854a4439e36449343bfdb)

SRC=$PKG/src
BUILD_DIR=$PKG/build
unpack "$TARBALL" "$SRC"
apply_patches "$SRC" "$PKG/patches"
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"
# meson's view of the target: Linux on wasm32, through wasixcc. Its compile checks
# (getrandom, __fpending, error.h, struct icmp6_nodeinfo) run against the sysroot.
cat > "$BUILD_DIR/cross.ini" <<CROSS
[binaries]
c = '$CC'
ar = '$AR'
strip = 'true'

[host_machine]
system = 'linux'
cpu_family = 'wasm32'
cpu = 'wasm32'
endian = 'little'
CROSS
"${MESON:-meson}" setup "$BUILD_DIR/out" "$SRC" --cross-file "$BUILD_DIR/cross.ini" \
  --buildtype=plain -Dc_args="-g -O2" \
  -DBUILD_PING=true -DBUILD_ARPING=false -DBUILD_CLOCKDIFF=false -DBUILD_TRACEPATH=false \
  -DBUILD_MANS=false -DBUILD_HTML_MANS=false -DSKIP_TESTS=true -DNO_SETCAP_OR_SUID=true \
  -DUSE_CAP=false -DUSE_IDN=false -DUSE_GETTEXT=false
# meson links with --start-group, which wasm-ld does not take: ninja builds the objects and
# libcommon, and the link is wasixcc's.
cd "$BUILD_DIR/out"
OBJS=($(ninja -t targets all | sed -n 's|^\(ping/ping\.p/[^:]*\.o\):.*|\1|p'))
ninja -j"$JOBS" libcommon.a "${OBJS[@]}"
"$CC" -o ping/ping "${OBJS[@]}" libcommon.a -lm -lresolv
install_wasm ping/ping ping
ln -sf ping "$OUT/bin/ping6"
