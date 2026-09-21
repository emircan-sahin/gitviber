import { useEffect, useRef, useState } from "react";
import { api, type DiffKind, errorMessage } from "@/lib/api";
import { cn } from "@/lib/utils";

type MediaKind = "image" | "video" | "audio" | "pdf";

// SVG stays out: it is text, so it gets the code view and a real diff.
const TYPES: Record<string, [MediaKind, string]> = {
  png: ["image", "image/png"],
  jpg: ["image", "image/jpeg"],
  jpeg: ["image", "image/jpeg"],
  gif: ["image", "image/gif"],
  webp: ["image", "image/webp"],
  avif: ["image", "image/avif"],
  bmp: ["image", "image/bmp"],
  ico: ["image", "image/x-icon"],
  tif: ["image", "image/tiff"],
  tiff: ["image", "image/tiff"],
  heic: ["image", "image/heic"],
  mp4: ["video", "video/mp4"],
  m4v: ["video", "video/x-m4v"],
  mov: ["video", "video/quicktime"],
  webm: ["video", "video/webm"],
  ogv: ["video", "video/ogg"],
  mp3: ["audio", "audio/mpeg"],
  wav: ["audio", "audio/wav"],
  m4a: ["audio", "audio/mp4"],
  aac: ["audio", "audio/aac"],
  flac: ["audio", "audio/flac"],
  ogg: ["audio", "audio/ogg"],
  oga: ["audio", "audio/ogg"],
  opus: ["audio", "audio/ogg"],
  aif: ["audio", "audio/aiff"],
  aiff: ["audio", "audio/aiff"],
  pdf: ["pdf", "application/pdf"],
};

function typeOf(path: string) {
  const name = path.slice(path.lastIndexOf("/") + 1).toLowerCase();
  const dot = name.lastIndexOf(".");
  return dot > 0 ? TYPES[name.slice(dot + 1)] : undefined;
}

export function mediaKind(path: string): MediaKind | null {
  return typeOf(path)?.[0] ?? null;
}

export interface MediaSource {
  kind: DiffKind;
  path: string;
  oldPath: string | null;
  sha: string | null;
  base: string | null;
  /** Changes whenever the content may have changed (see pairArgs). */
  key: string;
}

/** Previews a media file, side by side when both the before and after versions exist. */
export function MediaView({ src, before, after }: { src: MediaSource; before: boolean; after: boolean }) {
  const both = before && after;
  return (
    <div className="flex h-full min-h-0 divide-x divide-border overflow-hidden">
      {before && <Side src={src} original label={both ? "Before" : undefined} tone="removed" />}
      {after && <Side src={src} original={false} label={both ? "After" : undefined} tone="added" />}
    </div>
  );
}

function Side({ src, original, label, tone }: { src: MediaSource; original: boolean; label?: string; tone: "added" | "removed" }) {
  const { url, size, error } = useMediaUrl(src, original);
  const [dims, setDims] = useState<string | null>(null);
  const kind = mediaKind(original ? (src.oldPath ?? src.path) : src.path);

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      {(label || size !== null) && (
        <div className="flex h-7 shrink-0 items-center gap-2 border-b border-border px-3 text-[11.5px] text-subtle">
          {label && <span className={cn("font-medium", tone === "added" ? "text-added" : "text-removed")}>{label}</span>}
          <span className="ml-auto font-mono">{[dims, size !== null && formatBytes(size)].filter(Boolean).join(" · ")}</span>
        </div>
      )}
      <div className="relative flex min-h-0 flex-1 items-center justify-center overflow-auto p-4">
        {error ? (
          <div className="text-[12.5px] text-muted-foreground">{error}</div>
        ) : !url ? null : kind === "image" ? (
          <img
            src={url}
            onLoad={(e) => setDims(`${e.currentTarget.naturalWidth}×${e.currentTarget.naturalHeight}`)}
            className="checkerboard max-h-full max-w-full object-contain"
          />
        ) : kind === "video" ? (
          <video src={url} controls className="max-h-full max-w-full" />
        ) : kind === "audio" ? (
          <audio src={url} controls className="w-full max-w-md" />
        ) : (
          <iframe src={url} className="absolute inset-0 size-full border-0" />
        )}
      </div>
    </div>
  );
}

export function useMediaUrl(src: MediaSource, original: boolean) {
  const [state, setState] = useState<{ url: string | null; size: number | null; error: string | null }>({ url: null, size: null, error: null });
  const current = useRef<string | null>(null);
  const { kind, path, oldPath, sha, base, key } = src;

  useEffect(() => {
    let live = true;
    const file = original ? (oldPath ?? path) : path;
    // SVG is not in TYPES (it is text), but markdown can still embed it as an image.
    const mime = typeOf(file)?.[1] ?? (file.toLowerCase().endsWith(".svg") ? "image/svg+xml" : undefined);
    // The old URL stays until its replacement is ready, so a working-tree refresh doesn't blank the view.
    const show = (url: string | null, size: number | null, error: string | null) => {
      if (current.current) URL.revokeObjectURL(current.current);
      current.current = url;
      setState({ url, size, error });
    };
    api
      .media(kind, path, oldPath, sha, base, original)
      .then((bytes) => live && show(URL.createObjectURL(new Blob([bytes], { type: mime })), bytes.byteLength, null))
      .catch((e) => live && show(null, null, errorMessage(e)));
    return () => {
      live = false;
    };
  }, [key, kind, path, oldPath, sha, base, original]);
  useEffect(() => () => void (current.current && URL.revokeObjectURL(current.current)), []);
  return state;
}

function formatBytes(n: number) {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}
