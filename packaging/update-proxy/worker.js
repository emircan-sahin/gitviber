// Download mirror for GitViber's updates (see README.md): a Cloudflare Worker that serves the
// latest release's latest.json with its download URLs pointed at itself, and streams the release
// files from GitHub. GitHub's asset host can be very slow for some users; a Worker fetches it
// datacenter to datacenter. The app still checks every download against its minisign public key,
// so a bad mirror can refuse to serve but can't make anyone install something unsigned.
//
// Never an open proxy: the upstream host and repository are fixed here, and the only things
// taken from a request are a tag and a file name that pass the checks below.

const REPO = "emircan-sahin/gitviber";
const RELEASES = `https://github.com/${REPO}/releases`;
const LATEST_JSON = `${RELEASES}/latest/download/latest.json`;

const TAG = /^v\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$/;
const FILE = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/;
const FILE_NAMES = ["SHA256SUMS", "latest.json"];
const FILE_SUFFIXES = [".app.tar.gz", ".app.tar.gz.sig", ".dmg", ".AppImage", ".AppImage.sig", ".deb", ".deb.sig", ".rpm", ".rpm.sig"];

// A release file redirects to a signed URL on one of GitHub's content hosts.
const HOP_HOSTS = (host) => host === "github.com" || host.endsWith(".githubusercontent.com");
const MAX_HOPS = 4;
// To the response headers: a stalled GitHub shouldn't hold a request open until the platform cuts it.
const UPSTREAM_TIMEOUT_MS = 15_000;

// One byte range, as curl, browsers and download managers send it.
const RANGE = /^bytes=\d*-\d*$/;
const FORWARDED = ["content-length", "content-type", "content-range", "accept-ranges", "etag", "last-modified"];
const IMMUTABLE = "public, max-age=31536000, immutable";

const validTag = (tag) => tag.length <= 64 && TAG.test(tag) && !tag.includes("..");
const validFile = (file) => FILE.test(file) && !file.includes("..") && (FILE_NAMES.includes(file) || FILE_SUFFIXES.some((s) => file.endsWith(s) && file.length > s.length));

const text = (status, body, extra = {}) =>
  new Response(`${body}\n`, { status, headers: { "content-type": "text/plain; charset=utf-8", "x-content-type-options": "nosniff", "cache-control": "no-store", ...extra } });

/** GET/HEAD of `url`, following GitHub's redirects by hand so none can lead off GitHub's hosts. */
async function upstream(url, method, range) {
  const headers = { "user-agent": "gitviber-update-mirror" };
  if (range) headers.range = range;
  for (let hop = 0; hop <= MAX_HOPS; hop++) {
    const abort = new AbortController();
    const timer = setTimeout(() => abort.abort(), UPSTREAM_TIMEOUT_MS);
    let res;
    try {
      res = await fetch(url, { method, headers, redirect: "manual", signal: abort.signal });
    } finally {
      clearTimeout(timer);
    }
    if (![301, 302, 303, 307, 308].includes(res.status)) return res;
    res.body?.cancel().catch(() => {});
    const next = new URL(res.headers.get("location") ?? "", url);
    if (next.protocol !== "https:" || !HOP_HOSTS(next.hostname)) throw new Error("redirect off GitHub");
    url = next.href;
  }
  throw new Error("too many redirects");
}

async function latestJson(origin) {
  let res;
  try {
    res = await upstream(LATEST_JSON, "GET");
  } catch {
    return text(502, "GitHub could not be reached");
  }
  if (!res.ok) return text(res.status === 404 ? 404 : 502, "No latest release");
  const manifest = await res.json().catch(() => null);
  const entries = Object.entries(manifest?.platforms ?? {});
  const prefix = `${RELEASES}/download/`;
  // All or nothing: an entry left out would fail that platform's check without trying the app's
  // next endpoint (GitHub itself), but a 502 makes every install fall back to it.
  const rewritten = entries.map(([key, entry]) => {
    const [tag, file, ...rest] = typeof entry?.url === "string" && entry.url.startsWith(prefix) ? entry.url.slice(prefix.length).split("/") : [];
    return validTag(tag ?? "") && validFile(file ?? "") && !rest.length ? [key, { ...entry, url: `${origin}/download/${tag}/${file}` }] : null;
  });
  if (!rewritten.length || rewritten.includes(null)) return text(502, "latest.json has a download URL this mirror won't serve");
  const body = JSON.stringify({ ...manifest, platforms: Object.fromEntries(rewritten) });
  return new Response(body, {
    headers: { "content-type": "application/json; charset=utf-8", "cache-control": "public, max-age=300", "x-content-type-options": "nosniff" },
  });
}

async function download(request, ctx, url, tag, file) {
  const method = request.method;
  const range = request.headers.get("range");
  if (range && !RANGE.test(range)) return text(416, "Unsupported range");
  // Keyed by what was asked for, never by the request's own URL (query strings, casing).
  const key = new Request(`${url.origin}/download/${tag}/${file}`);
  // The Cache API does nothing on workers.dev domains; on a custom domain it serves repeat downloads
  // (and Range requests into them) from the datacenter. Only whole 200s are stored.
  const cache = method === "GET" ? globalThis.caches?.default : undefined;
  const hit = await cache?.match(range ? new Request(key, { headers: { range } }) : key).catch(() => undefined);
  if (hit) return hit;

  let res;
  try {
    res = await upstream(`${RELEASES}/download/${tag}/${file}`, method, range);
  } catch {
    return text(502, "GitHub could not be reached");
  }
  if (res.status !== 200 && res.status !== 206) {
    res.body?.cancel().catch(() => {});
    // GitHub's error pages are HTML: only the status goes through.
    return text(res.status >= 500 || res.status < 400 ? 502 : res.status, "Not available");
  }
  const headers = new Headers({ "x-content-type-options": "nosniff", "cache-control": IMMUTABLE, "accept-ranges": "bytes" });
  for (const name of FORWARDED) {
    const value = res.headers.get(name);
    if (value) headers.set(name, value);
  }
  const out = new Response(res.body, { status: res.status, headers });
  if (cache && res.status === 200 && !range && headers.has("content-length")) ctx?.waitUntil?.(cache.put(key, out.clone()).catch(() => {}));
  return out;
}

export default {
  async fetch(request, _env, ctx) {
    if (request.method !== "GET" && request.method !== "HEAD") return text(405, "Method not allowed", { allow: "GET, HEAD" });
    const url = new URL(request.url);
    if (url.pathname === "/latest.json") return latestJson(url.origin);
    const [, root, tag, file, ...rest] = url.pathname.split("/");
    if (root === "download" && !rest.length && validTag(tag ?? "") && validFile(file ?? "")) return download(request, ctx, url, tag, file);
    return text(404, "Not found");
  },
};
