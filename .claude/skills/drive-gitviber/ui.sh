#!/bin/zsh
# Drives the dev GitViber window. State (pid, window id, origin) lives in $UI_DIR.
#   ui.sh init                 find the dev window, save pid/id/origin
#   ui.sh shot <name>          window screenshot: <name>.png (2x) and <name>-s.png (1200 wide, for reading)
#   ui.sh click <x> <y>        real click at a point of the -s screenshot (app brought front first)
#   ui.sh key <code> [mods]    key into the webview only (mods: cmd,shift,alt,ctrl)
#   ui.sh type <text>          text into the webview only
#   ui.sh panel <path>         type a path into a native open panel (⌘⇧G, path, return, return)
#   ui.sh esc-panel            cancel a native open panel
set -e
UI_DIR=${UI_DIR:?set UI_DIR to a scratchpad folder}
HERE=${0:a:h}
cd $UI_DIR
for t in keypid winid; do [[ -x $t ]] || swiftc -O $HERE/$t.swift -o $t; done

# System Events keystrokes go to whatever app is in front; they once typed into the user's
# terminal. Only send them when GitViber is verifiably frontmost.
front_keys() {
  osascript -e 'tell application "System Events"' \
    -e "set frontmost of (first process whose unix id is $(cat pid)) to true" -e 'delay 0.3' \
    -e "if (unix id of first process whose frontmost is true) is $(cat pid) then" "$@" -e 'end if' -e 'end tell'
}

case $1 in
  init)
    # The main window is the big one; tauri also owns a small hidden 500x500 one.
    ./winid gitviber | sort -k4 -n | tail -1 | read WID WX WY WW WH PID
    echo $PID > pid; echo $WID > win; echo "$WX $WY $WW" > origin
    echo "pid $PID window $WID at $WX,$WY width $WW";;
  shot)
    screencapture -x -o -l $(cat win) $2.png && sips -Z 1200 $2.png --out $2-s.png >/dev/null;;
  click)
    read WX WY WW < origin; k=$(( WW / 1200.0 ))
    osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $(cat pid)) to true" -e 'delay 0.3'
    X=$(( WX + $2 * k )); Y=$(( WY + $3 * k )); cliclick c:${X%.*},${Y%.*};;
  key) ./keypid $(cat pid) key $2 $3;;
  type) ./keypid $(cat pid) type "$2";;
  panel)
    front_keys -e 'keystroke "g" using {command down, shift down}' -e 'delay 0.8' \
      -e "keystroke \"$2\"" -e 'delay 0.8' -e 'keystroke return' -e 'delay 1' -e 'keystroke return';;
  esc-panel) front_keys -e 'key code 53';;
  *) sed -n '2,9p' $0; exit 1;;
esac
