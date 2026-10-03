# Update mirror

GitHub's release asset host (`release-assets.githubusercontent.com`) is very slow for some
users: a 28 MB update measured about 0.1 MB/s where Cloudflare gave 8 MB/s on the same machine.
This Cloudflare Worker is a download mirror for GitViber's updates. It needs no storage, no
card, and no step per release: it reads the latest GitHub release when asked.

- `GET /latest.json` fetches the latest release's `latest.json` from GitHub and returns it with
  every `platforms.*.url` pointed at the Worker.
- `GET /download/<tag>/<file>` streams that release file from GitHub. The Worker follows GitHub's
  redirect to its CDN itself (datacenter to datacenter) and passes `Range`, `Content-Length`,
  `Content-Type` and `Accept-Ranges` through.

It is not an open proxy. The upstream is fixed to `github.com/emircan-sahin/gitviber/releases`;
a request can only choose a tag (`vX.Y.Z` or `vX.Y.Z-rc.1`) and a file name made of letters,
digits, `.`, `_` and `-` that is one of the release's file kinds (`.app.tar.gz`, `.dmg`,
`.AppImage`, `.deb`, `.rpm` with their `.sig`, `SHA256SUMS`, `latest.json`). Everything else is
a 404 (other methods a 405), and the Worker only follows redirects that stay on `github.com` and
`*.githubusercontent.com`. `latest.json` is all or nothing: if one download URL in it isn't a
release file of this repository, the Worker answers 502 and serves nothing.

**An update is as safe as it was.** The app checks every download against the minisign public key
in `tauri.conf.json`, so a compromised or misbehaving Worker can refuse to serve, but can't make
anyone install something that wasn't signed with the release key.

## Deploy

One Cloudflare account (free plan) and no other setup. Nothing in this repository changes until
the Worker's URL exists.

Dashboard:

1. Cloudflare dashboard -> Workers & Pages -> Create -> Create Worker.
2. Name it `gitviber-updates` and deploy the Hello World it offers.
3. Edit code, replace everything with the contents of [`worker.js`](worker.js), Deploy.
4. The URL is `https://gitviber-updates.gitviber.workers.dev`.

Or with Wrangler (not a dependency of this repository; `npx` fetches it):

```sh
cd packaging/update-proxy
npx wrangler login
npx wrangler deploy
```

[`wrangler.toml`](wrangler.toml) names the Worker `gitviber-updates` and turns the `workers.dev`
URL on. Deploy again after any change to `worker.js`; a release needs no deploy.

## Test it

```sh
MIRROR=https://gitviber-updates.gitviber.workers.dev
curl -s $MIRROR/latest.json | head -c 600                 # version, and urls under $MIRROR/download/
curl -s $MIRROR/latest.json | grep -o '"url":"[^"]*"' | sort -u   # every url under $MIRROR/download/
curl -sI $MIRROR/download/v0.1.9/SHA256SUMS                # 200, content-length, accept-ranges: bytes
curl -s -r 0-99 -o /dev/null -w '%{http_code} %{size_download}\n' $MIRROR/download/v0.1.9/GitViber_0.1.9_universal.dmg   # 206 100
curl -s -o /dev/null -w '%{speed_download} B/s\n' $MIRROR/download/v0.1.9/GitViber_0.1.9_universal.app.tar.gz
curl -s -o /dev/null -w '%{http_code}\n' "$MIRROR/download/v0.1.9/..%2f..%2fx.dmg"      # 404
curl -s -o /dev/null -w '%{http_code}\n' -X POST $MIRROR/latest.json                    # 405
```

The Worker's own checks run offline with `pnpm test` (`worker.test.mjs`, a stubbed `fetch`).
Not covered there: GitHub's real redirects and a `HEAD` to its CDN, which only the curl lines
above show.

## Point the app at it

The mirror goes first and GitHub second, in `plugins.updater.endpoints` of
`src-tauri/tauri.conf.json`:

```json
"endpoints": ["https://gitviber-updates.gitviber.workers.dev/latest.json", "https://github.com/emircan-sahin/gitviber/releases/latest/download/latest.json"]
```

It takes effect in the release that ships it; installed versions keep asking GitHub only.

How the updater (`tauri-plugin-updater` 2.11) treats two endpoints: it asks them in order and
takes the first answer that is a 2xx with a valid manifest. A network error, a non-2xx status
(the Worker's 502, or a Cloudflare error page when the free quota is spent) or a body that isn't
a manifest moves on to the next one. A 204 means "no update" and ends the search. The
`{{target}}`, `{{arch}}` and `{{current_version}}` placeholders aren't needed: `latest.json`
already has one entry per platform.

What stays as it is: a manifest that answers 200 but whose download then fails doesn't fall back
to GitHub (the user can try again from the update dialog), and the app needs no CSP change. The
check and the download run in Rust (`reqwest`), not in the page, and the page's `connect-src`
(`ipc: http://ipc.localhost`) lists no host today, GitHub's included. Nothing in `src/` or
`src-tauri/src/` assumes a `github.com` download URL; `RELEASES_URL` is only the link the
`.deb`/`.rpm` installs open.

## Limits and cost

Free plan: 100,000 Worker requests a day (resets at 00:00 UTC), 10 ms CPU a request (streaming a
body costs almost none) and 50 subrequests a request (a download uses two or three). There is no
bandwidth charge for Workers. At one check every four hours per install plus a download, that
covers a few thousand installs a day. Past the quota Cloudflare answers with an error, and
installs fall back to GitHub for the check.

Caching: on a custom domain the Worker stores each whole file in Cloudflare's cache (the Cache
API, keyed by tag and file, `immutable` since a published release never changes) and serves
repeat downloads and Range requests from the datacenter. The Cache API does nothing on
`workers.dev`, so there every download is fetched from GitHub; that is still the fast path the
mirror exists for. Add a custom domain only if the repeat traffic ever matters.

## Roll back

- Dashboard: the Worker -> Deployments -> an earlier version -> Roll back; or
  `npx wrangler rollback`.
- To switch the mirror off without touching Cloudflare, delete the Worker: the app's next
  endpoint is GitHub, so installs keep updating. Remove the first endpoint in the next release.
