#!/usr/bin/env bash
# Info-ZIP UnZip 6.0 for WASIX v2 (asyncify), with Debian's patches (6.0-28:
# the CVE fixes and timestamp/uid fixes upstream never released). LTP's
# unzip01.sh compares Info-ZIP's output format.
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/unzip
TARBALL=$(fetch https://downloads.sourceforge.net/infozip/unzip60.tar.gz \
  036d96991646d0449ed0aa952e4fbe21b476ce994abc276e49d30e686708bd37)
DEBIAN=$(fetch https://deb.debian.org/debian/pool/main/u/unzip/unzip_6.0-28.debian.tar.xz \
  e51364116c84739c591728ecc841113a914fa11358fd10ff0d6813524d811bb9)

# unix/Makefile builds in the source tree; build in a fresh extraction.
SRC=$PKG/build
unpack "$TARBALL" "$SRC"
tar -xJf "$DEBIAN" -C "$SRC"
# Debian's series minus its manpage section and "this is Debian" banner.
while read -r p; do
  case $p in ''|'#'*|01-*|02-*) continue ;; esac
  patch -d "$SRC" -p1 --batch --quiet < "$SRC/debian/patches/$p"
done < "$SRC/debian/patches/series"
apply_patches "$SRC" "$PKG/patches"
cd "$SRC"
# unix/configure runs its test programs, which a cross build cannot: the
# defines are Debian's (minus bzip2, which is not in the sysroot). gnu89: the
# sources use K&R definitions, which C23 does not have.
DEFINES="-DUNIX -DACORN_FTYPE_NFS -DWILD_STOP_AT_DIR -DLARGE_FILE_SUPPORT \
 -DUNICODE_SUPPORT -DUNICODE_WCHAR -DUTF8_MAYBE_NATIVE -DNO_LCHMOD \
 -DDATE_FORMAT=DF_YMD -DIZ_HAVE_UXUIDGID -DNOMEMCPY -DNO_WORKING_ISPRINT \
 -DHAVE_DIRENT_H -DHAVE_TERMIOS_H"
make -f unix/Makefile -j"$JOBS" unzip funzip CC="$CC" LD="$CC" \
  CF="-std=gnu89 -g -O2 -I. $DEFINES" LF2=""
install_wasm unzip unzip
install_wasm funzip funzip
