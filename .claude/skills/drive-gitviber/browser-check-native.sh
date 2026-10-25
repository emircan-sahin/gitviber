#!/bin/zsh
# Live check of the native app's browser (target/debug/gitviber), the user's tabs and agents'
# tabs end to end. Prints a PASS/FAIL/SKIP/INFO table; exits 1 on any FAIL.
#   browser-check-native.sh [check...]
# Build first: `command cargo build -p gitviber`. The app runs on a fake HOME (CFFIXED_USER_HOME
# too, so WebKit's data and Downloads are in it), a config folder and a short TMPDIR (its socket)
# of the run's own, with a `git clone --local` of this repository as its project and agent control
# on. Agents' commands come from its first terminal pane, whose shell writes its GITVIBER_*
# environment from the fake home's .zshrc, so no key reaches a shell. The user's side gets keys
# sent to the app's pid alone and clicks only on its own window while it is in front and on top
# (gvwin.swift checks both before each); NO_CLICKS=1 skips what needs a click. Nothing here can flip
# a switch in Settings, so a check of another setting relaunches the app with it.
# Never: a CryptoKey (a keychain password prompt), pbcopy/pbpaste, ~/Downloads, a real site or
# account, a real agent session (Ask runs a stand-in `claude`). KEEP=1 keeps the logs when all
# pass. Everything it made is undone on exit, failed or not.
setopt extended_glob
HERE=${0:a:h}

# zsh reads a script as it runs it: an edit mid-run would land it on another line. So a run goes
# on in a copy of its own; NATIVE_CHECK_STATE says it's the copy.
COPIED=(browser-check-native.sh browser-check.py browser-check-site.py gvwin.swift)
if [[ -z ${NATIVE_CHECK_STATE:-} ]]; then
  NATIVE_CHECK_STATE=$(mktemp -d "${${TMPDIR:-/tmp}%/}/gitviber-native-check.XXXXXX") || exit 1
  mkdir $NATIVE_CHECK_STATE/bin && cp $HERE/$^COPIED $NATIVE_CHECK_STATE/bin/ ||
    { rm -rf $NATIVE_CHECK_STATE; exit 1 }
  NATIVE_CHECK_STATE=$NATIVE_CHECK_STATE NATIVE_CHECK_ROOT=${HERE:h:h:h} \
    exec zsh $NATIVE_CHECK_STATE/bin/browser-check-native.sh "$@"
fi

STATE=$NATIVE_CHECK_STATE
ROOT=$NATIVE_CHECK_ROOT
APP=${APP:-$ROOT/target/debug/gitviber}
RESULTS=$STATE/results.tsv
FAKE=$STATE/home
CONFIG=$STATE/config
REPO=$STATE/repo
GVWIN=$STATE/bin/gvwin
typeset -a PICS SESSIONS SERVERS
APP_PID= APP_TMP= ENV_FILE=

py() { python3 -I $HERE/browser-check.py "$@" }
say() { print -r -- "· $*"; print -ru2 -- "· $*" }
record() { local n=${3:-}; n=${n//$'\n'/ }; print -r -- "$1"$'\t'"$2"$'\t'"${n//$'\t'/ }" >> $RESULTS }
# A row: PASS when the condition (a command line, evaluated) holds. Take $? into a variable
# before calling: the note's own $(…) would change it.
pass_if() { if eval "$2"; then record "$1" PASS "$3"; else record "$1" FAIL "$3"; fi }
# Polls `cond` (a command line) every half second for up to `secs`; its status at the end.
until_ok() {
  local secs=$1 i; shift
  for (( i = 0; i < secs * 2; i++ )); do eval "$*" && return 0; sleep 0.5; done
  eval "$*"
}

# ---- `gitviber browser` as the pane's agent runs it: its environment alone, in the repo.

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
gbx() { OUT=$(gb "$@" 2>$STATE/err); RC=$?; ERR=$(<$STATE/err) }
value() { gbx --tab $1 eval "$2"; py eval-value <<<$OUT }
state() { gb --json tab list | py tab-state $1 }
page() { gbx tab new $1 $2 ${@:3} && gbx --tab $1 wait --load }
close() { local t; for t; do gb tab close $t >/dev/null 2>&1; done }
# A page's title set, so the tab strip's text tells its tab apart.
titled() { page $1 $2 && value $1 "document.title = '$3', 'titled'" >/dev/null }

# ---- the app: launched per phase, each a set of settings (nothing here flips one in Settings).

typeset -A PHASE_SETTINGS=(
  main ''
  park '"browserLiveHidden": 0'
  hidden '"browserShowAgentTabs": false'
  unmuted '"browserMuteAgentTabs": false'
  off ''
)

write_config() {
  local control=true
  [[ $1 == off ]] && control=false
  rm -rf $CONFIG && mkdir -p $CONFIG
  # Ask runs the stand-in by its full path: a `claude` found on PATH could be the real one.
  print -r -- "{\"browserAgentControl\": $control, \"autoUpdate\": false,
    \"reviewCommand\": \"$STATE/bin/claude\", \"suggestCommand\": \"$STATE/bin/claude\"${PHASE_SETTINGS[$1]:+, ${PHASE_SETTINGS[$1]}}}" > $CONFIG/settings.json
  # At a known place and size, the left panel shut (the strip starts at the window's edge) and the
  # terminal open, which starts the pane the agents' commands come from.
  print -r -- "{\"window\": {\"x\": 40, \"y\": 60, \"width\": 1400, \"height\": 900, \"maximized\": false, \"fullscreen\": false},
    \"panels\": {\"left\": {\"size\": null, \"collapsed\": true}, \"right\": {\"size\": null, \"collapsed\": true},
      \"bottom\": {\"size\": 260, \"collapsed\": false}},
    \"recentProjects\": [\"$REPO\"], \"recentCommands\": [],
    \"workspaces\": [{\"root\": \"$REPO\", \"tabs\": [], \"active\": null, \"listTab\": null, \"rightPanel\": null, \"viewed\": [], \"review\": null}],
    \"settingsSection\": null}" > $CONFIG/state.json
}

# The env file of the lowest pane whose shell wrote one.
pane_env() {
  local f
  for f in $STATE/pane-<->.env(Nn); do
    grep -q '^GITVIBER_BROWSER_SOCKET=' $f && { print -r -- $f; return 0 }
  done
  return 1
}

launch_app() {
  quit_app
  write_config $1
  rm -f $STATE/pane-*(N)
  ( cd $REPO && exec env -i PATH=/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin USER=$USER LOGNAME=$LOGNAME \
      SHELL=/bin/zsh LANG=en_US.UTF-8 HOME=$FAKE CFFIXED_USER_HOME=$FAKE TMPDIR=$APP_TMP/ \
      GITVIBER_CONFIG_DIR=$CONFIG $APP $REPO ) > $STATE/app-$1.log 2>&1 &
  APP_PID=$!
  until_ok 60 'pane_env >/dev/null || ! kill -0 $APP_PID 2>/dev/null'
  ENV_FILE=$(pane_env) || return 1
  until_ok 15 '[[ -n $(win) ]]'
}

quit_app() {
  [[ -n $APP_PID ]] || return 0
  kill $APP_PID 2>/dev/null
  until_ok 10 '! kill -0 $APP_PID 2>/dev/null' || kill -9 $APP_PID 2>/dev/null
  APP_PID=
}

# ---- its window: found by pid, read by OCR, keys to the pid alone, clicks guarded.

win() { $GVWIN win $APP_PID 2>/dev/null | head -1 }
# Lines of text in the window, "x y w h text" in its points.
ocr() {
  local id x y w h
  read -r id x y w h <<<"$(win)"
  [[ -n $id ]] && screencapture -x -o -l $id $STATE/ocr.png 2>/dev/null || return 1
  local pw=$(py png $STATE/ocr.png | cut -d' ' -f1)
  $GVWIN ocr $STATE/ocr.png $(printf %.4f $(( ${pw:-$w} / ${w:-1}.0 )))
}
# The tab strip's text, the address bar's, and the page's below them.
strip() { ocr | awk '$2 >= 42 && $2 <= 72 { $1 = $2 = $3 = $4 = ""; print }' }
address() { ocr | awk '$2 >= 78 && $2 <= 104 { $1 = $2 = $3 = $4 = ""; print }' | grep -o 'http[^ ]*' | head -1 }
in_strip() { strip | grep -ci -- "$1" }
front() { $GVWIN front $APP_PID }
key() { $GVWIN key $APP_PID "$@"; sleep 0.3 }
type_text() { $GVWIN type $APP_PID "$1"; sleep 0.2 }
# A click at a point of the window (its points); 3 when it isn't in front and on top there.
click() {
  local id x y w h
  read -r id x y w h <<<"$(win)"
  [[ -n $id ]] || return 3
  $GVWIN click $APP_PID $(( x + $1 )) $(( y + $2 )) ${3:-}
}
# Clicks the middle of the first text line OCR finds with $1 (a grep pattern), below y $2.
click_text() {
  local line=$(ocr | awk -v top=${2:-0} '$2 >= top' | grep -m1 -- "$1")
  [[ -n $line ]] || return 1
  local x y w h; read -r x y w h _ <<<$line
  click $(( x + ${3:-w / 2} )) $(( y + h / 2 ))
}
# Why a check that needs the window can't run, or nothing.
no_window() {
  [[ -n ${NO_CLICKS:-} ]] && { print "NO_CLICKS is set"; return 0 }
  front || { print "the app isn't in front (another app took the screen): no clicks into it"; return 0 }
  return 1
}
# Key codes, as a US keyboard has them; the app reads them through the Mac's layout.
K_B=11 K_L=37 K_R=15 K_F=3 K_M=46 K_C=8 K_T=17 K_W=13 K_H=4 K_0=29 K_EQUAL=24 K_A=0 K_RET=36 K_ESC=53

# ---- the checks: each a function check_<name>, recording one row or more. NEEDS names what of
# `gitviber browser help` it uses; a check whose needs aren't there is skipped.

CHECKS=(visibility fps isolation sessions session_idb unknown_tab device screenshot network sse tab_close form
  stale_ref covered smooth_scroll fill_honest change_line find_get screenshot_1x screenshot_full touch untrusted secret
  frames trusted hover keys background_keys clipboard_safe clipboard_bypass clipboard_tab popup download
  agent_audio agent_show ownership
  user_tab user_address user_nav user_focus user_find user_zoom user_device user_responsive user_pick_ask
  user_download user_popup memory hidden_window agent_e2e
  idle_park agent_hide pane_close pane_exit agent_unmuted control_off)
# Run with the app launched on another setting: the panes' checks where agents' tabs are left
# out of the strip, as one that leaves its agents comes into it.
typeset -A PHASE=(idle_park park agent_hide hidden pane_close hidden pane_exit hidden agent_unmuted unmuted control_off off)
typeset -A NEEDS=(
  visibility "eval" fps "eval" isolation "eval" sessions "session save|session load|--session"
  session_idb "--indexeddb" unknown_tab "--tab" device "device <name|WxH|off>" screenshot "screenshot"
  network "network <id>" sse "network" idle_park "--idle" tab_close "tab close" form "fill|select|click|wait"
  memory "tab list" stale_ref "fails as stale" covered "covered by" smooth_scroll "covered by"
  fill_honest "check | uncheck" change_line "→ no DOM change" find_get "find <|get styles|-o <file>"
  screenshot_1x "--clip|--scale|--refs" screenshot_full "--full" touch "touchstart" untrusted "--reveal"
  secret "--secret" frames "frame of the page's origin" trusted "--synthetic" hover "CSS :hover applies"
  keys "key down <key>|--hold" background_keys "key down <key>|--idle|canvas repainted"
  clipboard_tab "frame of the page's origin|--synthetic" popup "named for its opener"
  download "--download|downloads [--clear]" agent_audio "audio: playing" agent_show "--show"
  agent_hide "tab list" agent_unmuted "audio: playing" control_off "--show" hidden_window "eval"
  ownership "tab release" pane_close "tab list" pane_exit "tab list" user_focus "eval"
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
  page iso3 $URL_A/ --shared
  local shared=$(value iso3 "localStorage.getItem('token')")
  pass_if "--shared is the user's store, not a private tab's" '[[ $shared == null ]]' "token there: $shared"
  close iso1 iso2 iso3
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
  gbx session list
  pass_if "session list" '[[ $OUT == *lc-alice* ]]' "${OUT[1,120]}"
  local files=($CONFIG/browser-sessions/**/*(N.))
  local modes=${(u)${(f)"$(stat -f %Lp $files 2>/dev/null)"}}
  pass_if "saved sessions are the user's alone" '(( $#files )) && [[ $modes == 600 ]]' "${#files} files, modes: ${modes:-none}"
  close s1 s2 s3
}

# A login kept in IndexedDB comes back in another tab; a database with an ImageData (a kind of
# value a session can't carry) is named and left out. Nothing here stores the kind of key that
# makes macOS ask for the login keychain's password: no prompt, ever, from a check.
check_session_idb() {
  page si $URL_A/ || { record session_idb FAIL "tab new: $ERR"; return }
  SESSIONS+=(lc-idb)
  gb session delete lc-idb >/dev/null 2>&1
  local make="(async () => {
    const open = (name, version, fill) => new Promise((ok, no) => { const r = indexedDB.open(name, version); r.onupgradeneeded = () => fill(r.result); r.onsuccess = () => { r.result.close(); ok() }; r.onerror = () => no(r.error) });
    await open('gv-check', 3, (db) => { const s = db.createObjectStore('auth', { keyPath: 'id' }); s.createIndex('by_name', 'name'); s.put({ id: 'u1', name: 'alice', token: 'tok-idb', at: new Date(0), raw: new Uint8Array([1, 2, 3]) }); db.createObjectStore('plain').put('v', 'k') });
    await open('gv-check-pixels', 1, (db) => { db.createObjectStore('pixels').put(new ImageData(1, 1), 'p') });
    return 'made' })()"
  gbx --tab si eval $make
  local made=$(py eval-value <<<$OUT)
  pass_if "IndexedDB made in the page" '[[ $made == made ]]' "${${OUT:-$ERR}[1,200]}"
  gbx --tab si session save lc-idb
  pass_if "session save: IndexedDB left alone unless asked" \
    '(( RC == 0 )) && [[ $OUT != *"IndexedDB databases ("* && $OUT == *"--indexeddb"* ]]' "$OUT$ERR"
  gbx --tab si session save lc-idb --force --indexeddb
  pass_if "session save --indexeddb: IndexedDB kept, an ImageData's database named" \
    '(( RC == 0 )) && [[ $OUT == *"IndexedDB databases"* && $OUT == *"left out: gv-check-pixels (holds a ImageData"* ]]' "$OUT$ERR"
  gbx tab new si2 --session lc-idb && gbx --tab si2 wait --load
  local read="(async () => {
    const db = await new Promise((ok, no) => { const r = indexedDB.open('gv-check'); r.onsuccess = () => ok(r.result); r.onerror = () => no(r.error) });
    const s = db.transaction('auth').objectStore('auth');
    const row = await new Promise((ok) => { const g = s.get('u1'); g.onsuccess = () => ok(g.result) });
    const names = (await indexedDB.databases()).map((d) => d.name).sort().join('+');
    const out = [row.token, db.version, s.keyPath, [...s.indexNames].join(), row.at.toISOString(), [...new Uint8Array(row.raw)].join(), names].join('|');
    db.close(); return out })()"
  local got=$(value si2 $read)
  pass_if "tab new --session: IndexedDB comes back" '[[ $got == "tok-idb|3|id|by_name|1970-01-01T00:00:00.000Z|1,2,3|gv-check" ]]' "$got $ERR"
  close si si2
}

check_unknown_tab() {
  gbx --tab lc-no-such-tab url
  pass_if "unknown --tab" '(( RC != 0 )) && [[ -n $ERR ]]' "exit $RC: $ERR"
}

check_device() {
  page d1 $URL_A/ && page d2 $URL_A/ || { record device FAIL "tab new: $ERR"; return }
  gbx --tab d1 device 400x700
  local one=$(value d1 '`${innerWidth}x${innerHeight}`') two=$(value d2 innerWidth)
  pass_if "device per tab" '[[ $one == 400x700 && $two != 400 ]]' "400x700 tab: $one, the other: $two px wide"
  gbx --tab d1 device "iPhone 16 Pro"
  one=$(value d1 '`${innerWidth}x${innerHeight} @${devicePixelRatio}`')
  pass_if "device iPhone 16 Pro" '[[ $one == "402x778 @3" ]]' "$one"
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
  pass_if "screenshot in the fake home's cache" '[[ $pic == $FAKE/* ]]' "${pic:-none}"
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
  value net "console.error('lc-console-line'), 'logged'" >/dev/null
  gbx --tab net console
  pass_if "console" '[[ $OUT == *lc-console-line* ]]' "${${(M)${(f)OUT}:#*lc-console*}:-${OUT[1,120]}$ERR}"
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

# The tab strip names an agent's tab (its page's title), and loses it once closed.
check_tab_close() {
  titled cl1 $URL_A/ "tabstrip" || { record tab_close FAIL "tab new: $ERR"; return }
  until_ok 8 '(( $(in_strip tabstrip) == 1 ))'
  local before=$(in_strip tabstrip)
  gbx tab close cl1
  until_ok 8 '(( $(in_strip tabstrip) == 0 ))'
  local ok=$?
  pass_if "an agent's tab in the strip, gone once closed" '(( ok == 0 && before == 1 ))' "in the strip: $before, then $(in_strip tabstrip)"
}

check_form() {
  page fm $URL_A/form || { record form FAIL "tab new: $ERR"; return }
  gbx --tab fm fill '#name' Ada && gbx --tab fm fill '#pw' hunter2 && gbx --tab fm select '#color' green &&
    gbx --tab fm click '#send' && gbx --tab fm wait --text 'sent Ada green pw:yes'
  pass_if "form: fill, select, click" '(( RC == 0 ))' "${ERR:-$(value fm "document.getElementById('out').textContent")}"
  close fm
}

ref_of() { gb --tab $1 snapshot -i | py snapshot-ref $2 }

check_stale_ref() {
  page st $URL_A/act || { record stale_ref FAIL "tab new: $ERR"; return }
  local boom=$(ref_of st '"Boom"') noop=$(ref_of st '"Nothing"')
  value st "document.getElementById('boom').remove(), 'gone'" >/dev/null
  gbx --tab st click ${boom:-e0}
  pass_if "stale ref fails" '(( RC == 1 )) && [[ $ERR == *stale* ]]' "click $boom: $ERR"
  local again=$(ref_of st '"Nothing"')
  pass_if "other refs keep their numbers" '[[ -n $noop && $again == $noop ]]' "Nothing: $noop, then $again"
  close st
}

check_covered() {
  page cv $URL_A/covered || { record covered FAIL "tab new: $ERR"; return }
  gbx --tab cv click '#under'
  pass_if "click: covered" '(( RC != 0 )) && [[ $ERR == *"covered by <div#veil"* ]]' "$ERR"
  gbx --tab cv click '#off'
  pass_if "click: disabled fieldset" '(( RC != 0 )) && [[ $ERR == *"is disabled"* ]]' "$ERR"
  close cv
}

check_smooth_scroll() {
  page sm $URL_A/smooth || { record smooth_scroll FAIL "tab new: $ERR"; return }
  gbx --tab sm click '#far'
  local title=$(value sm document.title)
  pass_if "click 4000 px down, smooth scrolling" '(( RC == 0 )) && [[ $title == "far clicked" ]]' "${ERR:-$title}"
  close sm
}

check_fill_honest() {
  page fl $URL_A/fill || { record fill_honest FAIL "tab new: $ERR"; return }
  gbx --tab fl fill '#short' abcdefgh
  pass_if "fill: maxlength" '(( RC != 0 )) && [[ $ERR == *maxlength* ]] || [[ $OUT == *abcd* && $OUT != *abcde* ]]' "$OUT$ERR"
  gbx --tab fl fill '#cb' yes
  pass_if "fill: a checkbox points to check" '(( RC != 0 )) && [[ $ERR == *"use check"* ]]' "$ERR"
  gbx --tab fl fill '#phone' 5551234567
  pass_if "fill: a masked field" '(( RC == 0 )) && [[ $OUT == *"(555) 123-4567"* ]]' "$OUT$ERR"
  gbx --tab fl check '#cb'; local on=$(gb --tab fl get value '#cb' | py eval-value)
  gbx --tab fl uncheck '#cb'; local off=$(gb --tab fl get value '#cb' | py eval-value)
  pass_if "check, uncheck" '[[ $on == checked && $off == unchecked ]]' "$on, then $off"
  close fl
}

check_change_line() {
  page cl $URL_A/act || { record change_line FAIL "tab new: $ERR"; return }
  gbx --tab cl click '#boom'
  pass_if "change line names a 500" '[[ $OUT == *"→"*500* ]]' "${${(M)${(f)OUT}:#→*}:-$OUT$ERR}"
  gbx --tab cl click '#noop'; gbx --tab cl click '#noop'; gbx --tab cl click '#noop'
  pass_if "third no-op gets the nudge" '[[ $OUT$ERR == *"same command"* ]]' "$OUT$ERR"
  close cl
}

check_find_get() {
  page fg $URL_A/act || { record find_get FAIL "tab new: $ERR"; return }
  gbx --tab fg find '/sign.?in/i'
  pass_if "find /regex/i" '(( RC == 0 )) && [[ $OUT == *"Sign in"* ]]' "${${OUT:-$ERR}[1,100]}"
  gbx --tab fg get styles color '#who'
  pass_if "get styles color" '[[ $OUT == *"rgb(0, 128, 0)"* ]]' "${${OUT:-$ERR}[1,100]}"
  gbx --tab fg get box '#who'
  pass_if "get box" '(( RC == 0 )) && [[ $OUT == *[0-9]* ]]' "${${OUT:-$ERR}[1,100]}"
  local ref=$(ref_of fg '"Open popup"')
  gbx --tab fg snapshot --ref ${ref:-e0}
  pass_if "snapshot --ref" '[[ $OUT == *"Open popup"* && $OUT != *Boom* ]]' "$ref: ${${OUT:-$ERR}[1,100]}"
  gbx --tab fg snapshot -o $STATE/snapshot.txt
  pass_if "snapshot -o" '[[ -s $STATE/snapshot.txt ]]' "${${OUT:-$ERR}[1,100]}"
  close fg
}

check_screenshot_1x() {
  page s1x $URL_A/ || { record screenshot_1x FAIL "tab new: $ERR"; return }
  local css=$(value s1x '[innerWidth, innerHeight]') size want
  local w=${${css#\[}%%,*} h=${${css%\]}##*, }
  gbx --tab s1x screenshot $STATE/one.png
  size=$(py png $STATE/one.png 2>/dev/null) want=$(py shot-expect $w $h)
  pass_if "screenshot: 1 px a CSS px" '[[ -n $size && $size == $want ]]' "css ${w}x$h, png ${size:-none: $ERR}"
  gbx --tab s1x screenshot $STATE/half.png --scale 0.5
  size=$(py png $STATE/half.png 2>/dev/null) want=$(py shot-expect $w $h 0.5)
  pass_if "screenshot --scale 0.5" '[[ -n $size && $size == $want ]]' "png ${size:-none: $ERR}, want $want"
  gbx --tab s1x screenshot $STATE/clip.png --clip 0,0,200,100
  size=$(py png $STATE/clip.png 2>/dev/null)
  pass_if "screenshot --clip" '[[ $size == "200 100" ]]' "png ${size:-none: $ERR}"
  gbx --tab s1x screenshot $STATE/refs.png --refs
  local plain=$(stat -f %z $STATE/one.png 2>/dev/null) labelled=$(stat -f %z $STATE/refs.png 2>/dev/null)
  pass_if "screenshot --refs" '(( ${labelled:-0} > ${plain:-0} ))' "${plain:-?} bytes, with refs ${labelled:-none: $ERR}"
  close s1x
}

check_screenshot_full() {
  page full $URL_A/tall || { record screenshot_full FAIL "tab new: $ERR"; return }
  gbx --tab full scroll down
  local before=$(value full scrollY) w=$(value full innerWidth) h=$(value full innerHeight)
  gbx --tab full screenshot $STATE/tall.png --full
  local files=($STATE/tall.png(N) $STATE/tall-<->.png(N)) sizes="" f
  for f in $files; do sizes+="${$(py png $f 2>/dev/null)/ /x} "; done
  local want="$(py tall-sizes $w $h) "
  pass_if "screenshot --full: pictures of 2000 px" '[[ -n $sizes && $sizes == $want ]]' "want $want, got ${sizes:-none: $OUT$ERR}"
  pass_if "screenshot --full: the page once, fixed parts once" 'py tall-check $(py tall-scale $w $h) $files >$STATE/tall.out 2>&1' "$(<$STATE/tall.out 2>/dev/null)"
  local after=$(value full scrollY) head=$(value full 'getComputedStyle(document.getElementById("head")).visibility')
  pass_if "screenshot --full: page put back" '[[ $after == $before && $head == visible ]]' "scroll $before then $after, header $head"
  gbx --tab full screenshot --full --clip 0,0,10,10
  pass_if "screenshot --full takes no --clip" '(( RC != 0 ))' "exit $RC: $ERR"
  rm -f $files
  close full
}

check_touch() {
  page touch $URL_A/touch || { record touch FAIL "tab new: $ERR"; return }
  local probe='`${navigator.maxTouchPoints} ${"ontouchstart" in window} ${matchMedia("(pointer: coarse)").matches} ${matchMedia("(hover: none)").matches} ${getComputedStyle(document.getElementById("probe")).color}`'
  gbx --tab touch device "iPhone 15"
  local seen=$(value touch $probe)
  pass_if "device: a phone has touch" '[[ $seen == "5 true true true rgb(255, 0, 0)" ]]' "$seen ($OUT$ERR)"
  gbx --tab touch click '#tap'
  local log=$(value touch 'log_.join(" ")')
  pass_if "device: a click is a tap" '[[ $log == "pointerdown:touch touchstart mousedown pointerup:touch touchend mouseup click" ]]' "$log"
  gbx --tab touch device 1280x800
  seen=$(value touch $probe)
  pass_if "device WxH: the mouse stays" '[[ $seen == "0 false false false rgb(0, 128, 0)" ]]' "$seen"
  gbx --tab touch click '#tap'
  log=$(value touch 'log_.join(" ")')
  pass_if "device WxH: a click is a click" '[[ $log != *touch* && $log == *click ]]' "$log"
  gbx --tab touch device off
  close touch
}

check_untrusted() {
  page ut "$URL_A/?code=abc123&access_token=tok456" || { record untrusted FAIL "tab new: $ERR"; return }
  gbx --tab ut eval document.title
  pass_if "page text marked untrusted" '[[ $OUT == *"page content (untrusted)"*nonce=* ]]' "${${(f)OUT}[1]}"
  gbx --tab ut url
  pass_if "tokens hidden in url" '[[ $OUT != *abc123* && $OUT != *tok456* ]]' "$OUT"
  gbx --reveal --tab ut url
  pass_if "--reveal shows them" '[[ $OUT == *abc123* && $OUT == *tok456* ]]' "$OUT"
  close ut
}

check_secret() {
  page sc $URL_A/form || { record secret FAIL "tab new: $ERR"; return }
  export LIVE_CHECK_PW=correct-horse-42
  gbx --tab sc fill '#pw' --secret LIVE_CHECK_PW
  gbx --tab sc eval "document.getElementById('pw').value"
  unset LIVE_CHECK_PW
  pass_if "fill --secret, never shown" '[[ $OUT == *"[secret \$LIVE_CHECK_PW]"* && $OUT != *correct-horse* ]]' "$OUT$ERR"
  close sc
}

check_frames() {
  page fr "$URL_A/frames?other=$SB" || { record frames FAIL "tab new: $ERR"; return }
  gbx --tab fr snapshot -i
  local snap=$OUT ref=$(py snapshot-ref '"Inner"' <<<$OUT)
  pass_if "snapshot: a frame nested" '[[ -n $ref ]]' "${${snap:-$ERR}[1,120]}"
  gbx --tab fr click ${ref:-e0}
  pass_if "click in a frame" '(( RC == 0 ))' "${OUT:-$ERR}"
  gbx --tab fr click "$(py snapshot-ref '"Oops"' <<<$snap)"
  gbx --tab fr console
  pass_if "console: a frame's error" '[[ $OUT == *"(frame"* && $OUT == *"inner oops"* ]]' "${${OUT:-$ERR}[1,120]}"
  gbx --tab fr get text 'frame=#same >> #inner'
  pass_if "css in a same-origin frame" '[[ $OUT == *Inner* ]]' "${OUT:-$ERR}"
  gbx --tab fr wait --timeout 3000 'frame=#same >> #inner'
  local waited=$RC
  gbx --tab fr wait --timeout 3000 --frame '#same' --text Inner
  pass_if "wait in a frame: a selector, a text" '(( waited == 0 && RC == 0 ))' "css $waited, text $RC: ${OUT:-$ERR}"
  gbx --tab fr wait --timeout 600 'frame=#nope >> #inner'
  pass_if "wait for a frame not there is a timeout that says so" '(( RC != 0 )) && [[ $ERR == *"No frame matches #nope"* ]]' "$RC: $ERR"
  gbx --tab fr get text 'frame=#cross >> #inner'
  pass_if "css in another origin's frame says why" '(( RC != 0 )) && [[ $ERR == *"another origin"* ]]' "$RC: $ERR"
  gbx --tab fr snapshot
  pass_if "cross-origin frame's hint" '[[ $OUT == *"127.0.0.1:$SB"* ]]' "${${(M)${(f)OUT}:#*$SB*}:-${${OUT:-$ERR}[1,120]}}"
  close fr
}

check_trusted() {
  page tr $URL_A/native || { record trusted FAIL "tab new: $ERR"; return }
  gbx --tab tr click '#trust'
  local t=$(value tr document.title)
  pass_if "click is trusted" '[[ $t == true ]]' "isTrusted: $t $ERR"
  gbx --tab tr type '#txt' 'typed é'
  local typed=$(value tr "document.getElementById('txt').value")
  pass_if "type: text as an input method puts it" '[[ $typed == "typed é" ]]' "field: $typed $ERR"
  close tr
}

check_hover() {
  page hv $URL_A/native || { record hover FAIL "tab new: $ERR"; return }
  local style="getComputedStyle(document.getElementById('hov')).color"
  local before=$(value hv $style)
  gbx --tab hv hover '#hov'
  local after=$(value hv $style)
  pass_if "hover applies :hover" '[[ $before != $after && $after == "rgb(255, 0, 0)" ]]' "$before, then $after $ERR"
  close hv
}

check_keys() {
  page ky $URL_A/native || { record keys FAIL "tab new: $ERR"; return }
  gbx --tab ky key down Shift; gbx --tab ky click '#lnk'; gbx --tab ky key up Shift
  local t=$(value ky document.title)
  pass_if "key down Shift + click" '[[ $t == shift:true ]]' "$t $ERR"
  gbx --tab ky press w --hold 300
  local gap=$(value ky window.keyGap_)
  pass_if "press --hold 300" '(( ${gap:-0} >= 250 ))' "keydown to keyup: ${gap:-none} ms $ERR"
  gbx --tab ky type '#txt' $'one\ntwo'
  pass_if "type: line break into <input> refused" '(( RC != 0 ))' "$ERR"
  close ky
}

# Keys and clicks to several tabs at once, none of them on show: each tab gets its own events,
# trusted, with the page focused and visible, and a held key reaches the page's frames. A tab
# holding a key doesn't sleep with its idle time up, while one that holds none does.
check_background_keys() {
  local t held up up2
  for t in bk1 bk2; do page $t $URL_A/game || { record background_keys FAIL "tab new $t: $ERR"; return }; done
  page bk3 $URL_A/game --idle 1 && page bk4 $URL_A/game --idle 1 || { record background_keys FAIL "tab new: $ERR"; return }
  gbx --tab bk1 key down ArrowRight
  local k1=$RC e1=$ERR said=$OUT
  gbx --tab bk2 key down ArrowRight; local k2=$RC e2=$ERR
  gbx --tab bk3 key down ArrowRight; local k3=$RC e3=$ERR
  gbx --tab bk4 key down ArrowRight
  pass_if "key down on four tabs, none on show" '(( k1 == 0 && k2 == 0 && k3 == 0 && RC == 0 ))' "$k1 $e1 | $k2 $e2 | $k3 $e3 | $RC $ERR"
  pass_if "key down on a canvas game says the canvas repainted" '[[ $said == *"canvas repainted"* ]]' "${${(M)${(f)said}:#→*}:-$said}"
  gbx --tab bk2 press a; gbx --tab bk2 click '#fire'
  sleep 1
  local log1=$(value bk1 'log_.join(" ")') log2=$(value bk2 'log_.join(" ")')
  pass_if "each tab gets its own trusted key events, focused and visible" \
    '[[ $log1 == "keydown:ArrowRight:true:true:visible" && $log2 == "keydown:ArrowRight:true:true:visible keydown:a:true:true:visible keyup:a:true:true:visible" ]]' "bk1: $log1; bk2: $log2"
  local a=$(value bk1 moved_); sleep 1; local b=$(value bk1 moved_) held=$(value bk1 'held_()')
  pass_if "a held key moves the page's frames on" '(( ${b:-0} - ${a:-0} >= 30 )) && [[ $held == ArrowRight ]]' "moved $a then $b, held: $held"
  local fired=$(value bk2 fired_) blurs=$(value bk2 blurs_)
  pass_if "click on a tab with a key held down" '[[ $fired == 1 && $blurs == 0 ]]' "fired $fired, blurs $blurs"
  gbx --tab bk1 key down ArrowRight
  pass_if "key down on one held already says so" '(( RC != 0 )) && [[ $ERR == *"held already"* ]]' "$RC $ERR"
  gbx --tab bk1 key up ArrowRight; up=$RC
  gbx --tab bk2 key up ArrowRight; up2=$RC
  held=$(value bk1 'held_()')$(value bk2 'held_()')
  pass_if "key up on each" '(( up == 0 && up2 == 0 )) && [[ -z $held ]]' "$up $up2 held: ${held:-none}"
  gbx --tab bk2 click '#fire'
  pass_if "nothing moving: no DOM change" '[[ $OUT == *"→ no DOM change"* ]]' "${${(M)${(f)OUT}:#→*}:-$OUT$ERR}"
  gbx --tab bk4 key up ArrowRight
  local t0=$SECONDS
  until_ok 100 '[[ $(state bk4) == sleeping ]]'
  local ok=$?
  pass_if "a tab holding no key sleeps with its idle time up" '(( ok == 0 ))' "bk4: $(state bk4) after $(( SECONDS - t0 )) s"
  local s3=$(state bk3) h3=$(value bk3 'held_()') b3=$(value bk3 blurs_)
  pass_if "a tab holding a key stays awake past its idle time" '[[ $s3 == awake && $h3 == ArrowRight && $b3 == 0 ]]' "bk3: $s3, held $h3, blurs $b3"
  gbx --tab bk3 key up ArrowRight; up=$RC
  held=$(value bk3 'held_()')
  pass_if "key up on it after" '(( up == 0 )) && [[ -z $held ]]' "$up, held: ${held:-none}"
  close bk1 bk2 bk3 bk4
}

# Both read the user's clipboard back with pbpaste, which no unattended run may do.
check_clipboard_safe() {
  record "page can't write your clipboard" SKIP "reads your clipboard (pbpaste): by hand, /native's Copy must leave it as it was"
}
check_clipboard_bypass() {
  record "a page past the shim can't write your clipboard" SKIP "reads your clipboard (pbpaste): by hand, /clipbypass's button must leave it as it was"
}

# The tab's own clipboard, from the page's Copy buttons, read back by the page and pasted with the
# agent's Meta+v; a frame of another origin has a copy of its own.
check_clipboard_tab() {
  page ct "$URL_A/clipframes?other=$SB" || { record clipboard_tab FAIL "tab new: $ERR"; return }
  gbx --tab ct snapshot -i
  local same=$(py snapshot-ref '"Copy same"' <<<$OUT)
  local cross=$(value ct "(r => Math.round(r.x + 30) + ',' + Math.round(r.y + 20))(document.getElementById('cross').getBoundingClientRect())")
  local read="(async () => await navigator.clipboard.readText())()"
  gbx --tab ct click '#copy'
  local got=$(value ct $read)
  pass_if "tab clipboard: the page's Copy button" '[[ $got == main-$SA ]]' "readText: $got"
  gbx --tab ct click ${same:-e0}
  got=$(value ct $read)
  pass_if "tab clipboard: a frame of the page's origin shares it" '[[ $got == same-$SA ]]' "readText: $got"
  gbx --tab ct click ${cross:-e0}
  got=$(value ct $read)
  pass_if "tab clipboard: a frame of another origin neither replaces nor is read by the page's" '[[ $got == same-$SA ]]' "readText: $got"
  gbx --tab ct click '#box'; gbx --tab ct press Meta+v
  got=$(value ct "document.getElementById('box').value")
  pass_if "Meta+v pastes the page's copy" '[[ $got == same-$SA ]]' "field: $got"
  gbx --tab ct fill '#box' 'cut and kept'
  gbx --tab ct press Meta+a; gbx --tab ct press Meta+x
  got=$(value ct $read)
  pass_if "Meta+x keeps the field's text in it" '[[ $got == "cut and kept" ]]' "readText: $got"
  local made="(async () => {
    const wait = (i) => new Promise((ok) => { i.onload = () => ok(i) });
    const kind = (i) => typeof i.contentWindow.__gvClipboard;
    const blank = document.body.appendChild(document.createElement('iframe'));
    const doc = document.body.appendChild(document.createElement('iframe')); doc.srcdoc = '<p>x</p>'; await wait(doc);
    const blob = document.body.appendChild(document.createElement('iframe')); blob.src = URL.createObjectURL(new Blob(['<p>y</p>'], { type: 'text/html' })); await wait(blob);
    return [kind(blank), kind(doc), kind(blob)].join(',') })()"
  got=$(value ct $made)
  pass_if "tab clipboard: guarded in about:blank, srcdoc and blob: frames" '[[ $got == function,function,function ]]' "$got"
  gbx --tab ct click '#pop'
  local popup=$(py popup-name <<<$OUT)
  got=$(value ct "typeof window.pop?.__gvClipboard")
  pass_if "tab clipboard: guarded in a window.open popup" '[[ $got == function ]]' "$got"
  close $popup ct
}

check_popup() {
  page pp $URL_A/act || { record popup FAIL "tab new: $ERR"; return }
  gbx --tab pp click '#pop'
  local name=$(py popup-name <<<$OUT)
  pass_if "popup opens as a tab" '[[ -n $name ]]' "${${(M)${(f)OUT}:#*popup*}:-$OUT$ERR}"
  until_ok 5 '[[ $(value pp window.msg_) == hi-from-popup ]]'
  local ok=$?
  pass_if "popup posts to its opener" '(( ok == 0 ))' "window.msg_: $(value pp window.msg_)"
  local said
  [[ -n $name ]] && gbx --tab $name click '#closeme' && said=$OUT
  until_ok 5 '[[ $(state ${name:-none}) == missing ]]'
  ok=$?
  gbx --tab pp url; said+=$'\n'$OUT
  pass_if "window.close() closes its tab" '(( ok == 0 )) && py closed-note ${name:-none} <<<$said' \
    "$name: $(state ${name:-none}); ${${(M)${(f)said}:#*closed*}:-no close note}"
  close pp $name
}

check_download() {
  page dl $URL_A/act || { record download FAIL "tab new: $ERR"; return }
  gbx --tab dl click '#dl'
  gbx --tab dl wait --download
  local -a got=("${(@ps:\t:)$(py download <<<$OUT)}")
  local file=${got[4]:-}
  pass_if "wait --download" '[[ ${got[1]:-} == gitviber-check.txt && -f $file ]]' "${OUT:-$ERR}"
  pass_if "an agent's download stays in the fake home, not ~/Downloads" '[[ $file == $FAKE/* && $file != */Downloads/* ]]' "${file:-none}"
  gbx --tab dl downloads
  pass_if "downloads lists it" '[[ $OUT == *gitviber-check.txt* ]]' "${${OUT:-$ERR}[1,120]}"
  close dl
}

check_agent_audio() {
  page au $URL_A/audio || { record agent_audio FAIL "tab new: $ERR"; return }
  local quiet=$(gb --json tab list | py tab-audio au)
  pass_if "silent page: no audio" '[[ $quiet == false ]]' "audio: $quiet"
  gbx --tab au click '#play'
  until_ok 8 '[[ $(gb --json tab list | py tab-audio au) == true ]]'
  local ok=$?
  pass_if "audio: playing while muted (the default)" '(( ok == 0 ))' "audio $(gb --json tab list | py tab-audio au); page says $(value au window.audio_)"
  gb tab list > $STATE/tab-list.txt 2>&1
  pass_if "tab list prints it" 'grep -q "audio: playing" $STATE/tab-list.txt' "$(grep -m1 'audio' $STATE/tab-list.txt)"
  gbx --tab au click '#stop'
  until_ok 8 '[[ $(gb --json tab list | py tab-audio au) == false ]]'
  ok=$?
  pass_if "stopped: audio goes away" '(( ok == 0 ))' "audio $(gb --json tab list | py tab-audio au)"
  record "sound really muted" INFO "not measurable here: listen once, or trust the probe's _mediaMutedState = 1"
  close au
}

# `tab new --show`: a page for the user, theirs from the start; nothing but `tab list` reaches it.
# The page shows as the active tab, its form's labels in the window; only the user's own Give back
# button hands it to the agent.
check_agent_show() {
  gbx tab new s1 "$URL_A/form?show=s1" --show
  pass_if "tab new --show" '(( RC == 0 ))' "${OUT:-$ERR}"
  (( RC == 0 )) || return
  until_ok 10 'ocr | grep -q Password'
  local ok=$?
  pass_if "shown tab: the active tab, its page on screen" '(( ok == 0 ))' "strip: $(strip | tr -s ' ' | head -c 120)"
  gb tab list > $STATE/tab-list.txt 2>&1
  pass_if "tab list: it's the user's" 'grep -qE "^s1 .*  user  " $STATE/tab-list.txt' "$(grep -m1 '^s1' $STATE/tab-list.txt)"
  pass_if "tab list: private, not the user's logins" 'grep -qE "^s1 .*  private  " $STATE/tab-list.txt' "$(grep -m1 '^s1' $STATE/tab-list.txt)"
  local cmd
  for cmd in "click body" url snapshot "get text body" console "network" screenshot "session save x" "tab release s1"; do
    gbx --tab s1 ${=cmd}
    pass_if "refused on a page opened for the user: $cmd" '(( RC != 0 )) && [[ $ERR == *"opened for the user"* ]]' "rc $RC: ${ERR[1,100]}"
  done
  gbx --tab s1 --reveal url
  pass_if "refused on a page opened for the user: --reveal url" '(( RC != 0 )) && [[ $ERR == *"opened for the user"* ]]' "rc $RC: ${ERR[1,100]}"
  gbx tab new s1b "$URL_A/form?show=s1b" --show
  pass_if "--show again within 3 s is refused" '(( RC != 0 ))' "rc $RC: ${ERR[1,100]}"
  local why
  if why=$(no_window); then
    record "the Give back button hands it to the agent" SKIP "$why"
  else
    click_text 'Give back' 76
    until_ok 5 'gb --tab s1 url >/dev/null 2>&1'
    ok=$?
    pass_if "the Give back button hands it to the agent" '(( ok == 0 ))' "$(gb tab list 2>&1 | grep -m1 '^s1')"
  fi
  close s1 s1b
}

# The user takes an agent's tab by clicking in it: acting commands stop, reading ones run, and
# `tab release` (or Give back) hands it back.
check_ownership() {
  local why
  why=$(no_window) && { record ownership SKIP "$why"; return }
  titled ow $URL_A/form "ownertab" || { record ownership FAIL "tab new: $ERR"; return }
  until_ok 8 '(( $(in_strip ownertab) == 1 ))'
  click_text ownertab 42 && sleep 1 && click_text Name 110 && sleep 1
  gb tab list > $STATE/tab-list.txt 2>&1
  pass_if "a click in an agent's tab makes it the user's" 'grep -qE "^ow .*  user  " $STATE/tab-list.txt' "$(grep -m1 '^ow' $STATE/tab-list.txt)"
  gbx --tab ow click '#send'
  local acted=$RC
  gbx --tab ow get text 'label'
  pass_if "then acting stops, reading runs" '(( acted != 0 && RC == 0 ))' "click rc $acted, get rc $RC"
  gbx tab release ow
  gbx --tab ow fill '#name' back
  pass_if "tab release gives it back" '(( RC == 0 ))' "${OUT:-$ERR}"
  close ow
}

# GitViber hidden (⌘H): an agent's tab keeps drawing, and answers.
check_hidden_window() {
  local why
  why=$(no_window) && { record hidden_window SKIP "$why"; return }
  page hw $URL_A/ || { record hidden_window FAIL "tab new: $ERR"; return }
  # ⌘H to the app: another app can't hide it.
  key $K_H cmd
  until_ok 5 '[[ -z $(win) ]]'
  local hidden=$?
  local a=$(value hw '[window.frames_, performance.now()]')
  sleep 10
  local b=$(value hw '[window.frames_, performance.now()]') v=$(value hw document.visibilityState)
  local f=$(py fps "$a" "$b" 2>/dev/null)
  $GVWIN unhide $APP_PID
  until_ok 5 '[[ -n $(win) ]]'
  pass_if "hidden window: an agent's tab keeps drawing" '(( hidden == 0 && ${f:-0} >= 50 )) && [[ $v == visible ]]' \
    "hidden: $(( hidden == 0 )), ${f:-?} rAF a second, visibilityState $v"
  close hw
}

# ---- the user's side: keys to the app, clicks on its window, its pages served with a log.

beacon() { grep -o 'GET /beacon?[^ ]*' $STATE/www.log | tail -1 }
served() { grep -c "\"GET $1 " $STATE/www.log }

check_user_tab() {
  local why
  why=$(no_window) && { record user_tab SKIP "$why"; return }
  key $K_B cmd,shift
  until_ok 5 'strip | grep -q "New Tab"'
  local ok=$?
  pass_if "⇧⌘B opens a browser tab" '(( ok == 0 ))' "strip: $(strip | tr -s ' ' | head -c 100)"
}

check_user_address() {
  local why
  why=$(no_window) && { record user_address SKIP "$why"; return }
  key $K_L cmd; type_text "127.0.0.1:$SW/"; key $K_RET
  until_ok 8 '(( $(served /) > 0 )) && [[ $(address) == "http://127.0.0.1:$SW/" ]]'
  local ok=$?
  pass_if "address bar: a typed address loads" '(( ok == 0 ))' "address $(address), served / $(served /) times"
  until_ok 5 'strip | grep -q "user check"'
  pass_if "the tab takes the page's title" 'strip | grep -q "user check"' "strip: $(strip | tr -s ' ' | head -c 100)"
}

# Back and forward by their buttons (left of the address), reload by ⌘R.
check_user_nav() {
  local why
  why=$(no_window) && { record user_nav SKIP "$why"; return }
  key $K_L cmd; type_text "127.0.0.1:$SW/page2.html"; key $K_RET
  until_ok 8 '[[ $(address) == *page2.html ]]'
  # Where the layout here (the left panel shut) puts them, left of the address.
  click 18 92
  until_ok 5 '[[ $(address) == "http://127.0.0.1:$SW/" ]]'
  local back=$?
  click 44 92
  until_ok 5 '[[ $(address) == *page2.html ]]'
  local fwd=$?
  pass_if "back and forward buttons" '(( back == 0 && fwd == 0 ))' "back $back, forward $fwd, at $(address)"
  local n=$(served /page2.html)
  key $K_R cmd
  until_ok 5 '(( $(served /page2.html) > n ))'
  pass_if "⌘R reloads" '(( $(served /page2.html) > n ))' "page2 served $n, then $(served /page2.html) times"
  key $K_L cmd; type_text "127.0.0.1:$SW/"; key $K_RET; sleep 1
}

# After a click in the page, a click in the terminal or the address bar takes the keys from it.
check_user_focus() {
  local why
  why=$(no_window) && { record user_focus SKIP "$why"; return }
  click 700 300 && sleep 0.5 && click 600 760 && sleep 0.5
  type_text "echo typed-here"
  until_ok 3 'ocr | awk "\$2 > 640" | grep -q typed-here'
  local term=$?
  key 32 ctrl
  click 700 300 && sleep 0.5 && click 600 92 && sleep 0.5
  type_text "127.0.0.1:$SW/page2.html"; key $K_RET
  until_ok 5 '[[ $(address) == *page2.html ]]'
  local addr=$?
  pass_if "keys follow a click from the page to the terminal and the address bar" '(( term == 0 && addr == 0 ))' \
    "terminal: $(( term == 0 )), address: $(address)"
  key $K_L cmd; type_text "127.0.0.1:$SW/"; key $K_RET; sleep 1
}

check_user_find() {
  local why
  why=$(no_window) && { record user_find SKIP "$why"; return }
  key $K_F cmd; type_text zebra
  until_ok 5 'ocr | awk "\$2 > 100 && \$2 < 135" | grep -q zebra'
  local ok=$?
  pass_if "⌘F: the find bar takes the text" '(( ok == 0 ))' "$(ocr | awk '$2 > 100 && $2 < 135' | head -2 | tr '\n' ' ')"
  record "⌘F: the match is highlighted, ↵ the next" INFO "only pixels show it: checked by eye in the live pass"
  key $K_ESC
}

# Page zoom out and back, read from the page's own report (its CSS px per point).
check_user_zoom() {
  local why
  why=$(no_window) && { record user_zoom SKIP "$why"; return }
  local n=$(grep -c beacon $STATE/www.log)
  # ⌘= by its US code: another layout may make it ⌘− (a Turkish one does), so either counts.
  key $K_EQUAL cmd
  until_ok 5 '(( $(grep -c beacon $STATE/www.log) > n ))'
  local out=$(beacon)
  key $K_0 cmd; sleep 1
  local back=$(beacon)
  pass_if "page zoom changes the page's scale, ⌘0 puts it back" '[[ $out != $back && $back == *dpr=2* ]]' "zoomed: ${out#GET /beacon?}; reset: ${back#GET /beacon?}"
}

check_user_device() {
  local why
  why=$(no_window) && { record user_device SKIP "$why"; return }
  key $K_M cmd,shift
  until_ok 5 '[[ $(beacon) == *dpr=3*touch=5* ]]'
  pass_if "⇧⌘M: a phone's size, pixel ratio and touch" '[[ $(beacon) == *w=402\&h=778\&dpr=3\&touch=5* ]]' "iPhone 16 Pro is 402×778 @3: ${$(beacon)#GET /beacon?}"
  record "device frame: the screen inside the body" INFO "only pixels show it: checked by eye in the live pass"
}

check_user_responsive() {
  local why
  why=$(no_window) && { record user_responsive SKIP "$why"; return }
  click_text 'iPhone 16 Pro' 110 && sleep 1 && click_text '^[0-9 ]* Responsive' 140
  until_ok 5 '[[ $(beacon) == *w=400\&h=800*touch=0* ]]'
  pass_if "Responsive: 400×800, the mouse kept" '[[ $(beacon) == *w=400\&h=800*touch=0* ]]' "${$(beacon)#GET /beacon?}"
  # A width typed, taken on ↵.
  click_text '^[0-9]* [0-9]* [0-9]* [0-9]* 400$' 110 && key $K_A cmd && type_text 500 && key $K_RET
  until_ok 5 '[[ $(beacon) == *w=500\&h=800\&* ]]'
  pass_if "Responsive: a typed width" '[[ $(beacon) == *w=500\&h=800\&* ]]' "${$(beacon)#GET /beacon?}"
  # The right edge's grip, dragged: the page widens by twice the drag, as the frame stays
  # centered. The grip sits 4 to 9 points right of the page, whose half is its CSS width times
  # the zoom (its pixel ratio over the screen's 2), over 2.
  local was=$(beacon) id x y w h
  read -r id x y w h <<<"$(win)"
  local css=${${was#*w=}%%\&*} ratio=${${was#*dpr=}%%\&*}
  local grip=$(( w / 2 + css * ratio / 4 + 6 ))
  $GVWIN drag $APP_PID $(( x + ${grip%.*} )) $(( y + 378 )) $(( x + ${grip%.*} + 60 )) $(( y + 378 )) >/dev/null 2>&1
  sleep 1.5
  local now=$(beacon)
  local fields=$(ocr | awk '$2 >= 110 && $2 <= 135' | grep -o '[0-9]*×[0-9]*' | head -1)
  pass_if "Responsive: a dragged edge resizes the page, whole px" \
    '[[ $now != $was && $now == *h=800\&* && $fields == ${${now#*w=}%%\&*}×800 ]]' "${now#GET /beacon?}; bar says $fields"
  key $K_M cmd,shift; sleep 1
}

check_user_pick_ask() {
  local why
  why=$(no_window) && { record user_pick_ask SKIP "$why"; return }
  rm -f $STATE/claude.log
  key $K_C cmd,shift; sleep 0.5
  click_text 'Pick me' 110 && sleep 1.5
  until_ok 5 'ocr | grep -q "Ask .* about"'
  local asked=$?
  type_text "why so grey"; key $K_RET
  until_ok 20 '[[ -s $STATE/claude.log ]]'
  local got=$(<$STATE/claude.log 2>/dev/null)
  pass_if "pick an element, Ask: the agent gets its selector, styles, picture and the question" \
    '(( asked == 0 )) && [[ $got == *"selector: #b"* && $got == *"styles:"* && $got == *"screenshot:"* && $got == *"My question: why so grey"* ]]' \
    "${${got//$'\n'/ | }[1,160]}"
}

check_user_download() {
  local why
  why=$(no_window) && { record user_download SKIP "$why"; return }
  rm -f $FAKE/Downloads/*(N)
  click_text 'Get the file' 110 20
  until_ok 10 '[[ -f $FAKE/Downloads/gitviber-check.txt ]]'
  local ok=$?
  pass_if "a user tab's download lands in Downloads (the fake home's)" '(( ok == 0 ))' "$(ls $FAKE/Downloads 2>&1)"
  until_ok 5 'ocr | grep -q "Saved to Downloads"'
  pass_if "the tab says where it went" 'ocr | grep -q "Saved to Downloads"' ""
}

check_user_popup() {
  local why
  why=$(no_window) && { record user_popup SKIP "$why"; return }
  local line=$(ocr | grep -m1 'New window')
  local x y w h; read -r x y w h _ <<<$line
  [[ -n $line ]] && click $(( x + w - 25 )) $(( y + h / 2 ))
  until_ok 8 'strip | grep -q "second page"'
  local ok=$?
  pass_if "target=_blank opens a tab of its own" '(( ok == 0 ))' "strip: $(strip | tr -s ' ' | head -c 100)"
}

# A pane the user closes (⌘W in the terminal): its agents' tabs are the user's from then on.
check_pane_close() {
  local why
  why=$(no_window) && { record pane_close SKIP "$why"; return }
  local main=$ENV_FILE before=($STATE/pane-<->.env(N))
  key $K_T cmd
  until_ok 15 '(( ${#${(f)"$(print -l $STATE/pane-<->.env(N))"}} > ${#before} ))'
  local f new=
  for f in $STATE/pane-<->.env(N); do (( ${before[(Ie)$f]} )) || new=$f; done
  [[ -n $new ]] || { record pane_close FAIL "⌘T started no pane"; return }
  ENV_FILE=$new
  titled pk1 $URL_A/ "panetab" || { ENV_FILE=$main; record pane_close FAIL "tab new: $ERR"; return }
  sleep 1
  local hid=$(in_strip panetab)
  gb tab list > $STATE/tab-list.txt 2>&1
  key $K_W cmd
  until_ok 8 '! kill -0 $(<${new%.env}.pid) 2>/dev/null'
  local gone=$?
  gbx tab list
  local after=$OUT$ERR
  ENV_FILE=$main
  pass_if "⌘W closes the pane's shell" '(( gone == 0 ))' "pane ${${new:t}%.env}"
  until_ok 5 '(( $(in_strip panetab) == 1 ))'
  pass_if "its tabs leave its agents: the user's now, in the strip" '[[ $after != *pk1* ]] && (( hid == 0 && $(in_strip panetab) == 1 ))' \
    "before: hidden $(( hid == 0 )), $(grep -m1 '^pk1' $STATE/tab-list.txt | tr -s ' '); after: ${after[1,60]}; in the strip: $(in_strip panetab)"
}

# A pane whose shell ends by itself (exit, or killed): its agents' tabs go the same way.
check_pane_exit() {
  local why
  why=$(no_window) && { record pane_exit SKIP "$why"; return }
  local main=$ENV_FILE before=($STATE/pane-<->.env(N))
  key $K_T cmd
  until_ok 15 '(( ${#${(f)"$(print -l $STATE/pane-<->.env(N))"}} > ${#before} ))'
  local f new=
  for f in $STATE/pane-<->.env(N); do (( ${before[(Ie)$f]} )) || new=$f; done
  [[ -n $new ]] || { record pane_exit FAIL "⌘T started no pane"; return }
  ENV_FILE=$new
  titled px1 $URL_A/ "exittab" || { ENV_FILE=$main; record pane_exit FAIL "tab new: $ERR"; return }
  sleep 1
  local hid=$(in_strip exittab)
  kill -HUP $(<${new%.env}.pid) 2>/dev/null
  until_ok 8 '(( $(in_strip exittab) == 1 ))'
  local shown=$(in_strip exittab)
  ENV_FILE=$main
  pass_if "a shell that ended by itself: its tabs leave its agents, in the strip" '(( hid == 0 && shown == 1 ))' \
    "hidden before: $(( hid == 0 )); in the strip after: $shown"
}

footprint_mb() { footprint -f bytes $APP_PID 2>/dev/null | awk '/Footprint:/ { for (i = 1; i <= NF; i++) if ($i == "Footprint:") { printf "%.0f", $(i + 1) / 1048576; exit } }' }

# 10 agents' tabs opened and closed, twice: the app's footprint comes back near where it started.
check_memory() {
  local i round start mid end
  sleep 3
  start=$(footprint_mb)
  for round in 1 2; do
    for (( i = 1; i <= 10; i++ )); do page mem$i $URL_A/ >/dev/null || break; done
    (( round == 1 )) && mid=$(footprint_mb)
    for (( i = 1; i <= 10; i++ )); do close mem$i; done
    sleep 8
    (( round == 1 )) && local after1=$(footprint_mb)
  done
  end=$(footprint_mb)
  record "memory: 10 agents' tabs open" INFO "${start:-?} MB before, ${mid:-?} MB with 10 tabs open"
  pass_if "memory: 10 agents' tabs opened and closed, twice" '[[ -n $start && -n $end ]] && (( end - start <= 25 && end - after1 <= 5 ))' \
    "${start:-?} MB before, ${after1:-?} MB after round 1, ${end:-?} MB after round 2"
  local why
  why=$(no_window) && { record "memory: 10 of your tabs opened and closed, twice" SKIP "$why"; return }
  start=$(footprint_mb)
  for round in 1 2; do
    for (( i = 1; i <= 10; i++ )); do key $K_B cmd,shift; type_text "127.0.0.1:$SW/?m$i"; key $K_RET; sleep 0.7; done
    sleep 2
    (( round == 1 )) && mid=$(footprint_mb)
    # ⌘W from the page area: the tab's own close, not a terminal pane's.
    for (( i = 1; i <= 10; i++ )); do click 700 300; key $K_W cmd; sleep 0.4; done
    sleep 8
    (( round == 1 )) && after1=$(footprint_mb)
  done
  end=$(footprint_mb)
  record "memory: 10 of your tabs open" INFO "${start:-?} MB before, ${mid:-?} MB with 10 open, strip: $(strip | tr -s ' ' | head -c 60)"
  pass_if "memory: 10 of your tabs opened and closed, twice" '[[ -n $start && -n $end ]] && (( end - start <= 25 && end - after1 <= 5 ))' \
    "${start:-?} MB before, ${after1:-?} MB after round 1, ${end:-?} MB after round 2 (the old app's demo: 65, 82, 81)"
}

check_agent_e2e() {
  record agent_e2e SKIP "a real Claude Code session spends the owner's account: not run unattended"
}

# ---- checks run in a phase of their own: the app launched with that setting.

check_idle_park() {
  page idl1 "$URL_A/?idl1" --idle 1 && page idl2 "$URL_A/?idl2" --idle 1 || { record idle_park FAIL "tab new: $ERR"; return }
  local t0=$SECONDS
  until_ok 100 '[[ $(state idl1) == (sleeping|parked) && $(state idl2) == (sleeping|parked) ]]'
  local ok=$?
  pass_if "--idle 1 sleeps" '(( ok == 0 ))' "after $(( SECONDS - t0 )) s: $(state idl1), $(state idl2)"
  until_ok 15 '[[ $(state idl2) == parked ]]'
  ok=$?
  pass_if "park (no hidden page kept live)" '(( ok == 0 ))' "sleeping tab: $(state idl2)"
  value idl1 1+1 >/dev/null
  pass_if "a parked tab wakes on a command" '[[ $(state idl1) == awake ]]' "after a command: $(state idl1)"
  gbx --tab idl2 wait --load
  sleep 1
  local hello=$(gb --json --tab idl2 network | py net-id fetch /api/hello 200)
  pass_if "network after restore" '[[ -n $hello && $(state idl2) == awake ]]' "$(state idl2), fetch #${hello:-none}"
  close idl1 idl2
}

# Show agents' tabs off: theirs stay out of the strip and keep working; a page shown to the user
# with --show is in it.
check_agent_hide() {
  titled hd1 $URL_A/ "hidetaba" && titled hd2 $URL_A/form "hidetabb" || { record agent_hide FAIL "tab new: $ERR"; return }
  sleep 2
  local shown=$(( $(in_strip hidetaba) + $(in_strip hidetabb) ))
  pass_if "agents' tabs left out of the strip" '(( shown == 0 ))' "in the strip: $shown"
  local v=$(value hd1 document.visibilityState) st=$(state hd1)
  pass_if "hidden tabs still answer" '[[ $v == visible && $st == awake ]]' "hd1 visibilityState $v, state $st"
  gbx tab new hs1 "$URL_A/form?show=hs1" --show
  until_ok 10 'ocr | grep -q Password'
  pass_if "not hidden by Show agents' tabs: a page shown to the user" 'ocr | grep -q Password' "strip: $(strip | tr -s ' ' | head -c 100)"
  close hd1 hd2
}

check_agent_unmuted() {
  page au $URL_A/audio || { record agent_unmuted FAIL "tab new: $ERR"; return }
  gbx --tab au click '#play'
  until_ok 8 '[[ $(gb --json tab list | py tab-audio au) == true ]]'
  pass_if "unmuted: the page plays, audio: playing" '[[ $(value au window.audio_) == playing && $(gb --json tab list | py tab-audio au) == true ]]' "page says $(value au window.audio_)"
  gbx --tab au click '#stop'
  close au
}

# Let agents use the browser off: only `tab new --show` works.
check_control_off() {
  gbx tab new off1 $URL_A/
  pass_if "control off: tab new without --show refuses" '(( RC == 2 )) && [[ $ERR == *--show* ]]' "rc $RC: $ERR"
  gbx url
  pass_if "control off: url refuses" '(( RC == 2 ))' "rc $RC: $ERR"
  gbx eval 1
  pass_if "control off: eval refuses" '(( RC == 2 ))' "rc $RC: $ERR"
  gbx open $URL_A/
  pass_if "control off: open refuses" '(( RC == 2 ))' "rc $RC: $ERR"
  gbx tab new s2 "$URL_A/form?show=s2" --show
  pass_if "control off: tab new --show works" '(( RC == 0 ))' "${OUT:-$ERR}"
  until_ok 10 'ocr | grep -q Password'
  pass_if "control off: the page is the active tab" 'ocr | grep -q Password' "strip: $(strip | tr -s ' ' | head -c 100)"
}

# ---- setup and cleanup

finish() {
  trap - EXIT INT TERM
  say "cleaning up"
  if [[ -n $APP_PID && -s $ENV_FILE ]]; then
    for s in $SESSIONS; do gb session delete $s >/dev/null 2>&1; done
  fi
  quit_app
  for p in $SERVERS; do kill $p 2>/dev/null; done
  [[ -n $APP_TMP && $APP_TMP == /tmp/gvnc.* ]] && rm -rf $APP_TMP
  # The fake home and config hold the run's pages' data, sessions and downloads: gone with it.
  rm -rf $FAKE $CONFIG $REPO $STATE/www $STATE/pane-*(N) $STATE/ocr.png $STATE/gitviber
  print
  py table $RESULTS; local rc=$?
  if (( rc == 0 )) && [[ -z ${KEEP:-} ]]; then rm -rf $STATE; else print "Logs: $STATE"; fi
  exit $rc
}

# Sourced for the functions alone.
[[ -n ${NATIVE_CHECK_LIB:-} ]] && return 0

touch $RESULTS
trap finish EXIT
trap 'exit 130' INT TERM
[[ -x $APP ]] || { record setup FAIL "no app at $APP: command cargo build -p gitviber"; exit 1 }
# A copy of its own (a clone, no space taken): a build mid-run doesn't swap it, nor the CLI.
cp -c $APP $STATE/gitviber && APP=$STATE/gitviber || { record setup FAIL "couldn't copy $APP"; exit 1 }
swiftc -O $HERE/gvwin.swift -o $GVWIN 2>$STATE/swiftc.log || { record setup FAIL "gvwin.swift: $(<$STATE/swiftc.log)"; exit 1 }
# The socket's path must stay short: a folder of the run's own in /tmp.
APP_TMP=$(mktemp -d /tmp/gvnc.XXXXXX) || { record setup FAIL "no folder in /tmp"; exit 1 }

SA=$(py free-port); SB=$(py free-port); SW=$(py free-port)
URL_A=http://127.0.0.1:$SA URL_B=http://127.0.0.1:$SB
python3 -I $HERE/browser-check-site.py $SA > $STATE/site-a.log 2>&1 & SERVERS+=($!)
python3 -I $HERE/browser-check-site.py $SB > $STATE/site-b.log 2>&1 & SERVERS+=($!)
# The user's pages: each reports its viewport to the log, which is how the checks read a user tab.
mkdir -p $STATE/www
print -r -- "<!doctype html><html><head><title>user check</title></head><body style=\"font: 16px sans-serif\">
<h1>Zebra page</h1><p>zebra one, zebra two, zebra three</p><p id=\"w\"></p>
<button id=\"b\" style=\"padding: 20px\">Pick me</button>
<p><a href=\"$URL_A/file\">Get the file</a> <a href=\"/page2.html\" target=\"_blank\">New window</a></p>
<script>
const say = () => { document.getElementById('w').textContent = 'width ' + innerWidth;
  fetch('/beacon?w=' + innerWidth + '&h=' + innerHeight + '&dpr=' + devicePixelRatio + '&touch=' + navigator.maxTouchPoints).catch(() => {}) };
addEventListener('resize', say); say();
</script></body></html>" > $STATE/www/index.html
sed 's/Zebra page/Second page/; s/<title>user check/<title>second page/' $STATE/www/index.html > $STATE/www/page2.html
( cd $STATE/www && exec python3 -I -m http.server $SW --bind 127.0.0.1 ) > $STATE/www.log 2>&1 & SERVERS+=($!)

# The fake home: a git identity (else a dialog asks), the PATH a Mac has, and a shell that writes
# its pane's environment where only this user can read it, with the stand-in `claude` first.
mkdir -p $FAKE/Downloads
print -r -- $'[user]\n\tname = Live Check\n\temail = live@check.invalid' > $FAKE/.gitconfig
print -r -- 'export PATH=/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin' > $FAKE/.zprofile
print -r -- "if [[ -n \$GITVIBER_PTY ]]; then
  (umask 077; env | grep '^GITVIBER_' > '$STATE/pane-'\$GITVIBER_PTY.env; echo \$\$ > '$STATE/pane-'\$GITVIBER_PTY.pid)
fi
export PATH='$STATE/bin':\$PATH
PS1='%1~ %# '" > $FAKE/.zshrc
print -r -- "#!/bin/sh
# Stands in for Claude Code: what it was asked, then gone.
printf 'argv: %s\n' \"\$*\" >> '$STATE/claude.log'" > $STATE/bin/claude
chmod +x $STATE/bin/claude

say "cloning the repository as the project"
command git clone -q --local $ROOT $REPO || { record setup FAIL "git clone --local $ROOT failed"; exit 1 }

RUN=(${@:-$CHECKS})
for c in $RUN; do (( ${+functions[check_$c]} )) || { record $c FAIL "no such check"; RUN=(${RUN:#$c}) }; done
for phase in main park hidden unmuted off; do
  todo=()
  for c in $RUN; do [[ ${PHASE[$c]:-main} == $phase ]] && todo+=($c); done
  (( $#todo )) || continue
  say "phase $phase: launching the app"
  if ! launch_app $phase; then
    for c in $todo; do record $c FAIL "the app didn't come up ($phase): see app-$phase.log"; done
    continue
  fi
  gb help > $STATE/help.txt 2>&1 || { record setup FAIL "gitviber browser help: $(<$STATE/help.txt)"; continue }
  for c in $todo; do
    lack=$(py missing $STATE/help.txt "${(@s:|:)NEEDS[$c]}")
    if [[ -n $lack ]]; then record $c SKIP "help has no $lack"; continue; fi
    say $c
    rows=$(wc -l < $RESULTS)
    check_$c
    (( $(wc -l < $RESULTS) > rows )) || record $c FAIL "recorded nothing"
  done
done
