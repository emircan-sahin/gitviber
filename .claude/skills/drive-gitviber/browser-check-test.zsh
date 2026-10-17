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

lacking=()
for c in $CHECKS; do (( ${+functions[check_$c]} )) && [[ -n ${NEEDS[$c]} ]] || lacking+=$c; done
expect "every check has its function and its needs" "${lacking[*]}" ""
help=$T/help.txt
print -r -- "  click <ref|css>   Click an element
  tab release [name]   Give a tab back" > $help
expect "a check's needs missing from help are named" "$(py missing $help "key down <key>" "--hold" "tab release")" "key down <key>, --hold"

# clipboard_safe against a stand-in clipboard: never the user's own here. In a subshell, as it
# stands in for commands; its failures come back as its status.
(
  fails=0
  CLIP=""
  pbpaste() { print -rn -- "$CLIP" }
  pbcopy() { CLIP=$(cat) }
  page() { return 0 }
  close() { : }
  sleep() { : }
  writes=1
  gbx() { (( writes )) && CLIP=gitviber-check-copied; RC=0 }
  : > $RESULTS
  CLIP="the user's own copied text, which is longer than sixty characters in all"
  check_clipboard_safe
  expect "a page that wrote it fails, and the user's text goes back" "${$(<$RESULTS)//$'\t'/|}|$CLIP" \
    "page can't write your clipboard|FAIL|clipboard holds: the page's text; put back what it held before|the user's own copied text, which is longer than sixty characters in all"
  : > $RESULTS; writes=0
  check_clipboard_safe
  expect "a page that didn't passes, naming none of the user's text" "${$(<$RESULTS)//$'\t'/|}" \
    "page can't write your clipboard|PASS|clipboard holds: the user's text, 72 characters"
 exit $fails
)
(( fails += $? ))

# Setup's waits against a stand-in page, in a subshell as it stands in for js and sleep. The page
# keeps its counts in files: setup reads it through $(…), whose own changes would be lost.
(
  fails=0
  sleep() { : }
  say() { : }
  P=$T/page && mkdir -p $P
  count() { print -r -- $(( $(cat $P/$1 2>/dev/null || print 0) + ${2:-0} )) }
  bump() { count $1 1 > $P/$1.new && mv $P/$1.new $P/$1 }
  reset() { rm -f $P/*(N); ENV_FILE=$T/env-setup; rm -f $ENV_FILE }
  # Its terminal button shows once `shows_after` reads have passed, and only after a reload when
  # `needs_reload`; the shell writes its environment on the `env_on`th send (0: never).
  shows_after=0 needs_reload=0 env_on=3 panel=closed
  js() {
    case $1 in
      *'Hide terminal'*)
        bump reads
        if (( $(count reads) > shows_after && (!needs_reload || $(count reloaded)) )); then print $panel; else print none; fi ;;
      *location.reload*) bump reloaded; print true ;;
      *readyState*) print complete ;;
      *press\(*) bump pressed; print true ;;
      *pty_write*)
        bump sends
        (( env_on && $(count sends) >= env_on )) && print GITVIBER_BROWSER_SOCKET=/s > $ENV_FILE
        print 0 ;;
      *'.xterm'*) print 1 ;;
      *document.title*) print GitViber ;;
      *) print true ;;
    esac
  }
  reset; shows_after=5
  open_terminal
  expect "the terminal button waited for, then pressed" "$? $(count reloaded) $(count pressed)" "0 0 1"
  reset; needs_reload=1
  open_terminal
  expect "no button: a reload, then pressed" "$? $(count reloaded) $(count pressed)" "0 1 1"
  reset; needs_reload=0 shows_after=0 panel=open
  open_terminal
  expect "an open terminal isn't pressed again" "$? $(count pressed)" "0 0"
  reset; shows_after=1000
  open_terminal
  expect "never a button: setup stops" "$?" "1"

  reset
  wait_env
  expect "the environment sent again until it comes" "$? $(count sends) $ACCEPTED" "0 3 0"
  reset; env_on=0
  wait_env
  expect "never comes: setup stops after 15 sends" "$? $(count sends)" "1 15"
  expect "and says what the page shows" "$(setup_note "the terminal's environment never came")" \
    "the terminal's environment never came (xterm elements: 1, pty ids that took the write: 0, title: GitViber)"
  exit $fails
)
(( fails += $? ))

# popup and download against stand-in replies, worded as popups.rs, notes.rs and downloads.rs
# word them.
(
  fails=0
  sleep() { : }
  page() { return 0 }
  close() { : }
  mkdir -p $T/dl && print -n "gitviber check download" > $T/dl/gitviber-check.txt
  gbx() {
    RC=0 ERR=
    case "$*" in
      *"click #pop") OUT=$'Clicked button "Open popup" → no DOM change\n→ popup opened as tab t1-popup1 (http://127.0.0.1:5/popup)' ;;
      *"click #closeme") OUT='Clicked button "Close" → no DOM change' ;;
      *" url") OUT=$'http://127.0.0.1:5/act\n→ tab t1-popup1 closed: its page called window.close()' ;;
      *"wait --download") OUT="Downloaded gitviber-check.txt (24 B, text/plain): $T/dl/gitviber-check.txt" ;;
      *" downloads") OUT="gitviber-check.txt  24 B  text/plain  $T/dl/gitviber-check.txt" ;;
      *) OUT= ;;
    esac
  }
  value() { print hi-from-popup }
  state() { [[ $1 == pp ]] && print awake || print missing }
  : > $RESULTS
  check_popup
  check_download
  expect "popup and download pass on the real wording" "$(cut -f1,2 $RESULTS | tr '\t\n' ':,')" \
    "popup opens as a tab:PASS,popup posts to its opener:PASS,window.close() closes its tab:PASS,wait --download:PASS,downloads lists it:PASS,"
  expect "the download is kept for cleanup" "$DOWNLOADS" "$T/dl/gitviber-check.txt"
  exit $fails
)
(( fails += $? ))

# agent_e2e, without a session: its opt-in, the skill files put back as found, what it types.
(
  fails=0
  sleep() { : }
  say() { : }
  home=$T/home
  SKILL_FILES=("$home/.claude/skills/gitviber-browser/SKILL.md" "$home/.agents/skills/gitviber-browser/SKILL.md")
  STATE=$T/e2e && mkdir -p $STATE $home/.claude/skills/gitviber-browser
  : > $RESULTS
  unset AGENT_E2E
  check_agent_e2e
  expect "off unless AGENT_E2E=1" "$(cut -f1,2 $RESULTS)" "agent_e2e	SKIP"

  # The user's own release skill at the first, none at the second.
  release=$'---\nname: gitviber-browser\ndescription: x\n---\n<!-- managed by GitViber 0.1.13; edits are replaced -->\nrelease'
  print -r -- $release > $SKILL_FILES[1]
  save_skills
  dev=${release/release/dev build}
  print -r -- $dev > $SKILL_FILES[1]
  mkdir -p ${SKILL_FILES[2]:h} && print -r -- $dev > $SKILL_FILES[2]
  restore_skills
  expect "the user's skill comes back as it was" "$(<$SKILL_FILES[1])" "$release"
  expect "one the dev build made goes, its folder too" "$([[ -e ${SKILL_FILES[2]:h} ]] && print kept || print gone)" "gone"

  # One of the user's own (no marker) where none was: never removed.
  rm -f $STATE/skill-*
  rm -f $SKILL_FILES[2]
  save_skills
  mkdir -p ${SKILL_FILES[2]:h} && print -r -- "mine" > $SKILL_FILES[2]
  restore_skills
  expect "a skill without the marker stays" "$(<$SKILL_FILES[2])" "mine"

  # A typed line reaches the pane as JSON, ending in a return.
  js() { print -r -- "$1" > $T/typed; print typed }
  type_line 3 $'echo "it\'s" > \'/a b\''
  expect "a typed line" "$(<$T/typed)" \
    "await window.__TAURI_INTERNALS__.invoke('pty_write', { id: 3, data: \"echo \\\"it's\\\" > '/a b'\\r\", binary: false }); return 'typed'"

  # A whole run against a stand-in pane: claude found, its stream and the site's log as they'd be.
  AGENT_E2E=1 pty=0 URL_A=http://127.0.0.1:5
  : > $RESULTS
  js() {
    case $1 in
      *'command -v claude'*) print 0 > $STATE/claude.rc ;;
      *'claude -p'*)
        local run=${${1#*form\?run=}%%,*}
        print -r -- '{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"gitviber browser open x"}}]}}' > $STATE/agent.json
        print -r -- '{"type":"result","is_error":false,"result":"200"}' >> $STATE/agent.json
        print -r -- "form-submit run=$run name=Ada color=green ua=Mozilla/5.0 AppleWebKit/605.1.15 (KHTML, like Gecko)" > $STATE/site-a.log
        print 0 > $STATE/agent.done ;;
    esac
    print typed
  }
  check_agent_e2e
  expect "a run that did it all passes" "$(cut -f1,2 $RESULTS | tr '\t\n' ':,')" \
    "agent: answer and browser:PASS,agent: the submit came from its tab:PASS,"
  expect "and leaves no transcript" "$(ls $STATE | command grep -c agent)" "0"
  exit $fails
)
(( fails += $? ))

# The dev log as tauri writes it: cargo's lines in color.
print -r -- $'     Running DevCommand (`cargo  run --no-default-features --color always --`)\n   Compiling gitviber v0.1.13' > $T/dev.log
built $T/dev.log
expect "not built while cargo compiles" "$?" "1"
print -r -- $'\e[1m\e[32m    Finished\e[0m `dev` profile\n\e[1m\e[32m     Running\e[0m `/x/target/debug/gitviber`' >> $T/dev.log
built $T/dev.log
expect "built once the colored Running line comes" "$?" "0"
grep -q "Running \`" $T/dev.log
expect "which a plain grep misses" "$?" "1"

expect "progress goes to stdout" "$(say step 2>/dev/null)" "· step"
expect "and to stderr" "$(say step 2>&1 >/dev/null)" "· step"

# A run goes on in a copy of its own: an edit to the script mid-run can't move it.
mkdir -p $T/skill $T/tmp $T/wt && cp $HERE/$^COPIED $T/skill/
out=$(TMPDIR=$T/tmp BROWSER_CHECK_SELF_TEST=1 zsh $T/skill/browser-check.sh $T/wt 2>&1)
expect "the run is a copy in its temp folder" "${out//$T\/tmp\/gitviber-live-check.[A-Za-z0-9]##\/bin/<state>/bin}" \
  "running from <state>/bin: browser-check-site.py browser-check.py browser-check.sh prelude.js"
expect "it copied once and cleaned up" "$(ls $T/tmp)" ""
TMPDIR=$T/tmp zsh $T/skill/browser-check.sh >/dev/null 2>&1
expect "a usage error leaves no temp folder" "$? $(ls $T/tmp)" "2 "

python3 -I $HERE/browser-check-test.py 2>&1 | tail -1 | read py_result
expect "browser-check.py's tests" "$py_result" "OK"

(( fails == 0 )) && print "all passed" || { print "$fails failed"; exit 1 }
