#!/usr/bin/env bash
# Runs inside one v2 machine; writable data stays on the guest tmpfs.
set -euo pipefail
export PATH=/bin:/usr/bin HOME=/root LC_ALL=C LANG=C
workflow=${1:?workflow}
mkdir -p "/tmp/v11-$workflow"
cd "/tmp/v11-$workflow"
case $workflow in
  sqlite)
    test "$(sqlite3 db 'create table t(v); insert into t values(1),(2); select sum(v) from t;')" = 3
    test "$(sqlite3 db 'pragma journal_mode=wal;')" = wal
    # Hold a write transaction open while an independent reader sees its snapshot.
    { printf 'begin immediate; insert into t values(4);\n.shell touch /tmp/v11-sqlite/ready\n';
      while [ ! -e reader-done ]; do sleep .05; done
      printf 'commit;\n'; } | sqlite3 db > writer.log 2>&1 &
    writer=$!
    for ((n=0;n<100;n++)); do [ -e ready ] && break; sleep .05; done
    test -e ready
    test "$(sqlite3 db 'select sum(v) from t;')" = 3
    if sqlite3 db 'insert into t values(99);' > contender.log 2>&1; then
      echo 'second writer incorrectly acquired the WAL write lock' >&2
      exit 1
    fi
    grep -q 'database is locked' contender.log
    touch reader-done
    wait "$writer"
    test "$(sqlite3 db 'select sum(v) from t;')" = 7
    sqlite3 db 'vacuum;'
    sqlite3 db '.backup backup.db'
    test "$(sqlite3 backup.db 'pragma integrity_check; select sum(v) from t;')" = $'ok\n7'
    ;;
  git)
    export GIT_CONFIG_NOSYSTEM=1 GIT_AUTHOR_NAME=V11 GIT_AUTHOR_EMAIL=v11@example.invalid
    export GIT_COMMITTER_NAME=$GIT_AUTHOR_NAME GIT_COMMITTER_EMAIL=$GIT_AUTHOR_EMAIL
    export GIT_EXEC_PATH=/bin
    git init -b main repo
    cd repo
    printf 'base\n' > base
    git add base
    git commit -m base
    git checkout -b topic
    printf 'topic\n' > topic
    git add topic
    git commit -m topic
    git checkout main
    printf '#!/bin/sh\nprintf hook-ok > hook-result\n' > .git/hooks/post-commit
    chmod +x .git/hooks/post-commit
    printf 'main\n' > main
    git add main
    git commit -m main
    test "$(cat hook-result)" = hook-ok
    git merge --no-edit topic
    test "$(git rev-list --count HEAD)" = 4
    git log --oneline --all
    git gc
    git fsck --full
    cd ..
    git clone repo clone
    cmp repo/topic clone/topic
    test "$(git -C clone rev-parse HEAD)" = "$(git -C repo rev-parse HEAD)"
    ;;
  make)
    cat > Makefile <<'MAKE'
.PHONY: all child
all: one two three four child
one two three four:
	@sleep .1; printf '%s\n' '$@' > $@
child:
	+$(MAKE) --no-print-directory -f Child.mk
MAKE
    cat > Child.mk <<'MAKE'
all: joined
joined: five six seven eight
	cat five six seven eight > joined
five six seven eight:
	@sleep .1; printf '%s\n' '$@' > $@
MAKE
    make -j4 --jobserver-style=pipe > make.log 2>&1
    cat make.log
    test "$(cat joined)" = $'five\nsix\nseven\neight'
    test "$(cat one two three four)" = $'one\ntwo\nthree\nfour'
    ! grep -i 'jobserver unavailable' make.log
    ;;
  rsync)
    mkdir -p src/sub dst daemon
    printf 'payload\n' > src/sub/file
    chmod 640 src/sub/file
    touch -t 202401020304.05 src/sub/file
    ln src/sub/file src/hard
    ln -s sub/file src/sym
    rsync -aH src/ dst/
    cmp src/sub/file dst/sub/file
    test "$(stat -c %a dst/sub/file)" = 640
    test "$(stat -c %Y dst/sub/file)" = "$(stat -c %Y src/sub/file)"
    test "$(readlink dst/sym)" = sub/file
    test "$(stat -c %i dst/hard)" = "$(stat -c %i dst/sub/file)"
    touch dst/obsolete
    rsync -aH --delete src/ dst/
    test ! -e dst/obsolete
    cat > rsyncd.conf <<'CONF'
pid file = /tmp/v11-rsync/rsyncd.pid
log file = /tmp/v11-rsync/rsyncd.log
use chroot = no
uid = 0
gid = 0
[data]
path = /tmp/v11-rsync/daemon
read only = no
CONF
    rsync --daemon --no-detach --address=127.0.0.1 --port=18730 --config="$PWD/rsyncd.conf" &
    server=$!
    trap 'kill "$server" 2>/dev/null || true' EXIT
    for ((n=0;n<100;n++)); do rsync rsync://127.0.0.1:18730/ >/dev/null 2>&1 && break; sleep .05; done
    rsync -aH src/ rsync://127.0.0.1:18730/data/
    cmp src/sub/file daemon/sub/file
    test "$(stat -c %i daemon/hard)" = "$(stat -c %i daemon/sub/file)"
    kill "$server"
    wait "$server" || test $? = 143
    trap - EXIT
    ;;
  lua)
    cp -r /lua-tests /tmp/v11-lua/tests
    cd tests
    # Exact upstream files; omit tests specifically exercising dynamic modules.
    for t in strings.lua utf8.lua math.lua bitwise.lua sort.lua constructs.lua closure.lua events.lua; do
      echo "LUA $t"
      lua -e "_soft=true; _port=true; dofile('$t')"
    done
    ;;
  perl)
    cd /perl/t
    /bin/miniperl -I../lib TEST base/cond.t base/if.t base/lex.t base/num.t base/pat.t base/term.t \
      op/arith.t op/concat.t op/fork.t op/pipe.t op/readdir.t op/stat.t
    ;;
  openssh)
    mkdir -p /etc/ssh /root/.ssh /var/empty /usr/libexec
    chmod 700 /root/.ssh
    cp /bin/sshd-session /usr/libexec/sshd-session
    cp /bin/sshd-auth /usr/libexec/sshd-auth
    cp /bin/sftp-server /usr/libexec/sftp-server
    # Privilege-separation account required by upstream sshd, independent of auth.
    printf 'sshd:x:74:74:sshd:/var/empty:/bin/false\n' >> /etc/passwd
    printf 'sshd:x:74:\n' >> /etc/group
    ssh-keygen -q -t ed25519 -N '' -f /etc/ssh/ssh_host_ed25519_key
    ssh-keygen -q -t ed25519 -N '' -f /root/.ssh/id_ed25519
    cp /root/.ssh/id_ed25519.pub /root/.ssh/authorized_keys
    chmod 600 /root/.ssh/authorized_keys
    cat > /etc/ssh/sshd_config <<'CONF'
Port 18222
ListenAddress 127.0.0.1
HostKey /etc/ssh/ssh_host_ed25519_key
PidFile /tmp/v11-openssh/sshd.pid
PermitRootLogin prohibit-password
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
StrictModes yes
AuthorizedKeysFile .ssh/authorized_keys
Subsystem sftp /usr/libexec/sftp-server
CONF
    /bin/sshd -D -e -f /etc/ssh/sshd_config > sshd.log 2>&1 &
    server=$!
    trap 'kill "$server" 2>/dev/null || true; cat sshd.log' EXIT
    opts=(-o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -i /root/.ssh/id_ed25519)
    for ((n=0;n<100;n++)); do grep -q 'Server listening' sshd.log && break; sleep .05; done
    test "$(ssh "${opts[@]}" -p 18222 root@127.0.0.1 'printf remote-ok')" = remote-ok
    printf 'scp-payload\n' > payload
    scp "${opts[@]}" -P 18222 payload root@127.0.0.1:/tmp/v11-openssh/copied
    cmp payload copied
    kill "$server"
    wait "$server" || test $? = 143
    trap - EXIT
    ;;
  *) exit 2;;
esac
echo "$workflow PASS"
