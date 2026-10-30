#!/bin/bash
# Unmodified upstream applet filters; scratch lives in the guest's tmpfs.
set -eu
ROOT=${USERLAND10:-$HOME/wasixv2-tools/userland-10}
CLI=${1:?release CLI required}
FILTER=${2:?known or portable}
case $FILTER in
 known) applets='awk cpio date hostname mount sed start-stop-daemon strings unzip';;
 portable) applets='ash bc bunzip2 bzip2 cat chmod chown cmp cp cut dd df dirname du echo expr find grep gzip head ln ls mkdir mkfifo mktemp mv printf pwd readlink rm rmdir seq sleep sort stat tail tar tee test touch tr uniq wc xargs';;
 *) exit 2;;
esac
exec "$CLI" run --compiler-threads 3 --no-tty --user 0 \
 --volume "$ROOT/bin:/bin" --volume "$ROOT/bin:/usr/bin" \
 --volume "$ROOT/busybox-src:/busybox" "$ROOT/bin/bash" -- -c "
 mkdir -p /tmp/busybox
 cp -r /busybox/testsuite /tmp/busybox/
 cp /busybox/.config /busybox/busybox /tmp/busybox/
 cd /tmp/busybox/testsuite
 exec /bin/bash ./runtest -v $applets
 "
