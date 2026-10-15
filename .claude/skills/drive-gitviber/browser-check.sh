#!/bin/zsh
# Live check of agents' browser tabs in a dev build, through the dev bridge and the CLI only: no
# mouse, no keys to the screen, no window brought forward. Prints a PASS/FAIL table.
#   browser-check.sh <worktree> [port] [check...]
# Starts `pnpm tauri dev --port <port>` (1437 unless given; never 1420, whose storage is the
# user's) in <worktree> with its own target dir, turns agent control on, opens a scratch repo and a
# terminal, and runs `gitviber browser` as that terminal's agent would. Names after the port run
# only those checks (see CHECKS). KEEP=1 keeps the logs when all pass. Everything it made is undone
# on exit, failed or not.
setopt extended_glob
HERE=${0:a:h}

py() { python3 -I $HERE/browser-check.py "$@" }
say() { print -r -- "· $*" }

record() { local n=${3:-}; n=${n//$'\n'/ }; print -r -- "$1"$'\t'"$2"$'\t'"${n//$'\t'/ }" >> $RESULTS }
# A row: PASS when the condition (a command line, evaluated) holds. Take $? into a variable
# before calling: the note's own $(…) would change it.
pass_if() { if eval "$2"; then record "$1" PASS "$3"; else record "$1" FAIL "$3"; fi }

# The main webview, through dev_bridge.rs on the dev port + 100, with prelude.js's helpers.
js() { { cat $HERE/prelude.js; print -r -- "$1" } | py bridge $(( PORT + 100 )) }
invoke() { js "return await window.__TAURI_INTERNALS__.invoke(\"$1\", $2)" }

# `gitviber browser` as the terminal's agent runs it: its pane's environment alone (not the one
# this script may have from a terminal of the installed app), in the repo.
gb() {
  (
    unset -m 'GITVIBER_*'
    for line in "${(@f)$(<$ENV_FILE)}"; do
      [[ $line == GITVIBER_[A-Z0-9_]##=* ]] && export "$line"
    done
    # macOS has no timeout(1): an alarm ends a command that hangs. `wait` waits 120 s at most.
    cd $REPO && perl -e 'alarm shift; exec @ARGV' ${GB_LIMIT:-150} "$GITVIBER_EXE" --browser "$@"
  )
}
# gb with its output in OUT and ERR, its status in RC.
gbx() { OUT=$(gb "$@" 2>$STATE/err); RC=$?; ERR=$(<$STATE/err) }
value() { gbx --tab $1 eval "$2"; py eval-value <<<$OUT }
state() { gb --json tab list | py tab-state $1 }
# Polls `cond` (a command line) every second for up to `secs`; its status at the end.
until_ok() { local secs=$1 i; shift; for (( i = 0; i < secs; i++ )); do eval "$*" && return 0; sleep 1; done; eval "$*" }
page() { gbx tab new $1 $2 ${@:3} && gbx --tab $1 wait --load }
close() { local t; for t; do gb tab close $t >/dev/null 2>&1; done }

kill_tree() { local p; for p in $(pgrep -P $1); do kill_tree $p; done; kill $1 2>/dev/null }

finish() {
  trap - EXIT INT TERM
  say "cleaning up"
  if [[ -s $ENV_FILE ]]; then
    for s in $SESSIONS; do gb session delete $s >/dev/null 2>&1; done
  fi
  [[ -n $APP_PID ]] && js 'localStorage.clear(); return true' >/dev/null 2>&1
  [[ -n $APP_PID ]] && kill $APP_PID 2>/dev/null
  [[ -n $DEV_PID ]] && kill_tree $DEV_PID
  for p in $SERVERS; do kill $p 2>/dev/null; done
  for f in $PICS; do [[ -f $f && $f == *browser-picks/* ]] && rm -f $f; done
  rm -f $ENV_FILE
  print
  py table $RESULTS; local rc=$?
  if (( rc == 0 )) && [[ -z ${KEEP:-} ]]; then rm -rf $STATE; else print "Logs: $STATE"; fi
  exit $rc
}

# ---- the checks: each a function check_<name>, recording one row or more. NEEDS names what of
# `gitviber browser help` it uses; a check whose needs aren't there is skipped, so a branch can
# add its own here before the commands land.

CHECKS=(visibility fps isolation sessions unknown_tab device screenshot network sse idle_park tab_close form memory)
typeset -A NEEDS=(
  visibility "eval" fps "eval" isolation "eval" sessions "session save|session load|--session"
  unknown_tab "--tab" device "device <name|WxH|off>" screenshot "screenshot" network "network <id>"
  sse "network" idle_park "--idle" tab_close "tab close" form "fill|select|click|wait"
  memory "tab list"
)

check_visibility() {
  page vis $URL_A/ || { record visibility FAIL "tab new: $ERR"; return }
  local v=$(value vis document.visibilityState)
  pass_if visibility '[[ $v == visible ]]' "off-screen tab's visibilityState: $v"
  close vis
}

check_fps() {
  page fps $URL_A/ || { record fps FAIL "tab new: $ERR"; return }
  local a=$(value fps '[window.frames_, performance.now()]')
  sleep 20
  local b=$(value fps '[window.frames_, performance.now()]')
  local f=$(py fps "$a" "$b" 2>/dev/null)
  pass_if "fps (20 s, no commands)" '(( ${f:-0} >= 50 ))' "${f:-?} rAF a second"
  close fps
}

check_isolation() {
  page iso1 $URL_A/ && page iso2 $URL_A/ || { record isolation FAIL "tab new: $ERR"; return }
  value iso1 "localStorage.setItem('token', 'iso-1'), 'set'" >/dev/null
  local other=$(value iso2 "localStorage.getItem('token')") own=$(value iso1 "localStorage.getItem('token')")
  pass_if "private tabs' localStorage" '[[ $own == iso-1 && $other == null ]]' "same origin: one has $own, the other $other"
  close iso1 iso2
}

check_sessions() {
  page s1 $URL_A/ || { record sessions FAIL "tab new: $ERR"; return }
  SESSIONS+=(lc-alice)
  gb session delete lc-alice >/dev/null 2>&1
  value s1 "localStorage.setItem('token', 'tok-alice'), 'set'" >/dev/null
  gbx --tab s1 session save lc-alice
  pass_if "session save" '(( RC == 0 ))' "$OUT$ERR"
  gbx --tab s1 session save lc-alice
  pass_if "session save over one" '(( RC != 0 )) && [[ $ERR == *--force* ]]' "$ERR"
  gbx --tab s1 session save lc-alice --force
  pass_if "session save --force" '(( RC == 0 ))' "$OUT$ERR"
  gbx tab new s2 --session lc-alice && gbx --tab s2 wait --load
  local token=$(value s2 "localStorage.getItem('token')")
  pass_if "tab new --session" '[[ $token == tok-alice ]]' "token in the new tab: $token"
  page s3 $URL_B/
  gbx --tab s3 session load lc-alice
  pass_if "session load on another origin" '(( RC != 0 ))' "refused: $ERR"
  close s1 s2 s3
}

check_unknown_tab() {
  gbx --tab lc-no-such-tab url
  pass_if "unknown --tab" '(( RC != 0 )) && [[ -n $ERR ]]' "exit $RC: $ERR"
}

check_device() {
  page d1 $URL_A/ && page d2 $URL_A/ || { record device FAIL "tab new: $ERR"; return }
  gbx --tab d1 device 400x700
  local one=$(value d1 innerWidth) two=$(value d2 innerWidth)
  pass_if "device per tab" '[[ $one == 400 && $two != 400 ]]' "400x700 tab: $one px wide, the other: $two"
  gbx --tab d1 device off
  close d1 d2
}

check_screenshot() {
  page shot $URL_A/ || { record screenshot FAIL "tab new: $ERR"; return }
  local css=$(value shot '`${innerWidth}x${innerHeight} @${devicePixelRatio}`')
  gbx --tab shot screenshot
  local pic=$(py pic-path <<<$OUT)
  [[ -n $pic ]] && PICS+=($pic)
  local size=$([[ -f $pic ]] && py png $pic)
  pass_if screenshot '[[ -n $size ]]' "css $css, png ${${size/ /x}:-none: $OUT$ERR}"
  close shot
}

check_network() {
  page net $URL_A/ || { record network FAIL "tab new: $ERR"; return }
  sleep 1
  local list=$(gb --json --tab net network)
  local hello=$(py net-id fetch /api/hello 200 <<<$list) missing=$(py net-id xhr /api/missing 404 <<<$list)
  pass_if "network lists fetch, XHR, 404" '[[ -n $hello && -n $missing ]]' "fetch #${hello:-none}, xhr 404 #${missing:-none}"
  gbx --tab net network ${hello:-0}
  pass_if "network <id>" '[[ $OUT == *world* ]]' "${${OUT:-$ERR}[1,120]}"
  close net
}

check_sse() {
  page sse $URL_A/ || { record sse FAIL "tab new: $ERR"; return }
  local gone=$(grep -c 'went away' $STATE/site-a.log)
  value sse "startSse(), 'started'" >/dev/null
  sleep 2
  local events=$(value sse window.events_)
  value sse "stopSse(), 'stopped'" >/dev/null
  until_ok 10 '(( $(grep -c "went away" $STATE/site-a.log) > gone ))'
  local ok=$?
  pass_if "SSE close reaches the server" '(( ok == 0 ))' "$events events before close"
  close sse
}

check_idle_park() {
  page idl1 $URL_A/ --idle 1 && page idl2 $URL_A/ --idle 1 || { record idle_park FAIL "tab new: $ERR"; return }
  local t0=$SECONDS
  until_ok 100 '[[ $(state idl1) == sleeping && $(state idl2) == sleeping ]]'
  local ok=$?
  pass_if "--idle 1 sleeps" '(( ok == 0 ))' "after $(( SECONDS - t0 )) s: $(state idl1), $(state idl2)"
  value idl1 1+1 >/dev/null
  local s=$(state idl1)
  pass_if "a sleeping tab wakes" '[[ $s == awake ]]' "after a command: $s"
  # What the app has now, to put back: the frontend only sends it again when a setting changes.
  local policy='JSON.parse(localStorage.getItem("gitviber.settings.v2") || "{}")'
  invoke browser_configure '{ liveHidden: 0, parkAfterMin: 10, console: true, agentControl: true }' >/dev/null
  until_ok 15 '[[ $(state idl2) == parked ]]'
  ok=$?
  pass_if "park (liveHidden 0)" '(( ok == 0 ))' "sleeping tab: $(state idl2)"
  js "const s = $policy; return await window.__TAURI_INTERNALS__.invoke('browser_configure', { liveHidden: s.browserLiveHidden ?? 2,
    parkAfterMin: s.browserParkAfterMin ?? 10, console: s.browserConsole ?? true, agentControl: true })" >/dev/null
  gbx --tab idl2 wait --load
  sleep 1
  local hello=$(gb --json --tab idl2 network | py net-id fetch /api/hello 200)
  pass_if "network after restore" '[[ -n $hello && $(state idl2) == awake ]]' "$(state idl2), fetch #${hello:-none}"
  close idl1 idl2
}

agent_tabs() { js 'return $$("[role=tab] [aria-label=\"Opened by an agent\"]").length' }

check_tab_close() {
  page cl1 $URL_A/ || { record tab_close FAIL "tab new: $ERR"; return }
  local before=$(agent_tabs)
  gbx tab close cl1
  until_ok 5 '(( $(agent_tabs) < before ))'
  local ok=$?
  pass_if "tab close leaves the strip" '(( ok == 0 && before > 0 ))' "agent tabs in the strip: $before, then $(agent_tabs)"
}

check_form() {
  page fm $URL_A/form || { record form FAIL "tab new: $ERR"; return }
  gbx --tab fm fill '#name' Ada && gbx --tab fm fill '#pw' hunter2 && gbx --tab fm select '#color' green &&
    gbx --tab fm click '#send' && gbx --tab fm wait --text 'sent Ada green pw:yes'
  pass_if "form: fill, select, click" '(( RC == 0 ))' "${ERR:-$(value fm "document.getElementById('out').textContent")}"
  close fm
}

check_memory() {
  local n=${TABS:-3} i
  ps -axo pid=,rss=,comm= > $STATE/ps-before
  for (( i = 1; i <= n; i++ )); do page mem$i $URL_A/ >/dev/null; done
  sleep 3
  ps -axo pid=,rss=,comm= > $STATE/ps-after
  local awake=0
  for (( i = 1; i <= n; i++ )); do [[ $(state mem$i) == awake ]] && (( awake++ )); done
  record memory INFO "$(py memory $STATE/ps-before $STATE/ps-after $awake)"
  for (( i = 1; i <= n; i++ )); do close mem$i; done
}

# ---- setup

# Sourced by browser-check-test.zsh for the functions alone.
[[ -n ${BROWSER_CHECK_LIB:-} ]] && return 0

[[ -d ${1:-} ]] || { sed -n '4p' $0; exit 2; }
WT=${1:a}; shift
PORT=1437
if [[ ${1:-} == <-> ]]; then PORT=$1; shift; fi
ONLY=($@)
(( PORT != 1420 )) || { print -u2 "Not 1420: that port's storage is the user's own dev run."; exit 2; }
TARGET=$WT/src-tauri/target
STATE=$(mktemp -d "${${TMPDIR:-/tmp}%/}/gitviber-live-check.XXXXXX") || exit 1
ENV_FILE=$STATE/env
RESULTS=$STATE/results.tsv
REPO=$STATE/repo
touch $RESULTS
typeset -a PICS SESSIONS SERVERS
DEV_PID= APP_PID=
trap finish EXIT
trap 'exit 130' INT TERM


for p in $PORT $(( PORT + 100 )); do
  py port-free $p || { record setup FAIL "port $p is taken"; exit 1 }
done
SA=$(py free-port); SB=$(py free-port)
URL_A=http://127.0.0.1:$SA URL_B=http://127.0.0.1:$SB
python3 -I $HERE/browser-check-site.py $SA > $STATE/site-a.log 2>&1 & SERVERS+=($!)
python3 -I $HERE/browser-check-site.py $SB > $STATE/site-b.log 2>&1 & SERVERS+=($!)

mkdir -p $REPO && git -C $REPO init -q -b main && print hi > $REPO/a.txt &&
  git -C $REPO add . && git -C $REPO -c user.name=t -c user.email=t@t commit -qm init

say "starting the dev app on port $PORT (log: $STATE/dev.log; a cold build takes minutes)"
( cd $WT && CARGO_TARGET_DIR=$TARGET exec pnpm tauri dev --port $PORT ) > $STATE/dev.log 2>&1 &
DEV_PID=$!
until_ok 1200 'grep -q "Running \`" $STATE/dev.log || ! kill -0 $DEV_PID 2>/dev/null'
kill -0 $DEV_PID 2>/dev/null && grep -q 'Running `' $STATE/dev.log || { record setup FAIL "the dev app didn't start: see dev.log"; exit 1 }
# This worktree's build only, by its path: the installed app has the same bundle id.
until_ok 120 'APP_PID=$(pgrep -f "^$TARGET/debug/(gitviber|GitViber Dev\\.app/)" | head -1); [[ -n $APP_PID ]] && js "return 1" >/dev/null 2>&1'
[[ -n $APP_PID ]] || { record setup FAIL "no app process from $TARGET"; exit 1 }
say "app pid $APP_PID"

js 'const k = "gitviber.settings.v2"; const s = JSON.parse(localStorage.getItem(k) || "{}");
  s.browserAgentControl = true; localStorage.setItem(k, JSON.stringify(s)); setTimeout(() => location.reload(), 100); return true' >/dev/null
sleep 2
until_ok 30 '[[ $(js "return document.readyState" 2>/dev/null) == complete ]]'
invoke open_dropped "{ paths: [\"$REPO\"] }" >/dev/null
js 'press(await waitFor(() => $("[aria-label^=\"Show terminal\"]"), 15000)); return true' >/dev/null ||
  { record setup FAIL "no terminal button"; exit 1 }
# The pane's shell writes its environment where only this user can read it.
pty=$(js "const data = \"(umask 077; env | grep '^GITVIBER_' > '$ENV_FILE')\\r\";
  return await waitFor(async () => {
    for (let id = 0; id <= 5; id++) {
      try { await window.__TAURI_INTERNALS__.invoke('pty_write', { id, data, binary: false }); return String(id); } catch {}
    }
  }, 15000);")
until_ok 15 'grep -q "^GITVIBER_BROWSER_SOCKET=" $ENV_FILE 2>/dev/null' ||
  { record setup FAIL "the terminal's environment never came (pty ${pty:-none})"; exit 1 }
say "terminal pty $pty; pages on $URL_A and $URL_B"

gb help > $STATE/help.txt 2>&1 || { record setup FAIL "gitviber browser help: $(<$STATE/help.txt)"; exit 1 }

for c in ${ONLY:-$CHECKS}; do
  (( ${+functions[check_$c]} )) || { record $c FAIL "no such check"; continue }
  lack=$(py missing $STATE/help.txt "${(@s:|:)NEEDS[$c]}")
  if [[ -n $lack ]]; then record $c SKIP "help has no $lack"; continue; fi
  say $c
  rows=$(wc -l < $RESULTS)
  check_$c
  (( $(wc -l < $RESULTS) > rows )) || record $c FAIL "recorded nothing"
done
