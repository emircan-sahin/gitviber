import { useEffect, useState } from "react";
import { buildStars } from "../release.ts";

const KEY = "gv-stars";
const API = "https://api.github.com/repos/emircan-sahin/gitviber";

/**
 * The repo's star count: the build's, then GitHub's live one. Kept for the tab's session, since
 * the unauthenticated API allows 60 calls an hour per visitor.
 */
export function useStars() {
  const [stars, setStars] = useState(buildStars);
  useEffect(() => {
    try {
      const saved = sessionStorage.getItem(KEY);
      if (saved) return setStars(Number(saved));
    } catch {}
    const ctrl = new AbortController();
    fetch(API, { signal: ctrl.signal })
      .then((r) => (r.ok ? r.json() : Promise.reject()))
      .then(({ stargazers_count: n }: { stargazers_count: number }) => {
        setStars(n);
        try {
          sessionStorage.setItem(KEY, String(n));
        } catch {}
      })
      .catch(() => {});
    return () => ctrl.abort();
  }, []);
  return stars;
}

/** 1234 → "1.2k", the way GitHub shows it. */
export const formatStars = (n: number) => (n < 1000 ? String(n) : `${(n / 1000).toFixed(n < 10000 ? 1 : 0).replace(/\.0$/, "")}k`);
