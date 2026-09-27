import { LoaderCircle, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";

/** ✦ runs the configured agent CLI (`program`) for `what`; while it runs, it stops it. */
export function SuggestButton({ what, program, suggesting, disabled, shortcut, onSuggest, onCancel }: { what: string; program: string; suggesting: boolean; disabled: boolean; shortcut?: string; onSuggest: () => void; onCancel: () => void }) {
  return (
    <Tip label={suggesting ? `Stop ${program}` : `Suggest ${what} with ${program}`} shortcut={suggesting ? undefined : shortcut}>
      <Button type="button" variant="ghost" size="icon" aria-label={suggesting ? "Stop suggesting" : `Suggest ${what}`} disabled={!suggesting && disabled} onClick={suggesting ? onCancel : onSuggest}>
        {suggesting ? <LoaderCircle className="animate-spin" /> : <Sparkles />}
      </Button>
    </Tip>
  );
}
