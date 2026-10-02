import { createContext, type ComponentProps, type MouseEvent, use, useEffect, useId, useMemo } from "react";
import type { Components } from "react-markdown";
import { PageFind } from "@/components/FindBox";
import { type FileText, vaultApi } from "@/lib/api";
import { toast } from "@/lib/app/toast";
import { useAsyncValue } from "@/hooks/useAsyncValue";
import { safeDecode } from "@/lib/github/markdown";
import { type LinkIndex, linkIndex, resolveLink, splitTarget } from "@/lib/obsidian/links";
import { useVaultFiles, useVaultRevision, vaultFiles } from "@/lib/obsidian/vault";
import { basename } from "@/lib/path";
import { type Selection, selectionKey } from "@/lib/repo/selection";
import { createStore } from "@/lib/store";
import { cn } from "@/lib/utils";
import { followLink, isMarkdown, MarkdownBody, slug } from "@/features/viewer/MarkdownView";
import { isSvg, mediaKind, useBytesUrl } from "@/features/viewer/MediaView";
import { type MarkdownHost, MarkdownHostContext } from "@/features/viewer/markdown/host";

type Open = (s: Selection, pin?: boolean) => void;

/** The note being rendered, for the links, images and embeds in it. */
interface Note {
  vault: string;
  path: string;
  /** 0 for the tab's note, one more per embed it's inside. */
  depth: number;
  /** The notes embedding this one, to stop one embedding itself. */
  trail: string[];
  idPrefix: string;
  files: LinkIndex | null;
  open: Open;
}
const NoteContext = createContext<Note | null>(null);

// Notes embedded in notes embedded in…: deeper than this shows a link, as a cycle does.
const MAX_DEPTH = 4;

// A link to a heading or block in another note: where to scroll once that note's tab shows.
const pending = createStore<{ key: string; anchor: string } | null>(null);

/** The element a #Heading or #^block in a link names, among ids made with `prefix`. */
function anchorTarget(prefix: string, anchor: string) {
  const last = anchor.split("#").filter(Boolean).at(-1) ?? "";
  const id = last.startsWith("^") ? last : slug(last);
  return document.getElementById(prefix + id) ?? document.getElementById(prefix + last) ?? document.getElementById(last);
}

const vaultSel = (vault: string, path: string): Selection => ({ kind: "vault", vault, path });

/** Opens a note of the vault in a tab, scrolled to `anchor` when there's one. */
function openNote(note: Note, path: string, anchor: string) {
  if (path === note.path && note.depth === 0) {
    if (anchor) anchorTarget(note.idPrefix, anchor)?.scrollIntoView();
    return;
  }
  const sel = vaultSel(note.vault, path);
  if (anchor) pending.set({ key: selectionKey(sel), anchor });
  note.open(sel, true);
}

/** Follows a [[wikilink]] or a markdown link written in `note`, as Obsidian does. */
async function follow(note: Note, target: string, wiki: boolean) {
  const { path, anchor } = splitTarget(wiki ? target : safeDecode(target));
  if (!path) return openNote(note, note.path, anchor);
  const file = resolveLink(note.files ?? linkIndex(await vaultFiles(note.vault)), path, note.path);
  if (file) openNote(note, file, anchor);
  else toast("info", `No note named “${path}”`, "This vault has no file by that name.");
}

const stop = (e: MouseEvent) => e.preventDefault();
const isExternal = (href: string) => /^[a-z][a-z0-9+.-]*:/i.test(href);

/** A link in a note: [[wikilinks]] (unresolved ones faded, as Obsidian has them), links to notes, and the web. */
function VaultLink({ node: _node, href, children, ...rest }: ComponentProps<"a"> & { node?: unknown; "data-wikilink"?: string }) {
  const note = use(NoteContext)!;
  const wiki = rest["data-wikilink"];
  if (wiki === undefined && !href) return <span id={rest.id}>{children}</span>;
  const target = wiki ?? href ?? "";
  const local = !isExternal(target) && !target.startsWith("#");
  const path = local ? splitTarget(wiki === undefined ? safeDecode(target) : target).path : "";
  const missing = !!path && !!note.files && !resolveLink(note.files, path, note.path);
  const onClick = (e: MouseEvent) => {
    e.preventDefault();
    if (local || (wiki !== undefined && target.startsWith("#"))) void follow(note, target, wiki !== undefined);
    else if (target.startsWith("#")) {
      const el = anchorTarget(note.idPrefix, safeDecode(target.slice(1)));
      if (el) el.scrollIntoView();
      else followLink(target, () => {}, note.idPrefix);
    } else followLink(target, () => {}, note.idPrefix);
  };
  return (
    <a {...rest} href={href ?? "#"} title={missing ? `${path} isn't in this vault` : rest.title} className={cn(missing && "unresolved")} onClick={onClick} onContextMenu={stop} onAuxClick={stop}>
      {children}
    </a>
  );
}

// "alt|300" or "alt|300x200" in an image's alt text sets its size, as Obsidian reads it.
const SIZED = /^(.*?)\|?\s*(\d+)(?:x(\d+))?$/;
function sized(alt: string) {
  const m = SIZED.exec(alt);
  return m ? { alt: m[1], width: m[2], height: m[3] } : { alt, width: undefined, height: undefined };
}

/** An image in a note: from the web as written, from the vault wherever it is in there. */
function VaultImage({ node: _node, src, alt = "", width, height, title }: ComponentProps<"img"> & { node?: unknown }) {
  const note = use(NoteContext)!;
  if (typeof src !== "string" || !src) return null;
  const size = sized(alt);
  if (isExternal(src)) return <img src={src} alt={size.alt} width={width ?? size.width} height={height ?? size.height} title={title} />;
  const path = note.files ? resolveLink(note.files, safeDecode(src), note.path) : null;
  if (!path) return note.files ? <Missing name={safeDecode(src)} /> : null;
  return <VaultMedia vault={note.vault} path={path} alt={size.alt} width={width ?? size.width} height={height ?? size.height} />;
}

const COMPONENTS: Components = { a: VaultLink as Components["a"], img: VaultImage as Components["img"] };

/** An image, player or PDF from the vault, by its path in there. */
function VaultMedia({ vault, path, alt, width, height, anchor = "" }: { vault: string; path: string; alt?: string; width?: string | number; height?: string | number; anchor?: string }) {
  const rev = useVaultRevision(vault);
  const { url, error } = useBytesUrl(`${vault}\0${path}\0${rev}`, path, () => vaultApi.media(vault, path));
  if (error) return <Missing name={basename(path)} detail={error} />;
  if (!url) return null;
  const kind = isSvg(path) ? "image" : mediaKind(path);
  if (kind === "video") return <video src={url} controls width={width} height={height} className="embed-media" />;
  if (kind === "audio") return <audio src={url} controls className="embed-media" />;
  // #page=3 opens the PDF at that page, as in Obsidian.
  if (kind === "pdf") return <iframe src={anchor ? `${url}#${anchor}` : url} title={basename(path)} className="embed-media" />;
  return <img src={url} alt={alt} width={width} height={height} />;
}

function Missing({ name, detail }: { name: string; detail?: string }) {
  return (
    <span className="text-[12px] text-subtle italic" title={detail}>
      “{name}” isn't in this vault
    </span>
  );
}

/** ![[embed]]: a note (or one of its parts) shown in place, an image, a player, a PDF; anything else as a link. */
function Embed({ target, alias, block }: { target: string; alias: string; block: boolean }) {
  const note = use(NoteContext)!;
  const { path, anchor } = splitTarget(target);
  if (!note.files) return null;
  const file = path ? resolveLink(note.files, path, note.path) : note.path;
  if (!file) return <Missing name={path} />;
  if (isMarkdown(file)) {
    if (note.depth >= MAX_DEPTH || (note.trail.includes(file) && !anchor)) return <EmbedLink note={note} file={file} anchor={anchor} label={alias} />;
    return <NoteEmbed file={file} anchor={anchor} alias={alias} block={block} />;
  }
  if (isSvg(file) || mediaKind(file)) {
    const size = sized(alias);
    return <VaultMedia vault={note.vault} path={file} alt={size.alt} width={size.width} height={size.height} anchor={anchor} />;
  }
  return <EmbedLink note={note} file={file} anchor={anchor} label={alias} />;
}

function EmbedLink({ note, file, anchor, label }: { note: Note; file: string; anchor: string; label: string }) {
  return (
    <a href="#" onClick={(e) => (e.preventDefault(), openNote(note, file, anchor))} onContextMenu={stop} onAuxClick={stop}>
      {label || [basename(file).replace(/\.md$/i, ""), ...anchor.split("#").filter(Boolean)].join(" > ")}
    </a>
  );
}

/** Another note's text, or one heading's part or block of it, shown inside this one. */
function NoteEmbed({ file, anchor, alias, block }: { file: string; anchor: string; alias: string; block: boolean }) {
  const note = use(NoteContext)!;
  const rev = useVaultRevision(note.vault);
  const loaded = useAsyncValue<FileText | null>(() => vaultApi.readFile(note.vault, file), [note.vault, file, rev], null);
  const id = useId();
  const inner = useMemo<Note>(() => ({ ...note, path: file, depth: note.depth + 1, trail: [...note.trail, file], idPrefix: `embed${id.replace(/:/g, "")}-` }), [note, file, id]);
  const Tag = block ? "div" : "span";
  return (
    <Tag className="embed block">
      <span className="embed-title block">
        <EmbedLink note={note} file={file} anchor={anchor} label={alias} />
      </span>
      {loaded && (loaded.exists && !loaded.binary ? <NoteBody text={loaded.text} note={inner} section={anchor || undefined} /> : <Missing name={basename(file)} />)}
    </Tag>
  );
}

function NoteBody({ text, note, section }: { text: string; note: Note; section?: string }) {
  const host = useMemo<MarkdownHost>(
    () => ({
      embed: (target, alias, block) => <Embed target={target} alias={alias} block={block} />,
      wikilink: (target, label) => <VaultLink data-wikilink={target}>{label}</VaultLink>,
    }),
    [],
  );
  return (
    <NoteContext value={note}>
      <MarkdownHostContext value={host}>
        <MarkdownBody text={text} components={COMPONENTS} idPrefix={note.idPrefix} flavor="obsidian" section={section} />
      </MarkdownHostContext>
    </NoteContext>
  );
}

/** A vault note rendered as Obsidian's reading view shows it: links, embeds and all. */
export function VaultMarkdown({ vault, path, text, tabKey, onOpen }: { vault: string; path: string; text: string; tabKey: string; onOpen: Open }) {
  const files = useVaultFiles(vault);
  const note = useMemo<Note>(() => ({ vault, path, depth: 0, trail: [path], idPrefix: "user-content-", files: files && linkIndex(files), open: onOpen }), [vault, path, files, onOpen]);
  const want = pending.use();
  // A link to a part of this note, followed from another: there once it's drawn.
  useEffect(() => {
    if (want?.key !== tabKey) return;
    const frame = requestAnimationFrame(() => {
      anchorTarget(note.idPrefix, want.anchor)?.scrollIntoView();
      pending.set(null);
    });
    return () => cancelAnimationFrame(frame);
  }, [want, tabKey, note.idPrefix, text]);
  return (
    // Focusable so the keyboard can scroll it (focusPanel("code") lands here).
    <div data-code-scroll tabIndex={0} className="h-full overflow-auto outline-none">
      <PageFind />
      <article className="markdown mx-auto max-w-[860px] px-8 py-6 select-text">
        <NoteBody text={text} note={note} />
      </article>
    </div>
  );
}
