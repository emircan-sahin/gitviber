# Contributing

GitViber is early and has one maintainer, so small and focused lands much faster than large and ambitious. Bug reports and fixes are genuinely welcome.

## Get it running

You need macOS, a current stable Rust toolchain, Node.js 22+, pnpm and git. The [GitHub CLI](https://cli.github.com) is optional; it's the easiest way to sign in for the pull request features.

```sh
pnpm install
pnpm tauri dev
```

macOS is the only platform tested today. If you get it running on Linux or Windows, an issue describing what broke is a great first contribution.

## Where things live

- `src/features/` - the UI: top bar, changes / history / PR panels, file tree, and the viewers (`CodeView` is the diff and file viewer)
- `src/components/ui/` - shadcn/ui-style primitives on Radix and Tailwind
- `src/lib/` - `api.ts` wraps every Tauri command, `useRepo.ts` holds repo state, `highlight.worker.ts` runs Shiki off the main thread
- `src-tauri/src/` - the Rust side: `git.rs` (the git CLI wrapper), `diff.rs`, `github.rs` (pull requests), `fs.rs` (file explorer), `watch.rs` (file watcher)
- `src-tauri/src/scenario_tests.rs` - end-to-end git scenarios against temporary repos and a local bare remote

## Ground rules

These are the product, not style preferences. A PR that breaks one will be sent back:

- **Plain git.** Every repo action shells out to the `git` CLI so the user's config, hooks, credentials and signing apply. Don't add libgit2, and don't write any GitViber-specific state (files, refs, config) into the user's repo.
- **Stay inside the repo.** Paths from the frontend are resolved and checked in `fs.rs`. New file access goes through that, never around it.
- **No stored secrets.** The GitHub token is borrowed from `gh` or git's credential store and kept in memory only. Text from GitHub is rendered as markdown whose inline HTML is cut down to GitHub's own allowlist (`rehype-sanitize`'s default schema); raw HTML never reaches the page and nothing from it runs.
- **Fast.** Heavy work (diffs, highlighting) stays off the UI thread. If a change makes scrolling or a large diff slower, say so in the PR.
- **Few dependencies.** Explain a new package in the PR, and pin its exact version like the existing ones.

## Before you push

```sh
pnpm check
```

That runs what CI runs: `tsc`, a production build, `cargo fmt --check`, `cargo clippy -D warnings` and `cargo test`. If you change how a git operation behaves, add or update a scenario in `scenario_tests.rs`.

## Pull requests

Keep a PR to one thing, and say what changed and why. The [PR template](.github/pull_request_template.md) covers the rest. If it changes the UI, a before/after screenshot helps a lot.

Write commit messages in English with [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/), e.g. `fix(diff): keep scroll position when the file changes`.

For anything that moves product direction (a new panel, a new git workflow, a refactor that changes the shape of the app), open an issue first. That's not gatekeeping. I'd just rather you hear "I'm already halfway through that" before you write it than after.

Using an AI agent is fine; this is a tool for exactly that. But you have to understand the code you submit and be able to explain how it interacts with the rest of the app.

I might close a PR, ask you to shrink it, or implement the idea differently. That's a call about scope and timing, not about you or the quality of your work.

Be kind: [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). Security reports: [SECURITY.md](SECURITY.md).
