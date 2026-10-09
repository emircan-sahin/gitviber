// What a browser tab's address bar makes of what's typed, and how its page is named.

/** A tab before its first address. */
export const BLANK = "about:blank";

const PORT = /^:?(\d{1,5})$/;
// This machine (127.1 too, as ping takes it) and the LAN, which dev servers serve without a certificate.
const PLAIN_HOST = /^(?:localhost|[\w-]+\.localhost|\d{1,3}(?:\.\d{1,3}){3}|127(?:\.\d{1,3}){1,2}|\[[\da-f:.]+\])(?::\d{1,5})?$/i;

/**
 * A web page's address as the URL parser writes it; null for anything else (javascript:, file:,
 * mailto:), for port 0, and for one with a user name, which only serves to dress up its host
 * (`localhost:3000@evil.example` is evil.example).
 */
function webUrl(text: string): string | null {
  try {
    const url = new URL(text);
    const web = url.protocol === "http:" || url.protocol === "https:";
    return web && !url.username && !url.password && url.port !== "0" ? url.href : null;
  } catch {
    return null;
  }
}

/**
 * The address to load for what's typed: a port is this machine's (`5173`, `:5173`), this machine
 * and the LAN load over http, a name with a dot over https. There's no search: a bare word or text
 * with spaces is no address (null), and only web pages load.
 */
export function normalizeUrl(input: string): string | null {
  const text = input.trim();
  if (!text || /\s/.test(text)) return null;
  if (text === BLANK) return BLANK;
  const port = PORT.exec(text)?.[1];
  if (port) return Number(port) >= 1 && Number(port) <= 65535 ? `http://localhost:${Number(port)}/` : null;
  if (/^https?:\/\//i.test(text)) return webUrl(text);
  const host = text.split(/[/?#]/)[0];
  if (PLAIN_HOST.test(host)) return webUrl(`http://${text}`);
  // A scheme of its own (javascript:, mailto:), unlike a name with a port (example.dev:8443).
  if (/^[a-z][\w+.-]*:(?!\d)/i.test(text)) return null;
  return host.includes(".") ? webUrl(`https://${text}`) : null;
}

/** What a stored tab may load again: a web page, or the blank one it started on. */
export const isPageUrl = (url: unknown): url is string => typeof url === "string" && (url === BLANK || webUrl(url) === url);

/** A page by its address, without the scheme or a lone trailing slash: `localhost:5173/docs`. */
export function pageLabel(url: string): string {
  if (url === BLANK || !url) return "New Tab";
  const label = url.replace(/^https?:\/\//, "");
  return label.endsWith("/") && label.indexOf("/") === label.length - 1 ? label.slice(0, -1) : label;
}

/** The host and port the page is on, or "" for a blank tab. */
export function pageHost(url: string): string {
  try {
    return url === BLANK ? "" : new URL(url).host;
  } catch {
    return "";
  }
}

/** On Linux a tab frames only localhost and 127.0.0.1: what the app's CSP (frame-src) lets in. */
export function isFrameable(url: string): boolean {
  try {
    const u = new URL(url);
    return (u.protocol === "http:" || u.protocol === "https:") && (u.hostname === "localhost" || u.hostname === "127.0.0.1");
  } catch {
    return false;
  }
}

/** A port's page on this machine. */
export const portUrl = (port: number) => `http://localhost:${port}/`;
