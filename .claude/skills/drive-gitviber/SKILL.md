---
name: drive-gitviber
description: Launch the GitViber dev app and drive its real window (clicks, keys, native file pickers, screenshots) to check a UI change. Only after the user said yes to a live UI test — ask first; scripted checks come before this.
---

# Driving the real GitViber window

## Gate: ask before using this

A live UI run takes 15–30 minutes of screenshot/click round trips and takes over the mouse.

1. Verify with scripts first: `pnpm typecheck`, `pnpm test`, `pnpm check:rust`, a node/cargo
   test for the logic, plain `git` in a scratch repo for what the backend does.
2. If what's left can only be seen in the window (layout, a dialog's flow, a setting's
   effect through the UI), **ask the user** with AskUserQuestion whether to run a live UI
   test, saying what the scripts already covered and what only the window would show.
   Don't launch on your own. A "yes" covers that one change, not the rest of the session.

## Setup

```zsh
export UI_DIR=<scratchpad>/ui                     # helpers, state and screenshots go here
UI=$(git rev-parse --show-toplevel)/.claude/skills/drive-gitviber/ui.sh
mkdir -p $UI_DIR/repos/proj && git -C $UI_DIR/repos/proj init -q -b main &&
  echo hi > $UI_DIR/repos/proj/a.txt && git -C $UI_DIR/repos/proj add . &&
  git -C $UI_DIR/repos/proj -c user.name=t -c user.email=t@t commit -qm init
```

Test in that scratch repo, never the real one: other agents' worktrees live in git_viber.

Launch with Bash `run_in_background`: `pnpm tauri dev > $UI_DIR/dev.log 2>&1`. Wait until
the log has ``Running `target/debug/gitviber` `` (a cold build takes minutes), give the window
~4s, then `$UI init`.

The dev app reopens its last repo, often the real git_viber. Switch first: `$UI key 31 cmd`
(⌘O), then `$UI panel $UI_DIR/repos/proj`.

## Driving

`$UI shot a1`, then Read `a1-s.png`. Coordinates for `$UI click x y` are points of that
1200-wide `-s` image. For detail, crop the 2x `a1.png`: `sips -c H W --cropOffset Y X`
(Y comes first).

Key codes: return 36, esc 53, O 31, comma 43, G 5. ⌘, opens Settings.

What already failed, so don't retry it:
- Clicks posted to the pid (CGEvent postToPid) never reach the WKWebView, so `click` uses a
  real cliclick click after bringing the app to the front. The mouse moves.
- Keys posted to the pid do reach the webview (`key`, `type`), but not a native open panel.
  That runs in another process, so `panel`/`esc-panel` use System Events, and only while
  GitViber is verified frontmost. An unguarded keystroke once typed into the user's terminal.
- After a hot reload, Settings opens on Appearance again, not the last section.

## Cleanup (always)

- Undo any setting you changed. Dev localStorage (localhost:1420) carries over to the
  user's next dev run.
- Reopen the repo the dev app had before (⌘O + `panel`).
- `pkill -f target/debug/gitviber; pkill -f "tauri.mjs dev"`. Leave
  `/Applications/GitViber.app` alone: that one is the user's own.
