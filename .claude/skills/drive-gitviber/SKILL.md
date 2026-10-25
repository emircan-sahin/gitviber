---
name: drive-gitviber
description: Launch a debug build of GitViber and drive its real window (keys to its pid, guarded clicks, window shots and OCR) to check a UI change. Only after the user said yes to a live UI test — ask first; scripted checks come before this.
---

# Driving the real GitViber window

## Gate: ask before using this

A live UI run takes 15–30 minutes of screenshot/click round trips and takes over the mouse.

1. Verify with scripts first: `command cargo nextest run -p <crate>` and
   `command cargo clippy -p <crate> --all-targets -- -D warnings` for the logic and the crate's
   leak test, plain `git` in a scratch repo for what the backend does, and an off-screen picture
   with the `snapshot` feature for layout (CONTRIBUTING.md, "Check a UI change off screen").
2. If what's left can only be seen in the window (a dialog's flow, focus and keys, a setting's
   effect through the UI), **ask the user** with AskUserQuestion whether to run a live UI
   test, saying what the scripts already covered and what only the window would show.
   Don't launch on your own. A "yes" covers that one change, not the rest of the session.

## Setup

```zsh
export UI_DIR=<scratchpad>/ui                     # tools, config, state and screenshots go here
ROOT=$(git rev-parse --show-toplevel)
mkdir -p $UI_DIR/config $UI_DIR/repos/proj && git -C $UI_DIR/repos/proj init -q -b main &&
  echo hi > $UI_DIR/repos/proj/a.txt && git -C $UI_DIR/repos/proj add . &&
  git -C $UI_DIR/repos/proj -c user.name=t -c user.email=t@t commit -qm init
swiftc -O $ROOT/.claude/skills/drive-gitviber/gvwin.swift -o $UI_DIR/gvwin
command cargo build -p gitviber && cp -c $ROOT/target/debug/gitviber $UI_DIR/gitviber
```

Test in that scratch repo, never the real one: other agents' worktrees live in git_viber. The
copy of the binary keeps someone else's build in the shared `target/` from swapping it mid-run.

Launch with Bash `run_in_background`:
`GITVIBER_CONFIG_DIR=$UI_DIR/config $UI_DIR/gitviber $UI_DIR/repos/proj > $UI_DIR/app.log 2>&1`.
Take the pid with `PID=$(pgrep -nf "^$UI_DIR/gitviber")` (the app restarts itself once with
`MallocLargeCache=0`, same pid) and wait until `$UI_DIR/gvwin win $PID` prints its window.

The config folder is the run's own, so the user's settings, recent repos and window layout are
neither read nor changed. To start with a setting, write it to `$UI_DIR/config/settings.json`
before launching; `write_config` in `browser-check-native.sh` shows the files and keys. Open
repos by passing them at launch, not through ⌘O's panel.

## Driving

`gvwin` acts on the pid alone; its header lists every command.

- `$UI_DIR/gvwin win $PID` prints `id x y w h` of the window, in screen points.
- `$UI_DIR/gvwin key $PID <code> [cmd,shift,alt,ctrl]` and `$UI_DIR/gvwin type $PID '<text>'`
  post to that pid only.
- `$UI_DIR/gvwin click $PID <x> <y> [right|double]` is a real click at a screen point (the
  window's x and y plus the point in it), refused with exit 3 unless GitViber is frontmost and its
  window is the top one there. The mouse moves. `drag` checks the same at its start.
- `screencapture -x -o -l <id> $UI_DIR/a1.png` shoots the window alone (2x on a Retina display);
  then Read it. For detail, crop it: `sips -c H W --cropOffset Y X` (Y comes first).
- `$UI_DIR/gvwin ocr $UI_DIR/a1.png 2` prints `x y w h text` for each line of text, in window
  points. Start there rather than with reading pictures: find a label, click its middle.

Key codes are a US keyboard's, and the app reads them through the Mac's layout (a Turkish one
makes ⌘= ⌘−, and ⌘, is code 42), so prefer keys that layouts don't move. Return 36, esc 53,
T 17, W 13, O 31. ⌘, opens Settings.

What already failed, so don't retry it:
- Keystrokes through System Events go to whatever is frontmost: one typed into the user's
  terminal. Keys go to the pid.
- A window behind others draws only its first frame: a stale or empty shot means bring it to the
  front first, not that the view is broken.

## Browser live check

One script for the browser end to end, the user's tabs and agents' tabs. It drives the window:
keys to the app's pid, real clicks only while it's frontmost and its window is on top at the
point, and OCR of its own window shots (`gvwin.swift`). Build first (`command cargo build -p
gitviber`); a run takes about 12 minutes.

```zsh
.claude/skills/drive-gitviber/browser-check-native.sh [check...]
```

- It runs a copy of `target/debug/gitviber` with HOME and CFFIXED_USER_HOME on a fake home,
  GITVIBER_CONFIG_DIR and a short TMPDIR of its own, on a `git clone --local` of this repository.
  The fake home's `.zshrc` writes each pane's `GITVIBER_*` environment, so nothing is typed into
  a shell to get it. Ask runs a stand-in `claude` by its full path, never the real one.
- The test pages are served on free ports by `browser-check-site.py`. It prints a
  PASS/FAIL/SKIP/INFO table and exits 1 on any FAIL; `KEEP=1` keeps the logs when all pass.
- Each check is a `check_<name>` function in `CHECKS`, with what it needs from
  `gitviber browser help` in `NEEDS`; a check whose commands aren't there is skipped.
- Settings can't be flipped from here: checks of another setting (Show or Mute agents' tabs, agent
  control off, no hidden page kept live) relaunch the app with it.
- `NO_CLICKS=1` skips what needs the window; a check skips itself when another app took the front.
- Never used: pbcopy/pbpaste (the two clipboard checks are SKIP), CryptoKey, ~/Downloads, real
  sites or agent sessions (`agent_e2e` is SKIP).
- Its parsing and table are tested without an app:
  `python3 -I .claude/skills/drive-gitviber/browser-check-test.py`.

## Cleanup (always)

- Quit the app you started: `kill $PID`. By pid, not by name: other agents may be running their
  own `target/debug/gitviber`. Leave `/Applications/GitViber.app` alone: that one is the user's own.
- Remove `$UI_DIR` once its screenshots are no longer needed.
