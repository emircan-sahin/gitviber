export interface Release {
  /** "0.1.0", or null when the build couldn't reach GitHub. */
  version: string | null;
  url: string;
  assets: Partial<Record<"dmg" | "deb" | "rpm" | "appImage", string>>;
}

declare const __RELEASE__: Release;
declare const __SITE_URL__: string;
declare const __VERSION__: string;
declare const __STARS__: number | null;

export const release = __RELEASE__;
export const siteUrl = __SITE_URL__;
/** The published release, or package.json's when GitHub couldn't be reached. */
export const version = __VERSION__;
/** The repo's stars when the site was built, or null when GitHub couldn't be reached. */
export const buildStars = __STARS__;
export const REPO_URL = "https://github.com/emircan-sahin/gitviber";
export const AUTHOR = {
  name: "Emircan Sahin",
  github: "https://github.com/emircan-sahin",
  x: "https://x.com/EmircanSahinX",
  xHandle: "@EmircanSahinX",
  linkedin: "https://www.linkedin.com/in/emircan-sahin/",
};
export const BREW = "brew install --cask emircan-sahin/tap/gitviber";
