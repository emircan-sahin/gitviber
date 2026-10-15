---
name: drive-gitviber
description: Launch the GitViber dev app and drive its real window (DOM through the dev bridge, clicks, keys, native file pickers, screenshots) to check a UI change. Only after the user said yes to a live UI test — ask first; scripted checks come before this.
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

Start with the DOM, not screenshots. Debug builds run `src-tauri/src/dev_bridge.rs`, so
`$UI js '<function body>'` runs in the page and prints `{"ok":…,"value"|"error":…}`. The
helpers in `prelude.js` are in scope: `$`, `$$`, `byText`, `press` (pointer events too, which
Radix menus need), `rightClick`, `key`, `typeInto`, `waitFor`, `text`, `toasts`. Example:
`$UI js 'press(byText("History")); await sleep(500); return toasts()'`. Keep what it returns
small: `innerText` of a region, not `textContent` of everything (that includes `<style>`).

- Labels can differ from what shows: the PR/Issue filters are "open/closed/all" in the DOM,
  capitalized by CSS.
- Success toasts close after a few seconds; press their buttons in the same script.
- Hot reload doesn't re-run module-level listeners (keybindings): after changing those, ⌘R.
- Synthetic events are untrusted but the app doesn't check. Native menus, open panels and
  real key equivalents still need the tools below.

Screenshots are for what only pixels show (layout, colors, Monaco). `$UI shot a1`, then Read `a1-s.png`. Coordinates for `$UI click x y` are points of that
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
- A window behind others paints nothing (WebKit stops drawing it): a black or empty shot
  means bring it to the front first, not that the page is broken.
- The native menu is scriptable with System Events on the app's pid (`click menu item …`),
  which covers Help and Git menu items, and the clipboard reads back with `pbpaste`.

## Browser live check (agents' tabs)

One script for what agents' browser tabs do in a real dev build, DOM and CLI only: no mouse, no
keys to the screen, no window brought forward (the dev window still opens). A run takes 3–4
minutes after the build. The user runs it; it launches the app, so the gate above applies.

```zsh
.claude/skills/drive-gitviber/browser-check.sh <worktree> [port] [check...]
```

- It runs `pnpm tauri dev --port <port>` in `<worktree>` (port 1437 by default, never 1420) with
  that worktree's own target dir. It finds the app by its path, turns agent control on in that
  port's localStorage, opens a scratch repo and a terminal, and reads the pane's `GITVIBER_*`
  environment into a 0600 file. `gb` then runs `gitviber browser` as that pane's agent.
- The test pages are served on two free ports by `browser-check-site.py`.
- It prints a PASS/FAIL/SKIP/INFO table and exits 1 on any FAIL. Logs stay in the printed folder
  when something failed, or with `KEEP=1`.
- Each check is a `check_<name>` function in `CHECKS`, with what it needs from
  `gitviber browser help` in `NEEDS`. A check whose commands aren't there yet is skipped, so a
  branch can add checks for its new commands. The checks for acting as a person could, the
  change line, reading in parts and honest output, native input, and popups and downloads are
  already there, and skip themselves on builds without those commands.
- Cleanup always runs: test sessions deleted, its screenshots removed, the port's localStorage
  cleared, the app, `tauri dev` and the servers killed, the env file removed.
- `AGENT_E2E=1` adds `agent_e2e`: a real Claude Code session (`claude -p`, on your account) in
  the pane fills the test form through `gitviber browser`. It passes when the answer has the
  request's status, the site saw the submit from a WebKit tab rather than Chrome, and the run used
  no other browser; only a one-line summary is kept. The dev app writes the skill for it
  (`GITVIBER_DEV_SKILL=1`), and each skill file is put back as it was found afterwards.
- Its parsing and table are tested without an app: `zsh .claude/skills/drive-gitviber/browser-check-test.zsh`.

## Cleanup (always)

- Undo any setting you changed. Dev localStorage (localhost:1420) carries over to the
  user's next dev run.
- Reopen the repo the dev app had before (⌘O + `panel`).
- `pkill -f 'target/debug/(gitviber|GitViber Dev\.app/)'; pkill -f "tauri.mjs dev"`. The dev
  build reruns itself as `target/debug/GitViber Dev.app` (dev_bundle.rs), same pid. Leave
  `/Applications/GitViber.app` alone: that one is the user's own.
