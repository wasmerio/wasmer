#!/bin/bash
# Run inside a WASIX machine with userland-4 plus userland-5 binaries in /bin.
# Each workflow is deliberately independent; failures retain their output/status.
set -uo pipefail
case $1 in
  network)
    set -ex
    ss -lntup
    ss-listener | tee /tmp/v3-ss.out
    grep -q ':31337' /tmp/v3-ss.out
    grep -q 'users:.*ss' /tmp/v3-ss.out
    ip netns add v3-red
    ip -n v3-red link set lo up
    ip -n v3-red address add 127.0.0.2/8 dev lo
    ip netns exec v3-red ip -brief address show lo
    ip netns exec v3-red ss -lntup
    ip netns delete v3-red
    ;;
  namespaces)
    set -ex
    mkdir -p /mnt
    unshare --user --map-root-user /bin/id
    unshare --mount /bin/bash -ec 'mount -t tmpfs none /mnt; touch /mnt/private; test -f /mnt/private; umount /mnt'
    unshare --pid --fork --mount-proc /bin/bash -ec 'test "$$" = 1; nsenter --target 1 --pid /bin/true'
    unshare --net /bin/bash -ec 'ip link set lo up; nsenter --target $$ --net /bin/ip link show lo'
    unshare --time --monotonic 7 /bin/bash -ec 'cat /proc/self/timens_offsets; nsenter --target $$ --time /bin/true'
    nsenter --target $$ --mount --net --time /bin/true
    unshare --user --map-root-user --mount --net --time /bin/bash -ec \
      'echo ready > /tmp/v3-ns-ready; exec /bin/sleep 30' &
    child=$!
    trap 'kill "$child" 2>/dev/null || :; wait "$child" 2>/dev/null || :' EXIT
    for ((i=0;i<100;i++)); do
      test -f /tmp/v3-ns-ready && break
      sleep 0.05
    done
    test -f /tmp/v3-ns-ready
    nsenter --target "$child" --user --mount --net --time /bin/id
    kill "$child"
    wait "$child" || :
    trap - EXIT
    ;;
  privileges)
    set -ex
    setpriv --dump
    setpriv --no-new-privs /bin/bash -ec 'grep -q "NoNewPrivs:.*1" /proc/self/status'
    setpriv --reuid=65534 --regid=65534 --clear-groups /bin/bash -ec 'test "$(id -u)" = 65534; test "$(id -g)" = 65534'
    ;;
  keys)
    exec keyctl session v3-programs /bin/bash -exc '
      k=$(keyctl add user v3 payload @s)
      test "$(keyctl pipe "$k")" = payload
      keyctl describe "$k"
      keyctl update "$k" changed
      test "$(keyctl pipe "$k")" = changed
      keyctl show @s
      keyctl unlink "$k" @s
      keyctl clear @s
    '
    ;;
  seccomp)
    set -ex
    # WASIX uses the riscv32 audit architecture, with its own syscall numbers.
    # The host runner reads the number from libc2's generated bits/syscall.h.
    ls / > /dev/null
    r=0
    seccomp-dropper 0x400000f3 "$2" 13 /bin/ls / > /tmp/seccomp-ls.out 2>&1 || r=$?
    cat /tmp/seccomp-ls.out
    test "$r" -ne 0
    grep -q 'Permission denied' /tmp/seccomp-ls.out
    ;;
  fuse)
    set -ex
    mkdir -p /mnt
    squashfuse_ll -f /work/test.sqsh /mnt &
    daemon=$!
    trap 'kill "$daemon" 2>/dev/null || :; wait "$daemon" 2>/dev/null || :' EXIT
    ready=0
    for ((i=0;i<100;i++)); do
      if test -f /mnt/hello; then ready=1; break; fi
      sleep 0.05
    done
    test "$ready" = 1
    test "$(cat /mnt/hello)" = squashfuse-content
    test "$(readlink /mnt/hello-link)" = hello
    test "$(cat /mnt/hello-link)" = squashfuse-content
    test "$(wc -c < /mnt/nested/large)" = 131072
    ls -la /mnt
    umount /mnt
    wait "$daemon"
    trap - EXIT
    ;;
  *) exit 2 ;;
esac
