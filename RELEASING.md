# Releasing

A pushed `v*` tag runs [`release.yml`](.github/workflows/release.yml): it builds a universal
macOS app (signed and notarized) and the Linux `.deb`, `.rpm` and AppImage, then leaves them on a
**draft** release with `SHA256SUMS`, the updater's `latest.json` and notes from
[CHANGELOG.md](CHANGELOG.md). Nothing is public until you publish the draft.

## How it's locked down

- **Secrets live in the `release` environment**, not the repository, and its deployment policy
  only lets `v*` tags use it. A branch, a pull request or a dry run never sees them.
- **Only admins can create, move or delete `v*` tags** (the "Release tags" ruleset), so only an
  admin can start a signed build.
- **Only the last job can write to the repository.** The build jobs have read access; `publish`
  gets `contents: write` to create the draft, and runs for tags only.
- Actions are pinned to commit SHAs. The certificate goes into a temporary keychain and the
  certificate and API key files into `$RUNNER_TEMP`; all of them are deleted when the job ends,
  failed or not.

## One-time setup

### Signing and notarization

Run the helper in a terminal (it asks for passwords, so it needs a real one):

```sh
sh scripts/release-secrets.sh
```

It asks for your Developer ID certificate as a `.p12` and for either an App Store Connect API key
(recommended) or an Apple ID with an app-specific password, checks them, and stores them as
secrets in the `release` environment with `gh secret set --env release`. Nothing touches disk or
the screen. Its header explains how to export the `.p12` from Keychain Access and where to make the
API key. Delete the exported `.p12` afterwards; the certificate stays in your keychain.

| Secret | What |
| --- | --- |
| `APPLE_CERTIFICATE` | The Developer ID Application `.p12`, base64 |
| `APPLE_CERTIFICATE_PASSWORD` | Its export password |
| `APPLE_SIGNING_IDENTITY` | `Developer ID Application: … (L64GDMKU5T)` |
| `APPLE_API_KEY`, `APPLE_API_ISSUER`, `APPLE_API_PRIVATE_KEY` | API key ID, issuer ID, the `.p8`'s contents |
| or `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | Apple ID, app-specific password, team ID |
| `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The updater key, already set |

A tag build fails if a signing, notarization or updater secret is missing, rather than publish
an app Gatekeeper blocks or one that can't update. Re-run the script when the certificate is renewed or the API key revoked.

### The updater key

`tauri.conf.json` leaves updater bundles off, so any build works without this key; tag builds
turn them on and sign them with it, and installed copies only accept updates signed by it. The private key is `~/.tauri/gitviber-updater.key`, its password is in the macOS Keychain
(`security find-generic-password -s "gitviber updater key password" -w`), and both are already in
the `release` environment. **Back both up somewhere safe and offline.** If the key or its password
is lost, no installed copy can be updated again; everyone has to download the next version by
hand. If it leaks, anyone who can serve a `latest.json` to users can ship them an update.

### The Homebrew tap

Once, create the tap repository:

```sh
gh repo create emircan-sahin/homebrew-tap --public --description "Homebrew casks for GitViber"
```

## Each release

1. **Bump the version** everywhere at once: `pnpm version:set 0.2.0`. It writes `package.json`,
   `src-tauri/Cargo.toml` and `Cargo.lock` (`tauri.conf.json` reads `package.json`), and
   `pnpm check` fails if they ever disagree.
2. **Write the changelog.** Move what's under `## [Unreleased]` into
   `## [0.2.0] - YYYY-MM-DD` with today's date, and update the links at the bottom. The section
   becomes the release notes and what the updater shows; the workflow refuses an undated or
   missing one.
3. **Commit and push** to `main` (`chore(release): 0.2.0`), and let CI pass.
4. **Tag and push the tag:**

   ```sh
   git tag v0.2.0 && git push origin v0.2.0
   ```

   The workflow checks that the tag matches the version before it builds anything.
5. **Check the draft** under Releases once the run is green (about half an hour): the `.dmg`,
   `.app.tar.gz` and `.sig`, `.deb`, `.rpm`, `.AppImage` and their `.sig`, `SHA256SUMS` and
   `latest.json`. Download the `.dmg` and open it on a Mac that has never run GitViber: it
   should open without a Gatekeeper warning.
6. **Publish** the draft. From then on `releases/latest/download/latest.json` points at it and
   installed copies offer the update.
7. **Update the Homebrew cask** (below).

If a build job fails, fix the cause and re-run the failed jobs: the publish job reuses its draft
and replaces what's there. It never touches a published release; for a broken published one,
tag the next patch version.

### Test a signed build without releasing

A test tag signs and notarizes like a release, but its draft is marked as a pre-release, which
`releases/latest` (and so the updater) skips. It builds the current version, so there's no bump:

```sh
git tag v0.2.0-rc.1 && git push origin v0.2.0-rc.1
```

Delete the draft and the tag afterwards (`gh release delete v0.2.0-rc.1 --cleanup-tag`).

### Dry run

Actions → Release → Run workflow runs the same builds on any branch, without the `release`
environment: no signing, no notarization, no updater signatures, and no release. The bundles,
`SHA256SUMS` and a filled-in cask are kept on the run as artifacts for two weeks. Use it to check
a workflow or build change before tagging.

## Homebrew

The official `homebrew/cask` only takes apps that meet its notability rules, so GitViber has its
own tap until it does. [`packaging/homebrew/gitviber.rb`](packaging/homebrew/gitviber.rb) is the
cask's template; every release run fills in the version and the `.dmg`'s SHA-256 and prints the
result on the run's summary page (also in the `release` artifact as `gitviber.rb`).

After publishing a release, commit it to the tap:

```sh
gh run download <run id> --repo emircan-sahin/gitviber --name release --dir /tmp/gitviber-release
gh repo clone emircan-sahin/homebrew-tap /tmp/homebrew-tap
mkdir -p /tmp/homebrew-tap/Casks
cp /tmp/gitviber-release/gitviber.rb /tmp/homebrew-tap/Casks/gitviber.rb
git -C /tmp/homebrew-tap add Casks/gitviber.rb
git -C /tmp/homebrew-tap commit -m "gitviber 0.2.0" && git -C /tmp/homebrew-tap push
```

Users install it with
`brew install --cask emircan-sahin/tap/gitviber`. The cask sets `auto_updates true`, so
`brew upgrade` leaves updates to the app itself.
