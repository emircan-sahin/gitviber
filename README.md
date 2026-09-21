<p align="center">
  <img alt="GitViber" src="assets/icon.svg" width="112">
</p>

<h1 align="center">GitViber</h1>

<p align="center">
  A desktop git client for reviewing what your coding agents wrote.
</p>

<p align="center">
  <a href="https://github.com/emircan-sahin/gitviber/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/emircan-sahin/gitviber/ci.yml?branch=main&style=flat-square&label=CI"></a>
  <img alt="macOS" src="https://img.shields.io/badge/platform-macOS-lightgrey?style=flat-square">
  <a href="LICENSE"><img alt="MIT" src="https://img.shields.io/badge/license-MIT-blue?style=flat-square"></a>
</p>

![GitViber](assets/screenshot-dark.png)

I run a few agents at once, each in its own worktree, and spend most of the day reading their
diffs. Editors show hunks, GitHub shows them after the push, and most git clients weren't built
for a repo that changes while you look at it. GitViber is the tool I wanted for that: full files,
live updates, every worktree one click away, and a terminal right under the diff.

It's plain git underneath. Every action runs the `git` CLI with your own config, hooks,
credentials and signing, and GitViber writes nothing of its own into the repo.

## Features

| | |
| --- | --- |
| **Diffs** | Whole files, unified or split, with word-level highlights and collapsible unchanged regions |
| **Review** | `J` / `K` through changed files, `V` to mark one viewed. The mark clears when the agent touches the file again |
| **Live** | Status, diffs and the file tree refresh as files change, without losing your scroll position |
| **Worktrees** | Switch worktrees from the top bar, see each one's change count, check a branch out into a new worktree in one step |
| **Terminal** | Your login shell under the diff (`⌘J`), with tabs and splits. Each worktree keeps its own terminals |
| **Branches** | Merge, rebase, and pull with fast-forward, merge or rebase |
| **Conflicts** | Resolve block by block (current, incoming, both, or edit by hand), then continue, skip or abort |
| **Pull requests** | List, read, review in the same viewer, create, merge, check out. GitHub only for now |
| **Explorer** | Browse any file with change bars in the gutter and ignored files dimmed |
| **Look** | Light and dark themes, eight syntax themes, SF Mono, Geist Mono or JetBrains Mono |

Worktrees and repos nested inside the project show up in Changes but are kept out of staging, so a
stray `git add` can't turn them into gitlinks.

<table>
  <tr>
    <td><img alt="Light theme" src="assets/screenshot-light.png"></td>
    <td><img alt="Pull request view" src="assets/screenshot-pr.png"></td>
  </tr>
</table>

## Build

There are no release builds yet. You need Rust (stable), Node 22+, pnpm and git.

```sh
pnpm install
pnpm tauri dev      # run the app
pnpm tauri build    # release bundle
pnpm check          # what CI runs: build, tests, rustfmt, clippy
```

GitViber is built and tested on macOS. The code compiles elsewhere, but nobody has used it there yet.

## Shortcuts

| Keys | |
| --- | --- |
| `⌘1` `⌘2` `⌘3` | Changes, History, PRs |
| `J` `K` | Next / previous changed file |
| `V` | Mark file viewed |
| `⌥↓` `⌥↑` | Next / previous change |
| `⌥S` `⌥C` `⌥Z` | Split view, collapse unchanged, word wrap |
| `⌘B` `⌥⌘B` | Toggle the git panel, the file explorer |
| `⇧⌘E` | Show the file explorer |
| `⌘J` or `⌃` `` ` `` | Toggle the terminal |
| `⌘T` `⌘D` `⌘K` | New terminal, split, clear (in the terminal) |
| `⌘+` `⌘−` `⌘0` | Code font size |
| `⌘W` | Close tab, or the terminal pane in focus |
| `⌘↵` | Commit |
| `⌘R` | Refresh |
| `⌘O` | Open repository |

## GitHub sign-in

GitViber never asks for a token. For pull requests it borrows a login you already have: the GitHub
CLI (`gh auth token`) first, then git's stored github.com credential (Keychain, GitHub Desktop, Git
Credential Manager). The token stays in memory.

## Privacy

No API keys, no telemetry. The app talks to your git remotes and, for pull requests,
`api.github.com`. It only reads files inside the repository you open, and text from GitHub is
never rendered as HTML.

## Stack

Tauri 2 and Rust on the backend, React 19 with Radix and Tailwind v4 on the front. Diffs are
computed in Rust with `similar`, highlighting is Shiki in a web worker, and the UI runs in the
system webview, so there's no bundled Chromium.

## Contributing

Bug reports and small, focused PRs are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) covers setup and
the few rules the app depends on. Report security issues privately, see [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE)
