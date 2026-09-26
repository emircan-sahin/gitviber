// llms.txt and llms-full.txt (llmstxt.org), written on every build from en.json and the README's
// Install, Privacy and FAQ sections, so what they tell a model never drifts from the site.

// Plain-text readers garble typographic punctuation; keep the files ASCII where it's only style.
const ascii = (s) =>
  s
    .replace(/[—–]/g, "-")
    .replace(/…/g, "...")
    .replace(/→/g, "->")
    .replace(/·/g, "-")
    .replace(/[‘’]/g, "'")
    .replace(/[“”]/g, '"')
    .replace(/⌘/g, "Cmd+");

/** A `## heading` section of the README, without the heading, links made absolute. */
function section(readme, heading, repo) {
  const lines = readme.split("\n");
  const start = lines.indexOf(`## ${heading}`);
  if (start < 0) throw new Error(`llms: README has no "## ${heading}"`);
  const end = lines.findIndex((l, i) => i > start && l.startsWith("## "));
  return lines
    .slice(start + 1, end < 0 ? undefined : end)
    .join("\n")
    .trim()
    .replace(/\]\(#([^)]+)\)/g, `](${repo}#$1)`)
    .replace(/\]\((?!https?:)([^)]+)\)/g, `](${repo}/blob/main/$1)`);
}

export function llms({ t, readme, locales, pageUrl, siteUrl, version, repo, brew, author, chapters, chapterText }) {
  const home = `${siteUrl}/`;
  // The FAQ opens with what GitViber is: the summary both files start with.
  const intro = `${t.faq.items[0].a} Licensed under the GPL-3.0, with no account and no telemetry.`;
  const faq = t.faq.items.map(({ q, a }) => `### ${q}\n\n${a}`);
  // The README's FAQ answers what the site's doesn't (logs, settings, uninstalling, Gatekeeper).
  const asked = new Set(t.faq.items.map(({ q }) => q));
  const readmeFaq = section(readme, "FAQ", repo)
    .split(/\n\n+/)
    .map((p) => p.replace(/\s*\n\s*/g, " ").match(/^\*\*(.+?)\*\* (.+)$/))
    .filter((m) => m && !asked.has(m[1]))
    .map(([, q, a]) => `### ${q}\n\n${a}`);
  const links = [
    `- [Source and README](${repo}): features, install, shortcuts, FAQ`,
    `- [Latest release](${repo}/releases/latest): version ${version}. macOS universal .dmg (signed and notarized), Linux .deb, .rpm and AppImage`,
    `- [Changelog](${repo}/blob/main/CHANGELOG.md)`,
    `- [License](${repo}/blob/main/LICENSE): GPL-3.0`,
  ];
  const pages = locales.map((l) => `- [GitViber, ${l.name}](${pageUrl(l)})`);
  const contact = [
    `- Bugs and feature requests: [GitHub Issues](${repo}/issues)`,
    `- Security reports: [security policy](${repo}/security/policy)`,
    `- Author: ${author.name}, [GitHub](${author.github}), [X](${author.x}) (${author.xHandle}), [LinkedIn](${author.linkedin})`,
  ];

  const short = `# GitViber

> ${t.meta.description}

${intro}

For every feature, install steps, privacy details and the FAQ in one file, read [llms-full.txt](${home}llms-full.txt).

## Site

${pages.join("\n")}

## Links

${links.join("\n")}

## Install

- Homebrew: \`${brew}\`
- Or download from the [latest release](${repo}/releases/latest). Needs git 2.36 or newer.

## Contact

${contact.join("\n")}
`;

  const feature = (title, text) => `- **${title}** ${text}`;
  const full = `# GitViber

> ${t.meta.description}

${intro}

- Website: ${home}
- Version: ${version}
- Made by ${author.name} (${author.github})
- Site languages: ${locales.map((l) => l.name).join(", ")}. The app's interface is in English.

## Links

${links.join("\n")}

## Why GitViber

${t.numbers.title} ${t.numbers.subtitle}

${t.numbers.items.map((i) => feature(`${i.big}: ${i.title}`, i.text)).join("\n")}

## Features

### ${t.story.title}

${t.story.text}

${chapters(t)
  .map((c, i) => feature(`${t.story.labels[i]}: ${c.title}`, chapterText(c.text, i)))
  .join("\n")}

### ${t.rest.title}

${t.rest.subtitle}

${t.rest.items.map((i) => feature(`${i.title}:`, i.text)).join("\n")}

## Install

${section(readme, "Install", repo)}

## Privacy

${section(readme, "Privacy", repo)}

## FAQ

${[...faq, ...readmeFaq].join("\n\n")}

## Contact

${contact.join("\n")}
`;

  return { short: ascii(short), full: ascii(full) };
}
