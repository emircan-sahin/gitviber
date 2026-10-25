# Contributing

GitViber is early and has one maintainer, so small and focused lands much faster than large and ambitious. Bug reports and fixes are genuinely welcome.

## Get it running

You need macOS 13+ or Linux, [rustup](https://rustup.rs) and git. `rust-toolchain.toml` pins the Rust version (1.99, which GPUI needs), and rustup installs it on the first build. On macOS, Xcode or its Command Line Tools (`xcode-select --install`) bring the compiler and the SDK. The [GitHub CLI](https://cli.github.com) is optional; it's the easiest way to sign in for the pull request features.

On Linux, install the libraries GPUI and the clipboard build against first. On Debian and Ubuntu (what CI runs):

```sh
sudo apt install libgtk-3-dev libxkbcommon-dev libxkbcommon-x11-dev libx11-xcb-dev \
  libxcb1-dev libwayland-dev libfontconfig-dev libfreetype-dev
```

Then build and run it on a repository:

```sh
cargo run -p gitviber -- /path/to/repo
```

The first build compiles the whole GPUI stack and takes a while. macOS is where GitViber is used daily; Linux is being worked through in #35, and nobody has tried Windows. An issue describing what broke on either is a great first contribution.

## Where things live

A Cargo workspace, one crate per area under `crates/`. Each crate's `lib.rs` opens with what it holds.

- `app` (the `gitviber` binary) - the window, its layout, the top bar, menus, the palette, the settings window and the dialogs
- `ui` - the theme, settings, the command registry and keymap, shared widgets, and the workspace contract every feature crate plugs into; `dev::run` opens one feature on its own
- `core` - the backend with no UI in it: git, GitHub, terminals and coding agents. The app calls `api::*` from background threads and hears back through `Event`s. `src/scenario_tests/` runs end-to-end git scenarios against temporary repos and a local bare remote, one file per topic
- `diff` - the diff viewer, shared by Changes, History, pull requests, Compare and guided review
- `changes` - the Changes panel: staging, the commit box, stashes, submodules, a merge or rebase in progress, conflicts
- `history` - the commit graph with its search, menus and rewrites, a commit's tab, and the Compare screen
- `refs` - branches, worktrees and tags, and the top bar's git actions: pickers, dialogs, pull, push, undo and redo, background fetch, credential prompts
- `files` - the explorer, search in files, quick open, the file view and media, files outside the repo
- `markdown` - GitHub-flavored markdown drawn natively, Mermaid and math included
- `obsidian` - the vault panel, notes as Obsidian's reading view shows them, canvases
- `github` - the pull request and issue panels and tabs, checks, and the account they're read with
- `review` - guided review, Find Risks, Ask Agent and review notes
- `terminal` - alacritty's emulation painted by GPUI, with tabs, splits, zoom, a set per worktree, command marks and find
- `browser` - browser tabs (a WKWebView each) and the agents' tabs they drive with `gitviber browser`; macOS only for now
- `updater` - the app's own updates: the release feed, the signature check, the install in place
- `vendor/gpui-base` - gpui-base 0.8.0 with one fix, wired in through `[patch.crates-io]`; `PATCH.md` says what and why

## Ground rules

These are the product, not style preferences. A PR that breaks one will be sent back:

- **Plain git.** Every repo action runs the `git` CLI (core's `git/`, through `process.rs`), so the user's config, hooks, credentials and signing apply. Don't add libgit2 or gitoxide, and don't write any GitViber-specific state (files, refs, config) into the user's repo.
- **Stay inside the repo.** Paths from the UI are resolved and checked in core's `fs.rs` (`resolve`, `confine`): one that would leave the repo, through `..` or a symlink, is refused. New file access goes through that, never around it.
- **No stored secrets.** The GitHub token is borrowed from `gh` or git's credential store and kept in memory only. Nothing writes a token or a password to disk or to the log; `errors.rs` blanks logins in URLs and GitHub tokens before a line is written. Text from GitHub is drawn by the app itself, not a web page, and images from outside GitHub's hosts become links.
- **Off the UI thread.** Core's calls block: run them in `cx.background_spawn`, never in a render or an event handler. That goes for reading the disk too, since a gone network volume can take seconds to say so. If a change makes scrolling or a large diff slower, say so in the PR.
- **Few dependencies.** Explain a new crate in the PR, pin its exact version with `=`, and pick a release published at least two weeks ago (its sub-crates too). gpui-kit's features are chosen once in the workspace `Cargo.toml`; a crate asking for its own set builds the whole GPUI stack again. Then regenerate the license list, so the crate shows under About → Third-Party Licenses:

  ```sh
  GITVIBER_WRITE_LICENSES=1 cargo nextest run -p gitviber licenses_are_current
  ```

## Memory

A view that's closed gives all of its memory back. That takes a few habits:

- **Weak handles in kept closures.** A subscription, an observer, a spawned task or a callback another entity keeps holds a `WeakEntity`, never an `Entity`: a strong one keeps the view alive after its tab closes.
- **Tasks and subscriptions as fields.** A `Task` or `Subscription` that should stop with its view is a field of that view (`_subs`, `_poll`), so dropping the view ends it. Detach only work that finishes on its own.
- **Bounded caches.** A cache has a limit (entries or bytes) and belongs to the view that uses it, so it goes with the view.
- **A leak test per crate.** Every UI crate opens and closes its views ten times between `cx.leak_detector_snapshot()` and `cx.assert_no_new_leaks(&snapshot)`. A new view or tab kind goes in it, and carries a `_live: Live::new("Name")` field so the debug build's memory readout in the status bar counts it.
- **Measure with `MallocLargeCache=0`.** With macOS malloc's large cache on, freed multi-MB blocks stay in the footprint and look like a leak. The app restarts itself without it (the bundle sets it in `Info.plist`); set it yourself for an example or a probe.

## Check a UI change off screen

A window behind others draws only its first frame, so a screenshot of it misses whatever loaded later. With the `snapshot` feature, `GITVIBER_SNAPSHOT=out.png` draws the same view with gpui's headless Metal renderer (so on macOS), with no window on screen, writes the PNG and exits:

```sh
cargo build -p gitviber --features snapshot
GITVIBER_SNAPSHOT=out.png target/debug/gitviber /path/to/repo
```

`GITVIBER_SNAPSHOT_WAIT` sets how many seconds it waits for loads to land (3 unless you say), `GITVIBER_APPEARANCE=light` or `dark` stands in for the OS appearance, and `GITVIBER_SNAPSHOT_OPEN` pictures more than the main window: `settings:<section>`, `palette`, `files`, `overlay`, `about`, `licenses` or `clone`.

Most feature crates have a `<crate>-demo` example that opens just its panel or tab on a repository through `dev::run`, with its own config folder so it never rewrites yours. Those of browser, changes, files, github, history, obsidian, review and terminal build with the snapshot feature too:

```sh
GITVIBER_SNAPSHOT=out.png cargo run -p gitviber-changes --example changes-demo -- /path/to/repo --open
```

## Before you push

```sh
cargo fmt --all
cargo clippy -p <crate> --all-targets -- -D warnings
cargo nextest run -p <crate>
```

Use [nextest](https://nexte.st) (`cargo install cargo-nextest --locked`): it runs each test in a process of its own, which the tests that point the config folder elsewhere depend on. CI runs the same three on macOS and Linux for the whole workspace (`--workspace`), plus `scripts/version.sh check`. If you change how a git operation behaves, add or update a scenario in core's `scenario_tests/`.

## Pull requests

Keep a PR to one thing, and say what changed and why. The [PR template](.github/pull_request_template.md) covers the rest. If it changes the UI, a before/after screenshot helps a lot; the off-screen snapshot above makes one without touching your windows.

Write commit messages in English with [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/), e.g. `fix(diff): keep scroll position when the file changes`.

For anything that moves product direction (a new panel, a new git workflow, a refactor that changes the shape of the app), open an issue first. That's not gatekeeping. I'd just rather you hear "I'm already halfway through that" before you write it than after.

Using an AI agent is fine; this is a tool for exactly that. But you have to understand the code you submit and be able to explain how it interacts with the rest of the app.

I might close a PR, ask you to shrink it, or implement the idea differently. That's a call about scope and timing, not about you or the quality of your work.

Be kind: [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). Security reports: [SECURITY.md](SECURITY.md).
