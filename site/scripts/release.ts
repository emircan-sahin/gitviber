import type { Release } from "../src/release.ts";

const REPO = "emircan-sahin/gitviber";
export const RELEASES_URL = `https://github.com/${REPO}/releases/latest`;

// Asset names carry the version (GitViber_0.1.0_universal.dmg), so direct download links have
// to be looked up per release; the site is rebuilt when a release is published.
const PATTERNS: Record<keyof Release["assets"], RegExp> = {
  dmg: /_universal\.dmg$/,
  deb: /_amd64\.deb$/,
  rpm: /\.x86_64\.rpm$/,
  appImage: /_amd64\.AppImage$/,
};

interface GitHubRelease {
  tag_name: string;
  html_url: string;
  assets: { name: string; browser_download_url: string }[];
}

const headers = () => {
  const token = process.env.GITHUB_TOKEN;
  return { Accept: "application/vnd.github+json", ...(token && { Authorization: `Bearer ${token}` }) };
};

/** The repo's star count at build time; the page refreshes it live (src/hooks/useStars.ts). */
export async function repoStars(): Promise<number | null> {
  try {
    const res = await fetch(`https://api.github.com/repos/${REPO}`, { headers: headers(), signal: AbortSignal.timeout(8000) });
    if (!res.ok) throw new Error(`GitHub answered ${res.status}`);
    return ((await res.json()) as { stargazers_count: number }).stargazers_count;
  } catch (err) {
    console.warn(`site: no star count (${(err as Error).message})`);
    return null;
  }
}

/** The latest published release, or links to the releases page when GitHub can't be reached. */
export async function latestRelease(): Promise<Release> {
  const fallback: Release = { version: null, url: RELEASES_URL, assets: {} };
  try {
    const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, { headers: headers(), signal: AbortSignal.timeout(8000) });
    if (!res.ok) throw new Error(`GitHub answered ${res.status}`);
    const data = (await res.json()) as GitHubRelease;
    const assets: Release["assets"] = {};
    for (const [key, re] of Object.entries(PATTERNS) as [keyof Release["assets"], RegExp][]) {
      const asset = data.assets.find((a) => re.test(a.name));
      if (asset) assets[key] = asset.browser_download_url;
    }
    return { version: data.tag_name.replace(/^v/, ""), url: data.html_url, assets };
  } catch (err) {
    console.warn(`site: no release info (${(err as Error).message}); linking to the releases page`);
    return fallback;
  }
}
