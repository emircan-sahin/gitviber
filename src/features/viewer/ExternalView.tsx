import { Eye, FileCode2, FolderSearch } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Segmented } from "@/components/ui/segmented";
import { type DiffPair, errorMessage, externalApi, type FileText } from "@/lib/api";
import { failed } from "@/lib/app/toast";
import { REVEAL_FAILED, REVEAL_LABEL } from "@/lib/platform";
import { type Selection } from "@/lib/repo/selection";
import { updateSettings, useSettings } from "@/lib/settings";
import { FIT, type Zoom } from "@/lib/ui/svg";
import { FileHeaderPath, Placeholder } from "./FileHeader";
import { isSvg, MediaPanel, mediaKind, SvgView, useBytesUrl } from "./MediaView";
import { MonacoView } from "./MonacoView";
import { placeholderFor } from "./placeholders";
import type { Tab } from "./tabs";

type ExternalSelection = Extract<Selection, { kind: "external" }>;

const MISSING: FileText = { text: "", binary: false, tooLarge: false, exists: false, lossy: false, lfsMissing: null };

/**
 * A file outside the repo that a terminal link named (an agent's screenshot in /tmp): an image, a
 * player or a PDF, or its text as code. Read once as it opens, and never saved.
 */
export function ExternalView({ tab, sel }: { tab: Tab; sel: ExternalSelection }) {
  const s = useSettings();
  const { path } = sel;
  const media = mediaKind(path) !== null;
  // An SVG is drawn unless turned off, as in the repo; its setting is the repo's.
  const svg = isSvg(path);
  const [zoom, setZoom] = useState<Zoom>(FIT);
  const [state, setState] = useState<{ path: string; file: FileText | null; error: string | null }>({ path: "", file: null, error: null });
  useEffect(() => {
    if (media) return;
    let live = true;
    externalApi.readFile(path).then(
      (file) => live && setState({ path, file, error: null }),
      (e) => live && setState({ path, file: null, error: errorMessage(e) }),
    );
    return () => {
      live = false;
    };
  }, [path, media]);
  const { file, error } = state.path === path ? state : { file: null, error: null };
  const pair = useMemo<DiffPair | null>(() => file && { original: MISSING, modified: file, rows: [], whitespaceHidden: false, eolOnly: false }, [file]);
  const special = pair && placeholderFor(pair, true, null);

  return (
    <>
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border pr-2 pl-3">
        <FileHeaderPath path={path} shown={path} />
        <div className="ml-auto flex shrink-0 items-center gap-1">
          {svg && (
            <Segmented
              value={s.svgPreview ? "preview" : "code"}
              onChange={(v) => updateSettings({ svgPreview: v === "preview" })}
              options={[
                { value: "code", label: "Code", icon: FileCode2 },
                { value: "preview", label: "Preview", icon: Eye },
              ]}
            />
          )}
          <Button variant="secondary" size="sm" onClick={() => externalApi.reveal(path).catch(failed(REVEAL_FAILED))}>
            <FolderSearch /> {REVEAL_LABEL}
          </Button>
        </div>
      </div>
      <div className="relative min-h-0 flex-1">
        {media ? (
          <ExternalMedia path={path} />
        ) : error ? (
          <Placeholder title="Could not load" detail={error} />
        ) : special ? (
          <Placeholder title={special} />
        ) : !pair ? null : svg && s.svgPreview ? (
          <SvgView before={null} after={file?.text ?? ""} stacked={false} zoom={zoom} onZoom={setZoom} backdrop="theme" />
        ) : (
          <MonacoView pair={pair} path={path} mode="file" collapse={false} wrap={s.wordWrap} scrollKey={tab.key} editable={false} />
        )}
      </div>
    </>
  );
}

function ExternalMedia({ path }: { path: string }) {
  const media = useBytesUrl(path, path, () => externalApi.media(path));
  // Mounted again per file, so an image's size doesn't carry over to the next.
  return <MediaPanel key={path} {...media} path={path} />;
}
