# Contributing

GitViber is early and has one maintainer, so small and focused lands much faster than large and ambitious. Bug reports and fixes are genuinely welcome.

## Get it running

You need macOS or Linux, a current stable Rust toolchain, Node.js 22+, pnpm and git. The [GitHub CLI](https://cli.github.com) is optional; it's the easiest way to sign in for the pull request features.

```sh
pnpm install
pnpm tauri dev
```

On Linux, install WebKitGTK first (the README lists the packages). macOS is where GitViber is used daily; Linux is being worked through in #35, and nobody has tried Windows. An issue describing what broke on either is a great first contribution.

## Where things live

- `src/features/` - the UI, a folder per area: `workspace/` (the window's layout), `topbar/`, `changes/`, `history/`, `github/` (`pulls/`, `issues/`, and `shared/` for what both use), `viewer/` (`MonacoView` is the diff and file viewer), `explorer/`, `branches/`, `worktrees/`, `projects/`, `terminal/`, `settings/`, `palette/`, and `app/` for app-wide dialogs. One area may import another's module; `components/` never imports a feature
- `src/components/` - UI shared across features (`FileIcon`, `StatusBadge`, `RowAction`, `ErrorBoundary`, …); `ui/` holds the shadcn/ui-style primitives on Radix and Tailwind
- `src/hooks/` - React hooks features share: `useGitAction` (busy state, toasts and undo around a git action), `useAsyncValue`, `usePickerIndex`
- `src/lib/` - the rest, by area: `api/` wraps every Tauri command, `repo/useRepo.ts` holds repo state, `editor/highlight.worker.ts` runs Shiki off the main thread; `git/`, `github/`, `links/`, `terminal/`, `commands/` (keys and the menu bar), `app/` and `ui/` hold the others. Helpers every area shares sit at the top: `storage.ts` (localStorage), `store.ts` (state outside React), `format.ts`, `path.ts`, `platform.ts`
- `*.test.ts` - run by `node --test`, no bundler: a tested module, and whatever it imports at runtime, imports other modules relatively and with the extension (`../path.ts`), never through `@/`
- `src-tauri/src/` - the Rust side: `commands/` (the Tauri commands, one file per area; `lib.rs` only registers them), `state.rs` (what they share: the open repo, the index lock, the blocking pool), `git/` (the git CLI wrapper, one file per area), `github/` (pull requests and issues), `process.rs` (running programs with the user's PATH), `diff.rs`, `fs.rs` (file explorer), `watch.rs` (file watcher)
- `src-tauri/src/scenario_tests/` - end-to-end git scenarios against temporary repos and a local bare remote, one file per topic

## Ground rules

These are the product, not style preferences. A PR that breaks one will be sent back:

- **Plain git.** Every repo action shells out to the `git` CLI so the user's config, hooks, credentials and signing apply. Don't add libgit2, and don't write any GitViber-specific state (files, refs, config) into the user's repo.
- **Stay inside the repo.** Paths from the frontend are resolved and checked in `fs.rs`. New file access goes through that, never around it.
- **No stored secrets.** The GitHub token is borrowed from `gh` or git's credential store and kept in memory only. Text from GitHub is rendered as markdown whose inline HTML is cut down to GitHub's own allowlist (`rehype-sanitize`'s default schema); raw HTML never reaches the page and nothing from it runs.
- **Fast.** Heavy work (diffs, highlighting) stays off the UI thread. If a change makes scrolling or a large diff slower, say so in the PR.
- **Few dependencies.** Explain a new package in the PR, and pin its exact version like the existing ones. Then run `pnpm licenses:generate` so its license shows under About → Third-Party Licenses.

## Before you push

```sh
pnpm check
```

That runs what CI runs: `tsc`, a production build, the third-party license list being up to date, `cargo fmt --check`, `cargo clippy -D warnings` and `cargo test`. If you change how a git operation behaves, add or update a scenario in `scenario_tests/`.

## Pull requests

Keep a PR to one thing, and say what changed and why. The [PR template](.github/pull_request_template.md) covers the rest. If it changes the UI, a before/after screenshot helps a lot.

Write commit messages in English with [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/), e.g. `fix(diff): keep scroll position when the file changes`.

For anything that moves product direction (a new panel, a new git workflow, a refactor that changes the shape of the app), open an issue first. That's not gatekeeping. I'd just rather you hear "I'm already halfway through that" before you write it than after.

Using an AI agent is fine; this is a tool for exactly that. But you have to understand the code you submit and be able to explain how it interacts with the rest of the app.

I might close a PR, ask you to shrink it, or implement the idea differently. That's a call about scope and timing, not about you or the quality of your work.

Be kind: [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). Security reports: [SECURITY.md](SECURITY.md).
