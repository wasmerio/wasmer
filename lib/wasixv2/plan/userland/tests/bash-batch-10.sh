#!/bin/bash
# Invoked by upstream make tests via its TESTSCRIPT override.
set +e
: "${BASH_SUITE_LOG:?}" "${BASH_TEST_FILTER:?}"
export TMPDIR=/tmp
mkdir -p "$BASH_SUITE_LOG"
result=0
for test in $BASH_TEST_FILTER; do
 export BASH_TSTOUT="/tmp/v12-$test-$$"
 # The expected help table starts in a UTF-8 locale; C selects its narrow formatter.
 if [ "$test" = run-builtins ]; then export LC_ALL=C.UTF-8 LANG=C.UTF-8; else export LC_ALL=C LANG=C; fi
 duration=90
 case $test in run-jobs) duration=300;; esac
 timeout --foreground -k 5 "$duration" /bin/dash "./$test" > "$BASH_SUITE_LOG/$test.stdout" 2> "$BASH_SUITE_LOG/$test.stderr"
 status=$?
 # Scripts use diff for expected output; several run more than one diff and
 # return only the final command's status. Preserve earlier differences too.
 if [ "$status" -eq 0 ] && [ -s "$BASH_SUITE_LOG/$test.stdout" ]; then status=1; fi
 echo "$test $status" >> "$BASH_SUITE_LOG/results.txt"
 echo "BASH_RESULT $test $status"
 test "$status" -eq 0 || result=1
 if [ "$status" -ne 0 ] && [ -f "$BASH_TSTOUT" ]; then
  cp "$BASH_TSTOUT" "$BASH_SUITE_LOG/$test.actual"
 fi
 rm -f "$BASH_TSTOUT"
done
exit "$result"
