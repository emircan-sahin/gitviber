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

# ---- agent_e2e: a real Claude Code session in the pane, on the user's account (AGENT_E2E=1).

# The skill files a dev build with GITVIBER_DEV_SKILL=1 may write. A dev build's marker carries
# the same version as a release, so it can't tell its own file from the user's: each is put
# back exactly as it was found, and one that wasn't there goes if it's GitViber's.
SKILL_FILES=("${SKILL_HOME:-$HOME}/.claude/skills/gitviber-browser/SKILL.md" "${SKILL_HOME:-$HOME}/.agents/skills/gitviber-browser/SKILL.md")
save_skills() {
  local i f
  for (( i = 1; i <= $#SKILL_FILES; i++ )); do
    f=$SKILL_FILES[i]
    if [[ -e $f || -L $f ]]; then cp -pP $f $STATE/skill-$i.saved; else : > $STATE/skill-$i.absent; fi
  done
}
restore_skills() {
  local i f
  for (( i = 1; i <= $#SKILL_FILES; i++ )); do
    f=$SKILL_FILES[i]
    if [[ -e $STATE/skill-$i.saved || -L $STATE/skill-$i.saved ]]; then
      mkdir -p ${f:h} && rm -f $f && cp -pP $STATE/skill-$i.saved $f
    elif [[ -e $STATE/skill-$i.absent && -f $f ]] && py skill-ours $f; then
      rm -f $f && rmdir ${f:h} 2>/dev/null
    fi
  done
}

# A line typed into pane `id`'s shell, as if at its prompt.
type_line() { js "await window.__TAURI_INTERNALS__.invoke('pty_write', { id: $1, data: $(py json-str "$2"$'\r'), binary: false }); return 'typed'" >/dev/null }

check_agent_e2e() {
  if [[ ${AGENT_E2E:-} != 1 ]]; then
    record agent_e2e SKIP "opt-in: AGENT_E2E=1 runs a Claude Code session on your account"
    return
  fi
  local id=${pty%%,*}
  type_line $id "command -v claude > /dev/null 2>&1; echo \$? > ${(qq)STATE}/claude.rc"
  until_ok 20 '[[ -s $STATE/claude.rc ]]' || { record agent_e2e FAIL "the pane's shell never answered"; return }
  [[ $(<$STATE/claude.rc) == 0 ]] || { record agent_e2e SKIP "no claude on the pane's PATH"; return }
  local skill="not written"
  py skill-ours $SKILL_FILES[1] && skill="written"
  local run=$(py nonce)
  local task="Using the browser available in this terminal, open $URL_A/form?run=$run, fill the name field with Ada, choose green, submit, and tell me the HTTP status of the request the form sent. Answer with just the number."
  # Only gitviber browser runs unasked; the transcript stays the user's alone and goes after.
  type_line $id "(umask 077; claude -p ${(qq)task} --output-format stream-json --verbose --allowedTools 'Bash(gitviber browser:*)' > ${(qq)STATE}/agent.json 2>&1; echo \$? > ${(qq)STATE}/agent.done)"
  say "agent_e2e: a Claude Code session works in the pane (5 min at most)"
  if ! until_ok 300 '[[ -s $STATE/agent.done ]]'; then
    record agent_e2e FAIL "no answer in 5 min (skill $skill)"
  else
    local note ok exited=$(<$STATE/agent.done)
    note=$(py agent $STATE/agent.json 200); ok=$?
    pass_if "agent: answer and browser" '(( ok == 0 ))' "$note; skill $skill; claude exit $exited"
    note=$(py form-submit $STATE/site-a.log $run); ok=$?
    pass_if "agent: the submit came from its tab" '(( ok == 0 ))' "$note"
  fi
  rm -f $STATE/agent.json $STATE/agent.done
}

# ---- setup steps, as functions so the tests can drive them against a stand-in page.

# The terminal panel: open, closed (its Show button there) or none (no button yet).
terminal_state() {
  js 'return $("[aria-label^=\"Hide terminal\"]") ? "open" : $("[aria-label^=\"Show terminal\"]") ? "closed" : "none"' 2>/dev/null
}

# The repo's terminal open: its button waited for, and if it never comes, once more after a reload.
open_terminal() {
  local round
  for round in 1 2; do
    if until_ok 45 '[[ $(terminal_state) == (open|closed) ]]'; then
      [[ $(terminal_state) == open ]] || js 'press($("[aria-label^=\"Show terminal\"]")); return true' >/dev/null
      return 0
    fi
    (( round == 1 )) || break
    say "no terminal button after 45 s: reloading the page"
    js 'setTimeout(() => location.reload(), 100); return true' >/dev/null 2>&1
    sleep 2
    until_ok 30 '[[ $(js "return document.readyState" 2>/dev/null) == complete ]]'
    invoke open_dropped "{ paths: [\"$REPO\"] }" >/dev/null 2>&1
  done
  return 1
}

# The pane's shell writes its environment where only this user can read it. Sent again every few
# seconds: a write that comes while the user's profile still runs can be lost before the prompt
# reads input. ACCEPTED: the pty ids that took the last one.
send_env() {
  js "const data = \"(umask 077; env | grep '^GITVIBER_' > '$ENV_FILE')\\r\";
    const took = [];
    for (let id = 0; id <= 5; id++) {
      try { await window.__TAURI_INTERNALS__.invoke('pty_write', { id, data, binary: false }); took.push(id); } catch {}
    }
    return took.join(',') || 'none';" 2>/dev/null
}
env_came() { grep -q "^GITVIBER_BROWSER_SOCKET=" $ENV_FILE 2>/dev/null }
wait_env() {
  local i
  ACCEPTED=none
  for (( i = 0; i < 15; i++ )); do
    ACCEPTED=$(send_env) || ACCEPTED=none
    until_ok 2 env_came && return 0
  done
  return 1
}

# Why setup stopped, with what the page shows.
setup_note() {
  local xterms=$(js 'return $$(".xterm").length' 2>/dev/null) title=$(js 'return document.title' 2>/dev/null)
  print -r -- "$1 (xterm elements: ${xterms:-?}, pty ids that took the write: ${ACCEPTED:-none}, title: ${title:-?})"
}

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
  # After the app is gone, so it can't write the skill again.
  [[ ${AGENT_E2E:-} == 1 ]] && restore_skills
  rm -f $STATE/agent.json $STATE/agent.done
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

CHECKS=(visibility fps isolation sessions session_idb unknown_tab device screenshot network sse idle_park tab_close form
  stale_ref covered smooth_scroll fill_honest change_line find_get screenshot_1x screenshot_full touch untrusted secret frames
  trusted hover keys background_keys clipboard_safe clipboard_bypass clipboard_tab ownership popup download agent_hide agent_audio agent_show agent_e2e memory)
typeset -A NEEDS=(
  visibility "eval" fps "eval" isolation "eval" sessions "session save|session load|--session"
  session_idb "IndexedDB"
  unknown_tab "--tab" device "device <name|WxH|off>" screenshot "screenshot" network "network <id>"
  sse "network" idle_park "--idle" tab_close "tab close" form "fill|select|click|wait"
  memory "tab list"
  # Acting as a person could, stable refs, the change line, reading in parts, honest output.
  stale_ref "fails as stale" covered "covered by" smooth_scroll "covered by" fill_honest "check | uncheck"
  change_line "→ no DOM change" find_get "find <|get styles|-o <file>" screenshot_1x "--clip|--scale|--refs"
  screenshot_full "--full" touch "touchstart"
  untrusted "--reveal" secret "--secret" frames "frame of the page's origin"
  # Native input.
  trusted "--synthetic" hover "CSS :hover applies" keys "key down <key>|--hold"
  background_keys "key down <key>|--idle|canvas repainted" clipboard_safe "--synthetic"
  clipboard_tab "frame of the page's origin|--synthetic"
  clipboard_bypass "--synthetic"
  ownership "tab release"
  # Popups and downloads.
  popup "named for its opener" download "--download|downloads [--clear]"
  # Agents' tabs shown or hidden, and muted.
  agent_hide "tab list" agent_audio "audio: playing" agent_show "--show"
  # Opt-in (AGENT_E2E=1): a Claude Code session on the user's account.
  agent_e2e "fill|select|network"
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

# A login kept in IndexedDB (as Firebase Auth does) comes back in another tab, with the store's
# key path, index and kinds of value; a database with an ImageData (a kind of
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
  # Addresses apart, so the strip's titles tell the two apart.
  page idl1 "$URL_A/?idl1" --idle 1 && page idl2 "$URL_A/?idl2" --idle 1 || { record idle_park FAIL "tab new: $ERR"; return }
  local t0=$SECONDS
  until_ok 100 '[[ $(state idl1) == sleeping && $(state idl2) == sleeping ]]'
  local ok=$?
  pass_if "--idle 1 sleeps" '(( ok == 0 ))' "after $(( SECONDS - t0 )) s: $(state idl1), $(state idl2)"
  value idl1 1+1 >/dev/null
  local s=$(state idl1)
  pass_if "a sleeping tab wakes" '[[ $s == awake ]]' "after a command: $s"
  local said=$(configure 0 2>&1)
  until_ok 15 '[[ $(state idl2) == parked ]]'
  ok=$?
  # A tab on show never parks: the strip's active tab tells whether idl2 was one.
  local shown=$(js 'const t = $("[role=tab][aria-selected=true]"); return t ? t.title.split("\n")[0] : "none"' 2>&1)
  pass_if "park (liveHidden 0)" '(( ok == 0 ))' "sleeping tab: $(state idl2); configure: $said; active tab: $shown"
  # Put back as the settings have it: the frontend only sends it again when a setting changes.
  said=$(configure 2>&1)
  [[ $said == configured ]] || record "configure put back" FAIL "$said"
  gbx --tab idl2 wait --load
  sleep 1
  local hello=$(gb --json --tab idl2 network | py net-id fetch /api/hello 200)
  pass_if "network after restore" '[[ -n $hello && $(state idl2) == awake ]]' "$(state idl2), fetch #${hello:-none}"
  close idl1 idl2
}

# browser_configure with the app's own settings, and `live_hidden` in place of its cap when given.
# A build with "Agent tabs drawing at once" needs agentDrawn; one without it is asked again
# without, should it refuse the key.
configure() {
  js "const s = JSON.parse(localStorage.getItem('gitviber.settings.v2') || '{}');
    const args = { liveHidden: ${1:-s.browserLiveHidden ?? 2}, parkAfterMin: s.browserParkAfterMin ?? 10,
      console: s.browserConsole ?? true, agentControl: true, agentDrawn: s.browserAgentDrawn ?? 12,
      agentMuted: s.browserMuteAgentTabs ?? true };
    const call = () => window.__TAURI_INTERNALS__.invoke('browser_configure', args);
    try { await call(); } catch (e) {
      if (!String(e).includes('agentDrawn')) throw e;
      delete args.agentDrawn;
      await call();
    }
    return 'configured';"
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

check_screenshot_full() {
  page full $URL_A/tall || { record screenshot_full FAIL "tab new: $ERR"; return }
  gbx --tab full scroll down
  local before=$(value full scrollY) w=$(value full innerWidth) h=$(value full innerHeight)
  gbx --tab full screenshot $STATE/tall.png --full
  # One picture is tall.png, several tall-1.png and on. Its pixels a CSS px: 1, less in a viewport past 2000 px.
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

# What a page tells of its input: a phone's has touch, a WxH size keeps the mouse.
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
# trusted, with the page focused and visible, and a held key reaches the page's frames. Another
# tab holding a key doesn't sleep with its idle time up, while one that holds none does.
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
  gbx --tab bk1 key up ArrowRight; local up=$RC
  gbx --tab bk2 key up ArrowRight; local up2=$RC
  held=$(value bk1 'held_()')$(value bk2 'held_()')
  pass_if "key up on each" '(( up == 0 && up2 == 0 )) && [[ -z $held ]]' "$up $up2 held: ${held:-none}"
  gbx --tab bk2 click '#fire'
  pass_if "nothing moving: no DOM change" '[[ $OUT == *"→ no DOM change"* ]]' "${${(M)${(f)OUT}:#→*}:-$OUT$ERR}"
  # bk3 holds its key and bk4 holds its own, both idle for a minute; bk4 lets go first.
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

check_clipboard_safe() {
  page cb $URL_A/native || { record clipboard_safe FAIL "tab new: $ERR"; return }
  local page_text=gitviber-check-copied
  local before=$(pbpaste 2>/dev/null)
  gbx --tab cb click '#copy'
  sleep 1
  local after=$(pbpaste 2>/dev/null)
  # Never any of the user's own text: a password may be there. Only whose it is and how long.
  local note="clipboard holds: the user's text, ${#after} characters"
  [[ -n $after ]] || note="clipboard holds: no text"
  if [[ $after == $page_text ]]; then
    note="clipboard holds: the page's text"
    # The page's text got through: what the user had there goes back (text only; none to put back
    # when it held something else).
    if [[ -n $before && $before != $page_text ]] && print -rn -- "$before" | pbcopy; then
      note+="; put back what it held before"
    else
      note+="; nothing to put back"
    fi
    record "page can't write your clipboard" FAIL "$note"
  else
    record "page can't write your clipboard" PASS "$note"
  fi
  close cb
}

# The tab's clipboard, from the page's own button (navigator.clipboard) in the page and in a frame
# of its origin, read back by the page and pasted with the agent's Meta+v; a frame of another
# origin has a copy of its own, which neither replaces the page's nor is read by it. None of it
# reaches the user's clipboard (clipboard_safe asks that); a Copy button of theirs after they take
# the tab is manual.
check_clipboard_tab() {
  page ct "$URL_A/clipframes?other=$SB" || { record clipboard_tab FAIL "tab new: $ERR"; return }
  gbx --tab ct snapshot -i
  local same=$(py snapshot-ref '"Copy same"' <<<$OUT)
  # A frame of another origin isn't in the snapshot: its button is clicked by where it is.
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
  # The guard is in every kind of frame a page can make, and in a popup: WebKit puts it into each
  # document at its start (a frame without it would copy to the user's clipboard unguarded).
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
  close $popup
  record "tab clipboard: the user's own Copy button" SKIP "manual: click in the tab, then its page's Copy button"
  close ct
}

# A page that tries to get past the shim (clipbypass in browser-check-site.py) and copy inside the
# agent's click: the user's clipboard must still be what it was. WebKit itself allows a copy during
# a click, so this is the shim's alone to hold.
check_clipboard_bypass() {
  page cx $URL_A/clipbypass || { record clipboard_bypass FAIL "tab new: $ERR"; return }
  local before=$(pbpaste 2>/dev/null)
  gbx --tab cx click '#bypass'
  sleep 1
  local after=$(pbpaste 2>/dev/null) res=$(value cx window.res)
  local note="the page's copy returned: $res; clipboard holds: the user's text, ${#after} characters"
  if [[ $after == gitviber-bypass-[1-4] ]]; then
    note="the page's copy returned: $res; clipboard holds: the page's text ($after)"
    if [[ -n $before && $before != gitviber-bypass-[1-4] ]] && print -rn -- "$before" | pbcopy; then
      note+="; put back what it held before"
    fi
    record "a page past the shim can't write your clipboard" FAIL "$note"
  else
    record "a page past the shim can't write your clipboard" PASS "$note"
  fi
  close cx
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
  # The close is told on the reply of the command that set it off, or the pane's next one.
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
  # Downloaded gitviber-check.txt (24 B, text/plain): <path>
  local -a got=("${(@ps:\t:)$(py download <<<$OUT)}")
  # Not `path`: zsh ties that one to PATH.
  local file=${got[4]:-}
  [[ -n $file ]] && DOWNLOADS+=($file)
  pass_if "wait --download" '[[ ${got[1]:-} == gitviber-check.txt && -f $file ]]' "${OUT:-$ERR}"
  gbx --tab dl downloads
  pass_if "downloads lists it" '[[ $OUT == *gitviber-check.txt* ]]' "${${OUT:-$ERR}[1,120]}"
  close dl
}

# Settings the way the app's own switches change them: the settings module the page runs, in dev.
set_setting() { js "const m = await import('/src/lib/settings.ts'); m.updateSettings($1); return 'set'" >/dev/null }

check_agent_hide() {
  page hd1 $URL_A/ && page hd2 $URL_A/form || { record agent_hide FAIL "tab new: $ERR"; return }
  until_ok 5 '(( $(agent_tabs) >= 2 ))'
  local shown=$(agent_tabs)
  set_setting '{ browserShowAgentTabs: false }'
  until_ok 5 '(( $(agent_tabs) == 0 ))'
  local ok=$? hidden=$(agent_tabs)
  pass_if "agents' tabs hidden from the strip" '(( ok == 0 && shown >= 2 ))' "in the strip: $shown, then $hidden"
  local v=$(value hd1 document.visibilityState) st=$(state hd1)
  pass_if "hidden tabs still answer" '[[ $v == visible && $st != missing ]]' "hd1 visibilityState $v, state $st"
  gbx tab close hd2
  sleep 1
  set_setting '{ browserShowAgentTabs: true }'
  until_ok 5 '(( $(agent_tabs) == 1 ))'
  ok=$?
  pass_if "shown again, minus the one closed while hidden" '(( ok == 0 ))' "in the strip: $(agent_tabs)"
  close hd1 hd2
}

check_agent_audio() {
  page au $URL_A/audio || { record agent_audio FAIL "tab new: $ERR"; return }
  gb --json tab list | py tab-audio au | read -r quiet
  pass_if "silent page: no audio" '[[ $quiet == false ]]' "audio: $quiet"
  gbx --tab au click '#play'
  until_ok 8 '[[ $(gb --json tab list | py tab-audio au) == true ]]'
  local ok=$?
  pass_if "audio: playing while muted (the default)" '(( ok == 0 ))' "audio $(gb --json tab list | py tab-audio au); page says $(value au window.audio_)"
  gb tab list > $STATE/tab-list.txt 2>&1
  pass_if "tab list prints it" 'grep -q "audio: playing" $STATE/tab-list.txt' "$(grep -m1 'audio' $STATE/tab-list.txt)"
  set_setting '{ browserMuteAgentTabs: false }'; sleep 1
  pass_if "unmuting leaves the page playing" '[[ $(value au window.audio_) == playing && $(gb --json tab list | py tab-audio au) == true ]]' "page says $(value au window.audio_)"
  set_setting '{ browserMuteAgentTabs: true }'; sleep 1
  pass_if "muting again leaves it playing" '[[ $(value au window.audio_) == playing ]]' "page says $(value au window.audio_)"
  gbx --tab au click '#stop'
  until_ok 8 '[[ $(gb --json tab list | py tab-audio au) == false ]]'
  ok=$?
  pass_if "stopped: audio goes away" '(( ok == 0 ))' "audio $(gb --json tab list | py tab-audio au)"
  record "sound really muted" INFO "not measurable here: listen once, or trust the probe's _mediaMutedState = 1"
  close au
}

# The strip's tab whose address has $1 (no spaces): none, or active or inactive, with +bot when it
# carries the agent mark.
strip_tab() {
  js 'const t = $$("[role=tab]").find((e) => e.title.includes("'$1'")); return !t ? "none" : (t.getAttribute("aria-selected") === "true" ? "active" : "inactive") + (t.querySelector("[aria-label=\"Opened by an agent\"]") ? "+bot" : "")'
}

# `tab new --show`: a page for the user, theirs from the start; and the only command that works
# with agent control off. Control is put back on after, whatever happened.
check_agent_show() {
  local needle="form?show=s1" needle2="form?show=s2" url=$URL_A/form?show=s1 marked=$(agent_tabs)
  {
    gbx tab new s1 $url --show
    pass_if "tab new --show" '(( RC == 0 ))' "${OUT:-$ERR}"
    (( RC == 0 )) || return
    until_ok 8 '[[ $(strip_tab $needle) == active ]]'
    pass_if "shown tab: in the strip, active, no agent mark" '[[ $(strip_tab $needle) == active ]] && (( $(agent_tabs) == marked ))' "strip: $(strip_tab $needle); agent marks $marked, then $(agent_tabs)"
    gb tab list > $STATE/tab-list.txt 2>&1
    pass_if "tab list: it's the user's" 'grep -qE "^s1 .*  user  " $STATE/tab-list.txt' "$(grep -m1 '^s1' $STATE/tab-list.txt)"
    # Its storage apart from the user's can't be read back here: eval acts, so it's refused on a tab the user has.
    pass_if "tab list: private, not the user's logins" 'grep -qE "^s1 .*  private  " $STATE/tab-list.txt' "$(grep -m1 '^s1' $STATE/tab-list.txt)"
    # Strictly the user's: nothing reaches it but `tab list`, reads included.
    local cmd
    for cmd in "click body" url snapshot "get text body" console "network" screenshot "session save x" "tab release s1"; do
      gbx --tab s1 ${=cmd}
      pass_if "refused on a page opened for the user: $cmd" '(( RC != 0 )) && [[ $ERR == *"opened for the user"* ]]' "rc $RC: ${ERR[1,100]}"
    done
    gbx --tab s1 --reveal url
    pass_if "refused on a page opened for the user: --reveal url" '(( RC != 0 )) && [[ $ERR == *"opened for the user"* ]]' "rc $RC: ${ERR[1,100]}"
    gbx tab list
    pass_if "tab list still lists it" '(( RC == 0 )) && [[ $OUT == *s1* ]]' "${OUT[1,100]}"
    set_setting '{ browserShowAgentTabs: false }'; sleep 1
    pass_if "not hidden by Show agents' tabs" '[[ $(strip_tab $needle) == active ]]' "strip: $(strip_tab $needle)"
    set_setting '{ browserShowAgentTabs: true }'

    set_setting '{ browserAgentControl: false }'
    until_ok 8 '! gb tab list >/dev/null 2>&1'
    gbx tab new off1 $URL_A/
    pass_if "control off: tab new without --show refuses" '(( RC == 2 )) && [[ $ERR == *--show* ]]' "rc $RC: $ERR"
    gbx --tab s1 url
    pass_if "control off: url refuses" '(( RC == 2 ))' "rc $RC: $ERR"
    gbx --tab s1 eval 1
    pass_if "control off: eval refuses" '(( RC == 2 ))' "rc $RC: $ERR"
    gbx open $URL_A/
    pass_if "control off: open refuses" '(( RC == 2 ))' "rc $RC: $ERR"
    sleep 3  # pages are shown 3 s apart at least
    gbx tab new s2 "$URL_A/form?show=s2" --show
    pass_if "control off: tab new --show works" '(( RC == 0 ))' "${OUT:-$ERR}"
    until_ok 8 '[[ $(strip_tab $needle2) == active ]]'
    pass_if "control off: the page is the active tab" '[[ $(strip_tab $needle2) == active ]]' "strip: $(strip_tab $needle2)"
  } always {
    set_setting '{ browserAgentControl: true }'
    until_ok 8 'gb tab list >/dev/null 2>&1'
  }
  # Only the user's own button gives a page back: pressed here as the dev bridge can, on the tab.
  # Then it is its agents' again: the mark, and theirs to close.
  js 'press($$("[role=tab]").find((e) => e.title.includes("'$needle'"))); await sleep(500); return press("Give back to agent")' >/dev/null
  until_ok 5 '[[ $(strip_tab $needle) == *+bot ]]'
  pass_if "the Give back button hands it to the agent" '[[ $(strip_tab $needle) == *+bot ]]' "strip: $(strip_tab $needle)"
  gbx --tab s1 url
  pass_if "given back, its agent reads it again" '(( RC == 0 ))' "${OUT:-$ERR}"
  js 'press($$("[role=tab]").find((e) => e.title.includes("'$needle2'"))); await sleep(500); return press("Give back to agent")' >/dev/null
  until_ok 5 '[[ $(strip_tab $needle2) == *+bot ]]'
  close s1 s2
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
# agent_e2e needs the skill written, which a dev build does only when told.
if [[ ${AGENT_E2E:-} == 1 ]]; then
  save_skills
  ( cd $WT && GITVIBER_DEV_SKILL=1 CARGO_TARGET_DIR=$TARGET exec pnpm tauri dev --port $PORT ) > $STATE/dev.log 2>&1 &
else
  ( cd $WT && CARGO_TARGET_DIR=$TARGET exec pnpm tauri dev --port $PORT ) > $STATE/dev.log 2>&1 &
fi
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
open_terminal || { record setup FAIL "$(setup_note "no terminal button, even after a reload")"; exit 1 }
wait_env || { record setup FAIL "$(setup_note "the terminal's environment never came")"; exit 1 }
pty=$ACCEPTED
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
