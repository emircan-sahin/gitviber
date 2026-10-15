#!/bin/zsh
# Tests of browser-check.sh's own functions, without an app: zsh browser-check-test.zsh
HERE=${0:a:h}
BROWSER_CHECK_LIB=1 source $HERE/browser-check.sh
T=$(mktemp -d "${${TMPDIR:-/tmp}%/}/gitviber-live-check-test.XXXXXX") || exit 1
trap 'rm -rf $T' EXIT
fails=0
expect() { if [[ $2 == "$3" ]]; then print "ok   $1"; else print "FAIL $1\n  got:  $2\n  want: $3"; (( fails++ )); fi }

# A stand-in for the app's binary, at a path with a space, as a dev bundle's is.
mkdir -p "$T/GitViber Dev.app" $T/repo
EXE="$T/GitViber Dev.app/gitviber"
print -r -- '#!/bin/zsh
print -r -- "args: ${(j:|:)@}"; print -r -- "pty: $GITVIBER_PTY"; print -r -- "token: $GITVIBER_BROWSER_TOKEN"
print -r -- "other: ${OTHER:-unset}"; print -r -- "cwd: $PWD"; print -r -- "tab: ${GITVIBER_BROWSER_TAB:-unset}"' > $EXE
chmod +x $EXE
ENV_FILE=$T/env REPO=$T/repo STATE=$T RESULTS=$T/results.tsv
print -r -- "GITVIBER_EXE=$EXE
GITVIBER_PTY=3
GITVIBER_BROWSER_TOKEN=a b  c
OTHER=leaked
not a line" > $ENV_FILE

# As if run from a terminal of the installed app: its values mustn't reach the dev one.
export GITVIBER_BROWSER_TAB=outer GITVIBER_PTY=99
out=$(gb --tab "two words" eval "1 + 1")
expect "gb passes its arguments whole" "${${(f)out}[1]}" "args: --browser|--tab|two words|eval|1 + 1"
expect "gb exports the pane's values, spaces kept" "${${(f)out}[3]}" "token: a b  c"
expect "gb takes GITVIBER_ lines only" "${${(f)out}[4]}" "other: unset"
expect "gb runs in the repo" "${${(f)out}[5]}" "cwd: ${REPO:a}"
expect "gb drops the GITVIBER_ values it was started with" "${${(f)out}[6]} ${${(f)out}[2]}" "tab: unset pty: 3"
expect "gb leaves this shell's environment alone" "$GITVIBER_PTY" "99"

gbx url
expect "gbx keeps the status and output" "$RC ${${(f)OUT}[2]}" "0 pty: 3"

: > $RESULTS
v=visible; record visibility PASS "line one
line	two"
pass_if fps '[[ $v == visible ]]' "60 a second"
pass_if device '(( 1 > 2 ))' "nope"
expect "rows are one line each" "$(<$RESULTS)" "visibility	PASS	line one line two
fps	PASS	60 a second
device	FAIL	nope"
py table $RESULTS >/dev/null
expect "the table fails on a FAIL" "$?" "1"

print -r -- '#!/bin/zsh
exec sleep 5' > $T/hang && chmod +x $T/hang
print -r -- "GITVIBER_EXE=$T/hang" > $ENV_FILE
start=$SECONDS
GB_LIMIT=1 gbx url
expect "a command that hangs is ended" "$RC $(( SECONDS - start < 3 ))" "142 1"

n=0
until_ok 3 '(( ++n >= 2 ))'
expect "until_ok stops once true" "$? $n" "0 2"
start=$SECONDS
until_ok 1 'false'
expect "until_ok gives up false" "$?" "1"

python3 -I $HERE/browser-check-test.py 2>&1 | tail -1 | read py_result
expect "browser-check.py's tests" "$py_result" "OK"

(( fails == 0 )) && print "all passed" || { print "$fails failed"; exit 1 }
