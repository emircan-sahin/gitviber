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

# zsh reads a script as it runs it: an edit mid-run would land it on another line. So a run goes
# on in a copy of its own, in its temp folder; BROWSER_CHECK_STATE says it's the copy.
COPIED=(browser-check.sh browser-check.py browser-check-site.py prelude.js)
if [[ -z ${BROWSER_CHECK_LIB:-}${BROWSER_CHECK_STATE:-} ]]; then
  BROWSER_CHECK_STATE=$(mktemp -d "${${TMPDIR:-/tmp}%/}/gitviber-live-check.XXXXXX") || exit 1
  mkdir $BROWSER_CHECK_STATE/bin && cp $HERE/$^COPIED $BROWSER_CHECK_STATE/bin/ ||
    { rm -rf $BROWSER_CHECK_STATE; exit 1 }
  BROWSER_CHECK_STATE=$BROWSER_CHECK_STATE exec zsh $BROWSER_CHECK_STATE/bin/browser-check.sh "$@"
fi

py() { python3 -I $HERE/browser-check.py "$@" }
# To stdout and stderr both, unbuffered (print writes at once), for whoever tails either.
say() { print -r -- "· $*"; print -ru2 -- "· $*" }

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

# Built once cargo says it runs the app. tauri runs cargo with --color always, so the line is
# ESC[1mESC[32m     RunningESC[0m `…/debug/gitviber`: matched across the escape codes.
built() { grep -aqE 'Running.*/debug/gitviber`' $1 }

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
  for f in $DOWNLOADS; do [[ -f $f && $f == */gitviber-check.txt ]] && rm -f $f; done
  rm -f $ENV_FILE
  print
  py table $RESULTS; local rc=$?
  if (( rc == 0 )) && [[ -z ${KEEP:-} ]]; then rm -rf $STATE; else print "Logs: $STATE"; fi
  exit $rc
}

# ---- the checks: each a function check_<name>, recording one row or more. NEEDS names what of
# `gitviber browser help` it uses; a check whose needs aren't there is skipped, so a branch can
# add its own here before the commands land.

CHECKS=(visibility fps isolation sessions unknown_tab device screenshot network sse idle_park tab_close form
  stale_ref covered smooth_scroll fill_honest change_line find_get screenshot_1x untrusted secret frames
  trusted hover keys clipboard_safe ownership popup download memory)
typeset -A NEEDS=(
  visibility "eval" fps "eval" isolation "eval" sessions "session save|session load|--session"
  unknown_tab "--tab" device "device <name|WxH|off>" screenshot "screenshot" network "network <id>"
  sse "network" idle_park "--idle" tab_close "tab close" form "fill|select|click|wait"
  memory "tab list"
  # Acting as a person could, stable refs, the change line, reading in parts, honest output.
  stale_ref "fails as stale" covered "covered by" smooth_scroll "covered by" fill_honest "check | uncheck"
  change_line "→ no visible change" find_get "find <|get styles|-o <file>" screenshot_1x "--clip|--scale|--refs"
  untrusted "--reveal" secret "--secret" frames "iframe"
  # Native input.
  trusted "--synthetic" hover "CSS :hover applies" keys "key down <key>|--hold" clipboard_safe "--synthetic"
  ownership "tab release"
  # Popups and downloads.
  popup "window.open" download "wait --download|downloads"
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

# ---- checks for commands newer builds add; NEEDS skips each where help doesn't have it.

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
  pass_if "cross-origin frame's hint" '[[ $snap == *"127.0.0.1:$SB"* ]]' "${${(M)${(f)snap}:#*$SB*}:-none}"
  close fr
}

check_trusted() {
  page tr $URL_A/native || { record trusted FAIL "tab new: $ERR"; return }
  gbx --tab tr click '#trust'
  local t=$(value tr document.title)
  pass_if "click is trusted" '[[ $t == true ]]' "isTrusted: $t $ERR"
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

check_clipboard_safe() {
  page cb $URL_A/native || { record clipboard_safe FAIL "tab new: $ERR"; return }
  # Read only: the check never writes the user's clipboard, so a failure leaves the page's text there.
  local before=$(pbpaste 2>/dev/null)
  gbx --tab cb click '#copy'
  sleep 1
  local after=$(pbpaste 2>/dev/null)
  local note="unchanged"
  [[ $after == "$before" ]] || note="your clipboard now holds the page's text"
  pass_if "page can't write your clipboard" '[[ $after == "$before" ]]' "$note"
  close cb
}

check_ownership() {
  record ownership SKIP "manual: a tab is taken by the user's own click or key"
}

check_popup() {
  page pp $URL_A/act || { record popup FAIL "tab new: $ERR"; return }
  gbx --tab pp click '#pop'
  local name=$(py popup-name <<<$OUT)
  pass_if "popup opens as a tab" '[[ -n $name ]]' "${${(M)${(f)OUT}:#*popup*}:-$OUT$ERR}"
  until_ok 5 '[[ $(value pp window.msg_) == hi-from-popup ]]'
  local ok=$?
  pass_if "popup posts to its opener" '(( ok == 0 ))' "window.msg_: $(value pp window.msg_)"
  [[ -n $name ]] && gbx --tab $name click '#closeme'
  until_ok 5 '[[ $(state ${name:-none}) == missing ]]'
  ok=$?
  pass_if "window.close() closes its tab" '(( ok == 0 )) && [[ -n $name ]]' "$name: $(state ${name:-none})"
  close pp $name
}

check_download() {
  page dl $URL_A/act || { record download FAIL "tab new: $ERR"; return }
  gbx --tab dl click '#dl'
  gbx --tab dl wait --download
  local path=$(py file-path gitviber-check.txt <<<$OUT$'\n'$ERR)
  [[ -n $path ]] && DOWNLOADS+=($path)
  pass_if "wait --download" '[[ -f $path && $OUT == *B* ]]' "${OUT:-$ERR}"
  gbx --tab dl downloads
  pass_if "downloads lists it" '[[ $OUT == *gitviber-check.txt* ]]' "${${OUT:-$ERR}[1,120]}"
  close dl
}

# ---- setup

# Sourced by browser-check-test.zsh for the functions alone.
[[ -n ${BROWSER_CHECK_LIB:-} ]] && return 0

STATE=$BROWSER_CHECK_STATE
[[ -d ${1:-} ]] || { sed -n '4p' $0; rm -rf $STATE; exit 2; }
WT=${1:a}; shift
PORT=1437
if [[ ${1:-} == <-> ]]; then PORT=$1; shift; fi
ONLY=($@)
(( PORT != 1420 )) || { print -u2 "Not 1420: that port's storage is the user's own dev run."; rm -rf $STATE; exit 2; }
TARGET=$WT/src-tauri/target
# browser-check-test.zsh: that the run is the copy, then nothing else.
if [[ -n ${BROWSER_CHECK_SELF_TEST:-} ]]; then
  print -r -- "running from $HERE: ${(j: :)${(of)"$(ls $HERE)"}}"; rm -rf $STATE; exit 0
fi
ENV_FILE=$STATE/env
RESULTS=$STATE/results.tsv
REPO=$STATE/repo
touch $RESULTS
typeset -a PICS SESSIONS SERVERS DOWNLOADS
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
until_ok 1200 'built $STATE/dev.log || ! kill -0 $DEV_PID 2>/dev/null'
kill -0 $DEV_PID 2>/dev/null && built $STATE/dev.log || { record setup FAIL "the dev app didn't start: see dev.log"; exit 1 }
say "built; waiting for the app's dev bridge"
# This worktree's app by its path, as the installed one has the same bundle id.
until_ok 120 '{ APP_PID=$(pgrep -f "^$TARGET/debug/(gitviber|GitViber Dev\\.app/)" | head -1)
  [[ -n $APP_PID ]] && js "return 1" >/dev/null 2>&1 } || ! kill -0 $DEV_PID 2>/dev/null'
[[ -n $APP_PID ]] && js "return 1" >/dev/null 2>&1 ||
  { record setup FAIL "no app from $TARGET answered on the dev bridge"; exit 1 }
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
