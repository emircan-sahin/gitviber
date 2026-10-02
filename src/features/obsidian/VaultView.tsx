import { Copy, ExternalLink, Eye, FileCode2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Segmented } from "@/components/ui/segmented";
import { Tip } from "@/components/ui/tooltip";
import { FileIcon } from "@/components/FileIcon";
import { PathLabel } from "@/components/StatusBadge";
import { type DiffPair, errorMessage, type FileText, vaultApi } from "@/lib/api";
import { copyText } from "@/lib/app/clipboard";
import { failed } from "@/lib/app/toast";
import { useCommands } from "@/lib/commands/keybindings";
import { editedText, saveEdit, useEdited } from "@/lib/editor/edits";
import { keepsLineEndings } from "@/lib/editor/lineEndings";
import { useVaultRevision, vaultList } from "@/lib/obsidian/vault";
import { basename } from "@/lib/path";
import { editPath, type Selection } from "@/lib/repo/selection";
import { useSettings } from "@/lib/settings";
import { FIT, type Zoom } from "@/lib/ui/svg";
import { isMarkdown } from "@/features/viewer/MarkdownView";
import { isSvg, MediaPanel, mediaKind, SvgView, useBytesUrl } from "@/features/viewer/MediaView";
import { MonacoView } from "@/features/viewer/MonacoView";
import type { Tab } from "@/features/viewer/tabs";
import { VaultMarkdown } from "./VaultMarkdown";

type VaultSelection = Extract<Selection, { kind: "vault" }>;

const MISSING: FileText = { text: "", binary: false, tooLarge: false, exists: false, lossy: false, lfsMissing: null };

/** The file's text, read again when the vault changes; the copy on show stays while it does, and when the new one says the same. */
function useVaultText(sel: VaultSelection, skip: boolean) {
  const rev = useVaultRevision(sel.vault);
  const [state, setState] = useState<{ key: string; file: FileText | null; error: string | null }>({ key: "", file: null, error: null });
  const key = `${sel.vault}\0${sel.path}`;
  useEffect(() => {
    if (skip) return;
    let live = true;
    vaultApi.readFile(sel.vault, sel.path).then(
      (file) =>
        live &&
        setState((s) => {
          const same = s.key === key && s.file && s.file.text === file.text && s.file.exists === file.exists && s.file.lossy === file.lossy;
          return same ? s : { key, file, error: null };
        }),
      (e) => live && setState({ key, file: null, error: errorMessage(e) }),
    );
    return () => {
      live = false;
    };
  }, [key, rev, skip, sel.vault, sel.path]);
  return state.key === key ? state : { key, file: null, error: null };
}

/**
 * A file of an Obsidian vault in a tab: a note rendered (or its source, typed into and saved
 * like a repo file), an image, a player or a PDF, an SVG drawn, anything else as code.
 */
export function VaultView({ tab, sel, onOpen }: { tab: Tab; sel: VaultSelection; onOpen: (s: Selection, pin?: boolean) => void }) {
  const s = useSettings();
  const path = sel.path;
  const media = mediaKind(path) !== null;
  const markdown = isMarkdown(path);
  const svg = isSvg(path);
  const key = editPath(sel)!;
  const { file, error } = useVaultText(sel, media);
  const [preview, setPreview] = useState(markdown ? s.markdownPreview : true);
  const [zoom, setZoom] = useState<Zoom>(FIT);
  const dirty = useEdited().has(key);
  const pair = useMemo<DiffPair | null>(() => file && { original: MISSING, modified: file, rows: [], whitespaceHidden: false, eolOnly: false }, [file]);
  // Text the code view can't turn back into the file's bytes stays read-only, as in the repo.
  const editable = !!file && file.exists && !file.lossy && !file.binary && !file.tooLarge && keepsLineEndings(file.text);
  const text = (dirty ? editedText(key) : undefined) ?? file?.text ?? "";
  const vaultName = vaultList.use()?.find((v) => v.path === sel.vault)?.name ?? basename(sel.vault);
  useCommands({ "file.save": dirty ? () => void saveEdit(key) : undefined });

  const switchable = markdown || svg;
  const rendered = switchable && preview;
  const problem = error ?? (file && (!file.exists ? "This file no longer exists" : file.tooLarge ? "This file is too large to show" : file.binary ? "This file can't be shown" : null));

  return (
    <>
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border pr-2 pl-3">
        <FileIcon path={path} />
        <PathLabel path={`${vaultName}/${path}`} className="min-w-0 text-[12px]" />
        <Tip label="Copy path">
          <button className="hit-area text-subtle hover:text-foreground focus-visible:text-foreground" onClick={() => copyText(`${sel.vault}/${path}`, "Path copied")}>
            <Copy className="size-3" />
          </button>
        </Tip>
        <div className="ml-auto flex shrink-0 items-center gap-1">
          <Button variant="secondary" size="sm" onClick={() => vaultApi.openInObsidian(sel.vault, path).catch(failed("Could not open Obsidian"))}>
            <ExternalLink /> Open in Obsidian
          </Button>
          {switchable && (
            <div className="ml-1">
              <Segmented
                value={preview ? "preview" : "code"}
                onChange={(v) => setPreview(v === "preview")}
                options={[
                  { value: "code", label: "Code", icon: FileCode2 },
                  { value: "preview", label: "Preview", icon: Eye },
                ]}
              />
            </div>
          )}
        </div>
      </div>
      <div className="relative min-h-0 flex-1">
        {media ? (
          <VaultMediaFile vault={sel.vault} path={path} />
        ) : problem ? (
          <div className="flex h-full items-center justify-center p-6 text-center text-[12.5px] text-muted-foreground">{problem}</div>
        ) : !pair ? null : rendered && svg ? (
          <SvgView before={null} after={text} stacked={false} zoom={zoom} onZoom={setZoom} backdrop="theme" />
        ) : rendered ? (
          <VaultMarkdown vault={sel.vault} path={path} text={text} tabKey={tab.key} onOpen={onOpen} />
        ) : (
          <MonacoView pair={pair} path={key} mode="file" collapse={false} wrap={s.wordWrap} scrollKey={tab.key} editable={editable} />
        )}
      </div>
    </>
  );
}

function VaultMediaFile({ vault, path }: { vault: string; path: string }) {
  const rev = useVaultRevision(vault);
  const media = useBytesUrl(`${vault}\0${path}\0${rev}`, path, () => vaultApi.media(vault, path));
  // Mounted again per file, so an image's size doesn't carry over to the next.
  return <MediaPanel key={path} {...media} path={path} />;
}
