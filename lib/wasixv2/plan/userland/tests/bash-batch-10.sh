#!/bin/bash
# Invoked by upstream make tests via its TESTSCRIPT override.
set +e
: "${BASH_SUITE_LOG:?}" "${BASH_TEST_FILTER:?}"
export TMPDIR=/tmp
mkdir -p "$BASH_SUITE_LOG"
result=0
for test in $BASH_TEST_FILTER; do
 export BASH_TSTOUT="/tmp/v12-$test-$$"
 timeout -k 5 90 /bin/sh "./$test" > "$BASH_SUITE_LOG/$test.stdout" 2> "$BASH_SUITE_LOG/$test.stderr"
 status=$?
 # Scripts use diff for expected output; several run more than one diff and
 # return only the final command's status. Preserve earlier differences too.
 if [ "$status" -eq 0 ] && [ -s "$BASH_SUITE_LOG/$test.stdout" ]; then status=1; fi
 echo "$test $status" >> "$BASH_SUITE_LOG/results.txt"
 echo "BASH_RESULT $test $status"
 test "$status" -eq 0 || result=1
 rm -f "$BASH_TSTOUT"
done
exit "$result"
