// Everything in <head> that depends on the language, rendered once per locale at build time
// (scripts/prerender.mjs). index.html keeps what's the same for every page.
import { LOCALES, type Locale, type Messages } from "./i18n/locales.ts";
import { REPO_URL, siteUrl, version } from "./release.ts";

const esc = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** Messages keep `code` in backticks for the page; structured data wants plain text. */
const plain = (s: string) => s.replace(/`/g, "");

const AUTHOR = { name: "Emircan Sahin", url: "https://github.com/emircan-sahin" };
// The README's screenshot, at a URL that doesn't change with every build like the site's hashed copy.
const SCREENSHOT = "https://raw.githubusercontent.com/emircan-sahin/gitviber/main/assets/screenshot-dark.png";

export const pageUrl = (locale: Locale) => `${siteUrl}/${locale.path}`;

/** The locale's link preview, rendered by scripts/og/render.mjs (pnpm og). */
export const ogImagePath = (locale: Locale) => `og/${locale.code.toLowerCase()}.png`;

export function renderHead(locale: Locale, t: Messages) {
  const url = pageUrl(locale);
  const home = `${siteUrl}/`;
  const image = `${siteUrl}/${ogImagePath(locale)}`;
  const id = { site: `${home}#website`, app: `${home}#app`, source: `${home}#source`, author: `${home}#author` };
  const jsonLd = {
    "@context": "https://schema.org",
    "@graph": [
      {
        "@type": "WebSite",
        "@id": id.site,
        url: home,
        name: "GitViber",
        description: t.meta.description,
        inLanguage: LOCALES.map((l) => l.code),
        publisher: { "@id": id.author },
      },
      // The page itself: one page per locale, and its questions are the FAQ section.
      {
        "@type": "FAQPage",
        "@id": `${url}#webpage`,
        url,
        name: t.meta.title,
        description: t.meta.description,
        inLanguage: locale.code,
        isPartOf: { "@id": id.site },
        about: { "@id": id.app },
        primaryImageOfPage: { "@type": "ImageObject", url: image, width: 1200, height: 630 },
        mainEntity: t.faq.items.map((item) => ({
          "@type": "Question",
          name: plain(item.q),
          acceptedAnswer: { "@type": "Answer", text: plain(item.a) },
        })),
      },
      {
        "@type": "SoftwareApplication",
        "@id": id.app,
        name: "GitViber",
        url: home,
        description: t.meta.description,
        applicationCategory: "DeveloperApplication",
        applicationSubCategory: "Git client",
        operatingSystem: "macOS 13 or later, Linux",
        softwareVersion: version,
        softwareRequirements: "git 2.36 or newer",
        downloadUrl: `${REPO_URL}/releases/latest`,
        releaseNotes: `${REPO_URL}/blob/main/CHANGELOG.md`,
        license: "https://www.gnu.org/licenses/gpl-3.0.html",
        isAccessibleForFree: true,
        // The app's own interface is English; the site's languages are on WebSite.
        inLanguage: "en",
        image,
        screenshot: { "@type": "ImageObject", url: SCREENSHOT, caption: t.hero.screenshotAlt, width: 2960, height: 1840 },
        featureList: [
          ...[...t.agents.steps, t.review, t.terminal, t.commit].map((c, i) => `${t.story.labels[i]}: ${c.text.replace("{keys}", "⌘J")}`),
          ...t.rest.items.map((r) => `${r.title}: ${r.text}`),
        ].map(plain),
        keywords: t.meta.keywords,
        offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
        author: { "@id": id.author },
        publisher: { "@id": id.author },
        sameAs: [REPO_URL],
      },
      {
        "@type": "SoftwareSourceCode",
        "@id": id.source,
        name: "GitViber",
        codeRepository: REPO_URL,
        programmingLanguage: ["Rust", "TypeScript"],
        license: "https://www.gnu.org/licenses/gpl-3.0.html",
        author: { "@id": id.author },
        targetProduct: { "@id": id.app },
      },
      { "@type": "Person", "@id": id.author, name: AUTHOR.name, url: AUTHOR.url, sameAs: [AUTHOR.url] },
    ],
  };

  return [
    `<title>${esc(t.meta.title)}</title>`,
    `<meta name="description" content="${esc(t.meta.description)}" />`,
    `<meta name="keywords" content="${esc(t.meta.keywords)}" />`,
    `<link rel="canonical" href="${url}" />`,
    ...LOCALES.map((l) => `<link rel="alternate" hreflang="${l.code}" href="${pageUrl(l)}" />`),
    `<link rel="alternate" hreflang="x-default" href="${pageUrl(LOCALES[0])}" />`,
    `<meta property="og:type" content="website" />`,
    `<meta property="og:url" content="${url}" />`,
    `<meta property="og:site_name" content="GitViber" />`,
    `<meta property="og:title" content="${esc(t.meta.ogTitle)}" />`,
    `<meta property="og:description" content="${esc(t.meta.ogDescription)}" />`,
    `<meta property="og:image" content="${image}" />`,
    `<meta property="og:image:type" content="image/png" />`,
    `<meta property="og:image:width" content="1200" />`,
    `<meta property="og:image:height" content="630" />`,
    `<meta property="og:image:alt" content="${esc(t.meta.ogImageAlt)}" />`,
    `<meta property="og:locale" content="${locale.og}" />`,
    ...LOCALES.filter((l) => l !== locale).map((l) => `<meta property="og:locale:alternate" content="${l.og}" />`),
    `<meta name="twitter:card" content="summary_large_image" />`,
    `<meta name="twitter:title" content="${esc(t.meta.ogTitle)}" />`,
    `<meta name="twitter:description" content="${esc(t.meta.ogDescription)}" />`,
    `<meta name="twitter:image" content="${image}" />`,
    `<meta name="twitter:image:alt" content="${esc(t.meta.ogImageAlt)}" />`,
    // "<" escaped so text in the data can never close the script tag.
    `<script type="application/ld+json">${JSON.stringify(jsonLd).replace(/</g, "\\u003c")}</script>`,
  ].join("\n    ");
}
