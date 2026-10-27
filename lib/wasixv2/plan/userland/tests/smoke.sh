#!/bin/bash
# Smoke test for the WASIX v2 userland: functions, pipelines, subshells,
# command substitution, background jobs, traps.
set -u
trap 'echo "trap EXIT rc=$?"' EXIT
fail=0
check() { if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got [$2] want [$3]"; fail=1; fi; }

greet() { local who=$1; echo "hello $who"; }
check function "$(greet world)" "hello world"
check pipeline "$(printf 'b\na\nc\n' | sort | head -n 2 | tr '\n' ' ')" "a b "
check subshell "$( (cd /tmp && pwd) ; pwd)" "/tmp
$(pwd)"
( exit 7 ); check subshell-rc "$?" 7
check cmdsubst-nested "$(echo $(echo $(echo deep)))" "deep"
sleep 1 & p1=$!
(echo bg > /tmp/bg.$$) & p2=$!
wait $p1; r1=$?
wait $p2; r2=$?
check bg-wait "$r1$r2$(cat /tmp/bg.$$)" "00bg"
rm -f /tmp/bg.$$
false & wait $!; check bg-rc "$?" 1
check wc "$(seq 1 100 | wc -l)" 100
check awk-sum "$(seq 1 10 | awk '{s+=$1} END {print s}')" 55
check sed-n "$(seq 1 5 | sed -n '2,3p' | paste -sd,)" "2,3"
check grep-E "$(printf 'foo\nbar\nbaz\n' | grep -E '^ba' | wc -l)" 2
check here-doc "$(cat <<X
line
X
)" line
check proc-subst "$(cat <(echo ps))" ps
t=$(mktemp); echo data > "$t"; check mktemp "$(cat "$t")" data; rm -f "$t"
check arith "$((6*7))" 42
check kill-trap "$(bash -c 'trap "echo got USR1" USR1; kill -USR1 $$; echo after')" "got USR1
after"
check timeout "$(timeout 1 sleep 5; echo $?)" 124
exit $fail
