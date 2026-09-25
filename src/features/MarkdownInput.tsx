import { Eye, Pencil } from "lucide-react";
import { type ComponentProps, useId, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { Segmented } from "@/components/ui/segmented";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";
import { type MarkdownHome, PullMarkdown } from "./PullView";

/** A textarea for PR or issue markdown with GitHub's Write/Preview switch. `pull`: where it will be posted. */
export function MarkdownInput({ pull, value, className, ...props }: { pull: MarkdownHome; value: string } & Omit<ComponentProps<typeof Textarea>, "value" | "ref">) {
  const [preview, setPreview] = useState(false);
  const [height, setHeight] = useState(0);
  const input = useRef<HTMLTextAreaElement>(null);
  const id = useId();
  const show = (on: boolean) => {
    // The preview takes the textarea's size, so nothing around it moves.
    if (on) setHeight(input.current?.offsetHeight ?? 0);
    // Shown right away, so it can take focus back.
    flushSync(() => setPreview(on));
    if (!on) input.current?.focus();
  };
  return (
    <div className="space-y-1.5">
      <Segmented
        value={preview ? "preview" : "write"}
        onChange={(v) => show(v === "preview")}
        options={[
          { value: "write", label: "Write", icon: Pencil },
          { value: "preview", label: "Preview", icon: Eye },
        ]}
      />
      {/* Hidden rather than unmounted, so it keeps its undo history and cursor. */}
      <Textarea ref={input} value={value} className={cn(className, preview && "hidden")} {...props} />
      {preview && (
        <div style={{ height }} className="overflow-y-auto rounded-md border border-border-strong bg-background">
          <PullMarkdown pull={pull} idPrefix={`draft${id}-`} text={value} empty="Nothing to preview" className="px-3 py-2" />
        </div>
      )}
    </div>
  );
}
