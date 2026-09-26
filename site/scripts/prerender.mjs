// Renders one static page per locale into dist/ (dist/index.html, dist/tr/index.html, …), so
// crawlers and link previews get the whole page in every language: GitHub Pages only serves
// files, there's nothing to render on request.
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { llms } from "./llms.mjs";

const root = path.resolve(import.meta.dirname, "..");
const out = process.env.SITE_OUT || "dist";
const dist = path.join(root, out);
const ssr = path.join(root, `${out}-ssr`);

const { render, LOCALES, pageUrl, ogImagePath, siteUrl, version, REPO_URL, BREW, AUTHOR, chapters, chapterText } = await import(pathToFileURL(path.join(ssr, "entry-server.js")).href);
const template = readFileSync(path.join(dist, "index.html"), "utf8");
for (const marker of ['<html lang="en">', "<!--head-->", "<!--app-->"]) {
  if (!template.includes(marker)) throw new Error(`prerender: no ${marker} in dist/index.html`);
}

for (const locale of LOCALES) {
  const { html, head } = render(locale.code);
  const page = template
    .replace('<html lang="en">', `<html lang="${locale.code}">`)
    .replace("<!--head-->", head)
    .replace("<!--app-->", html);
  const dir = path.join(dist, locale.path);
  mkdirSync(dir, { recursive: true });
  writeFileSync(path.join(dir, "index.html"), page);
}
rmSync(ssr, { recursive: true, force: true });

// A new translation shows the English link preview until `pnpm og` renders its own.
for (const locale of LOCALES.slice(1)) {
  const image = path.join(dist, ogImagePath(locale));
  if (existsSync(image)) continue;
  console.warn(`prerender: no ${ogImagePath(locale)}, using the English one; run pnpm og`);
  copyFileSync(path.join(dist, ogImagePath(LOCALES[0])), image);
}

// Every locale lists every other, the way hreflang in the pages does.
const home = pageUrl(LOCALES[0]);
// The last change to the site, not the build: a lastmod that moves on every build gets ignored.
let lastmod;
try {
  lastmod = execFileSync("git", ["log", "-1", "--format=%cs", "--", "."], { cwd: root, encoding: "utf8" }).trim();
} catch {}
lastmod ||= new Date().toISOString().slice(0, 10);
const alternates = LOCALES.map((l) => `    <xhtml:link rel="alternate" hreflang="${l.code}" href="${pageUrl(l)}" />`).join("\n");
const urls = LOCALES.map(
  (l) => `  <url>
    <loc>${pageUrl(l)}</loc>
    <lastmod>${lastmod}</lastmod>
    <changefreq>weekly</changefreq>
    <priority>1.0</priority>
${alternates}
    <xhtml:link rel="alternate" hreflang="x-default" href="${home}" />
  </url>`,
).join("\n");
writeFileSync(
  path.join(dist, "sitemap.xml"),
  `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">
${urls}
</urlset>
`,
);

// Search and AI crawlers welcome by name, and no block list: being read and cited by answer
// engines is the point of the page. robots.txt only counts at a host's root, so this one takes
// effect once the site has its own domain.
const CRAWLERS = [
  "Googlebot", "bingbot", "DuckDuckBot", "Applebot", "YandexBot",
  "GPTBot", "ChatGPT-User", "OAI-SearchBot",
  "ClaudeBot", "Claude-User", "Claude-SearchBot", "anthropic-ai",
  "PerplexityBot", "Perplexity-User",
  "Google-Extended", "Gemini-Deep-Research", "Google-CloudVertexBot",
  "MistralAI-User", "DuckAssistBot",
];
const robots = [...CRAWLERS, "*"].map((agent) => `User-agent: ${agent}\nAllow: /\n`).join("\n");
writeFileSync(path.join(dist, "robots.txt"), `${robots}\nSitemap: ${home}sitemap.xml\n`);

const en = JSON.parse(readFileSync(path.join(root, "src/i18n/messages/en.json"), "utf8"));
const readme = readFileSync(path.join(root, "../README.md"), "utf8");
const { short, full } = llms({ t: en, readme, locales: LOCALES, pageUrl, siteUrl, version, repo: REPO_URL, brew: BREW, author: AUTHOR, chapters, chapterText });
writeFileSync(path.join(dist, "llms.txt"), short);
writeFileSync(path.join(dist, "llms-full.txt"), full);

// GitHub Pages serves this for any path that doesn't exist, such as /pt-BR/ instead of /pt-br/.
writeFileSync(
  path.join(dist, "404.html"),
  `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<meta name="robots" content="noindex" />
<title>Not found - GitViber</title>
<style>
  body { margin: 0; min-height: 100svh; display: grid; place-items: center; background: #0b0b0c; color: #ececee; font: 16px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; text-align: center; }
  h1 { margin: 0 0 8px; font-size: 32px; letter-spacing: -0.02em; }
  p { margin: 0; color: #a2a2a8; }
  a { color: #4a9ff5; }
</style>
</head>
<body>
<main>
  <h1>Page not found</h1>
  <p>${LOCALES.map((l) => `<a href="${pageUrl(l)}" hreflang="${l.code}" lang="${l.code}">${l.name}</a>`).join(" · ")}</p>
</main>
</body>
</html>
`,
);
